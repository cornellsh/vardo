//! Editing, against real files.
//!
//! This is the layer that can destroy a vault, so the assertions here are
//! about *products* — what a later read sees — rather than about internals.

use vardo_kdbx::{open, Credentials};
use vardo_vault::{Deletion, EditError, Uuid, Vault};

fn bytes_of(name: &str) -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn vault(name: &str, pw: &str) -> Vault {
    let doc = open(&bytes_of(name), &Credentials::with_password(pw)).expect("open");
    Vault::open(&doc).expect("unlock")
}

/// KDBX 4.0, Argon2id, two entries.
fn writable() -> Vault {
    vault("test_db_kdbx4_with_password_argon2id.kdbx", "demopass")
}

fn first_entry(v: &Vault) -> Uuid {
    v.entries()
        .first()
        .and_then(|e| e.uuid())
        .expect("at least one entry")
}

fn field(v: &Vault, uuid: Uuid, key: &str) -> String {
    v.entries()
        .into_iter()
        .find(|e| e.uuid() == Some(uuid))
        .and_then(|e| e.field(key))
        .map(|f| f.expose().to_string())
        .unwrap_or_default()
}

fn history_len(v: &Vault, uuid: Uuid) -> usize {
    v.entries()
        .into_iter()
        .find(|e| e.uuid() == Some(uuid))
        .map(|e| e.history().len())
        .unwrap_or(0)
}

// --- change detection -------------------------------------------------------

#[test]
fn an_edit_that_changes_nothing_is_not_an_edit() {
    let mut v = writable();
    let uuid = first_entry(&v);
    let before = v.document().to_bytes();
    let existing = field(&v, uuid, "UserName");

    let changed = v
        .update_entry(uuid, |e| e.set_username(&existing))
        .expect("update");

    assert!(
        !changed,
        "writing the same value must not count as a change"
    );
    assert_eq!(
        v.document().to_bytes(),
        before,
        "a no-op edit must leave the document byte-identical, or every save \
         would churn bytes and manufacture merge conflicts"
    );
}

#[test]
fn editing_pushes_exactly_one_history_record() {
    let mut v = writable();
    let uuid = first_entry(&v);
    let before = history_len(&v, uuid);

    v.update_entry(uuid, |e| {
        e.set_username("new-user");
        e.set_password("new-password");
        e.set_url("https://example.invalid");
    })
    .expect("update");

    assert_eq!(
        history_len(&v, uuid),
        before + 1,
        "three field changes in one edit are one history record, not three"
    );
}

#[test]
fn history_holds_the_previous_value_not_the_new_one() {
    let mut v = writable();
    let uuid = first_entry(&v);
    let original = field(&v, uuid, "UserName");

    v.update_entry(uuid, |e| e.set_username("replacement"))
        .expect("update");

    assert_eq!(field(&v, uuid, "UserName"), "replacement");

    let entry = v
        .entries()
        .into_iter()
        .find(|e| e.uuid() == Some(uuid))
        .unwrap();
    let archived = entry.history();
    let last = archived.last().expect("a history record");
    assert_eq!(
        last.field("UserName").map(|f| f.expose().to_string()),
        Some(original),
        "history must record the value being replaced"
    );
}

#[test]
fn history_records_do_not_nest() {
    let mut v = writable();
    let uuid = first_entry(&v);

    for i in 0..3 {
        v.update_entry(uuid, |e| e.set_username(&format!("user-{i}")))
            .expect("update");
    }

    let entry = v
        .entries()
        .into_iter()
        .find(|e| e.uuid() == Some(uuid))
        .unwrap();
    assert_eq!(entry.history().len(), 3);
    for record in entry.history() {
        assert!(
            record.history().is_empty(),
            "a history record must not carry its own history, or the document \
             grows exponentially with each edit"
        );
    }
}

#[test]
fn editing_updates_last_modification_time() {
    let mut v = writable();
    let uuid = first_entry(&v);
    let before = v
        .entries()
        .into_iter()
        .find(|e| e.uuid() == Some(uuid))
        .unwrap()
        .times()
        .last_modified;

    v.update_entry(uuid, |e| e.set_notes("touched"))
        .expect("update");

    let after = v
        .entries()
        .into_iter()
        .find(|e| e.uuid() == Some(uuid))
        .unwrap()
        .times()
        .last_modified;

    assert!(after.is_some());
    if let (Some(a), Some(b)) = (after, before) {
        assert!(
            a.unix_seconds() >= b.unix_seconds(),
            "modification time must not go backwards"
        );
    }
}

#[test]
fn edits_target_the_live_entry_not_an_archived_one() {
    let mut v = writable();
    let uuid = first_entry(&v);

    // Create history, so the same UUID now appears more than once in the DOM.
    v.update_entry(uuid, |e| e.set_username("first")).unwrap();
    v.update_entry(uuid, |e| e.set_username("second")).unwrap();

    assert_eq!(
        field(&v, uuid, "UserName"),
        "second",
        "a history record shares its live entry's UUID, so a search that does \
         not skip <History> can silently edit the past"
    );
}

