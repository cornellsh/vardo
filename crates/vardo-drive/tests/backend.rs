//! The Drive backend, driven through a scripted HTTP transport.
//!
//! Without this the backend would be untestable: running it needs a Google
//! account, so the code paths that matter — token refresh, error
//! classification, upload framing, revision ordering — would ship unexercised.
//!
//! The requests asserted here are the contract these tests are checking
//! *against the documentation*. `tests/contract_notes.md` lists what must
//! eventually be confirmed against live Drive.

use std::collections::BTreeMap;

use vardo_drive::drive::DriveBackend;
use vardo_drive::http::{HttpTransport, Method, Request, Response};
use vardo_drive::oauth::{ClientConfig, Tokens};
use vardo_sync::backend::{StorageBackend, SyncError};

/// Replies from a script, recording everything it was asked.
struct MockHttp {
    replies: Vec<Response>,
    pub seen: Vec<Request>,
}

impl MockHttp {
    fn new(replies: Vec<Response>) -> Self {
        Self {
            replies,
            seen: Vec::new(),
        }
    }
}

impl HttpTransport for MockHttp {
    fn send(&mut self, request: Request) -> Result<Response, String> {
        self.seen.push(request);
        if self.replies.is_empty() {
            return Err("mock ran out of scripted replies".into());
        }
        Ok(self.replies.remove(0))
    }
}

fn ok(body: &str) -> Response {
    Response {
        status: 200,
        headers: BTreeMap::new(),
        body: body.as_bytes().to_vec(),
    }
}

fn ok_bytes(body: &[u8]) -> Response {
    Response {
        status: 200,
        headers: BTreeMap::new(),
        body: body.to_vec(),
    }
}

fn err(status: u16, body: &str) -> Response {
    Response {
        status,
        headers: BTreeMap::new(),
        body: body.as_bytes().to_vec(),
    }
}

fn config() -> ClientConfig {
    ClientConfig {
        client_id: "cid".into(),
        client_secret: "csecret".into(),
    }
}

fn tokens() -> Tokens {
    Tokens {
        access_token: "at-1".into(),
        refresh_token: Some("rt-1".into()),
        expires_in: Some(3599),
    }
}

const META: &str = r#"{"id":"FILE","name":"vault.kdbx","size":"1234",
  "modifiedTime":"2026-02-18T18:42:09.000Z","headRevisionId":"rev-7",
  "md5Checksum":"abc123","trashed":false}"#;

// ---------------------------------------------------------------------------

#[test]
fn stat_reads_the_head_revision_as_the_version() {
    let mut http = MockHttp::new(vec![ok(META)]);
    let mut backend = DriveBackend::new(&mut http, config(), tokens(), "FILE");

    let state = backend.stat().unwrap();
    assert_eq!(state.version, "rev-7");
    assert_eq!(state.size, 1234);
    assert_eq!(state.modified_unix, Some(1_771_440_129));
    assert_eq!(state.content_hash.as_deref(), Some("abc123"));
}

#[test]
fn a_trashed_file_reads_as_missing() {
    // It still resolves, but writing to it would put the vault in the bin.
    let trashed = META.replace("\"trashed\":false", "\"trashed\":true");
    let mut http = MockHttp::new(vec![ok(&trashed)]);
    let mut backend = DriveBackend::new(&mut http, config(), tokens(), "FILE");
    assert!(matches!(backend.stat(), Err(SyncError::NotFound)));
}

#[test]
fn writing_patches_the_existing_file_id() {
    // Update in place, never create-and-delete: a new file id would kill every
    // other client's per-file drive.file grant. This is exactly how KeePassXC's
    // safe save breaks mobile clients.
    let mut http = MockHttp::new(vec![ok(META)]);
    {
        let mut backend = DriveBackend::new(&mut http, config(), tokens(), "FILE");
        backend.write(b"new bytes").unwrap();
    }

    let request = &http.seen[0];
    assert_eq!(request.method, Method::Patch);
    assert!(
        request.url.contains("/upload/drive/v3/files/FILE"),
        "{}",
        request.url
    );
    assert!(request.url.contains("uploadType=media"), "{}", request.url);
    assert_eq!(request.body, b"new bytes");
    assert_ne!(request.method, Method::Delete, "must never delete the file");
}

