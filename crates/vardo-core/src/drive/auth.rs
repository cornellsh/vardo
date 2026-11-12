//! Connecting to Google Drive, and remembering the connection.
//!
//! Contract: OAuth runs with PKCE and a loopback redirect. Tokens live in a
//! file written 0600 under the app directory
//! (`fact:drive-refresh-token-persists`).
//!
//! # The flow
//!
//! Installed-app OAuth with PKCE and a loopback redirect (RFC 8252). We start a
//! server on an ephemeral localhost port, hand the user a URL to open in their
//! **system browser**, and catch the redirect. The browser is deliberately not
//! the app's own WebView: an embedded browser for a third-party login is
//! indistinguishable from a phishing page, and the user has no address bar to
//! check.
//!
//! # What is stored
//!
//! The refresh token, which is a long-lived credential for the `drive.file`
//! scope. It is written with `0600` permissions to the app config directory.
//!
//! That is **not** as good as the OS keyring, and this module says so rather
//! than implying otherwise. A file readable by the user's own account is
//! readable by anything running as that user. The mitigation that actually
//! bounds the damage is the scope: `drive.file` grants access only to files
//! this app created or the user explicitly picked, so a stolen token cannot
//! enumerate or read the rest of the account. Moving to the Secret Service,
//! Keychain and DPAPI is `plan:os-keyring`.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use vardo_drive::oauth::{self, ClientConfig, Pkce, Tokens};
use vardo_drive::ureq_transport::UreqTransport;

use crate::CoreError;

/// The OAuth client this build ships with.
///
/// Per RFC 8252 section 8.5 an installed app's "secret" is not confidential -
/// it is inside every copy of the binary - which is exactly why PKCE exists
/// and why the loopback redirect is bound to a port we own.
#[derive(Clone)]
pub struct DriveClient {
    pub client_id: String,
    pub client_secret: String,
}

impl DriveClient {
    /// The OAuth client this build ships with, from `VARDO_CLIENT_ID` and
    /// `VARDO_CLIENT_SECRET` at compile time.
    ///
    /// `None` in a source build without them. That is why the app reports
    /// "configured" apart from "connected": "you have not signed in" and
    /// "this build has no client id" have different answers.
    pub fn built_in() -> Option<Self> {
        let id = option_env!("VARDO_CLIENT_ID").filter(|id| !id.is_empty())?;
        Some(DriveClient {
            client_id: id.to_string(),
            client_secret: option_env!("VARDO_CLIENT_SECRET").unwrap_or("").to_string(),
        })
    }

    pub fn config(&self) -> ClientConfig {
        ClientConfig {
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
        }
    }
}

fn token_path(app_dir: &Path) -> PathBuf {
    app_dir.join("drive-tokens")
}

pub fn stored_tokens(app_dir: &Path) -> Option<Tokens> {
    let text = std::fs::read_to_string(token_path(app_dir)).ok()?;
    let mut lines = text.lines();
    let access = lines.next()?.to_string();
    let refresh = lines.next().unwrap_or("").to_string();
    if refresh.is_empty() {
        return None;
    }
    Some(Tokens {
        access_token: access,
        refresh_token: Some(refresh),
        expires_in: None,
    })
}

pub fn is_connected(app_dir: &Path) -> bool {
    stored_tokens(app_dir).is_some()
}

