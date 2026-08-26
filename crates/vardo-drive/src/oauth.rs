//! OAuth 2.0 for an installed application.
//!
//! # Scope: `drive.file` and nothing else
//!
//! `drive.file` is a **non-sensitive** scope, so Google's own policy says an
//! app using only non-sensitive scopes "is not mandatory to complete the app
//! verification process". No review, no security assessment, no 100-user cap,
//! no annual re-verification. Requesting `drive` instead would make this a
//! *restricted* scope, triggering verification plus a paid third-party
//! security assessment — for a feature we do not need.
//!
//! The cost is that `drive.file` only grants access to files the app created
//! or the user explicitly picked, which is exactly why the Picker flow below
//! exists.
//!
//! # The Picker must not be embedded
//!
//! Google ships two Picker flows, and the difference is easy to get wrong. The
//! JavaScript one is for web pages and **cannot** work inside a desktop
//! WebView: the frame's origin is `file://` or `http://localhost`, `postMessage`
//! fails the origin check, and the request 403s — often only once the app is
//! packaged, having worked in development.
//!
//! The desktop flow is different. Adding `trigger_onepick=true` to the
//! *authorization request* makes Google show the picker in the system browser
//! and return the selection alongside the auth code. It also refuses to be
//! combined with any scope other than `drive.file`, which conveniently forces
//! the safe choice.
//!
//! # Loopback redirect and the "secret" that is not one
//!
//! Installed apps use a loopback redirect, still supported for the `Desktop
//! app` client type (deprecated only for iOS/Android/Chrome clients). Google
//! issues a client secret for desktop clients and the token endpoint requires
//! it even with PKCE — so it ships inside the binary. Google's own
//! documentation says the quiet part: "in this context, the client secret is
//! obviously not treated as a secret". PKCE is what actually protects the
//! exchange.

use sha2::{Digest, Sha256};

use crate::http::{base64url, urlencode, HttpTransport, Method, Request};

pub const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub client_id: String,
    /// Present because Google requires it for desktop clients; not secret.
    pub client_secret: String,
}

/// A PKCE verifier/challenge pair.
///
/// The verifier never leaves the process until the token exchange, so an
/// attacker who intercepts the authorization code on the loopback interface
/// cannot redeem it. That interception is a real concern: any local process
/// can race to bind the callback port, which is why Google deprecated this
/// flow for mobile clients.
#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    /// Builds a pair from caller-supplied entropy.
    ///
    /// Entropy is injected rather than generated here so tests are
    /// deterministic and the caller owns the RNG choice.
    pub fn from_entropy(entropy: &[u8]) -> Self {
        // RFC 7636 requires 43-128 characters from the unreserved set;
        // base64url of 32 bytes gives 43.
        let verifier = base64url(entropy);
        let challenge = base64url(&Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
        }
    }
}

/// The URL to open in the user's browser.
///
/// `trigger_onepick=true` asks Google to run the desktop file picker as part
/// of consent, so the user grants access to one specific file in the same step.
pub fn authorization_url(
    config: &ClientConfig,
    pkce: &Pkce,
    redirect_uri: &str,
    state: &str,
    pick_file: bool,
) -> String {
    let mut params = vec![
        ("client_id", config.client_id.clone()),
        ("redirect_uri", redirect_uri.to_string()),
        ("response_type", "code".to_string()),
        ("scope", SCOPE.to_string()),
        ("code_challenge", pkce.challenge.clone()),
        ("code_challenge_method", "S256".to_string()),
        ("state", state.to_string()),
        // Without this Google returns no refresh token, and the user would
        // have to re-authorise every hour.
        ("access_type", "offline".to_string()),
    ];
    if pick_file {
        params.push(("trigger_onepick", "true".to_string()));
    }

    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{AUTH_ENDPOINT}?{query}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access_token: String,
    /// Absent when Google declines to reissue one on refresh, which is normal.
    pub refresh_token: Option<String>,
    /// Seconds from issue.
    pub expires_in: Option<u64>,
}

#[derive(Debug)]
pub enum OauthError {
    Transport(String),
    /// The provider rejected the request, with its own error code.
    Rejected {
        status: u16,
        error: String,
    },
    Malformed(String),
}

impl std::fmt::Display for OauthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(d) => write!(f, "transport failure: {d}"),
            Self::Rejected { status, error } => {
                write!(f, "authorisation rejected ({status}): {error}")
            }
            Self::Malformed(d) => write!(f, "malformed token response: {d}"),
        }
    }
}

impl std::error::Error for OauthError {}