#[test]
fn reading_asks_for_the_media_body() {
    let mut http = MockHttp::new(vec![ok(META), ok_bytes(b"vault-bytes")]);
    let bytes = {
        let mut backend = DriveBackend::new(&mut http, config(), tokens(), "FILE");
        backend.read().unwrap().0
    };
    assert_eq!(bytes, b"vault-bytes");
    assert!(
        http.seen[1].url.contains("alt=media"),
        "{}",
        http.seen[1].url
    );
}

#[test]
fn revisions_are_returned_newest_first() {
    // Drive lists them oldest first; the protocol wants revisions[1] to be the
    // one our write replaced, so the order is load-bearing.
    let body = r#"{"revisions":[
      {"id":"r1","modifiedTime":"2026-01-01T00:00:00.000Z","size":"10"},
      {"id":"r2","modifiedTime":"2026-01-02T00:00:00.000Z","size":"20"},
      {"id":"r3","modifiedTime":"2026-01-03T00:00:00.000Z","size":"30"}]}"#;
    let mut http = MockHttp::new(vec![ok(body)]);
    let mut backend = DriveBackend::new(&mut http, config(), tokens(), "FILE");

    let revisions = backend.revisions().unwrap();
    let ids: Vec<_> = revisions.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["r3", "r2", "r1"]);
    assert_eq!(revisions[0].size, 30);
}

// ---------------------------------------------------------------------------
// Token refresh
// ---------------------------------------------------------------------------

