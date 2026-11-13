//! A [`vardo_sync::StorageBackend`] for one file on a WebDAV server.
//!
//! Contract: every write that must not destroy another device's edit is a
//! `PUT` with `If-Match` on the ETag just read, so the server refuses it when
//! the file moved (`inv:conditional-write-when-possible`). A server whose
//! ETags cannot support that is refused loudly, never written to blindly
//! (`fact:webdav-etag-hazards`): weak or missing ETags fail before writing,
//! and a refusal while the file is provably unchanged fails as broken ETags.
//! Apache marks the ETag of a file changed within the last second as weak, so
//! a weak tag is given one second to become strong before being refused.
//!
//! Every request asks for `Accept-Encoding: identity`. Apache's `mod_deflate`
//! appends `-gzip` to the ETag of a compressed response but checks `If-Match`
//! against the bare tag, so asking for no compression keeps the tag honest.
//!
//! Written against [`Transport`] so it is tested without a network. The real
//! client is behind the `network` feature.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use vardo_sync::backend::WriteOutcome;
use vardo_sync::{RemoteState, Revision, StorageBackend, SyncError, SyncResult};
use zeroize::Zeroizing;

#[cfg(feature = "network")]
pub mod ureq_transport;

/// One HTTP request.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: &'static str,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

/// One HTTP response. Header names are lowercase.
#[derive(Debug, Clone, Default)]
pub struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

/// Sends a request. An `Err` is a transport failure; any status is `Ok`.
pub trait Transport {
    fn send(&mut self, request: Request) -> Result<Response, String>;
}

pub struct WebDavBackend<'t, T: Transport> {
    transport: &'t mut T,
    url: String,
    authorization: Zeroizing<String>,
}

impl<'t, T: Transport> WebDavBackend<'t, T> {
    /// `url` is the file itself. The password only ever leaves in the
    /// `Authorization` header, over the connection the URL names.
    pub fn new(transport: &'t mut T, url: &str, username: &str, password: &str) -> Self {
        let credentials = Zeroizing::new(format!("{username}:{password}"));
        Self {
            transport,
            url: url.to_string(),
            authorization: Zeroizing::new(format!("Basic {}", base64(credentials.as_bytes()))),
        }
    }

    fn send(
        &mut self,
        method: &'static str,
        extra: &[(&str, &str)],
        body: Vec<u8>,
    ) -> SyncResult<Response> {
        let mut headers = BTreeMap::new();
        headers.insert("Authorization".into(), self.authorization.to_string());
        headers.insert("Accept-Encoding".into(), "identity".into());
        for (name, value) in extra {
            headers.insert((*name).into(), (*value).into());
        }
        let response = self
            .transport
            .send(Request {
                method,
                url: self.url.clone(),
                headers,
                body,
            })
            .map_err(SyncError::Transport)?;
        match response.status {
            200..=299 | 412 => Ok(response),
            401 | 403 => Err(SyncError::Unauthorized(format!(
                "the WebDAV server refused these credentials ({})",
                response.status
            ))),
            404 | 410 => Err(SyncError::NotFound),
            429 => Err(SyncError::RateLimited {
                retry_after_secs: None,
            }),
            500..=599 => Err(SyncError::Transport(format!(
                "WebDAV server error {}",
                response.status
            ))),
            other => Err(SyncError::Other(format!(
                "unexpected WebDAV status {other}"
            ))),
        }
    }

    /// Reads the file, its state, and the ETag exactly as the server sent it.
    fn get(&mut self) -> SyncResult<(Vec<u8>, RemoteState, String)> {
        let response = self.send("GET", &[], Vec::new())?;
        let state = state_of(&response, response.body.len() as u64);
        let raw = response.headers.get("etag").cloned().unwrap_or_default();
        Ok((response.body, state, raw))
    }
}

/// How long Apache keeps a just-written file's ETag weak, plus a margin.
const WEAK_ETAG_SETTLE: std::time::Duration = std::time::Duration::from_millis(1100);

/// The strong ETag a conditional write can rely on, or why there is none.
fn usable_etag(raw: &str) -> SyncResult<String> {
    let tag = raw;
    if tag.is_empty() {
        return Err(SyncError::Other(
            "this WebDAV server sends no ETag, so Vardo cannot write without risking another device's edit".into(),
        ));
    }
    if tag.starts_with("W/") {
        return Err(SyncError::Other(
            "this WebDAV server sends only weak ETags, which a conditional write cannot use".into(),
        ));
    }
    Ok(tag.to_string())
}

