//! The browser socket, over a real Unix socket, with no window.
//!
//! `browser.rs` tests what each request is answered with. These test the
//! plumbing around it: that the socket is private, that two Vardos cannot
//! both claim it, that locking takes effect immediately, and that an effect
//! which fails is reported as a failure rather than as success.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use vardo_core::browser::server::{bind, serve, Perform};
use vardo_core::browser::Effect;
use vardo_core::source::{SourceContext, VaultSource};
use vardo_core::Session;

const FIXTURE: &str = "../../fixtures/test_db_kdbx4_with_password_deleted_entry.kdbx";

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vardo-sock-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn unlocked(dir: &Path) -> Session {
    let path = dir.join("v.kdbx");
    std::fs::copy(FIXTURE, &path).unwrap();
    Session::open(
        &VaultSource::file(&path),
        &SourceContext::local_only(dir),
        "demopass",
        |_| {},
    )
    .unwrap()
}

/// Starts a server and waits until it accepts connections.
fn start(socket: &Path, session: Arc<Mutex<Option<Session>>>, perform: Arc<Perform>) {
    let path = socket.to_path_buf();
    std::thread::spawn(move || serve(&path, session, perform).unwrap());
    for _ in 0..200 {
        if UnixStream::connect(socket).is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the server never started");
}

struct Client {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Client {
    fn connect(socket: &Path) -> Self {
        let writer = UnixStream::connect(socket).unwrap();
        let reader = BufReader::new(writer.try_clone().unwrap());
        Client { writer, reader }
    }

    fn ask(&mut self, line: &str) -> Value {
        self.writer.write_all(line.as_bytes()).unwrap();
        self.writer.write_all(b"\n").unwrap();
        let mut reply = String::new();
        self.reader.read_line(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap()
    }
}

fn nothing() -> Arc<Perform> {
    Arc::new(|_| Ok(()))
}

#[test]
fn the_socket_and_its_directory_are_private() {
    let dir = scratch("private");
    let socket = dir.join("run/vardo/browser.sock");
    start(&socket, Arc::new(Mutex::new(None)), nothing());

    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(socket.parent().unwrap()), 0o700);
    assert_eq!(mode(&socket), 0o600);
}

#[test]
fn a_second_vardo_does_not_take_over_a_live_socket() {
    let dir = scratch("second");
    let socket = dir.join("browser.sock");
    start(&socket, Arc::new(Mutex::new(None)), nothing());

    let err = bind(&socket).expect_err("a live socket must not be claimed twice");
    assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
    // And the first one still answers.
    let reply = Client::connect(&socket).ask(r#"{"v":1,"id":1,"type":"status"}"#);
    assert_eq!(reply["type"], "status");
}

#[test]
fn a_socket_left_by_a_crash_is_replaced() {
    let dir = scratch("stale");
    let socket = dir.join("browser.sock");
    // Bound and dropped: the file stays, and nothing listens.
    drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
    assert!(socket.exists());

    start(&socket, Arc::new(Mutex::new(None)), nothing());
    let reply = Client::connect(&socket).ask(r#"{"v":1,"id":1,"type":"status"}"#);
    assert_eq!(reply["state"]["kind"], "locked");
}

#[test]
fn locking_takes_effect_on_the_next_request() {
    // The window locks by taking the session out of the shared slot. The
    // browser must see that at once, not on its next connection.
    let dir = scratch("lock");
    let socket = dir.join("browser.sock");
    let session = Arc::new(Mutex::new(Some(unlocked(&dir))));
    start(&socket, Arc::clone(&session), nothing());

    let mut client = Client::connect(&socket);
    assert_eq!(
        client.ask(r#"{"v":1,"id":1,"type":"status"}"#)["state"]["kind"],
        "unlocked"
    );
    session.lock().unwrap().take();
    let reply = client.ask(r#"{"v":1,"id":2,"type":"search","query":"a"}"#);
    assert_eq!(reply["code"], "locked");
    assert_eq!(reply["id"], 2);
}

#[test]
fn copying_is_done_by_the_app_and_a_failed_copy_is_not_reported_as_copied() {
    let dir = scratch("copy");
    let socket = dir.join("browser.sock");
    let session = unlocked(&dir);
    let uuid = session
        .list_entries(None, None)
        .into_iter()
        .find(|e| !e.username.is_empty())
        .expect("precondition: the fixture has an entry with a username")
        .uuid;

    let copied: Arc<Mutex<Vec<String>>> = Arc::default();
    let fail = Arc::new(Mutex::new(false));
    let perform: Arc<Perform> = {
        let copied = Arc::clone(&copied);
        let fail = Arc::clone(&fail);
        Arc::new(move |effect| match effect {
            Effect::Copy(value) if *fail.lock().unwrap() => {
                drop(value);
                Err("Could not copy: clipboard unavailable".into())
            }
            Effect::Copy(value) => {
                copied.lock().unwrap().push(value.to_string());
                Ok(())
            }
            _ => Ok(()),
        })
    };
    start(&socket, Arc::new(Mutex::new(Some(session))), perform);

    let mut client = Client::connect(&socket);
    let request = format!(r#"{{"v":1,"id":7,"type":"copy","uuid":"{uuid}","field":"username"}}"#);
    let reply = client.ask(&request);
    assert_eq!(reply["type"], "copied", "{reply}");
    assert_eq!(
        copied.lock().unwrap().len(),
        1,
        "the app must have been asked to copy"
    );

    *fail.lock().unwrap() = true;
    let reply = client.ask(&request);
    assert_eq!(reply["code"], "internal");
    assert_eq!(reply["id"], 7);
    assert!(reply["message"].as_str().unwrap().contains("clipboard"));
}

#[test]
fn one_connection_serves_many_requests() {
    // Chrome keeps the host running while a port is open, so the same
    // connection carries a whole popup's worth of requests.
    let dir = scratch("many");
    let socket = dir.join("browser.sock");
    start(
        &socket,
        Arc::new(Mutex::new(Some(unlocked(&dir)))),
        nothing(),
    );

    let mut client = Client::connect(&socket);
    for id in 1..=20 {
        let reply = client.ask(&format!(r#"{{"v":1,"id":{id},"type":"status"}}"#));
        assert_eq!(reply["id"], id);
    }
}