#[test]
fn an_expired_access_token_is_refreshed_and_the_request_retried() {
    let mut http = MockHttp::new(vec![
        err(401, r#"{"error":{"message":"Invalid Credentials"}}"#),
        ok(r#"{"access_token":"at-2","expires_in":3599}"#),
        ok(META),
    ]);
    let updated = {
        let mut backend = DriveBackend::new(&mut http, config(), tokens(), "FILE");
        let state = backend.stat().unwrap();
        assert_eq!(state.version, "rev-7");
        (backend.tokens_updated, backend.tokens().clone())
    };

    assert!(updated.0, "caller must be told to persist the new token");
    assert_eq!(updated.1.access_token, "at-2");
    // A refresh response omits the refresh token; losing it would force a
    // fresh consent flow on the next launch.
    assert_eq!(updated.1.refresh_token.as_deref(), Some("rt-1"));

    assert_eq!(http.seen.len(), 3);
    assert_eq!(
        http.seen[2].headers.get("Authorization").unwrap(),
        "Bearer at-2"
    );
}

#[test]
fn a_second_401_after_refreshing_is_not_retried_again() {
    // The grant has been revoked. Looping would spin against Google's endpoint
    // while the user waits for an error we already know is coming.
    let mut http = MockHttp::new(vec![
        err(401, r#"{"error":{"message":"Invalid Credentials"}}"#),
        ok(r#"{"access_token":"at-2"}"#),
        err(401, r#"{"error":{"message":"Invalid Credentials"}}"#),
    ]);
    let mut backend = DriveBackend::new(&mut http, config(), tokens(), "FILE");
    assert!(matches!(backend.stat(), Err(SyncError::Unauthorized(_))));
}

#[test]
fn without_a_refresh_token_a_401_is_terminal() {
    let mut http = MockHttp::new(vec![err(401, "{}")]);
    let none = Tokens {
        access_token: "at".into(),
        refresh_token: None,
        expires_in: None,
    };
    let mut backend = DriveBackend::new(&mut http, config(), none, "FILE");
    let err = backend.stat().unwrap_err();
    assert!(err.to_string().contains("no refresh token"), "{err}");
}

// ---------------------------------------------------------------------------
// Error classification
// ---------------------------------------------------------------------------

#[test]
fn rate_limits_and_permission_errors_share_403_and_must_be_told_apart() {
    // Retrying a permanent permission failure spins forever; treating a quota
    // error as permanent gives up on something that would have worked.
    let mut http = MockHttp::new(vec![err(
        403,
        r#"{"error":{"message":"User rate limit exceeded"}}"#,
    )]);
    let mut backend = DriveBackend::new(&mut http, config(), tokens(), "FILE");
    assert!(matches!(backend.stat(), Err(SyncError::RateLimited { .. })));

    let mut http = MockHttp::new(vec![err(
        403,
        r#"{"error":{"message":"The user does not have sufficient permissions"}}"#,
    )]);
    let mut backend = DriveBackend::new(&mut http, config(), tokens(), "FILE");
    assert!(matches!(backend.stat(), Err(SyncError::Unauthorized(_))));
}

#[test]
fn retryable_and_permanent_failures_are_classified() {
    /// A status, a body, and the predicate its classification must satisfy.
    type Case = (u16, &'static str, fn(&SyncError) -> bool);

    let cases: &[Case] = &[
        (404, "{}", |e| matches!(e, SyncError::NotFound)),
        (429, "{}", |e| matches!(e, SyncError::RateLimited { .. })),
        (500, "{}", |e| matches!(e, SyncError::Transport(_))),
        (503, "{}", |e| matches!(e, SyncError::Transport(_))),
        (418, "{}", |e| matches!(e, SyncError::Other(_))),
    ];
    for (status, body, check) in cases {
        let mut http = MockHttp::new(vec![err(*status, body)]);
        let mut backend = DriveBackend::new(&mut http, config(), tokens(), "FILE");
        let e = backend.stat().unwrap_err();
        assert!(check(&e), "status {status} classified as {e:?}");
        // 5xx must be retryable, 4xx generally not.
        assert_eq!(
            e.is_retryable(),
            (500..600).contains(status) || *status == 429
        );
    }
}

#[test]
fn a_transport_failure_is_reported_not_swallowed() {
    struct Dead;
    impl HttpTransport for Dead {
        fn send(&mut self, _: Request) -> Result<Response, String> {
            Err("connection reset".into())
        }
    }
    let mut http = Dead;
    let mut backend = DriveBackend::new(&mut http, config(), tokens(), "FILE");
    let e = backend.stat().unwrap_err();
    assert!(matches!(e, SyncError::Transport(_)));
    assert!(e.is_retryable());
}

#[test]
fn a_response_without_a_head_revision_is_an_error_not_a_silent_zero() {
    // headRevisionId is the change signal. Defaulting it would make every
    // remote look unchanged, and the merge step would be skipped entirely.
    let mut http = MockHttp::new(vec![ok(r#"{"id":"FILE","size":"1","trashed":false}"#)]);
    let mut backend = DriveBackend::new(&mut http, config(), tokens(), "FILE");
    let e = backend.stat().unwrap_err();
    assert!(e.to_string().contains("headRevisionId"), "{e}");
}

#[test]
fn every_request_carries_the_bearer_token() {
    let mut http = MockHttp::new(vec![ok(META), ok_bytes(b"x")]);
    {
        let mut backend = DriveBackend::new(&mut http, config(), tokens(), "FILE");
        let _ = backend.read();
    }
    for request in &http.seen {
        assert_eq!(
            request.headers.get("Authorization").map(String::as_str),
            Some("Bearer at-1"),
            "unauthenticated request to {}",
            request.url
        );
    }
}

#[test]
fn the_file_id_is_url_encoded() {
    // Drive ids are opaque; encoding them keeps a stray character from
    // changing which endpoint we hit.
    let mut http = MockHttp::new(vec![ok(META)]);
    {
        let mut backend = DriveBackend::new(&mut http, config(), tokens(), "a/b c");
        let _ = backend.stat();
    }
    assert!(
        http.seen[0].url.contains("a%2Fb%20c"),
        "{}",
        http.seen[0].url
    );
}

// --- listing vaults ---------------------------------------------------------

#[test]
fn listing_returns_only_kdbx_files() {
    // Drive has no registered MIME type for KDBX, so everything the account
    // has shared with us comes back and the extension is the only filter. A
    // list that offered someone's tax return as a vault would be a bug with
    // real consequences: they would pick it and Vardo would overwrite it.
    let body = r#"{"files":[
        {"id":"1","name":"passwords.kdbx","size":"646000","modifiedTime":"2026-08-01T10:00:00.000Z"},
        {"id":"2","name":"taxes.pdf","size":"12","modifiedTime":"2026-08-01T10:00:00.000Z"},
        {"id":"3","name":"family.KDBX","size":"900","modifiedTime":"2026-07-01T10:00:00.000Z"}
    ]}"#;
    let mut http = MockHttp::new(vec![ok(body)]);

    let (files, _, _) = vardo_drive::drive::list_vaults(&mut http, config(), tokens()).unwrap();

    let names: Vec<_> = files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["passwords.kdbx", "family.KDBX"]);
    assert_eq!(files[0].id, "1");
    assert_eq!(files[0].size, 646_000);
    assert!(files[0].modified_unix.is_some());
}

#[test]
fn listing_excludes_trashed_files_at_the_query() {
    // Filtering client-side would still show them for the moment before the
    // response is parsed, and more importantly would waste the page budget on
    // files that cannot be used.
    let mut http = MockHttp::new(vec![ok(r#"{"files":[]}"#)]);
    let _ = vardo_drive::drive::list_vaults(&mut http, config(), tokens());

    let url = &http.seen[0].url;
    assert!(
        url.contains("trashed") && url.contains("false"),
        "the query must exclude trashed files, got {url}"
    );
}

#[test]
fn an_empty_drive_is_not_an_error() {
    // This is the normal state for a new user, and under the drive.file scope
    // it is also the state for someone whose Drive already has a vault Vardo
    // has never been shown. Returning an error would make the UI say something
    // went wrong when nothing did.
    let mut http = MockHttp::new(vec![ok(r#"{"files":[]}"#)]);
    let (files, _, _) = vardo_drive::drive::list_vaults(&mut http, config(), tokens()).unwrap();
    assert!(files.is_empty());
}

#[test]
fn a_name_containing_braces_does_not_break_the_split() {
    // The brace-depth scan is only safe because it tracks string state. A file
    // literally named "{}.kdbx" is legal in Drive and would otherwise desync
    // the depth counter and silently drop every file after it.
    let body = r#"{"files":[
        {"id":"1","name":"{}.kdbx","size":"10","modifiedTime":"2026-08-01T10:00:00.000Z"},
        {"id":"2","name":"after.kdbx","size":"20","modifiedTime":"2026-08-01T10:00:00.000Z"}
    ]}"#;
    let mut http = MockHttp::new(vec![ok(body)]);
    let (files, _, _) = vardo_drive::drive::list_vaults(&mut http, config(), tokens()).unwrap();
    assert_eq!(
        files.len(),
        2,
        "the file after the braced name must survive"
    );
    assert_eq!(files[1].name, "after.kdbx");
}

#[test]
fn creating_a_vault_sends_one_multipart_request() {
    // Two requests (create metadata, then upload content) would leave a window
    // where an empty file exists under the vault's name and may already have
    // synced to the user's other devices.
    let mut http = MockHttp::new(vec![ok(r#"{"id":"newid123"}"#)]);
    let (id, _, _) = vardo_drive::drive::create(
        &mut http,
        config(),
        tokens(),
        "passwords.kdbx",
        &[0x03, 0xD9, 0xA2, 0x9A],
    )
    .unwrap();

    assert_eq!(id, "newid123");
    assert_eq!(http.seen.len(), 1, "create must be a single request");
    assert!(http.seen[0].url.contains("uploadType=multipart"));

    let body = String::from_utf8_lossy(&http.seen[0].body);
    assert!(body.contains(r#""name":"passwords.kdbx""#));
}

#[test]
fn a_vault_name_containing_a_quote_is_escaped() {
    // Legal on every platform Vardo runs on. Interpolated raw it produces a
    // malformed request and an error message about JSON, which sends the user
    // looking anywhere but at their filename.
    let mut http = MockHttp::new(vec![ok(r#"{"id":"x"}"#)]);
    let _ = vardo_drive::drive::create(
        &mut http,
        config(),
        tokens(),
        r#"my "old" vault.kdbx"#,
        &[0x03, 0xD9, 0xA2, 0x9A],
    );

    let body = String::from_utf8_lossy(&http.seen[0].body);
    assert!(
        body.contains(r#""name":"my \"old\" vault.kdbx""#),
        "quotes must be escaped, got: {body}"
    );
}

#[test]
fn create_reports_a_missing_id_rather_than_inventing_one() {
    // If Drive ever answers 200 without an id, silently continuing would leave
    // a vault uploaded and unreachable, with the app pointing at "".
    let mut http = MockHttp::new(vec![ok(r#"{"kind":"drive#file"}"#)]);
    let result = vardo_drive::drive::create(
        &mut http,
        config(),
        tokens(),
        "v.kdbx",
        &[0x03, 0xD9, 0xA2, 0x9A],
    );
    assert!(result.is_err());
}