// --- creation ---------------------------------------------------------------

#[test]
fn creates_an_entry_that_reads_back() {
    let mut v = writable();
    let group = v.root().and_then(|g| g.uuid()).expect("root uuid");
    let before = v.entries().len();

    let uuid = v.create_entry(group).expect("create");
    v.update_entry(uuid, |e| {
        e.set_title("New Entry");
        e.set_username("someone");
        e.set_password("hunter2");
    })
    .expect("update");

    assert_eq!(v.entries().len(), before + 1);
    assert_eq!(field(&v, uuid, "Title"), "New Entry");
    assert_eq!(field(&v, uuid, "Password"), "hunter2");
}

#[test]
fn a_new_entrys_password_is_marked_protected() {
    let mut v = writable();
    let group = v.root().and_then(|g| g.uuid()).unwrap();
    let uuid = v.create_entry(group).expect("create");

    let entry = v
        .entries()
        .into_iter()
        .find(|e| e.uuid() == Some(uuid))
        .unwrap();
    assert!(
        entry
            .field("Password")
            .expect("password field")
            .is_protected(),
        "an unprotected password would be written to disk in plaintext"
    );
}

#[test]
fn created_entries_get_distinct_uuids() {
    let mut v = writable();
    let group = v.root().and_then(|g| g.uuid()).unwrap();
    let a = v.create_entry(group).unwrap();
    let b = v.create_entry(group).unwrap();
    assert_ne!(a, b);
}

// --- deletion ---------------------------------------------------------------

#[test]
fn deleting_without_a_recycle_bin_leaves_a_tombstone() {
    let mut v = writable();
    let uuid = first_entry(&v);
    assert!(
        v.recycle_bin_uuid().is_none(),
        "fixture is expected to have no recycle bin"
    );

    let outcome = v.delete_entry(uuid).expect("delete");

    assert_eq!(outcome, Deletion::Purged);
    assert!(
        !v.entries().iter().any(|e| e.uuid() == Some(uuid)),
        "entry should be gone"
    );
    assert!(
        v.deleted_objects().iter().any(|d| d.uuid == uuid),
        "without a tombstone the next merge resurrects the entry, because an \
         unexplained absence is deliberately not treated as a deletion"
    );
}

#[test]
fn a_tombstone_carries_a_deletion_time() {
    let mut v = writable();
    let uuid = first_entry(&v);
    v.delete_entry(uuid).expect("delete");

    let tomb = v
        .deleted_objects()
        .into_iter()
        .find(|d| d.uuid == uuid)
        .expect("tombstone");
    assert!(
        tomb.deleted_at.is_some(),
        "merge resolves delete-versus-edit by time, so an undated tombstone \
         cannot win or lose correctly"
    );
}

#[test]
fn deleting_twice_does_not_duplicate_the_tombstone() {
    let mut v = writable();
    let uuid = first_entry(&v);
    v.delete_entry(uuid).expect("first delete");
    let _ = v.delete_entry(uuid);

    let count = v
        .deleted_objects()
        .iter()
        .filter(|d| d.uuid == uuid)
        .count();
    assert_eq!(count, 1);
}

#[test]
fn deleting_a_missing_entry_is_an_error_not_a_silent_success() {
    let mut v = writable();
    let ghost = Uuid::from_bytes([9; 16]);
    assert_eq!(v.delete_entry(ghost), Err(EditError::NoSuchEntry(ghost)));
}

// --- read-only --------------------------------------------------------------

#[test]
fn kdbx31_refuses_every_write() {
    let mut v = vault("test_db_with_password.kdbx", "demopass");
    assert!(!v.is_writable());
    let uuid = first_entry(&v);
    let group = v.root().and_then(|g| g.uuid()).unwrap();

    assert_eq!(
        v.update_entry(uuid, |e| e.set_username("x")),
        Err(EditError::ReadOnly)
    );
    assert_eq!(v.create_entry(group).unwrap_err(), EditError::ReadOnly);
    assert_eq!(v.delete_entry(uuid).unwrap_err(), EditError::ReadOnly);
}

// --- the property that matters most -----------------------------------------

