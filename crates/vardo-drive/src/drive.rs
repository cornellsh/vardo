//! The Google Drive [`StorageBackend`].
//!
//! # Update in place, never delete-and-recreate
//!
//! Content is replaced with `PATCH /upload/drive/v3/files/{id}?uploadType=media`,
//! which keeps the file's ID and adds a revision. The alternative — create a
//! new file and remove the old — mints a **new ID**, and a `drive.file` grant
//! is per-file, so every other client's reference to the vault dies.
//!
//! That is not hypothetical. It is precisely what KeePassXC's "safe save"
//! does through a cloud mount, and why Strongbox's support pages tell users to
//! turn safe saving off.
//!
//! # No conditional write exists
//!
//! Drive v3 removed `etag` from the file resource and ignores `If-Match` on
//! the upload endpoint, so there is no way to say "write only if unchanged".
//! This backend therefore does not offer one, and the protocol above it detects
//! lost races after the fact using revision history instead.

use vardo_sync::backend::{RemoteState, Revision, StorageBackend, SyncError, SyncResult};

use crate::http::{HttpTransport, Method, Request};
use crate::oauth::{self, ClientConfig, Tokens};

const FILES: &str = "https://www.googleapis.com/drive/v3/files";
const UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3/files";

/// Fields we ask Drive for. `headRevisionId` is the change signal; `md5Checksum`
/// lets us skip a download when content is unchanged.
const FIELDS: &str = "id,name,size,modifiedTime,headRevisionId,md5Checksum,trashed";

pub struct DriveBackend<'a> {
    http: &'a mut dyn HttpTransport,
    config: ClientConfig,
    tokens: Tokens,
    file_id: String,
    /// Set when a refresh produced a new access token, so the caller can
    /// persist it instead of re-authorising next launch.
    pub tokens_updated: bool,
}

impl<'a> DriveBackend<'a> {
    pub fn new(
        http: &'a mut dyn HttpTransport,
        config: ClientConfig,
        tokens: Tokens,
        file_id: impl Into<String>,
    ) -> Self {
        Self {
            http,
            config,
            tokens,
            file_id: file_id.into(),
            tokens_updated: false,
        }
    }

    pub fn tokens(&self) -> &Tokens {
        &self.tokens
    }

    /// Sends a request, refreshing the access token once on a 401.
    ///
    /// Once, not in a loop: if a freshly refreshed token is also rejected, the
    /// grant has been revoked and retrying would spin against Google's
    /// endpoint while the user waits for an error we already know is coming.
    fn send(&mut self, build: impl Fn(&str) -> Request) -> SyncResult<crate::http::Response> {
        let request = build(&self.tokens.access_token);
        let response = self.http.send(request).map_err(SyncError::Transport)?;

        if response.status != 401 {
            return classify(response);
        }

        let Some(refresh_token) = self.tokens.refresh_token.clone() else {
            return Err(SyncError::Unauthorized(
                "access token expired and no refresh token is stored".into(),
            ));
        };
        let refreshed = oauth::refresh(self.http, &self.config, &refresh_token)
            .map_err(|e| SyncError::Unauthorized(e.to_string()))?;
        self.tokens = refreshed;
        self.tokens_updated = true;

        let retry = build(&self.tokens.access_token);
        let response = self.http.send(retry).map_err(SyncError::Transport)?;
        classify(response)
    }

    fn metadata(&mut self) -> SyncResult<RemoteState> {
        let url = format!(
            "{FILES}/{}?fields={}",
            crate::http::urlencode(&self.file_id),
            crate::http::urlencode(FIELDS)
        );
        let response = self.send(|token| Request::new(Method::Get, &url).bearer(token))?;
        let body = response.text();

        // A trashed file still resolves, but writing to it would put the
        // user's vault in the bin. Treat it as gone.
        if body.contains("\"trashed\":true") {
            return Err(SyncError::NotFound);
        }

        let version = oauth::json_string(&body, "headRevisionId").ok_or_else(|| {
            SyncError::Other("Drive returned no headRevisionId; cannot detect changes".into())
        })?;
        Ok(RemoteState {
            version,
            size: oauth::json_string(&body, "size")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            modified_unix: oauth::json_string(&body, "modifiedTime")
                .as_deref()
                .and_then(parse_rfc3339),
            content_hash: oauth::json_string(&body, "md5Checksum"),
        })
    }
}

fn classify(response: crate::http::Response) -> SyncResult<crate::http::Response> {
    if response.is_success() {
        return Ok(response);
    }
    let body = response.text();
    let detail = oauth::json_string(&body, "message").unwrap_or_else(|| body.clone());

    Err(match response.status {
        401 => SyncError::Unauthorized(detail),
        // 403 is overloaded: quota exhaustion and permission denial share it,
        // and only the message distinguishes them. Treating a permanent
        // permission error as retryable would spin forever.
        403 if detail.contains("ate limit") || detail.contains("uota") => SyncError::RateLimited {
            retry_after_secs: response.retry_after(),
        },
        403 => SyncError::Unauthorized(detail),
        404 => SyncError::NotFound,
        429 => SyncError::RateLimited {
            retry_after_secs: response.retry_after(),
        },
        500..=599 => SyncError::Transport(format!("server error {}: {detail}", response.status)),
        other => SyncError::Other(format!("HTTP {other}: {detail}")),
    })
}

