//! Concurrency, faults, and the races a real provider will hand us.
//!
//! These run against `FakeDrive` on purpose. A real API is too slow for this
//! many iterations, non-deterministic, flaky in CI, and — fatally — offers no
//! way to force a write to land inside somebody else's write. Here that is a
//! callback.

use vardo_kdbx::write::Rng;
use vardo_kdbx::{open, Credentials};
use vardo_merge::ConflictPolicy;
use vardo_sync::backend::StorageBackend;
use vardo_sync::fake::Server;
use vardo_sync::protocol::{sync, Error, Options, Outcome, SyncPoint};
use vardo_vault::Vault;

const PW: &str = "demopass";

struct SeededRng(u64);

impl Rng for SeededRng {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), vardo_kdbx::Error> {
        for b in out.iter_mut() {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            *b = (self.0 >> 33) as u8;
        }
        Ok(())
    }
}

fn creds() -> Credentials {
    Credentials::with_password(PW)
}

fn fixture() -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/test_db_kdbx41_with_password_aes.kdbx");
    std::fs::read(p).unwrap()
}

/// A vault held by value alongside the document it borrows from.
struct Held {
    doc: vardo_kdbx::Document,
}

impl Held {
    fn from(bytes: &[u8]) -> Self {
        Self {
            doc: open(bytes, &creds()).expect("open"),
        }
    }
    fn vault(&self) -> Vault {
        Vault::open(&self.doc).expect("unlock")
    }
}

/// Edits an entry the way a real client would: change the field **and** bump
/// `LastModificationTime`.
///
/// Forgetting the timestamp makes two different edits look simultaneous, and
/// merge correctly refuses to pick between them. That is the intended
/// behaviour, so the test helper has to behave like a real writer.
fn set_username(v: &mut Vault, title: &str, value: &str, at_unix: i64) {
    use vardo_vault::{Encoding, Timestamp};

    let uuid = v
        .entries()
        .into_iter()
        .find(|e| e.title() == title)
        .and_then(|e| e.uuid())
        .expect("entry");
    let root = v
        .document_mut()
        .root
        .child_mut("Root")
        .unwrap()
        .child_mut("Group")
        .unwrap();
    let entry = vardo_merge::dom::entry_mut(root, uuid).expect("entry element");
    for child in entry.children.iter_mut() {
        let vardo_kdbx::xml::Node::Element(s) = child else {
            continue;
        };
        if s.name == "String" && s.child("Key").map(|k| k.text()).as_deref() == Some("UserName") {
            s.child_mut("Value").unwrap().set_text(value);
        }
    }
    let stamp = Timestamp::from_unix(at_unix, Encoding::Base64Seconds).to_text();
    entry
        .child_mut("Times")
        .expect("Times")
        .child_mut("LastModificationTime")
        .expect("LastModificationTime")
        .set_text(&stamp);
}

fn titles(bytes: &[u8]) -> Vec<String> {
    let held = Held::from(bytes);
    let v = held.vault();
    let mut t: Vec<_> = v.entries().iter().map(|e| e.title()).collect();
    t.sort();
    t
}

// ---------------------------------------------------------------------------

#[test]
fn first_sync_creates_the_remote_file() {
    let server = Server::new();
    let mut backend = server.connect();
    let held = Held::from(&fixture());
    let mut local = held.vault();

    let report = sync(
        &mut backend,
        &mut local,
        &creds(),
        None,
        &Options::default(),
        &mut SeededRng(1),
    )
    .expect("sync");

    assert!(matches!(report.outcome, Outcome::Created));
    assert!(server.current().is_some());
    assert!(report.point.is_some());
}

#[test]
fn an_unchanged_remote_takes_our_edits_without_merging() {
    let bytes = fixture();
    let server = Server::with_file(&bytes);
    let mut backend = server.connect();
    let point = SyncPoint {
        version: backend.stat().unwrap().version,
        base: bytes.clone(),
    };

    let held = Held::from(&bytes);
    let mut local = held.vault();
    set_username(
        &mut local,
        "entry with no quality check",
        "pushed",
        2_000_000_000,
    );

    let report = sync(
        &mut backend,
        &mut local,
        &creds(),
        Some(&point),
        &Options::default(),
        &mut SeededRng(2),
    )
    .expect("sync");

    assert!(matches!(report.outcome, Outcome::Pushed));
    assert_eq!(report.races_lost, 0);

    let remote = Held::from(&server.current().unwrap());
    let v = remote.vault();
    assert_eq!(
        v.entries()
            .into_iter()
            .find(|e| e.title() == "entry with no quality check")
            .unwrap()
            .username(),
        "pushed"
    );
}

