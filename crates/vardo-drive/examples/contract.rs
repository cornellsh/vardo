//! Runs the live Drive contract tests from `tests/contract_notes.md`.
//!
//! ```sh
//! cargo run -p vardo-drive --features network --example contract -- \
//!     --client-id <id> --client-secret <secret>
//! ```
//!
//! # Why this is safe to run against a real account
//!
//! These tests are destructive: they overwrite a file repeatedly and delete it
//! at the end. Three things make that safe, and none of them is a promise —
//! they are properties of the code:
//!
//! **1. The scope is `drive.file`.** That grants access only to files this
//! application creates, or that the user explicitly hands over with the file
//! picker. Every other file in the account is invisible to us: not readable,
//! not writable, not listable.
//!
//! **2. This harness never opens the picker.** It calls `files.create` and
//! works only on the file it just made. So the one mechanism that could hand
//! us somebody's real vault is never invoked, and its id is never learned.
//!
//! **3. Every destructive call is name-guarded.** Before any write or delete,
//! the file's name is re-read from Drive and must start with
//! [`TEST_PREFIX`]. If it does not, the harness aborts. This is belt-and-braces
//! against a bug in this file, not against Google.
//!
//! The account still sees one new file appear and disappear in its Drive root.
//! That is the entire blast radius.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

use vardo_drive::http::{HttpTransport, Method, Request};
use vardo_drive::oauth::{self, ClientConfig, Pkce, Tokens};
use vardo_drive::ureq_transport::UreqTransport;
use vardo_sync::StorageBackend;

/// Every file this harness touches must be named with this prefix.
const TEST_PREFIX: &str = "vardo-contract-test-";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let client_id = flag(&args, "--client-id").unwrap_or_else(|| {
        eprintln!("{}", USAGE);
        std::process::exit(2);
    });
    // Not a secret for an installed app — RFC 8252 §8.5 — but Google still
    // issues one and the token endpoint wants it.
    let client_secret = flag(&args, "--client-secret").unwrap_or_default();

    let mut http = UreqTransport::new();
    let config = ClientConfig {
        client_id,
        client_secret,
    };

    let tokens = match authorize(&mut http, &config) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("authorisation failed: {e}");
            std::process::exit(1);
        }
    };
    println!("\n✓ authorised\n");

    let file_id = match create_test_file(&mut http, &tokens) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("could not create the test file: {e}");
            std::process::exit(1);
        }
    };
    println!("✓ created test file {file_id}\n");

    let outcome = run_tests(&mut http, &config, &tokens, &file_id);

    // Always clean up, including after a failure.
    match delete_test_file(&mut http, &tokens, &file_id) {
        Ok(()) => println!("\n✓ test file deleted"),
        Err(e) => eprintln!("\n! could not delete the test file ({file_id}): {e}"),
    }

    if let Err(e) = outcome {
        eprintln!("\nFAILED: {e}");
        std::process::exit(1);
    }
}

const USAGE: &str = "\
usage: contract --client-id <id> [--client-secret <secret>]

Creates one file named vardo-contract-test-<timestamp> in the account's Drive
root, exercises it, and deletes it. Never opens the file picker, so no other
file in the account is reachable.";

fn flag(args: &[String], name: &str) -> Option<String> {
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1).cloned()
}

// --- authorisation ----------------------------------------------------------

fn authorize(http: &mut dyn HttpTransport, config: &ClientConfig) -> Result<Tokens, String> {
    // Loopback redirect, per RFC 8252: the browser comes back to a port we own
    // rather than to a custom scheme.
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}");

    let mut entropy = [0u8; 32];
    getrandom::fill(&mut entropy).map_err(|e| e.to_string())?;
    let pkce = Pkce::from_entropy(&entropy);

    let url = oauth::authorization_url(config, &redirect, &pkce, true);

    println!("Open this URL, sign in, and approve:\n\n{url}\n");
    println!("Waiting for the browser to come back on port {port}…");

    let code = wait_for_code(&listener)?;
    oauth::exchange_code(http, config, &redirect, &code, &pkce).map_err(|e| e.to_string())
}

