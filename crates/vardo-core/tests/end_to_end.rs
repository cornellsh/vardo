//! The whole loop, without a window.
//!
//! Everything here was previously verified by launching the GUI under Xvfb,
//! clicking with `xdotool`, and reading pixels out of a screenshot. That
//! caught real bugs, but it took minutes per run, could not assert on
//! anything precise, and was never going to run in CI.
//!
//! `LocalFile` syncs a path against itself, so "the other device" is simply
//! another process editing the same file — which is exactly the case a desktop
//! password manager has to survive, and the one KeePassXC handles by losing
//! data.

use std::path::{Path, PathBuf};

use vardo_core::model::FieldEdit;
use vardo_core::Session;

const PW: &str = "demopass";
const FIXTURE: &str = "test_db_kdbx4_with_password_argon2id.kdbx";

fn workspace(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vardo-e2e-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A per-device config directory.
///
/// The sync point is *device* state, not vault state: it records what this
/// device last agreed with the remote about. Two sessions sharing one config
/// directory therefore share one sync point and overwrite each other's, which
/// makes them one device with two windows rather than two devices — and makes
/// convergence tests quietly meaningless, because the second device's sync
/// point tells the first that it has already seen everything.
fn device(dir: &Path, name: &str) -> PathBuf {
    let d = dir.join(format!("device-{name}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn vault_in(dir: &Path) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(FIXTURE);
    let dest = dir.join("vault.kdbx");
    std::fs::copy(src, &dest).unwrap();
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

fn set(session: &mut Session, uuid: &str, key: &str, value: &str) {
    session
        .update_entry(
            uuid,
            &[FieldEdit {
                key: key.into(),
                value: Some(value.into()),
                protected: false,
            }],
        )
        .expect("update");
}

fn field(session: &Session, uuid: &str, key: &str) -> Option<String> {
    session
        .entry(uuid)
        .ok()?
        .fields
        .into_iter()
        .find(|f| f.key == key)
        .and_then(|f| f.value)
}

/// Wait long enough that the next edit gets a distinguishable timestamp.
///
/// KDBX stores whole seconds, and `vardo-merge` treats two edits in the same
/// second as a **conflict** rather than guessing which came first — which is
/// the correct behaviour and the reason this project exists. Tests run fast
/// enough to land inside one second, so any test whose meaning depends on "A
/// then B" has to say so explicitly rather than relying on wall-clock luck.
///
/// Without this the suite is not flaky-because-of-a-bug; it is flaky because
/// it is asking a question the format cannot answer.
fn pause_for_timestamp_resolution() {
    std::thread::sleep(std::time::Duration::from_millis(1100));
}

fn titles(session: &Session) -> Vec<String> {
    session
        .list_entries(None, None)
        .into_iter()
        .map(|e| e.title)
        .collect()
}

// --- the loop ---------------------------------------------------------------

#[test]
fn edit_save_reopen_is_lossless() {
    let dir = workspace("basic");
    let path = vault_in(&dir);

    let mut a = open(&path);
    let uuid = a.list_entries(None, None)[0].uuid.clone();
    set(&mut a, &uuid, "UserName", "first-edit");
    a.save(&lctx()).expect("save");

    let b = open(&path);
    assert_eq!(field(&b, &uuid, "UserName").as_deref(), Some("first-edit"));
}

#[test]
fn a_change_made_by_another_process_is_merged_in() {
    // The exact scenario checked by hand with xdotool and keepassxc-cli.
    let dir = workspace("merge");
    let path = vault_in(&dir);

    // This device opens the vault and starts working.
    let mut here = open(&path);
    let mine = here.list_entries(None, None)[0].uuid.clone();
    set(&mut here, &mine, "UserName", "edited-here");

    // Another process adds an entry to the same file and saves it.
    {
        let mut elsewhere = open(&path);
        let root = elsewhere.groups().uuid;
        let created = elsewhere.create_entry(&root).expect("create");
        set(&mut elsewhere, &created, "Title", "From Elsewhere");
        elsewhere.save(&lctx()).expect("their save");
    }

    // We have not saved yet, so our copy knows nothing about it.
    assert!(!titles(&here).contains(&"From Elsewhere".to_string()));

    let preview = here.sync_preview(&dir, &lctx()).expect("preview");
    assert!(
        preview.changes.iter().any(|c| c.title == "From Elsewhere"),
        "the other process's entry should appear in the plan: {:?}",
        preview.changes.iter().map(|c| &c.title).collect::<Vec<_>>()
    );

    let outcome = here.sync_apply(&dir, &lctx(), "duplicate").expect("apply");
    assert_ne!(outcome.kind, "needs-review");

    // Both changes survive.
    assert!(titles(&here).contains(&"From Elsewhere".to_string()));
    assert_eq!(
        field(&here, &mine, "UserName").as_deref(),
        Some("edited-here")
    );

    // And on disk, which is the only claim that matters.
    let reopened = open(&path);
    assert!(titles(&reopened).contains(&"From Elsewhere".to_string()));
    assert_eq!(
        field(&reopened, &mine, "UserName").as_deref(),
        Some("edited-here")
    );
}

#[test]
fn a_preview_changes_nothing() {
    // The whole reason preview and apply are separate: the user must be able to
    // look without committing.
    let dir = workspace("preview-pure");
    let path = vault_in(&dir);

    let here = open(&path);
    {
        let mut elsewhere = open(&path);
        let root = elsewhere.groups().uuid;
        let created = elsewhere.create_entry(&root).unwrap();
        set(&mut elsewhere, &created, "Title", "Should Not Appear Yet");
        elsewhere.save(&lctx()).unwrap();
    }

    let before = std::fs::read(&path).unwrap();
    let preview = here.sync_preview(&dir, &lctx()).expect("preview");
    assert!(!preview.changes.is_empty());

    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "preview must not write to the remote"
    );
    assert!(
        !titles(&here).contains(&"Should Not Appear Yet".to_string()),
        "preview must not mutate the local vault either"
    );
}

#[test]
fn two_devices_converge_when_each_syncs_before_editing() {
    let dir = workspace("converge");
    let path = vault_in(&dir);
    let uuid = open(&path).list_entries(None, None)[0].uuid.clone();

    let (da, db) = (device(&dir, "a"), device(&dir, "b"));

    let mut a = open(&path);
    set(&mut a, &uuid, "UserName", "from-a");
    a.save(&lctx()).expect("a save");
    a.sync_apply(&da, &lctx(), "duplicate").expect("a sync");

    // B picks up A's change *before* editing, which is the ordinary case: a
    // device syncs on open.
    let mut b = open(&path);
    b.sync_apply(&db, &lctx(), "duplicate").expect("b sync");
    assert_eq!(field(&b, &uuid, "UserName").as_deref(), Some("from-a"));

    pause_for_timestamp_resolution();
    set(&mut b, &uuid, "URL", "https://from-b.invalid");
    b.sync_apply(&db, &lctx(), "duplicate")
        .expect("b sync back");

    // B has both.
    assert_eq!(field(&b, &uuid, "UserName").as_deref(), Some("from-a"));
    assert_eq!(
        field(&b, &uuid, "URL").as_deref(),
        Some("https://from-b.invalid")
    );

    // And A converges on its next sync.
    a.sync_apply(&da, &lctx(), "duplicate").expect("a resync");
    assert_eq!(
        field(&a, &uuid, "URL").as_deref(),
        Some("https://from-b.invalid")
    );
    assert_eq!(field(&a, &uuid, "UserName").as_deref(), Some("from-a"));
}

#[test]
fn editing_a_stale_entry_replaces_the_whole_entry_and_archives_the_loser() {
    // A real property users need to know, found by writing the convergence
    // test the naive way and watching it fail.
    //
    // Merging is **entry-level**, not field-level: the newer version of an
    // entry wins wholesale, including fields the editor never touched. So a
    // device that edits a stale entry reverts the other device's change to a
    // *different* field of that same entry.
    //
    // This is the right trade rather than a bug. Field-level merging would
    // assemble an entry holding a username from one device and a password from
    // another — a credential that never existed anywhere and does not work.
    // KeePassXC resolves this the same way.
    //
    // What makes it survivable is that the loser is archived, not discarded.
    let dir = workspace("stale");
    let path = vault_in(&dir);
    let uuid = open(&path).list_entries(None, None)[0].uuid.clone();

    let (da, db) = (device(&dir, "a"), device(&dir, "b"));

    // Both devices open the vault at the same moment.
    let mut a = open(&path);
    let mut b = open(&path);

    set(&mut a, &uuid, "UserName", "from-a");
    a.save(&lctx()).expect("a save");
    a.sync_apply(&da, &lctx(), "duplicate").expect("a sync");

    // B never saw A's change and now edits a different field of the same
    // entry, strictly later so that B is unambiguously the newer version.
    pause_for_timestamp_resolution();
    set(&mut b, &uuid, "URL", "https://from-b.invalid");
    b.sync_apply(&db, &lctx(), "duplicate").expect("b sync");

    assert_eq!(
        field(&b, &uuid, "URL").as_deref(),
        Some("https://from-b.invalid")
    );
    assert_eq!(
        field(&b, &uuid, "UserName").as_deref(),
        Some(""),
        "B's whole entry wins, so A's username reverts to what B had"
    );

    // Nothing was destroyed: A's version is in the entry's history.
    let detail = b.entry(&uuid).expect("entry");
    assert!(
        detail.history_count > 0,
        "the losing version must be archived, which is the only reason this \
         behaviour is acceptable"
    );
}

#[test]
fn a_deletion_on_one_device_propagates_once_there_is_a_base() {
    // Without a merge base nothing is ever deleted, which is deliberate. Once
    // both sides have synced at least once, a deletion must actually take.
    let dir = workspace("delete");
    let path = vault_in(&dir);

    let (da, db) = (device(&dir, "a"), device(&dir, "b"));

    // Establish a shared base.
    let mut a = open(&path);
    a.sync_apply(&da, &lctx(), "duplicate").expect("first sync");

    let root = a.groups().uuid;
    let doomed = a.create_entry(&root).expect("create");
    set(&mut a, &doomed, "Title", "Doomed");
    a.save(&lctx()).expect("save");
    a.sync_apply(&da, &lctx(), "duplicate")
        .expect("sync the addition");

    // Another device sees it, then deletes it.
    {
        let mut b = open(&path);
        b.sync_apply(&db, &lctx(), "duplicate").expect("b sync");
        assert!(titles(&b).contains(&"Doomed".to_string()));
        pause_for_timestamp_resolution();
        b.delete_entry(&doomed).expect("delete");
        b.save(&lctx()).expect("b save");
        b.sync_apply(&db, &lctx(), "duplicate")
            .expect("b sync the deletion");
    }

    let after = open(&path);
    assert!(
        !titles(&after).contains(&"Doomed".to_string()),
        "a recorded deletion must survive the merge, or deleted passwords come \
         back on the next sync"
    );
}

#[test]
fn syncing_with_nothing_to_do_is_not_an_error() {
    let dir = workspace("noop");
    let path = vault_in(&dir);

    let mut a = open(&path);
    a.sync_apply(&dir, &lctx(), "duplicate")
        .expect("first sync");
    let outcome = a
        .sync_apply(&dir, &lctx(), "duplicate")
        .expect("second sync");
    assert!(
        matches!(outcome.kind.as_str(), "up-to-date" | "pushed" | "merged"),
        "unexpected outcome for a no-op sync: {}",
        outcome.kind
    );
}

#[test]
fn the_sync_point_survives_a_lock_and_reopen() {
    // The base is what makes deletions possible. Losing it across a restart
    // would silently downgrade every later sync to a two-way merge.
    let dir = workspace("point");
    let path = vault_in(&dir);

    {
        let mut a = open(&path);
        a.sync_apply(&dir, &lctx(), "duplicate").expect("sync");
        a.lock();
    }

    let b = open(&path);
    let preview = b.sync_preview(&dir, &lctx()).expect("preview");
    assert!(
        !preview.first_sync,
        "a stored sync point must be found again after reopening, otherwise \
         nothing can ever be deleted"
    );
    assert!(
        !preview.warnings.iter().any(|w| w.kind == "no-base"),
        "the stored base should still be readable: {:?}",
        preview.warnings
    );
}

#[test]
fn a_save_after_a_merge_still_verifies() {
    // The merge rewrites the in-memory DOM wholesale. If that produced
    // something that could not be re-encoded, the verify step in save() is the
    // last thing standing between it and the user's only copy.
    let dir = workspace("postmerge");
    let path = vault_in(&dir);

    let mut here = open(&path);
    {
        let mut elsewhere = open(&path);
        let root = elsewhere.groups().uuid;
        let created = elsewhere.create_entry(&root).unwrap();
        set(&mut elsewhere, &created, "Title", "Merged In");
        elsewhere.save(&lctx()).unwrap();
    }

    here.sync_apply(&dir, &lctx(), "duplicate").expect("merge");
    let root = here.groups().uuid;
    let extra = here.create_entry(&root).expect("create after merge");
    set(&mut here, &extra, "Title", "After Merge");
    here.save(&lctx())
        .expect("a post-merge save must verify and land");

    let reopened = open(&path);
    let names = titles(&reopened);
    assert!(names.contains(&"Merged In".to_string()));
    assert!(names.contains(&"After Merge".to_string()));
}

/// A source context with no Drive client.
///
/// Every test here uses a file-backed vault, so the Drive arm must be
/// unreachable rather than merely unused - if a test ever reaches it, that is
/// the bug, not a missing fixture.
fn lctx() -> vardo_core::source::SourceContext {
    vardo_core::source::SourceContext::local_only(std::env::temp_dir())
}
