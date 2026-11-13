//! The WebDAV backend against a fake server that misbehaves the ways real
//! ones do (`fact:webdav-etag-hazards`).

use std::collections::BTreeMap;

use vardo_sync::backend::WriteOutcome;
use vardo_sync::{StorageBackend, SyncError};
use vardo_webdav::{base64, Request, Response, Transport, WebDavBackend};

const URL: &str = "https://dav.example.com/remote.php/dav/files/alex/passwords.kdbx";

#[derive(Default)]
struct Server {
    content: Option<Vec<u8>>,
    version: u32,
    /// Apache `mod_deflate` at its default: a compressible response gets
    /// `-gzip` added to its ETag, but `If-Match` is checked against the bare tag.
    deflate: bool,
    weak: bool,
    /// Apache's habit: this many reads come back weak, as for a file written
    /// within the last second, before the tag turns strong.
    weak_reads: u32,
    no_etag: bool,
    /// Refuses every conditional write, whatever the tag.
    broken: bool,
    /// Another device's write, landing just before our next `PUT`.
    intruder: Option<Vec<u8>>,
    seen: Vec<Request>,
}

impl Server {
    fn with(content: &[u8]) -> Self {
        Server {
            content: Some(content.to_vec()),
            version: 1,
            ..Server::default()
        }
    }

    fn bare(&self) -> String {
        format!("\"v{}\"", self.version)
    }
}

impl Transport for Server {
    fn send(&mut self, request: Request) -> Result<Response, String> {
        self.seen.push(request.clone());
        let expected = format!("Basic {}", base64(b"alex:dav-password"));
        if request.headers.get("Authorization") != Some(&expected) {
            return Ok(Response {
                status: 401,
                ..Response::default()
            });
        }
        let identity =
            request.headers.get("Accept-Encoding").map(String::as_str) == Some("identity");
        let mut headers = BTreeMap::new();
        if !self.no_etag {
            let weak_now = self.weak || (self.weak_reads > 0 && request.method == "GET");
            if weak_now && self.weak_reads > 0 {
                self.weak_reads -= 1;
            }
            let tag = if weak_now {
                format!("W/{}", self.bare())
            } else if self.deflate && !identity {
                format!("\"v{}-gzip\"", self.version)
            } else {
                self.bare()
            };
            headers.insert("etag".into(), tag);
        }
        headers.insert(
            "last-modified".into(),
            "Sun, 06 Nov 1994 08:49:37 GMT".into(),
        );
        match request.method {
            "GET" | "HEAD" => {
                let Some(content) = self.content.clone() else {
                    return Ok(Response {
                        status: 404,
                        ..Response::default()
                    });
                };
                headers.insert("content-length".into(), content.len().to_string());
                let body = if request.method == "GET" {
                    content
                } else {
                    Vec::new()
                };
                Ok(Response {
                    status: 200,
                    headers,
                    body,
                })
            }
            "PUT" => {
                if let Some(other) = self.intruder.take() {
                    self.content = Some(other);
                    self.version += 1;
                }
                if let Some(tag) = request.headers.get("If-Match") {
                    if self.broken || *tag != self.bare() {
                        return Ok(Response {
                            status: 412,
                            ..Response::default()
                        });
                    }
                }
                self.content = Some(request.body.clone());
                self.version += 1;
                Ok(Response {
                    status: 204,
                    ..Response::default()
                })
            }
            other => Err(format!("fake server got {other}")),
        }
    }
}

fn backend(server: &mut Server) -> WebDavBackend<'_, Server> {
    WebDavBackend::new(server, URL, "alex", "dav-password")
}

#[test]
fn an_unchanged_file_is_replaced_with_a_conditional_write() {
    let mut server = Server::with(b"base");
    let outcome = backend(&mut server)
        .write_if_unchanged(b"merged", b"base")
        .unwrap();
    assert!(matches!(outcome, WriteOutcome::Written(_)));
    assert_eq!(server.content.as_deref(), Some(&b"merged"[..]));
    let put = server.seen.iter().find(|r| r.method == "PUT").unwrap();
    assert_eq!(
        put.headers.get("If-Match").map(String::as_str),
        Some("\"v1\"")
    );
}

#[test]
fn a_write_landing_between_our_read_and_our_write_is_handed_back() {
    let mut server = Server::with(b"base");
    server.intruder = Some(b"other device".to_vec());
    let outcome = backend(&mut server)
        .write_if_unchanged(b"merged", b"base")
        .unwrap();
    match outcome {
        WriteOutcome::Superseded { buried } => assert_eq!(buried, b"other device"),
        other => panic!("expected the other write back, got {other:?}"),
    }
    assert_eq!(
        server.content.as_deref(),
        Some(&b"other device"[..]),
        "the other device's write survives"
    );
}

