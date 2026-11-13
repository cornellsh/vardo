//! The browser pipe on Windows, over a real named pipe, with no window.
//!
//! One test, because it points `LOCALAPPDATA` at a scratch folder for the whole
//! process. It checks what the Unix socket tests check where it matters: the
//! name is random and published only in the user's folder, a client reaches
//! the vault through it, and a second Vardo cannot claim it.

#![cfg(windows)]

use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use vardo_core::browser::server::{serve_pipe, Perform};
use vardo_core::source::{SourceContext, VaultSource};
use vardo_core::Session;

const FIXTURE: &str = "../../fixtures/test_db_kdbx4_with_password_deleted_entry.kdbx";

#[test]
fn the_pipe_has_a_private_random_name_and_answers_through_it() {
    let dir = std::env::temp_dir().join("vardo-pipe-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // The only test in this binary, so changing the environment races nothing.
    std::env::set_var("LOCALAPPDATA", &dir);

    let vault = dir.join("v.kdbx");
    std::fs::copy(FIXTURE, &vault).unwrap();
    let session = Session::open(
        &VaultSource::file(&vault),
        &SourceContext::local_only(&dir),
        "demopass",
        |_| {},
    )
    .unwrap();
    let session = Arc::new(Mutex::new(Some(session)));
    let nothing: Arc<Perform> = Arc::new(|_| Ok(()));

    let (s, p) = (Arc::clone(&session), Arc::clone(&nothing));
    std::thread::spawn(move || serve_pipe(s, p).unwrap());
    let mut name = None;
    for _ in 0..300 {
        name = vardo_browser::socket::pipe_name();
        if name.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let name = name.expect("the server never published its pipe");
    let published = vardo_browser::socket::pipe_name_file().unwrap();
    assert!(
        published.starts_with(&dir),
        "published under the user's own folder"
    );
    let suffix = name.trim_start_matches(r"\\.\pipe\vardo-");
    assert_eq!(suffix.len(), 32, "128 random bits: {name}");
    assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));

    let pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&name)
        .unwrap();
    let mut reader = BufReader::new(pipe.try_clone().unwrap());
    let mut writer = pipe;
    writer
        .write_all(b"{\"v\":1,\"id\":7,\"type\":\"status\"}\n")
        .unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(reply["id"], 7);
    assert_eq!(reply["state"]["kind"], "unlocked");

    let second = serve_pipe(session, nothing).unwrap_err();
    assert_eq!(second.kind(), std::io::ErrorKind::AddrInUse);
}
