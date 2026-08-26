//! The loopback redirect that receives the authorization code.
//!
//! Still supported for the `Desktop app` client type; Google deprecated it
//! only for iOS, Android and Chrome clients.
//!
//! # Why the `state` check is not optional
//!
//! Any local process can connect to our listener and present a code. Comparing
//! `state` against the value we generated is what stops an attacker feeding us
//! *their* authorization code and silently binding the user's vault to an
//! account they control. PKCE protects the code we issued; `state` protects
//! against being handed someone else's.

/// What came back on the redirect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Callback {
    pub code: String,
    pub state: String,
    /// The file the user chose, when the request used `trigger_onepick`.
    pub file_id: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CallbackError {
    /// The provider reported a failure, most often `access_denied`.
    Provider(String),
    /// `state` did not match, so this callback is not ours.
    StateMismatch,
    Malformed(String),
}

impl std::fmt::Display for CallbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider(e) => write!(f, "authorisation failed: {e}"),
            Self::StateMismatch => write!(
                f,
                "the redirect carried a state we did not issue; ignoring it"
            ),
            Self::Malformed(d) => write!(f, "malformed redirect: {d}"),
        }
    }
}

/// Parses the query string of the redirect and verifies `state`.
pub fn parse_callback(query: &str, expected_state: &str) -> Result<Callback, CallbackError> {
    let params = parse_query(query);
    let get = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());

    if let Some(error) = get("error") {
        return Err(CallbackError::Provider(error));
    }

    let state = get("state").ok_or(CallbackError::Malformed("no state".into()))?;
    // Constant-time is unnecessary here: an attacker who can time our loopback
    // listener already has local code execution. Correctness is what matters.
    if state != expected_state {
        return Err(CallbackError::StateMismatch);
    }

    Ok(Callback {
        code: get("code").ok_or(CallbackError::Malformed("no code".into()))?,
        state,
        // The desktop picker returns the selection alongside the code.
        file_id: get("fileId").or_else(|| get("file_id")),
    })
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .trim_start_matches('?')
        .split('&')
        .filter(|s| !s.is_empty())
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((urldecode(k), urldecode(v)))
        })
        .collect()
}

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                Ok(byte) => {
                    out.push(byte);
                    i += 3;
                }
                Err(_) => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The page shown in the browser once the code has been received.
pub fn success_page() -> &'static str {
    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n\
     <!doctype html><meta charset=utf-8><title>Vardo</title>\
     <body style=\"font:16px system-ui;padding:3rem;max-width:32rem\">\
     <h1>Connected</h1><p>You can close this tab and return to Vardo.</p>"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matching_callback_is_accepted() {
        let c = parse_callback("?code=abc123&state=xyz", "xyz").unwrap();
        assert_eq!(c.code, "abc123");
        assert_eq!(c.file_id, None);
    }

    #[test]
    fn the_picked_file_is_carried_back() {
        let c = parse_callback("?code=abc&state=xyz&fileId=1a2b3c", "xyz").unwrap();
        assert_eq!(c.file_id.as_deref(), Some("1a2b3c"));
    }

    #[test]
    fn a_foreign_state_is_rejected() {
        // Any local process can hit our listener. Without this check an
        // attacker could hand us their authorization code and bind the user's
        // vault to an account they control.
        assert_eq!(
            parse_callback("?code=attacker&state=theirs", "ours"),
            Err(CallbackError::StateMismatch)
        );
    }

    #[test]
    fn a_missing_state_is_rejected() {
        assert!(matches!(
            parse_callback("?code=abc", "ours"),
            Err(CallbackError::Malformed(_))
        ));
    }

    #[test]
    fn provider_errors_are_surfaced() {
        assert_eq!(
            parse_callback("?error=access_denied&state=ours", "ours"),
            Err(CallbackError::Provider("access_denied".into()))
        );
    }

    #[test]
    fn percent_and_plus_encoding_are_decoded() {
        let c = parse_callback("?code=a%2Fb%2Bc&state=x+y", "x y").unwrap();
        assert_eq!(c.code, "a/b+c");
    }

    #[test]
    fn a_truncated_escape_does_not_panic() {
        let _ = parse_callback("?code=%2&state=ours", "ours");
        let _ = parse_callback("?code=%&state=ours", "ours");
        let _ = parse_callback("?=&&&=", "ours");
    }
}