fn state_of(response: &Response, size: u64) -> RemoteState {
    let header = |name: &str| response.headers.get(name).cloned();
    RemoteState {
        // Without the weak marker, so a file whose tag turns from weak to
        // strong as it ages does not look changed.
        version: header("etag")
            .map(|t| t.trim_start_matches("W/").to_string())
            .unwrap_or_default(),
        size: header("content-length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(size),
        modified_unix: header("last-modified").as_deref().and_then(http_date),
        content_hash: None,
    }
}

impl<T: Transport> StorageBackend for WebDavBackend<'_, T> {
    fn stat(&mut self) -> SyncResult<RemoteState> {
        let response = self.send("HEAD", &[], Vec::new())?;
        Ok(state_of(&response, 0))
    }

    fn read(&mut self) -> SyncResult<(Vec<u8>, RemoteState)> {
        let (bytes, state, _) = self.get()?;
        Ok((bytes, state))
    }

    /// A plain write, for creating the file. Anything that could overwrite
    /// another device's edit goes through [`Self::write_if_unchanged`].
    fn write(&mut self, bytes: &[u8]) -> SyncResult<RemoteState> {
        self.send("PUT", &[], bytes.to_vec())?;
        self.stat()
    }

    fn write_if_unchanged(&mut self, bytes: &[u8], expected: &[u8]) -> SyncResult<WriteOutcome> {
        let (mut current, _, mut raw) = self.get()?;
        if raw.starts_with("W/") {
            std::thread::sleep(WEAK_ETAG_SETTLE);
            (current, _, raw) = self.get()?;
        }
        if current != expected {
            return Ok(WriteOutcome::Superseded { buried: current });
        }
        let tag = usable_etag(&raw)?;
        let response = self.send("PUT", &[("If-Match", &tag)], bytes.to_vec())?;
        if response.status != 412 {
            return Ok(WriteOutcome::Written(self.stat()?));
        }
        // Refused. Either somebody wrote between our read and our write,
        // which is what the refusal is for, or the server's ETags lie.
        let (now, _, _) = self.get()?;
        if now == expected {
            return Err(SyncError::Other(
                "the WebDAV server refused a write although the file had not changed, so its ETags \
                 cannot be trusted (Apache mod_deflate does this unless DeflateAlterETag is NoChange). \
                 Nothing was written."
                    .into(),
            ));
        }
        Ok(WriteOutcome::Superseded { buried: now })
    }

    /// WebDAV keeps no history. [`Self::write_if_unchanged`] never needs it.
    fn revisions(&mut self) -> SyncResult<Vec<Revision>> {
        Ok(Vec::new())
    }

    fn read_revision(&mut self, _id: &str) -> SyncResult<Vec<u8>> {
        Err(SyncError::NotFound)
    }

    fn describe(&self) -> String {
        format!("WebDAV: {}", self.url)
    }
}

/// Standard base64, for the Basic `Authorization` header.
pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Seconds since 1970 from an HTTP date such as `Sun, 06 Nov 1994 08:49:37 GMT`.
pub fn http_date(text: &str) -> Option<i64> {
    let mut parts = text.split_whitespace().skip(1);
    let day: i64 = parts.next()?.parse().ok()?;
    let name = parts.next()?;
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| *m == name)? as i64
        + 1;
    let year: i64 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':').map(|p| p.parse::<i64>().ok());
    let (h, m, s) = (clock.next()??, clock.next()??, clock.next()??);
    if parts.next()? != "GMT" {
        return None;
    }
    // Days from the civil calendar date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3_600 + m * 60 + s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_vectors() {
        // RFC 4648 section 10.
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded);
        }
    }

    #[test]
    fn http_dates_read_as_unix_seconds() {
        assert_eq!(
            http_date("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(784_111_777)
        );
        assert_eq!(http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(
            http_date("Tue, 29 Feb 2028 12:00:00 GMT"),
            Some(1_835_438_400)
        );
        assert_eq!(http_date("yesterday"), None);
    }
}