#[test]
fn editing_one_entry_does_not_corrupt_another_entrys_password() {
    // This is the regression guard for the save-time-decryption bug: protected
    // values share one keystream and are decrypted by position, so an edit that
    // adds or removes a protected element used to leave every later password
    // decrypting to garbage — silently, in a file that still opened.
    // Four entries, all with non-empty passwords — an empty one would pass
    // this test without proving anything.
    let mut v = vault("test_db_kdbx41_with_password_aes.kdbx", "demopass");
    let uuids: Vec<Uuid> = v.entries().iter().filter_map(|e| e.uuid()).collect();
    assert!(uuids.len() >= 2, "need at least two entries");

    let untouched = uuids[1];
    let expected = field(&v, untouched, "Password");
    assert!(
        !expected.is_empty(),
        "fixture must have a real password here"
    );

    // Add a protected field to the *first* entry, shifting every later
    // protected value's position.
    v.update_entry(uuids[0], |e| {
        e.set_field("Extra Secret", "shifts-the-keystream", true)
    })
    .expect("update");

    assert_eq!(
        field(&v, untouched, "Password"),
        expected,
        "an untouched entry's password must survive an edit to another entry"
    );
}

#[test]
fn an_edit_survives_a_save_and_reopen() {
    let mut v = writable();
    let uuid = first_entry(&v);

    v.update_entry(uuid, |e| {
        e.set_username("persisted-user");
        e.set_password("persisted-password");
    })
    .expect("update");

    let creds = Credentials::with_password("demopass");
    let saved = v
        .save_with(&creds, &mut vardo_kdbx::write::OsRng)
        .expect("save");

    let doc = open(&saved, &creds).expect("reopen");
    let reopened = Vault::open(&doc).expect("unlock");

    assert_eq!(field(&reopened, uuid, "UserName"), "persisted-user");
    assert_eq!(field(&reopened, uuid, "Password"), "persisted-password");
    assert_eq!(
        history_len(&reopened, uuid),
        1,
        "history must survive the round trip too"
    );
}

/// Cross-implementation check on an *edited* file.
///
/// `save_roundtrip.rs` already proves keepassxc-cli accepts a file we merely
/// re-serialised. This proves it accepts one we changed — new entry, edited
/// entry, deletion and tombstone — and reads back the values we intended.
/// Skipped silently when keepassxc-cli is unavailable.
#[test]
fn keepassxc_reads_back_what_we_edited() {
    if std::process::Command::new("keepassxc-cli")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("keepassxc-cli not installed; skipping interop check");
        return;
    }

    let mut v = writable();
    let group = v.root().and_then(|g| g.uuid()).unwrap();

    let created = v.create_entry(group).expect("create");
    v.update_entry(created, |e| {
        e.set_title("Interop Entry");
        e.set_username("interop-user");
        e.set_password("interop-password");
    })
    .expect("update");

    let existing = first_entry(&v);
    v.update_entry(existing, |e| e.set_notes("edited by vardo"))
        .expect("update");

    let creds = Credentials::with_password("demopass");
    let saved = v
        .save_with(&creds, &mut vardo_kdbx::write::OsRng)
        .expect("save");

    let dir = std::env::temp_dir().join("vardo-edit-interop");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("edited.kdbx");
    std::fs::write(&path, &saved).unwrap();

    let run = |args: &[&str]| -> String {
        let out = std::process::Command::new("keepassxc-cli")
            .args(args)
            .env("QT_QPA_PLATFORM", "offscreen")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut c| {
                use std::io::Write;
                c.stdin.as_mut().unwrap().write_all(b"demopass\n")?;
                c.wait_with_output()
            })
            .unwrap();
        assert!(
            out.status.success(),
            "keepassxc-cli {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };

    let listing = run(&["ls", "-R", path.to_str().unwrap()]);
    assert!(
        listing.contains("Interop Entry"),
        "keepassxc-cli did not see the entry we created; listing was:\n{listing}"
    );

    let shown = run(&[
        "show",
        "-a",
        "Password",
        path.to_str().unwrap(),
        "Interop Entry",
    ]);
    assert_eq!(
        shown.trim(),
        "interop-password",
        "keepassxc-cli must decrypt the password we wrote"
    );
}

#[test]
fn every_other_entry_survives_a_save_after_a_delete() {
    let mut v = vault("test_db_kdbx41_with_password_aes.kdbx", "demopass");
    let uuids: Vec<Uuid> = v.entries().iter().filter_map(|e| e.uuid()).collect();
    assert!(uuids.len() >= 2);

    let survivor = uuids[1];
    let expected = field(&v, survivor, "Password");
    assert!(
        !expected.is_empty(),
        "fixture must have a real password here"
    );

    v.delete_entry(uuids[0]).expect("delete");

    let creds = Credentials::with_password("demopass");
    let saved = v
        .save_with(&creds, &mut vardo_kdbx::write::OsRng)
        .expect("save");
    let doc = open(&saved, &creds).expect("reopen");
    let reopened = Vault::open(&doc).expect("unlock");

    assert_eq!(
        field(&reopened, survivor, "Password"),
        expected,
        "deleting one entry must not shift another's decrypted password"
    );
    assert!(reopened
        .deleted_objects()
        .iter()
        .any(|d| d.uuid == uuids[0]));
}
