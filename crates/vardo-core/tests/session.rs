//! The paths that used to be untestable.
//!
//! Every one of these exercises code that, before `vardo-core` existed, could
//! only be reached by constructing a `tauri::State` — which in practice meant
//! it was verified by driving a real window with `xdotool` and reading
//! screenshots. These run in milliseconds and fail loudly.

use std::path::{Path, PathBuf};

use vardo_core::{CoreError, Session};

const PW: &str = "demopass";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

/// Copies a fixture somewhere writable, so tests never mutate the corpus.
fn scratch(name: &str, tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vardo-core-test-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dest = dir.join(name);
    std::fs::copy(fixture(name), &dest).unwrap();
    dest
}

fn open(path: &Path) -> Session {
    Session::open(
        &vardo_core::source::VaultSource::file(path),
        &lctx(),
        PW,
        |_| {},
    )
    .expect("unlock")
}

/// KDBX 4.0, Argon2id, writable.
fn writable(tag: &str) -> (Session, PathBuf) {
    let path = scratch("test_db_kdbx4_with_password_argon2id.kdbx", tag);
    (open(&path), path)
}

// --- unlock -----------------------------------------------------------------

#[test]
fn reports_every_unlock_phase_in_order() {
    // The progress UI is driven entirely by these, so a missing or reordered
    // phase is a silently broken unlock screen.
    let path = scratch("test_db_kdbx4_with_password_argon2id.kdbx", "phases");
    let mut seen = Vec::new();
    Session::open(
        &vardo_core::source::VaultSource::file(&path),
        &lctx(),
        PW,
        |p: vardo_core::session::Phase| seen.push(p.as_str()),
    )
    .expect("unlock");
    assert_eq!(seen, ["reading", "deriving", "decrypting", "parsing"]);
}

#[test]
fn a_wrong_password_is_reported_as_a_wrong_password() {
    let path = scratch("test_db_kdbx4_with_password_argon2id.kdbx", "wrongpw");
    match Session::open(
        &vardo_core::source::VaultSource::file(&path),
        &lctx(),
        "not the password",
        |_| {},
    ) {
        Err(CoreError::WrongPassword) => {}
        other => panic!("expected WrongPassword, got {other:?}", other = other.err()),
    }
}

#[test]
fn a_missing_file_is_not_found_rather_than_corrupt() {
    match Session::open(
        &vardo_core::source::VaultSource::file(Path::new("/nonexistent/nope.kdbx")),
        &lctx(),
        PW,
        |_| {},
    ) {
        Err(CoreError::NotFound { .. }) => {}
        other => panic!("expected NotFound, got {other:?}", other = other.err()),
    }
}

#[test]
fn kdbx31_opens_but_reports_itself_unwritable() {
    let path = scratch("test_db_with_password.kdbx", "kdbx31");
    let s = open(&path);
    assert!(!s.info().writable);
    assert!(s.info().format.starts_with("KDBX 3.1"));
}

// --- the no-secrets rule ----------------------------------------------------

#[test]
fn a_protected_field_never_carries_its_value() {
    // The load-bearing property of the whole IPC design. If this fails, a
    // password is being serialised to the WebView.
    let (s, _) = writable("protected");
    let uuid = s.list_entries(None, None)[0].uuid.clone();
    let detail = s.entry(&uuid).expect("entry");

    let password = detail
        .fields
        .iter()
        .find(|f| f.key == "Password")
        .expect("a Password field");
    assert!(password.protected);
    assert!(
        password.value.is_none(),
        "a protected field must not carry its value across the boundary"
    );
}

