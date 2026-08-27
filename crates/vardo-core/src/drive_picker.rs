//! Choosing a vault that already exists in someone's Drive.
//!
//! This exists because of a constraint that is easy to state and awkward to
//! live with: under the `drive.file` scope, Drive will not tell us a file
//! exists until the user has handed it over. Someone who already keeps
//! `passwords.kdbx` in Drive opens Vardo, sees an empty list, and reasonably
//! concludes the feature is broken.
//!
//! The intended answer is Google's own picker. A file selected there is granted
//! to this app permanently, one file at a time, without ever widening the
//! scope. The alternative would be asking for read access to the user's entire
//! Drive so we could populate a list, which for a password manager is not a
//! reasonable thing to ask.
//!
//! # Why this opens a browser rather than a window
//!
//! The picker is a JavaScript API that needs a real web origin, so it is served
//! from the loopback listener and opened in the user's own browser. That is
//! also the honest place for it: the picker shows the contents of someone's
//! Drive and is the moment they decide what this program may see, and putting
//! that inside an app's own frameless window is how phishing looks.
//!
//! I refused a picker once already in this project, and that refusal still
//! stands: the *login* request had been quietly asking for one, which put a
//! chooser over the user's whole Drive during what was nominally a sign-in.
//! Deliberate and asked-for is a different thing entirely.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use crate::drive_auth::{self, DriveClient};
use crate::session::CoreError;
use crate::source::VaultSource;

/// Long enough to find a file in a large Drive, short enough that a forgotten
/// tab does not hold a listener open for the rest of the session.
const TIMEOUT: Duration = Duration::from_secs(300);

pub struct PendingPick {
    listener: TcpListener,
    /// The page to open. Handed back so the caller can open a browser and also
    /// show the URL, which keeps the flow recoverable if opening one fails.
    pub url: String,
    html: String,
}

/// Prepares the picker page and starts listening.
pub fn begin(client: &DriveClient, app_dir: &std::path::Path) -> Result<PendingPick, CoreError> {
    let tokens = drive_auth::access(client, app_dir)?;

    let listener = TcpListener::bind("127.0.0.1:0").map_err(io)?;
    let port = listener.local_addr().map_err(io)?.port();

    // `setAppId` wants the Cloud project number, and it is required for
    // `drive.file` - it is what tells Drive which app the grant is for. It is
    // the leading digits of the client id, so there is nothing extra to ship
    // and nothing to get out of step.
    let app_id = client
        .client_id
        .split('-')
        .next()
        .unwrap_or_default()
        .to_string();

    Ok(PendingPick {
        listener,
        url: format!("http://127.0.0.1:{port}/"),
        html: page(&tokens.access_token, &app_id),
    })
}

/// Serves the page, then blocks until the user picks a file or gives up.
///
/// `Ok(None)` means they closed the picker without choosing, which is a normal
/// outcome and not a failure to report.
pub fn wait(pending: PendingPick) -> Result<Option<VaultSource>, CoreError> {
    pending.listener.set_nonblocking(false).map_err(io)?;

    let deadline = Instant::now() + TIMEOUT;

    loop {
        if Instant::now() > deadline {
            return Err(CoreError::Io {
                detail: "The file picker timed out".into(),
            });
        }

        let (mut stream, _) = pending.listener.accept().map_err(io)?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(io)?;

        let mut reader = BufReader::new(&stream);
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            continue;
        }

        let path = request_line.split_whitespace().nth(1).unwrap_or("/");

        if let Some(query) = path.strip_prefix("/picked?") {
            let id = param(query, "id");
            let name = param(query, "name");
            let _ = respond(&mut stream, "text/html", DONE_PAGE);

            return Ok(match (id, name) {
                (Some(file_id), Some(name)) if !file_id.is_empty() => {
                    Some(VaultSource::GoogleDrive { file_id, name })
                }
                _ => None,
            });
        }

        if path.starts_with("/cancelled") {
            let _ = respond(&mut stream, "text/html", DONE_PAGE);
            return Ok(None);
        }

        // Anything else is the initial page load, or a favicon probe.
        let _ = respond(&mut stream, "text/html; charset=utf-8", &pending.html);
    }
}