#[test]
fn a_changed_remote_is_merged_not_clobbered() {
    let bytes = fixture();
    let server = Server::with_file(&bytes);
    let mut backend = server.connect();
    let point = SyncPoint {
        version: backend.stat().unwrap().version,
        base: bytes.clone(),
    };

    // Another device edits a different entry and uploads.
    {
        let held = Held::from(&bytes);
        let mut other = held.vault();
        set_username(
            &mut other,
            "entry that was moved",
            "from-other-device",
            2_000_000_100,
        );
        let uploaded = other.save_with(&creds(), &mut SeededRng(50)).unwrap();
        server.connect().write(&uploaded).unwrap();
    }

    // We edit a different entry and sync.
    let held = Held::from(&bytes);
    let mut local = held.vault();
    set_username(
        &mut local,
        "entry with no quality check",
        "from-us",
        2_000_000_200,
    );

    let report = sync(
        &mut backend,
        &mut local,
        &creds(),
        Some(&point),
        &Options::default(),
        &mut SeededRng(3),
    )
    .expect("sync");

    assert!(matches!(report.outcome, Outcome::Merged(_)));

    // Both edits must be present. Clobbering would keep only one.
    let remote = Held::from(&server.current().unwrap());
    let v = remote.vault();
    let by = |t: &str| {
        v.entries()
            .into_iter()
            .find(|e| e.title() == t)
            .unwrap()
            .username()
    };
    assert_eq!(by("entry with no quality check"), "from-us");
    assert_eq!(by("entry that was moved"), "from-other-device");
}

