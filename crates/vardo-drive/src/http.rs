//! A minimal HTTP seam.
//!
//! The Drive backend talks through this rather than calling a client library
//! directly, so its logic — retry classification, upload framing, revision
//! handling, token refresh — is testable without a network or an account.
//!
//! That matters more than usual here. Nobody can run the real integration
//! suite without Google credentials, so if the only way to exercise this code
//! were against live Drive, it would effectively be untested.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Patch,
    Delete,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: BTreeMap::new(),
            body: Vec::new(),
        }
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.insert(name.to_string(), value.to_string());
        self
    }

    pub fn bearer(self, token: &str) -> Self {
        self.header("Authorization", &format!("Bearer {token}"))
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    pub fn form(self, pairs: &[(&str, &str)]) -> Self {
        let encoded = pairs
            .iter()
            .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
            .collect::<Vec<_>>()
            .join("&");
        self.header("Content-Type", "application/x-www-form-urlencoded")
            .body(encoded)
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// `Retry-After`, when the server sends one.
    pub fn retry_after(&self) -> Option<u64> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
            .and_then(|(_, v)| v.trim().parse().ok())
    }
}

pub trait HttpTransport {
    fn send(&mut self, request: Request) -> Result<Response, String>;
}

/// Percent-encoding for the unreserved set in RFC 3986.
///
/// Hand-rolled because this is the only place the project needs it and a URL
/// crate would be a dependency carrying a parser we do not want.
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// URL-safe base64 without padding, as required for PKCE.
pub fn base64url(bytes: &[u8]) -> String {
    vardo_kdbx::base64::encode(bytes)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_matches_rfc3986_unreserved() {
        assert_eq!(urlencode("abcXYZ019-_.~"), "abcXYZ019-_.~");
        assert_eq!(urlencode(" "), "%20");
        assert_eq!(urlencode("a+b"), "a%2Bb");
        assert_eq!(urlencode("a/b"), "a%2Fb");
        assert_eq!(urlencode("ä"), "%C3%A4");
        // The scopes and redirect URIs we send contain all of these.
        assert_eq!(
            urlencode("https://www.googleapis.com/auth/drive.file"),
            "https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fdrive.file"
        );
    }

    #[test]
    fn base64url_is_unpadded_and_url_safe() {
        // PKCE rejects '+', '/' and '='.
        let bytes: Vec<u8> = (0u8..64).collect();
        let encoded = base64url(&bytes);
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
        assert!(!encoded.contains('='));

        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
    }

    #[test]
    fn retry_after_is_parsed_case_insensitively() {
        let mut headers = BTreeMap::new();
        headers.insert("Retry-After".to_string(), "30".to_string());
        let r = Response {
            status: 429,
            headers,
            body: Vec::new(),
        };
        assert_eq!(r.retry_after(), Some(30));
    }

    #[test]
    fn form_encoding_escapes_values() {
        let r = Request::new(Method::Post, "https://example.com")
            .form(&[("code", "a/b+c"), ("grant_type", "authorization_code")]);
        assert_eq!(
            String::from_utf8(r.body).unwrap(),
            "code=a%2Fb%2Bc&grant_type=authorization_code"
        );
    }
}