pub fn store_tokens(app_dir: &Path, tokens: &Tokens) -> Result<(), CoreError> {
    let path = token_path(app_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    let text = format!(
        "{}\n{}\n",
        tokens.access_token,
        tokens.refresh_token.clone().unwrap_or_default()
    );
    std::fs::write(&path, text).map_err(io)?;
    restrict(&path);
    Ok(())
}

/// Owner-only. Best effort: a filesystem without permissions is not a reason to
/// refuse to store the token, but it is a reason not to pretend it is
/// protected.
fn restrict(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

pub fn disconnect(app_dir: &Path) -> Result<(), CoreError> {
    match std::fs::remove_file(token_path(app_dir)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io(e)),
    }
}

/// A connection attempt, in two halves so the caller can show the URL.
pub struct PendingAuth {
    listener: TcpListener,
    pkce: Pkce,
    redirect: String,
    state: String,
    /// The URL to open in the system browser.
    pub url: String,
}

/// Starts the loopback server and builds the authorisation URL.
pub fn begin(client: &DriveClient) -> Result<PendingAuth, CoreError> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(io)?;
    let port = listener.local_addr().map_err(io)?.port();
    let redirect = format!("http://127.0.0.1:{port}");

    let mut entropy = [0u8; 32];
    getrandom::fill(&mut entropy).map_err(|e| CoreError::Io {
        detail: format!("system RNG unavailable: {e}"),
    })?;
    let pkce = Pkce::from_entropy(&entropy);

    let mut state_bytes = [0u8; 16];
    getrandom::fill(&mut state_bytes).map_err(|e| CoreError::Io {
        detail: format!("system RNG unavailable: {e}"),
    })?;
    let state = state_bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();

    // `false` for the picker: this call establishes the grant, and a separate
    // step chooses the file. Requesting a picker here would put a chooser over
    // the user's whole Drive during what is nominally a login.
    let url = oauth::authorization_url(&client.config(), &pkce, &redirect, &state, false);

    Ok(PendingAuth {
        listener,
        pkce,
        redirect,
        state,
        url,
    })
}

/// Blocks until the browser comes back, then exchanges the code.
pub fn complete(
    client: &DriveClient,
    pending: PendingAuth,
    app_dir: &Path,
) -> Result<(), CoreError> {
    let code = wait_for_code(&pending.listener, &pending.state)?;
    let mut http = UreqTransport::new();
    let tokens = oauth::exchange_code(
        &mut http,
        &client.config(),
        &pending.pkce,
        &pending.redirect,
        &code,
    )
    .map_err(|e| CoreError::Io {
        detail: format!("could not complete sign-in: {e}"),
    })?;
    store_tokens(app_dir, &tokens)
}

/// A fresh access token, refreshing if we have to.
pub fn access(client: &DriveClient, app_dir: &Path) -> Result<Tokens, CoreError> {
    let stored = stored_tokens(app_dir).ok_or(CoreError::DriveNotConnected)?;
    let refresh = stored
        .refresh_token
        .clone()
        .ok_or(CoreError::DriveNotConnected)?;

    let mut http = UreqTransport::new();
    let mut fresh = oauth::refresh(&mut http, &client.config(), &refresh).map_err(|e| {
        // A revoked or expired grant is not a transport failure, and telling
        // the user to check their connection would send them somewhere
        // useless. Reconnecting is the only fix.
        CoreError::DriveAuthExpired {
            detail: e.to_string(),
        }
    })?;

    // Google omits the refresh token on a refresh; keep the one we have or the
    // next launch has to ask again.
    if fresh.refresh_token.is_none() {
        fresh.refresh_token = Some(refresh);
    }
    store_tokens(app_dir, &fresh)?;
    Ok(fresh)
}

/// Reads the single redirect request and extracts `?code=`.
fn wait_for_code(listener: &TcpListener, expected_state: &str) -> Result<String, CoreError> {
    let (stream, _) = listener.accept().map_err(io)?;
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(io)?;

    let respond = |body: &str| {
        let mut s = &stream;
        let _ = write!(
            s,
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
    };

    let target = line.split_whitespace().nth(1).unwrap_or("");
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");

    let mut code = None;
    let mut state = None;
    let mut error = None;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("code", v)) => code = Some(percent_decode(v)),
            Some(("state", v)) => state = Some(percent_decode(v)),
            Some(("error", v)) => error = Some(percent_decode(v)),
            _ => {}
        }
    }

    if let Some(e) = error {
        respond(&page("Sign-in was cancelled.", "You can close this tab."));
        return Err(CoreError::Io {
            detail: format!("authorisation refused: {e}"),
        });
    }

    // The state check is what stops another local process from feeding us a
    // code for an account we did not ask about.
    if state.as_deref() != Some(expected_state) {
        respond(&page(
            "Something went wrong.",
            "The response did not match this request.",
        ));
        return Err(CoreError::Io {
            detail: "the sign-in response did not match this request".into(),
        });
    }

    match code {
        Some(c) => {
            respond(&page(
                "Connected.",
                "You can close this tab and return to Vardo.",
            ));
            Ok(c)
        }
        None => {
            respond(&page(
                "Something went wrong.",
                "No authorisation code was returned.",
            ));
            Err(CoreError::Io {
                detail: "no authorisation code in the redirect".into(),
            })
        }
    }
}