#[test]
fn a_write_that_loses_a_race_is_detected_and_redone() {
    // The hard case, and the one no other KeePass client handles: another
    // device lands a write between our read and our write. There is no
    // conditional write to prevent it, so we must notice afterwards.
    let bytes = fixture();
    let server = Server::with_file(&bytes);
    let mut backend = server.connect();
    let point = SyncPoint {
        version: backend.stat().unwrap().version,
        base: bytes.clone(),
    };

    // Somebody else already moved the remote on, so we take the merge path.
    {
        let held = Held::from(&bytes);
        let mut other = held.vault();
        set_username(
            &mut other,
            "entry that was moved",
            "first-writer",
            2_000_000_100,
        );
        let uploaded = other.save_with(&creds(), &mut SeededRng(60)).unwrap();
        server.connect().write(&uploaded).unwrap();
    }

    // Inject a racer that fires exactly once, inside our write.
    {
        let server = server.clone();
        let bytes = bytes.clone();
        let fired = std::sync::atomic::AtomicBool::new(false);
        backend.race_before_write(move || {
            if fired.swap(true, std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            let held = Held::from(&bytes);
            let mut racer = held.vault();
            set_username(&mut racer, "entry with custom data", "racer", 2_000_000_300);
            let uploaded = racer.save_with(&creds(), &mut SeededRng(70)).unwrap();
            server.connect().write(&uploaded).unwrap();
        });
    }

    let held = Held::from(&bytes);
    let mut local = held.vault();
    set_username(
        &mut local,
        "entry with no quality check",
        "ours",
        2_000_000_400,
    );

    let report = sync(
        &mut backend,
        &mut local,
        &creds(),
        Some(&point),
        &Options::default(),
        &mut SeededRng(4),
    )
    .expect("sync");

    assert_eq!(report.races_lost, 1, "the race must have been detected");
    assert!(matches!(report.outcome, Outcome::Merged(_)));

    // Every writer's edit survived, including the one that raced us.
    let remote = Held::from(&server.current().unwrap());
    let v = remote.vault();
    let by = |t: &str| {
        v.entries()
            .into_iter()
            .find(|e| e.title() == t)
            .unwrap()
            .username()
    };
    assert_eq!(by("entry with no quality check"), "ours");
    assert_eq!(by("entry with custom data"), "racer");
}

#[test]
fn an_endless_racer_is_given_up_on_rather_than_looped_forever() {
    let bytes = fixture();
    let server = Server::with_file(&bytes);
    let mut backend = server.connect();
    let point = SyncPoint {
        version: "stale".into(),
        base: bytes.clone(),
    };

    // A racer that never stops, writing genuinely different content each time.
    //
    // The content matters: detection compares bytes, so a racer rewriting an
    // identical file is correctly *not* counted as a lost race — nothing was
    // lost. Only a differing write is one.
    {
        let server = server.clone();
        let bytes = bytes.clone();
        let round = std::sync::atomic::AtomicI64::new(0);
        backend.race_before_write(move || {
            let n = round.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let held = Held::from(&bytes);
            let mut racer = held.vault();
            set_username(
                &mut racer,
                "entry with custom data",
                &format!("racer-{n}"),
                2_100_000_000 + n,
            );
            let uploaded = racer
                .save_with(&creds(), &mut SeededRng(900 + n as u64))
                .unwrap();
            server.connect().write(&uploaded).unwrap();
        });
    }

    let held = Held::from(&bytes);
    let mut local = held.vault();
    let err = sync(
        &mut backend,
        &mut local,
        &creds(),
        Some(&point),
        &Options::default(),
        &mut SeededRng(5),
    )
    .unwrap_err();

    assert!(matches!(err, Error::RaceLimitExceeded(_)), "{err}");
    // The message must reassure: nothing was destroyed.
    assert!(err.to_string().contains("Nothing was lost"));
}

#[test]
fn every_attempt_remains_in_the_revision_history() {
    // This is what makes a lost race survivable rather than fatal.
    let bytes = fixture();
    let server = Server::with_file(&bytes);
    let before = server.revision_count();

    let mut backend = server.connect();
    let held = Held::from(&bytes);
    let mut local = held.vault();
    set_username(
        &mut local,
        "entry with no quality check",
        "v1",
        2_000_000_000,
    );
    let point = SyncPoint {
        version: backend.stat().unwrap().version,
        base: bytes.clone(),
    };
    sync(
        &mut backend,
        &mut local,
        &creds(),
        Some(&point),
        &Options::default(),
        &mut SeededRng(6),
    )
    .unwrap();

    assert!(server.revision_count() > before);
    let revs = backend.revisions().unwrap();
    assert!(revs.len() >= 2, "prior versions must be retrievable");

    // The original content is still fetchable.
    let oldest = revs.last().unwrap().id.clone();
    let restored = backend.read_revision(&oldest).unwrap();
    assert_eq!(titles(&restored), titles(&bytes));
}

// ---------------------------------------------------------------------------
// Faults
// ---------------------------------------------------------------------------

#[test]
fn transport_failures_are_retried() {
    let bytes = fixture();
    let server = Server::with_file(&bytes);
    let mut backend = server.connect();
    backend.fail_next_calls(2);

    let held = Held::from(&bytes);
    let mut local = held.vault();
    let report = sync(
        &mut backend,
        &mut local,
        &creds(),
        None,
        &Options::default(),
        &mut SeededRng(7),
    )
    .expect("should ride out transient failures");
    assert_eq!(report.retries, 2);
}

#[test]
fn rate_limits_are_retried() {
    let bytes = fixture();
    let server = Server::with_file(&bytes);
    let mut backend = server.connect();
    backend.rate_limit_next_calls(3);

    let held = Held::from(&bytes);
    let mut local = held.vault();
    let report = sync(
        &mut backend,
        &mut local,
        &creds(),
        None,
        &Options::default(),
        &mut SeededRng(8),
    )
    .expect("should ride out rate limiting");
    assert_eq!(report.retries, 3);
}

#[test]
fn persistent_failure_gives_up_rather_than_spinning() {
    let bytes = fixture();
    let server = Server::with_file(&bytes);
    let mut backend = server.connect();
    backend.fail_next_calls(usize::MAX);

    let held = Held::from(&bytes);
    let mut local = held.vault();
    let err = sync(
        &mut backend,
        &mut local,
        &creds(),
        None,
        &Options::default(),
        &mut SeededRng(9),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Storage(_)), "{err}");
}

#[test]
fn expired_credentials_are_reported_not_retried() {
    let bytes = fixture();
    let server = Server::with_file(&bytes);
    let mut backend = server.connect();
    backend.faults.unauthorized = true;

    let held = Held::from(&bytes);
    let mut local = held.vault();
    let err = sync(
        &mut backend,
        &mut local,
        &creds(),
        None,
        &Options::default(),
        &mut SeededRng(10),
    )
    .unwrap_err();

    assert!(err.to_string().contains("not authorised"), "{err}");
}