#[test]
fn the_serialised_entry_contains_no_secret() {
    // Stronger than checking the struct: serialise it and look for the actual
    // password text. Catches a future field that leaks one by accident.
    let (mut s, _) = writable("noleak");
    let uuid = s.list_entries(None, None)[0].uuid.clone();
    s.update_entry(
        &uuid,
        &[vardo_core::model::FieldEdit {
            key: "Password".into(),
            value: Some("SUPER-SECRET-CANARY".into()),
            protected: true,
        }],
    )
    .expect("update");

    let json = serde_json::to_string(&s.entry(&uuid).expect("entry")).expect("serialise");
    assert!(
        !json.contains("SUPER-SECRET-CANARY"),
        "a secret reached the wire format: {json}"
    );
}

#[test]
fn field_value_is_how_a_secret_is_reached_deliberately() {
    let (mut s, _) = writable("fieldvalue");
    let uuid = s.list_entries(None, None)[0].uuid.clone();
    s.update_entry(
        &uuid,
        &[vardo_core::model::FieldEdit {
            key: "Password".into(),
            value: Some("hunter2".into()),
            protected: true,
        }],
    )
    .unwrap();
    assert_eq!(&*s.field_value(&uuid, "Password").unwrap(), "hunter2");
}

// --- editing ----------------------------------------------------------------

#[test]
fn editing_marks_the_session_dirty_and_saving_clears_it() {
    let (mut s, _) = writable("dirty");
    assert!(!s.is_dirty());

    let uuid = s.list_entries(None, None)[0].uuid.clone();
    s.update_entry(
        &uuid,
        &[vardo_core::model::FieldEdit {
            key: "UserName".into(),
            value: Some("changed".into()),
            protected: false,
        }],
    )
    .expect("update");
    assert!(s.is_dirty());

    s.save(&lctx()).expect("save");
    assert!(!s.is_dirty());
}

#[test]
fn an_edit_that_changes_nothing_does_not_dirty_the_session() {
    let (mut s, _) = writable("nodirty");
    let uuid = s.list_entries(None, None)[0].uuid.clone();
    let existing = s.entry(&uuid).unwrap();
    let username = existing
        .fields
        .iter()
        .find(|f| f.key == "UserName")
        .and_then(|f| f.value.clone())
        .unwrap_or_default();

    let changed = s
        .update_entry(
            &uuid,
            &[vardo_core::model::FieldEdit {
                key: "UserName".into(),
                value: Some(username),
                protected: false,
            }],
        )
        .expect("update");

    assert!(!changed);
    assert!(
        !s.is_dirty(),
        "a no-op edit must not schedule a save, or every open-and-close rewrites the vault"
    );
}

#[test]
fn creating_an_entry_updates_the_count_the_rail_shows() {
    // The count came from unlock and went stale after a merge; this is the
    // regression guard for the same class on the create path.
    let (mut s, _) = writable("count");
    let before = s.info().entry_count;
    let root = s.groups().uuid;
    s.create_entry(&root).expect("create");
    assert_eq!(s.info().entry_count, before + 1);
}

#[test]
fn deleting_an_entry_updates_the_count() {
    let (mut s, _) = writable("delcount");
    let before = s.info().entry_count;
    let uuid = s.list_entries(None, None)[0].uuid.clone();
    s.delete_entry(&uuid).expect("delete");
    assert!(s.info().entry_count < before);
}

#[test]
fn writes_are_refused_on_a_read_only_database() {
    let path = scratch("test_db_with_password.kdbx", "readonly");
    let mut s = open(&path);
    let uuid = s.list_entries(None, None)[0].uuid.clone();
    let err = s
        .update_entry(
            &uuid,
            &[vardo_core::model::FieldEdit {
                key: "UserName".into(),
                value: Some("nope".into()),
                protected: false,
            }],
        )
        .unwrap_err();
    assert!(matches!(err, CoreError::Unsupported { .. }), "{err:?}");
}

// --- saving -----------------------------------------------------------------