/// Reads the single redirect request and extracts `?code=`.
fn wait_for_code(listener: &TcpListener) -> Result<String, String> {
    let (stream, _) = listener.accept().map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;

    let mut respond = |body: &str| {
        let mut stream = &stream;
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
    };

    // "GET /?code=... HTTP/1.1"
    let target = line.split_whitespace().nth(1).unwrap_or("");
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");

    let mut code = None;
    let mut error = None;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("code", v)) => code = Some(percent_decode(v)),
            Some(("error", v)) => error = Some(percent_decode(v)),
            _ => {}
        }
    }

    if let Some(e) = error {
        respond("<h1>Vardo</h1><p>Authorisation was refused. You can close this tab.</p>");
        return Err(format!("authorisation refused: {e}"));
    }

    match code {
        Some(c) => {
            respond("<h1>Vardo</h1><p>Authorised. You can close this tab.</p>");
            Ok(c)
        }
        None => {
            respond("<h1>Vardo</h1><p>No authorisation code received.</p>");
            Err("no code in redirect".into())
        }
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
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

// --- the guarded file -------------------------------------------------------

fn create_test_file(http: &mut dyn HttpTransport, tokens: &Tokens) -> Result<String, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = format!("{TEST_PREFIX}{stamp}.kdbx");

    let body = format!(r#"{{"name":"{name}"}}"#);
    let request = Request::new(Method::Post, format!("{}?fields=id,name", FILES))
        .bearer(&tokens.access_token)
        .header("Content-Type", "application/json")
        .body(body.into_bytes());

    let response = http.send(request).map_err(|e| e.to_string())?;
    if !response.is_success() {
        return Err(format!("{}: {}", response.status, response.text()));
    }
    oauth::json_string(&response.text(), "id").ok_or_else(|| "no id in response".to_string())
}

/// Re-reads the name from Drive and refuses to continue unless it is ours.
///
/// The whole safety argument rests on only ever touching a file we created, so
/// it is checked against the server rather than against a local variable.
fn assert_is_test_file(
    http: &mut dyn HttpTransport,
    tokens: &Tokens,
    file_id: &str,
) -> Result<(), String> {
    let request = Request::new(Method::Get, format!("{FILES}/{file_id}?fields=id,name"))
        .bearer(&tokens.access_token);
    let response = http.send(request).map_err(|e| e.to_string())?;
    if !response.is_success() {
        return Err(format!("{}: {}", response.status, response.text()));
    }
    let name = oauth::json_string(&response.text(), "name").unwrap_or_default();
    if !name.starts_with(TEST_PREFIX) {
        return Err(format!(
            "REFUSING TO CONTINUE: file {file_id} is named {name:?}, which is not a \
             {TEST_PREFIX}* file. This harness must only ever touch files it created."
        ));
    }
    Ok(())
}

fn delete_test_file(
    http: &mut dyn HttpTransport,
    tokens: &Tokens,
    file_id: &str,
) -> Result<(), String> {
    assert_is_test_file(http, tokens, file_id)?;
    let request =
        Request::new(Method::Delete, format!("{FILES}/{file_id}")).bearer(&tokens.access_token);
    let response = http.send(request).map_err(|e| e.to_string())?;
    if response.is_success() || response.status == 204 {
        Ok(())
    } else {
        Err(format!("{}: {}", response.status, response.text()))
    }
}

const FILES: &str = "https://www.googleapis.com/drive/v3/files";
const UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3/files";

// --- the assumptions --------------------------------------------------------

fn run_tests(
    http: &mut dyn HttpTransport,
    config: &ClientConfig,
    tokens: &Tokens,
    file_id: &str,
) -> Result<(), String> {
    assert_is_test_file(http, tokens, file_id)?;

    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut note = |ok: bool, n: u8, claim: &str, detail: String| {
        if ok {
            pass += 1;
            println!("  ✓ [{n}] {claim}\n      {detail}");
        } else {
            fail += 1;
            println!("  ✗ [{n}] {claim}\n      {detail}");
        }
    };

    // (2) files.update preserves the file id.
    assert_is_test_file(http, tokens, file_id)?;
    let first = media_write(http, tokens, file_id, b"vardo-contract-A")?;
    let second = media_write(http, tokens, file_id, b"vardo-contract-B")?;
    note(
        first.id == second.id && first.id == file_id,
        2,
        "files.update preserves the file id",
        format!("{} -> {}", first.id, second.id),
    );

    // (3) headRevisionId changes on every write, including identical bytes.
    let third = media_write(http, tokens, file_id, b"vardo-contract-B")?;
    note(
        second.head_revision != third.head_revision,
        3,
        "headRevisionId changes even for identical bytes",
        format!("{:?} -> {:?}", second.head_revision, third.head_revision),
    );

    // (1) If-Match is ignored.
    assert_is_test_file(http, tokens, file_id)?;
    let conditional = Request::new(
        Method::Patch,
        format!("{UPLOAD}/{file_id}?uploadType=media&fields=id,headRevisionId"),
    )
    .bearer(&tokens.access_token)
    .header("If-Match", "\"definitely-not-the-current-etag\"")
    .body(b"vardo-contract-C".to_vec());
    let response = http.send(conditional).map_err(|e| e.to_string())?;
    note(
        response.is_success(),
        1,
        "If-Match is ignored (a 412 would be good news and means redesign)",
        format!("status {}", response.status),
    );

    // (4) and (5) revisions.
    let mut backend = vardo_drive::drive::DriveBackend::new(
        http,
        config.clone(),
        tokens.clone(),
        file_id.to_string(),
    );
    match backend.revisions() {
        Ok(revisions) => {
            let ascending = revisions
                .windows(2)
                .all(|w| w[0].modified_unix <= w[1].modified_unix);
            note(
                ascending && revisions.len() >= 2,
                4,
                "revision list is ordered oldest first",
                format!("{} revisions, ascending={ascending}", revisions.len()),
            );

            if let Some(first_rev) = revisions.first() {
                match backend.read_revision(&first_rev.id) {
                    Ok(bytes) => note(
                        !bytes.is_empty(),
                        5,
                        "an overwritten revision is still fetchable",
                        format!("{} bytes from revision {}", bytes.len(), first_rev.id),
                    ),
                    Err(e) => note(false, 5, "an overwritten revision is still fetchable", e.to_string()),
                }
            }
        }
        Err(e) => {
            note(false, 4, "revision list is ordered oldest first", e.to_string());
            note(false, 5, "an overwritten revision is still fetchable", "skipped".into());
        }
    }

    println!("\n{pass} passed, {fail} failed");
    println!(
        "\nStill needing a human: (6) grant persists across sessions, \
         (7) quota error message text, (8) whether publishing demands verification.\n\
         See tests/contract_notes.md."
    );

    if fail == 0 {
        Ok(())
    } else {
        Err(format!("{fail} assumption(s) did not hold"))
    }
}

struct WriteResult {
    id: String,
    head_revision: Option<String>,
}

fn media_write(
    http: &mut dyn HttpTransport,
    tokens: &Tokens,
    file_id: &str,
    bytes: &[u8],
) -> Result<WriteResult, String> {
    assert_is_test_file(http, tokens, file_id)?;
    let request = Request::new(
        Method::Patch,
        format!("{UPLOAD}/{file_id}?uploadType=media&fields=id,headRevisionId"),
    )
    .bearer(&tokens.access_token)
    .body(bytes.to_vec());

    let response = http.send(request).map_err(|e| e.to_string())?;
    if !response.is_success() {
        return Err(format!("{}: {}", response.status, response.text()));
    }
    let text = response.text();
    Ok(WriteResult {
        id: oauth::json_string(&text, "id").unwrap_or_default(),
        head_revision: oauth::json_string(&text, "headRevisionId"),
    })
}