#[test]
fn a_replaced_remote_file_is_reported_as_a_dead_reference() {
    // Another app saved by writing a new file instead of updating this one,
    // which mints a new ID on Drive and kills our per-file grant. Exactly what
    // KeePassXC's safe-save does to mobile clients through a cloud mount.
    let bytes = fixture();
    let server = Server::with_file(&bytes);
    let mut backend = server.connect();

    server.replace_file(&bytes);

    let held = Held::from(&bytes);
    let mut local = held.vault();
    let err = sync(
        &mut backend,
        &mut local,
        &creds(),
        None,
        &Options::default(),
        &mut SeededRng(11),
    )
    .unwrap_err();

    assert!(
        err.to_string().contains("replaced rather than updated"),
        "{err}"
    );
}

#[test]
fn conflicts_can_block_an_upload_entirely() {
    let bytes = fixture();
    let server = Server::with_file(&bytes);
    let mut backend = server.connect();
    let point = SyncPoint {
        version: backend.stat().unwrap().version,
        base: bytes.clone(),
    };

    // Both sides edit the same field, and the remote moves on first.
    {
        let held = Held::from(&bytes);
        let mut other = held.vault();
        set_username(
            &mut other,
            "entry with no quality check",
            "theirs",
            2_000_000_500,
        );
        let uploaded = other.save_with(&creds(), &mut SeededRng(80)).unwrap();
        server.connect().write(&uploaded).unwrap();
    }
    let head_before = server.current().unwrap();

    let held = Held::from(&bytes);
    let mut local = held.vault();
    set_username(
        &mut local,
        "entry with no quality check",
        "ours",
        2_000_000_500,
    );

    let options = Options {
        policy: ConflictPolicy::Refuse,
        ..Options::default()
    };
    let report = sync(
        &mut backend,
        &mut local,
        &creds(),
        Some(&point),
        &options,
        &mut SeededRng(12),
    )
    .expect("sync should return, not error");

    match report.outcome {
        Outcome::NeedsReview(plan) => assert!(plan.needs_review()),
        other => panic!("expected NeedsReview, got {other:?}"),
    }
    assert!(
        report.point.is_none(),
        "a blocked sync must not advance the sync point"
    );
    assert_eq!(
        server.current().unwrap(),
        head_before,
        "a blocked sync must not upload anything"
    );
}

// ---------------------------------------------------------------------------
// Local backend
// ---------------------------------------------------------------------------

#[test]
fn cloud_mounted_paths_are_recognised() {
    use std::path::Path;
    use vardo_sync::local::LocalFile;

    for bad in [
        "/home/u/GoogleDrive/passwords.kdbx",
        "/home/u/Google Drive/x.kdbx",
        "/Users/u/Library/CloudStorage/Dropbox/x.kdbx",
        "/home/u/OneDrive/x.kdbx",
        "/home/u/Nextcloud/x.kdbx",
    ] {
        assert!(
            LocalFile::looks_cloud_synced(Path::new(bad)),
            "should flag {bad}"
        );
    }
    for ok in ["/home/u/.local/share/vardo/vault.kdbx", "/mnt/usb/x.kdbx"] {
        assert!(
            !LocalFile::looks_cloud_synced(Path::new(ok)),
            "should not flag {ok}"
        );
    }
}

#[test]
fn the_local_backend_round_trips_and_keeps_revisions() {
    use vardo_sync::local::LocalFile;

    let dir = std::env::temp_dir().join("vardo-local-backend-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut backend = LocalFile::new(dir.join("vault.kdbx"));
    assert!(matches!(
        backend.stat(),
        Err(vardo_sync::SyncError::NotFound)
    ));

    let bytes = fixture();
    backend.write(&bytes).unwrap();
    let (read_back, state) = backend.read().unwrap();
    assert_eq!(read_back, bytes);

    let mut changed = bytes.clone();
    changed.extend_from_slice(b"\x00");
    backend.write(&changed).unwrap();

    let after = backend.stat().unwrap();
    assert_ne!(after.version, state.version, "version must track content");
    assert_eq!(backend.revisions().unwrap().len(), 1);

    let id = backend.revisions().unwrap()[0].id.clone();
    assert_eq!(backend.read_revision(&id).unwrap(), bytes);
}