impl StorageBackend for DriveBackend<'_> {
    fn stat(&mut self) -> SyncResult<RemoteState> {
        self.metadata()
    }

    fn read(&mut self) -> SyncResult<(Vec<u8>, RemoteState)> {
        let state = self.metadata()?;
        let url = format!(
            "{FILES}/{}?alt=media",
            crate::http::urlencode(&self.file_id)
        );
        let response = self.send(|token| Request::new(Method::Get, &url).bearer(token))?;
        Ok((response.body, state))
    }

    fn write(&mut self, bytes: &[u8]) -> SyncResult<RemoteState> {
        // PATCH against the existing id: same file, new revision. Never
        // create-and-delete, which would mint a new id and break every other
        // client's per-file grant.
        let url = format!(
            "{UPLOAD}/{}?uploadType=media&fields={}",
            crate::http::urlencode(&self.file_id),
            crate::http::urlencode(FIELDS)
        );
        let body = bytes.to_vec();
        let response = self.send(move |token| {
            Request::new(Method::Patch, &url)
                .bearer(token)
                .header("Content-Type", "application/octet-stream")
                .body(body.clone())
        })?;

        let text = response.text();
        let version = oauth::json_string(&text, "headRevisionId")
            .ok_or_else(|| SyncError::Other("upload returned no headRevisionId".into()))?;
        Ok(RemoteState {
            version,
            size: bytes.len() as u64,
            modified_unix: oauth::json_string(&text, "modifiedTime")
                .as_deref()
                .and_then(parse_rfc3339),
            content_hash: oauth::json_string(&text, "md5Checksum"),
        })
    }

    fn revisions(&mut self) -> SyncResult<Vec<Revision>> {
        let url = format!(
            "{FILES}/{}/revisions?fields=revisions(id,modifiedTime,size)&pageSize=100",
            crate::http::urlencode(&self.file_id)
        );
        let response = self.send(|token| Request::new(Method::Get, &url).bearer(token))?;
        let body = response.text();

        // Newest last in Drive's response; the protocol wants newest first.
        let mut out: Vec<Revision> = split_objects(&body)
            .into_iter()
            .filter_map(|object| {
                Some(Revision {
                    id: oauth::json_string(&object, "id")?,
                    modified_unix: oauth::json_string(&object, "modifiedTime")
                        .as_deref()
                        .and_then(parse_rfc3339),
                    size: oauth::json_string(&object, "size")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0),
                })
            })
            .collect();
        out.reverse();
        Ok(out)
    }

    fn read_revision(&mut self, id: &str) -> SyncResult<Vec<u8>> {
        let url = format!(
            "{FILES}/{}/revisions/{}?alt=media",
            crate::http::urlencode(&self.file_id),
            crate::http::urlencode(id)
        );
        let response = self.send(|token| Request::new(Method::Get, &url).bearer(token))?;
        Ok(response.body)
    }

    fn describe(&self) -> String {
        format!("drive:{}", self.file_id)
    }
}

/// Splits a flat JSON array of objects into their text bodies.
///
/// Brace-counting rather than a parser, for the same reason as
/// [`oauth::json_string`]: the shape is fixed and known.
fn split_objects(json: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (i, c) in json.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => {
                if depth == 1 {
                    start = i;
                }
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 1 {
                    out.push(json[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    out
}

/// Parses `2026-02-18T18:42:09.000Z` to Unix seconds.
fn parse_rfc3339(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |r: std::ops::Range<usize>| text.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, s) = (num(11..13)?, num(14..16)?, num(17..19)?);

    // Days from the Unix epoch, Howard Hinnant's algorithm.
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86400 + h * 3600 + mi * 60 + s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_parses_drive_timestamps() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(
            parse_rfc3339("2026-02-18T18:42:09.000Z"),
            Some(1_771_440_129)
        );
        assert_eq!(parse_rfc3339("nonsense"), None);
        assert_eq!(parse_rfc3339(""), None);
    }

    #[test]
    fn revision_objects_are_split_correctly() {
        let body = r#"{"revisions":[{"id":"1","size":"10"},{"id":"2","size":"20"}]}"#;
        let objects = split_objects(body);
        assert_eq!(objects.len(), 2);
        assert_eq!(oauth::json_string(&objects[0], "id").unwrap(), "1");
        assert_eq!(oauth::json_string(&objects[1], "id").unwrap(), "2");
    }

    #[test]
    fn braces_inside_strings_do_not_confuse_the_splitter() {
        let body = r#"{"revisions":[{"id":"a}b","size":"1"},{"id":"c{d","size":"2"}]}"#;
        let objects = split_objects(body);
        assert_eq!(objects.len(), 2);
        assert_eq!(oauth::json_string(&objects[0], "id").unwrap(), "a}b");
        assert_eq!(oauth::json_string(&objects[1], "id").unwrap(), "c{d");
    }
}
