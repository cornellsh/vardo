//! Saving must not destroy an edit made somewhere else.
//!
//! The scenario is not exotic. Someone opens the vault on a laptop, adds a
//! password on their phone, then saves on the laptop. Nothing about that
//! sequence looks dangerous to the person doing it, and until this test existed
//! the phone's entry was silently gone.

use vardo_core::session::{CoreError, Session};
use vardo_core::source::{SourceContext, VaultSource};

const PW: &str = "demopass";
const FIXTURE: &str = "../../fixtures/test_db_kdbx4_with_password_argon2.kdbx";

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("vardo-lost-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ctx(dir: &std::path::Path) -> SourceContext {
    SourceContext::local_only(dir)
}

fn open(path: &std::path::Path, dir: &std::path::Path) -> Session {
    Session::open(&VaultSource::file(path), &ctx(dir), PW, |_| {}).expect("unlock")
}

#[test]
fn a_save_refuses_to_overwrite_an_edit_made_elsewhere() {
    let dir = scratch("refuse");
    let vault = dir.join("shared.kdbx");
    std::fs::copy(FIXTURE, &vault).unwrap();

    // This device opens the vault and starts editing.
    let mut here = open(&vault, &dir);
    let group = here.groups().uuid.clone();
    let mine = here.create_entry(&group).unwrap();
    here.update_entry(&mine, &[field("Title", "added on the laptop")])
        .unwrap();

    // Meanwhile the other device saves its own change to the same file.
    {
        let mut elsewhere = open(&vault, &dir);
        let g = elsewhere.groups().uuid.clone();
        let theirs = elsewhere.create_entry(&g).unwrap();
        elsewhere
            .update_entry(&theirs, &[field("Title", "added on the phone")])
            .unwrap();
        elsewhere.save(&ctx(&dir)).expect("their save");
    }

    // Saving now must not quietly replace what the other device wrote.
    let result = here.save(&ctx(&dir));

    assert!(
        matches!(result, Err(CoreError::RemoteChanged { .. })),
        "a save that would destroy another device's edit must refuse, got {:?}",
        result.map(|_| "saved")
    );

    // And the refusal must leave the other device's work intact, because a
    // failed save that had already half-written would be worse than no check.
    let after = open(&vault, &dir);
    let titles: Vec<String> = after
        .list_entries(None, None)
        .into_iter()
        .map(|e| e.title)
        .collect();
    assert!(
        titles.iter().any(|t| t == "added on the phone"),
        "the other device's entry must still be there: {titles:?}"
    );
}

#[test]
fn saving_an_untouched_remote_still_works() {
    // The check must not make ordinary saving fail. This is the common path
    // and it has to stay boring.
    let dir = scratch("normal");
    let vault = dir.join("solo.kdbx");
    std::fs::copy(FIXTURE, &vault).unwrap();

    let mut session = open(&vault, &dir);
    let group = session.groups().uuid.clone();
    let uuid = session.create_entry(&group).unwrap();
    session
        .update_entry(&uuid, &[field("Title", "only me")])
        .unwrap();

    session
        .save(&ctx(&dir))
        .expect("an ordinary save must work");

    // And twice in a row, which is where a naive version check breaks: after
    // the first save the remote has moved, and it moved because of us.
    session
        .update_entry(&uuid, &[field("Title", "only me, edited again")])
        .unwrap();
    session
        .save(&ctx(&dir))
        .expect("a second save must also work");
}

fn field(key: &str, value: &str) -> vardo_core::model::FieldEdit {
    vardo_core::model::FieldEdit {
        key: key.to_string(),
        value: Some(value.to_string()),
        protected: false,
    }
}

#[test]
fn saving_after_a_sync_is_not_blocked_by_our_own_upload() {
    // A sync writes to the stored copy, so the version it is compared against
    // moves - and it moved because of us. Leaving that stale made every save
    // after a successful merge refuse, which turns a safety check into a bug
    // that only shows up once someone has done the right thing.
    let dir = scratch("aftersync");
    let vault = dir.join("synced.kdbx");
    std::fs::copy(FIXTURE, &vault).unwrap();

    let mut session = open(&vault, &dir);
    let group = session.groups().uuid.clone();
    let uuid = session.create_entry(&group).unwrap();
    session
        .update_entry(&uuid, &[field("Title", "before sync")])
        .unwrap();

    session
        .sync_apply(
            &dir,
            &ctx(&dir),
            vardo_core::model::ConflictPolicy::Duplicate,
        )
        .expect("sync");

    session
        .update_entry(&uuid, &[field("Title", "after sync")])
        .unwrap();
    session
        .save(&ctx(&dir))
        .expect("a save after a sync must work");
}