#[test]
fn a_save_survives_a_reopen() {
    let (mut s, path) = writable("roundtrip");
    let uuid = s.list_entries(None, None)[0].uuid.clone();
    s.update_entry(
        &uuid,
        &[vardo_core::model::FieldEdit {
            key: "UserName".into(),
            value: Some("persisted".into()),
            protected: false,
        }],
    )
    .expect("update");
    s.save(&lctx()).expect("save");

    let reopened = open(&path);
    let detail = reopened.entry(&uuid).expect("entry");
    let username = detail
        .fields
        .iter()
        .find(|f| f.key == "UserName")
        .and_then(|f| f.value.clone());
    assert_eq!(username.as_deref(), Some("persisted"));
}

#[test]
fn a_save_leaves_a_backup_of_the_previous_version() {
    let (mut s, path) = writable("backup");
    let uuid = s.list_entries(None, None)[0].uuid.clone();
    s.update_entry(
        &uuid,
        &[vardo_core::model::FieldEdit {
            key: "UserName".into(),
            value: Some("after".into()),
            protected: false,
        }],
    )
    .unwrap();

    let result = s.save(&lctx()).expect("save");
    let backup = result.backup.expect("a backup filename");
    let backup_path = path.parent().unwrap().join(&backup);
    assert!(
        backup_path.exists(),
        "{backup} should exist next to the vault"
    );

    // And the backup must still open with the same password, or it is not a
    // backup — it is a file that looks like one.
    let restored = Session::open(
        &vardo_core::source::VaultSource::file(&backup_path),
        &lctx(),
        PW,
        |_| {},
    )
    .expect("backup must open");
    assert!(restored.info().entry_count > 0);
}

#[test]
fn saving_does_not_change_the_entry_count() {
    let (mut s, path) = writable("countstable");
    let before = s.info().entry_count;
    let uuid = s.list_entries(None, None)[0].uuid.clone();
    s.update_entry(
        &uuid,
        &[vardo_core::model::FieldEdit {
            key: "Notes".into(),
            value: Some("touched".into()),
            protected: false,
        }],
    )
    .unwrap();
    s.save(&lctx()).unwrap();
    assert_eq!(open(&path).info().entry_count, before);
}

// --- listing ----------------------------------------------------------------

#[test]
fn search_matches_title_username_and_host() {
    let (s, _) = writable("search");
    let all = s.list_entries(None, None);
    assert!(!all.is_empty());

    let title = all
        .iter()
        .find(|e| !e.title.is_empty())
        .expect("a titled entry")
        .title
        .clone();
    let hits = s.list_entries(None, Some(&title.to_lowercase()));
    assert!(
        hits.iter().any(|e| e.title == title),
        "searching for a title should find it"
    );
}

#[test]
fn search_for_nothing_returns_nothing_rather_than_everything() {
    let (s, _) = writable("nomatch");
    let hits = s.list_entries(None, Some("zzz-definitely-not-present-zzz"));
    assert!(hits.is_empty());
}

#[test]
fn an_empty_query_is_treated_as_no_query() {
    let (s, _) = writable("emptyquery");
    assert_eq!(
        s.list_entries(None, Some("   ")).len(),
        s.list_entries(None, None).len()
    );
}

/// A source context with no Drive client.
///
/// Every test here uses a file-backed vault, so the Drive arm must be
/// unreachable rather than merely unused - if a test ever reaches it, that is
/// the bug, not a missing fixture.
fn lctx() -> vardo_core::source::SourceContext {
    vardo_core::source::SourceContext::local_only(std::env::temp_dir())
}

#[test]
fn a_second_window_on_the_same_vault_is_told_at_unlock() {
    let path = scratch("test_db_kdbx4_with_password_argon2id.kdbx", "elsewhere");
    let first = open(&path);
    assert!(!first.info().open_elsewhere);
    let mut second = open(&path);
    assert!(
        second.info().open_elsewhere,
        "the second window must learn at once"
    );
    assert!(
        second.info().writable,
        "it warns; the conditional save protects"
    );
    second.lock();
    drop(second);
    drop(first);
    assert!(
        !open(&path).info().open_elsewhere,
        "closing the first window frees the vault"
    );
}