/// The page the browser lands on. Styled to match the app, because a raw white
/// page saying "OK" reads like the flow broke.
fn page(title: &str, detail: &str) -> String {
    format!(
        "<!doctype html><meta charset=utf-8><title>Vardo</title>\
         <body style=\"margin:0;display:grid;place-items:center;height:100vh;\
         background:#17212b;color:#f5f5f5;font:15px/1.5 'Open Sans',\
         'Segoe UI',system-ui,sans-serif\">\
         <div style=\"text-align:center\">\
         <div style=\"font-size:17px;font-weight:600\">{title}</div>\
         <div style=\"color:#8496a8;margin-top:6px\">{detail}</div>\
         </div></body>"
    )
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(
                    std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                    16,
                ) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn io(e: std::io::Error) -> CoreError {
    CoreError::Io {
        detail: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_token_file_reads_as_not_connected() {
        let dir = std::env::temp_dir().join("vardo-drive-auth-absent");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!is_connected(&dir));
        assert!(stored_tokens(&dir).is_none());
    }

    #[test]
    fn a_token_without_a_refresh_half_is_not_a_connection() {
        // An access token alone expires within the hour, so treating it as a
        // connection would show "connected" and then fail on the next sync.
        let dir = std::env::temp_dir().join("vardo-drive-auth-partial");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(token_path(&dir), "access-only\n").unwrap();
        assert!(!is_connected(&dir));
    }

    #[test]
    fn tokens_round_trip_and_are_owner_only() {
        let dir = std::env::temp_dir().join("vardo-drive-auth-rt");
        let _ = std::fs::remove_dir_all(&dir);
        let tokens = Tokens {
            access_token: "at".into(),
            refresh_token: Some("rt".into()),
            expires_in: None,
        };
        store_tokens(&dir, &tokens).unwrap();

        let loaded = stored_tokens(&dir).expect("stored");
        assert_eq!(loaded.access_token, "at");
        assert_eq!(loaded.refresh_token.as_deref(), Some("rt"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(token_path(&dir))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "the refresh token must not be world-readable"
            );
        }
    }

    #[test]
    fn disconnecting_removes_the_token_and_is_idempotent() {
        let dir = std::env::temp_dir().join("vardo-drive-auth-disc");
        let _ = std::fs::remove_dir_all(&dir);
        store_tokens(
            &dir,
            &Tokens {
                access_token: "at".into(),
                refresh_token: Some("rt".into()),
                expires_in: None,
            },
        )
        .unwrap();
        assert!(is_connected(&dir));

        disconnect(&dir).unwrap();
        assert!(!is_connected(&dir));
        // Disconnecting twice must not error; the user pressed the button.
        disconnect(&dir).unwrap();
    }

    #[test]
    fn the_authorisation_url_carries_pkce_and_the_narrow_scope() {
        let client = DriveClient {
            client_id: "cid".into(),
            client_secret: "secret".into(),
        };
        let pending = begin(&client).expect("begin");

        assert!(pending.url.contains("code_challenge="));
        assert!(pending.url.contains("code_challenge_method=S256"));
        assert!(
            pending.url.contains("drive.file"),
            "the scope must stay narrow"
        );
        assert!(
            !pending.url.contains("trigger_onepick"),
            "a login must not open a picker over the user's whole Drive"
        );
        assert!(pending.url.contains(&pending.state));
        assert!(pending.redirect.starts_with("http://127.0.0.1:"));
    }
}