fn respond(
    stream: &mut std::net::TcpStream,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

fn param(query: &str, key: &str) -> Option<String> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(&format!("{key}=")))
        .map(percent_decode)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.replace('+', " ").into_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The picker page.
///
/// The access token is inlined, which is safe only because this is served on
/// loopback to a page nobody else can reach, is fetched over a connection that
/// never leaves the machine, and the listener closes after one pick.
fn page(access_token: &str, app_id: &str) -> String {
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Choose your vault</title>
<style>
  body {{ background:#17212b; color:#f5f5f5; font:14px system-ui,sans-serif;
         display:flex; align-items:center; justify-content:center; height:100vh; margin:0 }}
  .box {{ text-align:center }}
  p {{ color:#7f91a4 }}
  button {{ background:#40a7e3; color:#fff; border:0; border-radius:6px;
            padding:10px 18px; font:inherit; font-weight:600; cursor:pointer }}
</style></head>
<body><div class="box">
  <h1>Choose your vault</h1>
  <p id="s">Opening Google Drive&hellip;</p>
  <button id="b" style="display:none">Choose a file</button>
</div>
<script src="https://apis.google.com/js/api.js"></script>
<script>
  const TOKEN = "{access_token}";
  const APP_ID = "{app_id}";

  function show(msg) {{ document.getElementById("s").textContent = msg; }}

  function open_picker() {{
    // DocsView rather than the default: a vault is an ordinary uploaded file,
    // and the default view leads with Google-native documents that can never
    // be one.
    const view = new google.picker.DocsView(google.picker.ViewId.DOCS)
      .setIncludeFolders(true)
      .setSelectFolderEnabled(false);

    new google.picker.PickerBuilder()
      .setAppId(APP_ID)
      .setOAuthToken(TOKEN)
      .addView(view)
      .setTitle("Choose your vault")
      .setCallback(function (data) {{
        const a = data[google.picker.Response.ACTION];
        if (a === google.picker.Action.PICKED) {{
          const doc = data[google.picker.Response.DOCUMENTS][0];
          show("Opening " + doc.name + "\u2026");
          location.href = "/picked?id=" + encodeURIComponent(doc.id) +
                          "&name=" + encodeURIComponent(doc.name);
        }} else if (a === google.picker.Action.CANCEL) {{
          location.href = "/cancelled";
        }}
      }})
      .build()
      .setVisible(true);
  }}

  gapi.load("picker", function () {{
    document.getElementById("b").style.display = "";
    show("");
    document.getElementById("b").onclick = open_picker;
    open_picker();
  }});
</script></body></html>"#
    )
}

const DONE_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\">\
<title>Done</title><style>body{background:#17212b;color:#f5f5f5;\
font:14px system-ui,sans-serif;display:flex;align-items:center;\
justify-content:center;height:100vh;margin:0}</style></head>\
<body><p>Done. You can close this tab.</p></body></html>";

fn io(e: std::io::Error) -> CoreError {
    CoreError::Io {
        detail: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_id_is_the_leading_digits_of_the_client_id() {
        // Required for drive.file, and deriving it means there is no second
        // value to ship and nothing to fall out of step with the client id.
        let client = DriveClient {
            client_id: "330996919366-abc123.apps.googleusercontent.com".into(),
            client_secret: String::new(),
        };
        let app_id = client.client_id.split('-').next().unwrap();
        assert_eq!(app_id, "330996919366");
    }

    #[test]
    fn a_picked_file_is_read_out_of_the_callback_query() {
        let query = "id=1AbC&name=my%20passwords.kdbx";
        assert_eq!(param(query, "id").as_deref(), Some("1AbC"));
        assert_eq!(param(query, "name").as_deref(), Some("my passwords.kdbx"));
    }

    #[test]
    fn a_name_with_a_plus_or_an_ampersand_survives() {
        // Both are legal in a filename and both break a naive query split.
        let query = "id=1&name=work%2Bhome%26more.kdbx";
        assert_eq!(param(query, "name").as_deref(), Some("work+home&more.kdbx"));
    }

    #[test]
    fn the_access_token_never_appears_in_the_done_page() {
        // The page left in the browser after a pick outlives the listener, so
        // it must not carry anything worth keeping.
        assert!(!DONE_PAGE.contains("TOKEN"));
        assert!(!DONE_PAGE.contains("ya29"));
    }
}