#[test]
fn a_file_that_moved_before_we_read_it_is_not_written_at_all() {
    let mut server = Server::with(b"newer");
    let outcome = backend(&mut server)
        .write_if_unchanged(b"merged", b"base")
        .unwrap();
    assert!(matches!(outcome, WriteOutcome::Superseded { ref buried } if buried == b"newer"));
    assert!(server.seen.iter().all(|r| r.method != "PUT"));
}

#[test]
fn mod_deflate_does_not_break_conditional_writes() {
    // The case dec:webdav-second requires. Asking for no compression keeps
    // the ETag bare, so If-Match names the tag the server checks.
    let mut server = Server::with(b"base");
    server.deflate = true;
    let outcome = backend(&mut server)
        .write_if_unchanged(b"merged", b"base")
        .unwrap();
    assert!(matches!(outcome, WriteOutcome::Written(_)));
    assert!(server
        .seen
        .iter()
        .all(|r| r.headers.get("Accept-Encoding").map(String::as_str) == Some("identity")));
}

#[test]
fn a_server_that_refuses_an_unchanged_file_is_reported_not_retried() {
    // What mod_deflate does to a client that accepts gzip: every write is
    // refused while nothing changed. Treating that as concurrency would make
    // sync one-way without anyone noticing.
    let mut server = Server::with(b"base");
    server.broken = true;
    let err = backend(&mut server)
        .write_if_unchanged(b"merged", b"base")
        .unwrap_err();
    assert!(
        matches!(err, SyncError::Other(ref m) if m.contains("ETags")),
        "{err}"
    );
    assert!(!err.is_retryable());
    assert_eq!(
        server.content.as_deref(),
        Some(&b"base"[..]),
        "nothing was written"
    );
}

#[test]
fn weak_or_missing_etags_are_refused_before_writing() {
    for (weak, no_etag) in [(true, false), (false, true)] {
        let mut server = Server::with(b"base");
        server.weak = weak;
        server.no_etag = no_etag;
        let err = backend(&mut server)
            .write_if_unchanged(b"merged", b"base")
            .unwrap_err();
        assert!(matches!(err, SyncError::Other(_)), "{err}");
        assert!(server.seen.iter().all(|r| r.method != "PUT"));
    }
}

#[test]
fn a_just_written_files_weak_etag_is_given_a_second_to_settle() {
    let mut server = Server::with(b"base");
    server.weak_reads = 1;
    let outcome = backend(&mut server)
        .write_if_unchanged(b"merged", b"base")
        .unwrap();
    assert!(matches!(outcome, WriteOutcome::Written(_)));
    assert_eq!(server.content.as_deref(), Some(&b"merged"[..]));
}

#[test]
fn a_tag_turning_from_weak_to_strong_is_the_same_version() {
    let mut server = Server::with(b"base");
    server.weak = true;
    let (_, weak) = backend(&mut server).read().unwrap();
    server.weak = false;
    let (_, strong) = backend(&mut server).read().unwrap();
    assert_eq!(weak.version, strong.version);
}

#[test]
fn reading_returns_the_bytes_and_their_version() {
    let mut server = Server::with(b"vault bytes");
    let (bytes, state) = backend(&mut server).read().unwrap();
    assert_eq!(bytes, b"vault bytes");
    assert_eq!(state.version, "\"v1\"");
    assert_eq!(state.size, 11);
    assert_eq!(state.modified_unix, Some(784_111_777));
}

#[test]
fn wrong_credentials_and_missing_files_say_so() {
    let mut server = Server::with(b"x");
    let err = WebDavBackend::new(&mut server, URL, "alex", "wrong")
        .read()
        .unwrap_err();
    assert!(matches!(err, SyncError::Unauthorized(_)));

    let mut empty = Server::default();
    assert!(matches!(
        backend(&mut empty).read().unwrap_err(),
        SyncError::NotFound
    ));
}

#[test]
fn the_password_travels_only_in_the_authorization_header() {
    let mut server = Server::with(b"base");
    backend(&mut server)
        .write_if_unchanged(b"merged", b"base")
        .unwrap();
    for request in &server.seen {
        assert!(!request.url.contains("dav-password"));
        assert!(request
            .headers
            .iter()
            .filter(|(name, _)| *name != "Authorization")
            .all(|(_, value)| !value.contains("dav-password")));
    }
}
