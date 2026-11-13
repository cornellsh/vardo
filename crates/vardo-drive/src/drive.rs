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
        let response = self.http.send(request).map_err(transport_error)?;

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
        let response = self.http.send(retry).map_err(transport_error)?;
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

/// Separates our own mistakes from the network's.
///
/// Everything a transport reports looks the same from here, but the advice is
/// opposite: a network failure is worth retrying and a malformed request never
/// will be. Collapsing them told a user to check their connection because we
/// had left a space in a URL.
fn transport_error(detail: String) -> SyncError {
    match detail.strip_prefix(crate::http::BUG_PREFIX) {
        Some(rest) => SyncError::Other(format!("Bug in Vardo, not your connection ({rest})")),
        None => SyncError::Transport(detail),
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
        // 403 is overloaded: rate limits, a full Drive and permission denial
        // all share it. Google's `reason` code tells them apart; the message
        // is prose that changes ("User Rate Limit Exceeded" is capitalised).
        403 => match oauth::json_string(&body, "reason").as_deref() {
            Some(
                "userRateLimitExceeded"
                | "rateLimitExceeded"
                | "dailyLimitExceeded"
                | "sharingRateLimitExceeded",
            ) => SyncError::RateLimited {
                retry_after_secs: response.retry_after(),
            },
            // Retrying cannot help until someone frees space.
            Some("storageQuotaExceeded") => SyncError::Other(format!(
                "Google Drive is full. Free some space there, then sync again. ({detail})"
            )),
            Some(_) => SyncError::Unauthorized(detail),
            None if detail.to_lowercase().contains("rate limit") => SyncError::RateLimited {
                retry_after_secs: response.retry_after(),
            },
            None => SyncError::Unauthorized(detail),
        },
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

/// A vault Vardo can see in the user's Drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub modified_unix: Option<i64>,
}

/// Lists the `.kdbx` files this app is allowed to see.
///
/// The important and unobvious part: under the `drive.file` scope this returns
/// *only* files Vardo created or that the user explicitly handed to it. A vault
/// that already exists in someone's Drive will **not** appear here, and that is
/// the scope working correctly rather than a bug to code around. The
/// alternative is asking for read access to the user's entire Drive, which for
/// a password manager is an absurd thing to request in order to populate a
/// list.
///
/// So an empty result is a normal state with a real answer - upload the vault
/// once, after which it is app-created and visible forever - and the UI has to
/// say that rather than showing an empty box.
pub fn list_vaults(
    http: &mut dyn HttpTransport,
    config: ClientConfig,
    tokens: Tokens,
) -> SyncResult<(Vec<DriveFile>, Tokens, bool)> {
    let mut backend = DriveBackend::new(http, config, tokens, String::new());

    // `trashed=false` because a vault in the bin is not a vault you want
    // offered as a sync target, and Drive happily lists them otherwise.
    let query = crate::http::urlencode(
        "mimeType != 'application/vnd.google-apps.folder' and trashed = false",
    );
    let fields = crate::http::urlencode("files(id,name,size,modifiedTime)");
    // Encoded like every other component. `modifiedTime desc` contains a
    // space, and interpolating it raw produced a URL the HTTP client refused to
    // build - which then reached the user as "check your connection".
    let order = crate::http::urlencode("modifiedTime desc");
    let url = format!("{FILES}?q={query}&fields={fields}&pageSize=100&orderBy={order}");

    let response = backend.send(|token| Request::new(Method::Get, &url).bearer(token))?;
    let body = response.text();

    let mut files = Vec::new();
    for chunk in split_objects(&body) {
        let Some(name) = oauth::json_string(&chunk, "name") else {
            continue;
        };
        // Filtering here rather than in the query: Drive has no reliable MIME
        // type for KDBX, so the extension is the only honest signal.
        if !name.to_ascii_lowercase().ends_with(".kdbx") {
            continue;
        }
        let Some(id) = oauth::json_string(&chunk, "id") else {
            continue;
        };
        files.push(DriveFile {
            id,
            name,
            size: oauth::json_string(&chunk, "size")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            modified_unix: oauth::json_string(&chunk, "modifiedTime")
                .as_deref()
                .and_then(parse_rfc3339),
        });
    }

    let updated = backend.tokens_updated;
    Ok((files, backend.tokens().clone(), updated))
}

/// Creates a new file in the user's Drive and returns its id.
///
/// Only used to put a vault into Drive for the first time. Everything after
/// that is [`StorageBackend::write`] against this id, because creating a
/// replacement file would mint a new id and kill every other device's per-file
/// grant.
///
/// A multipart upload rather than two requests, so there is no window in which
/// an empty file exists under the vault's name. A crash between "create
/// metadata" and "upload content" would otherwise leave something that looks
/// like a vault, opens as corrupt, and may already have been synced to the
/// user's other devices.
pub fn create(
    http: &mut dyn HttpTransport,
    config: ClientConfig,
    tokens: Tokens,
    name: &str,
    bytes: &[u8],
) -> SyncResult<(String, Tokens, bool)> {
    let mut backend = DriveBackend::new(http, config, tokens, String::new());

    const BOUNDARY: &str = "vardo-boundary-8f3a1c";
    let url = format!("{UPLOAD}?uploadType=multipart&fields=id");

    // The name is JSON-escaped rather than interpolated: a vault called
    // `my "old" vault.kdbx` is perfectly legal on every platform and would
    // otherwise produce a malformed request that Drive rejects with a message
    // about JSON, sending the user looking in entirely the wrong place.
    let metadata = format!(r#"{{"name":{}}}"#, json_escape(name));

    let mut body = Vec::new();
    body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Type: application/json; charset=UTF-8\r\n\r\n");
    body.extend_from_slice(metadata.as_bytes());
    body.extend_from_slice(format!("\r\n--{BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());

    let response = backend.send(move |token| {
        Request::new(Method::Post, &url)
            .bearer(token)
            .header(
                "Content-Type",
                &format!("multipart/related; boundary={BOUNDARY}"),
            )
            .body(body.clone())
    })?;

    let text = response.text();
    let id = oauth::json_string(&text, "id")
        .ok_or_else(|| SyncError::Other("Drive created the file but returned no id".into()))?;

    let updated = backend.tokens_updated;
    Ok((id, backend.tokens().clone(), updated))
}

/// Escapes a string as a JSON value, quotes included.
fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