/// Exchanges an authorization code for tokens.
pub fn exchange_code(
    http: &mut dyn HttpTransport,
    config: &ClientConfig,
    pkce: &Pkce,
    redirect_uri: &str,
    code: &str,
) -> Result<Tokens, OauthError> {
    let request = Request::new(Method::Post, TOKEN_ENDPOINT).form(&[
        ("client_id", &config.client_id),
        ("client_secret", &config.client_secret),
        ("code", code),
        ("code_verifier", &pkce.verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
    ]);
    parse_tokens(send(http, request)?)
}

pub fn refresh(
    http: &mut dyn HttpTransport,
    config: &ClientConfig,
    refresh_token: &str,
) -> Result<Tokens, OauthError> {
    let request = Request::new(Method::Post, TOKEN_ENDPOINT).form(&[
        ("client_id", &config.client_id),
        ("client_secret", &config.client_secret),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ]);
    let mut tokens = parse_tokens(send(http, request)?)?;
    // A refresh response usually omits the refresh token; keep the one we have
    // rather than losing offline access.
    if tokens.refresh_token.is_none() {
        tokens.refresh_token = Some(refresh_token.to_string());
    }
    Ok(tokens)
}

fn send(
    http: &mut dyn HttpTransport,
    request: Request,
) -> Result<crate::http::Response, OauthError> {
    let response = http.send(request).map_err(OauthError::Transport)?;
    if !response.is_success() {
        let body = response.text();
        let error = json_string(&body, "error_description")
            .or_else(|| json_string(&body, "error"))
            .unwrap_or(body);
        return Err(OauthError::Rejected {
            status: response.status,
            error,
        });
    }
    Ok(response)
}

fn parse_tokens(response: crate::http::Response) -> Result<Tokens, OauthError> {
    let body = response.text();
    let access_token = json_string(&body, "access_token")
        .ok_or_else(|| OauthError::Malformed("no access_token".into()))?;
    Ok(Tokens {
        access_token,
        refresh_token: json_string(&body, "refresh_token"),
        expires_in: json_number(&body, "expires_in"),
    })
}

/// Extracts a string field from a flat JSON object.
///
/// Deliberately not a full JSON parser: these responses are flat and
/// well-known, and a parser is attack surface plus a dependency. Anything
/// nested belongs in a real parser, and there is nothing nested here.
pub fn json_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = json.find(&needle)? + needle.len();
    let rest = json[start..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;

    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let cp = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(cp)?);
                }
                other => out.push(other),
            },
            other => out.push(other),
        }
    }
    None
}

pub fn json_number(json: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\"");
    let start = json.find(&needle)? + needle.len();
    let rest = json[start..].trim_start().strip_prefix(':')?.trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ClientConfig {
        ClientConfig {
            client_id: "123.apps.googleusercontent.com".into(),
            client_secret: "GOCSPX-not-really-secret".into(),
        }
    }

    #[test]
    fn pkce_challenge_matches_rfc7636_test_vector() {
        // RFC 7636 appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = base64url(&Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn pkce_verifier_is_a_legal_length() {
        let pkce = Pkce::from_entropy(&[7u8; 32]);
        assert!((43..=128).contains(&pkce.verifier.len()));
        assert!(pkce
            .verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c)));
    }

    #[test]
    fn authorization_url_requests_only_the_non_sensitive_scope() {
        // Asking for `drive` instead would trigger Google verification plus a
        // paid third-party security assessment.
        let url = authorization_url(
            &config(),
            &Pkce::from_entropy(&[1u8; 32]),
            "http://127.0.0.1:1410",
            "state123",
            true,
        );
        assert!(url.contains("drive.file"));
        assert!(
            !url.contains("auth%2Fdrive&") && !url.ends_with("auth%2Fdrive"),
            "must not request the broad drive scope: {url}"
        );
    }

    #[test]
    fn authorization_url_uses_pkce_and_offline_access() {
        let url = authorization_url(
            &config(),
            &Pkce::from_entropy(&[1u8; 32]),
            "http://127.0.0.1:1410",
            "state123",
            false,
        );
        assert!(url.contains("code_challenge_method=S256"));
        assert!(
            url.contains("access_type=offline"),
            "no refresh token without this"
        );
        assert!(url.contains("state=state123"));
        assert!(url.starts_with(AUTH_ENDPOINT));
    }

    #[test]
    fn the_picker_is_opt_in_via_trigger_onepick() {
        let pkce = Pkce::from_entropy(&[1u8; 32]);
        let with = authorization_url(&config(), &pkce, "http://127.0.0.1:1410", "s", true);
        let without = authorization_url(&config(), &pkce, "http://127.0.0.1:1410", "s", false);
        assert!(with.contains("trigger_onepick=true"));
        assert!(!without.contains("trigger_onepick"));
    }

    #[test]
    fn json_string_reads_flat_fields() {
        let body = r#"{"access_token":"ya29.abc","expires_in":3599,"token_type":"Bearer"}"#;
        assert_eq!(json_string(body, "access_token").unwrap(), "ya29.abc");
        assert_eq!(json_number(body, "expires_in").unwrap(), 3599);
        assert_eq!(json_string(body, "missing"), None);
    }

    #[test]
    fn json_string_handles_escapes() {
        let body = r#"{"error_description":"Bad \"request\"\nline two"}"#;
        assert_eq!(
            json_string(body, "error_description").unwrap(),
            "Bad \"request\"\nline two"
        );
        let unicode = r#"{"name":"caf\u00e9"}"#;
        assert_eq!(json_string(unicode, "name").unwrap(), "café");
    }

    #[test]
    fn json_string_does_not_match_a_key_inside_a_value() {
        // A naive `find` on the bare key would match the value below.
        let body = r#"{"note":"the access_token is elsewhere","access_token":"real"}"#;
        assert_eq!(json_string(body, "access_token").unwrap(), "real");
    }
}
