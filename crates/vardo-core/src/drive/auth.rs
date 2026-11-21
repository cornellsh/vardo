//! Connecting to Google Drive, and remembering the connection.
//!
//! Contract: OAuth runs with PKCE and a loopback redirect, in the system
//! browser. The refresh token is kept by [`super::tokens`].
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
//! Only the refresh token, by [`super::tokens`]. See there for where.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::time::{Duration, Instant};

use vardo_drive::oauth::{self, ClientConfig, Pkce, Tokens};
use vardo_drive::ureq_transport::UreqTransport;

use super::tokens;
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
///
/// `pick` adds Google's desktop picker to the same browser page, so signing in
/// and choosing a vault is one trip. Only for a click that asked to choose a
/// vault: a plain sign-in must not put a chooser over the user's Drive.
pub fn begin(client: &DriveClient, pick: bool) -> Result<PendingAuth, CoreError> {
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

    let url = oauth::authorization_url(&client.config(), &pkce, &redirect, &state, pick);

    Ok(PendingAuth {
        listener,
        pkce,
        redirect,
        state,
        url,
    })
}

/// Blocks until the browser comes back, then exchanges the code.
///
/// Returns the file the user picked, when [`begin`] asked for the picker.
pub fn complete(
    client: &DriveClient,
    pending: PendingAuth,
    app_dir: &Path,
) -> Result<Option<String>, CoreError> {
    let (code, picked) = wait_for_code(&pending.listener, &pending.state)?;
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
    super::tokens::store(app_dir, &tokens)?;
    Ok(picked)
}

/// Long enough to sign in and find a file in a large Drive, short enough that
/// a closed tab does not hold a port for the rest of the session.
const TIMEOUT: Duration = Duration::from_secs(600);

/// A fresh access token, refreshing if we have to.
pub fn access(client: &DriveClient, app_dir: &Path) -> Result<Tokens, CoreError> {
    let refresh = tokens::refresh_token(app_dir).ok_or(CoreError::DriveNotConnected)?;

    let mut http = UreqTransport::new();
    let mut fresh = oauth::refresh(&mut http, &client.config(), refresh.as_str()).map_err(|e| {
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
        fresh.refresh_token = Some(refresh.to_string());
    }
    tokens::store(app_dir, &fresh)?;
    Ok(fresh)
}

/// Waits for the redirect and extracts `code` and, after a pick, the file.
///
/// Requests that carry neither `state` nor `error` (a favicon probe) are
/// answered and skipped, so they cannot eat the one redirect we need.
fn wait_for_code(
    listener: &TcpListener,
    expected_state: &str,
) -> Result<(String, Option<String>), CoreError> {
    listener.set_nonblocking(true).map_err(io)?;
    let deadline = Instant::now() + TIMEOUT;
    let (stream, line) = loop {
        if Instant::now() > deadline {
            return Err(CoreError::Io {
                detail: "sign-in timed out; try again".into(),
            });
        }
        let stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(io(e)),
        };
        stream.set_nonblocking(false).map_err(io)?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(io)?;
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_err() {
            continue;
        }
        if line.contains("state=") || line.contains("error=") {
            break (stream, line);
        }
        let mut s = &stream;
        let _ = write!(
            s,
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
    };

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
    let mut picked = None;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("code", v)) => code = Some(percent_decode(v)),
            Some(("state", v)) => state = Some(percent_decode(v)),
            Some(("error", v)) => error = Some(percent_decode(v)),
            // Comma-separated; Vardo asks for one file, so the first is it.
            Some(("picked_file_ids", v)) => {
                picked = percent_decode(v)
                    .split(',')
                    .next()
                    .filter(|id| !id.is_empty())
                    .map(str::to_string)
            }
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
            Ok((c, picked))
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
    fn the_authorisation_url_carries_pkce_and_the_narrow_scope() {
        let client = DriveClient {
            client_id: "cid".into(),
            client_secret: "secret".into(),
        };
        let pending = begin(&client, false).expect("begin");

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

    #[test]
    fn choosing_a_vault_signs_in_and_returns_the_picked_file_in_one_trip() {
        let client = DriveClient {
            client_id: "cid".into(),
            client_secret: "secret".into(),
        };
        let pending = begin(&client, true).expect("begin");
        assert!(pending.url.contains("trigger_onepick=true"));
        assert!(pending.url.contains("prompt=consent"));

        let port = pending.redirect.rsplit(':').next().unwrap().to_string();
        let state = pending.state.clone();
        let browser = std::thread::spawn(move || {
            use std::io::Read;
            let get = |path: &str| {
                let mut s = std::net::TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
                write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
                let mut out = String::new();
                let _ = s.read_to_string(&mut out);
                out
            };
            // A favicon probe first must not consume the redirect.
            get("/favicon.ico");
            get(&format!(
                "/?state={state}&picked_file_ids=1AbC%2C2DeF&code=c0de&scope=x"
            ))
        });
        let got = wait_for_code(&pending.listener, &pending.state).expect("redirect");
        assert_eq!(got, ("c0de".to_string(), Some("1AbC".to_string())));
        assert!(browser.join().unwrap().contains("Connected."));
    }
}
