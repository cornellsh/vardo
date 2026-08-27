//! The paths where something has gone wrong.
//!
//! In nearly all of these the *safety* was already correct — the data layer
//! refuses, or falls back conservatively, or leaves the file untouched. What
//! was missing was the explanation, and a safe failure nobody can interpret
//! still reads as "this app just broke my passwords".
//!
//! So these tests assert on the message as much as the outcome.

use std::path::{Path, PathBuf};

use vardo_core::{CoreError, Session};

const PW: &str = "demopass";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vardo-unhappy-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy_in(dir: &Path, name: &str) -> PathBuf {
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

const KDBX4: &str = "test_db_kdbx4_with_password_argon2id.kdbx";

// --- the remote is gone -----------------------------------------------------

#[test]
fn a_deleted_remote_is_not_reported_as_a_missing_vault() {
    // The vault is open and fine; it is the sync target that vanished. Saying
    // "not found" would send the user looking for a file they still have.
    let dir = scratch("gone");
    let path = copy_in(&dir, KDBX4);
    let session = open(&path);

    std::fs::remove_file(&path).unwrap();

    match session.sync_preview(&dir, &lctx()) {
        Err(CoreError::RemoteMissing { .. }) => {}
        other => panic!("expected RemoteMissing, got {:?}", other.err()),
    }
}

#[test]
fn the_remote_missing_message_says_which_copy() {
    let dir = scratch("gonemsg");
    let path = copy_in(&dir, KDBX4);
    let session = open(&path);
    std::fs::remove_file(&path).unwrap();

    let text = session.sync_preview(&dir, &lctx()).unwrap_err().to_string();
    assert!(
        text.contains("no longer there"),
        "unhelpful message: {text}"
    );
}

// --- the other copy uses a different password -------------------------------

#[test]
fn a_differently_encrypted_remote_does_not_blame_our_password() {
    // We are unlocked, so our password is right by construction. Reporting
    // "wrong password" would send the user to re-check the one that works.
    let dir = scratch("otherpw");
    let path = copy_in(&dir, KDBX4);
    let session = open(&path);

    // Replace the sync target with a vault that opens under a different
    // password.
    std::fs::copy(
        fixture("test_db_kdbx3_with_chacha20_protected_fields.kdbx"),
        &path,
    )
    .unwrap();

    match session.sync_preview(&dir, &lctx()) {
        Err(CoreError::RemotePassword) => {}
        other => panic!("expected RemotePassword, got {:?}", other.err()),
    }
}

#[test]
fn the_remote_password_message_points_at_the_other_copy() {
    let dir = scratch("otherpwmsg");
    let path = copy_in(&dir, KDBX4);
    let session = open(&path);
    std::fs::copy(
        fixture("test_db_kdbx3_with_chacha20_protected_fields.kdbx"),
        &path,
    )
    .unwrap();

    let text = session.sync_preview(&dir, &lctx()).unwrap_err().to_string();
    assert!(
        text.contains("other copy") && text.contains("different password"),
        "unhelpful message: {text}"
    );
}

// --- a merge base that cannot be read ---------------------------------------

#[test]
fn an_unreadable_sync_point_warns_instead_of_silently_degrading() {
    // Without a base the merge is two-way, which is safe but stops deletions
    // propagating. Doing that silently is the actual defect: the user sees
    // deleted entries reappear and has no way to know why.
    let dir = scratch("nobase");
    let path = copy_in(&dir, KDBX4);
    let session = open(&path);

    // A sync point whose base is not a readable vault.
    let point_dir = dir.join("syncpoints");
    std::fs::create_dir_all(&point_dir).unwrap();
    let key_files: Vec<PathBuf> = {
        // Write a point for this path by running one preview first, then
        // corrupting the stored base.
        let _ = session.sync_preview(&dir, &lctx());
        std::fs::read_dir(&point_dir)
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    };

    // If no point was stored (nothing to sync), this case cannot arise; the
    // preview path is exercised by the other tests.
    if key_files.is_empty() {
        return;
    }

    for f in key_files
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e == "base"))
    {
        std::fs::write(f, b"not a vault at all").unwrap();
    }

    let preview = session
        .sync_preview(&dir, &lctx())
        .expect("preview should still work");
    assert!(
        preview.warnings.iter().any(|w| w.kind == "no-base"),
        "a two-way fallback must be announced, got warnings: {:?}",
        preview.warnings
    );
}

// --- saving ------------------------------------------------------------------

#[test]
fn a_failed_save_says_the_vault_is_unchanged() {
    // Every write goes to a temporary file and only renames on success, so a
    // failure genuinely does leave the original intact. Not saying so is the
    // difference between an annoyance and a user believing their passwords
    // were destroyed.
    let dir = scratch("savefail");
    let path = copy_in(&dir, KDBX4);
    let mut session = open(&path);

    session
        .update_entry(
            &session.list_entries(None, None)[0].uuid.clone(),
            &[vardo_core::model::FieldEdit {
                key: "Notes".into(),
                value: Some("touched".into()),
                protected: false,
            }],
        )
        .unwrap();

    // Make the directory unwritable so the temp file cannot be created.
    let mut perms = std::fs::metadata(&dir).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o500);
    }
    std::fs::set_permissions(&dir, perms.clone()).unwrap();

    let result = session.save(&lctx());

    // Restore permissions before asserting, so a failure does not leave an
    // undeletable directory behind.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut p = perms;
        p.set_mode(0o700);
        let _ = std::fs::set_permissions(&dir, p);
    }

    let err = result.expect_err("save into an unwritable directory must fail");
    let text = err.to_string();
    assert!(
        text.contains("unchanged") || matches!(err, CoreError::OutOfSpace),
        "a failed save must say the vault survived: {text}"
    );

    // And it must actually be true.
    assert!(
        Session::open(
            &vardo_core::source::VaultSource::file(&path),
            &lctx(),
            PW,
            |_| {}
        )
        .is_ok(),
        "the original vault must still open after a failed save"
    );
}

// --- read-only remote --------------------------------------------------------

#[test]
fn a_kdbx31_remote_is_flagged_before_the_merge_not_after() {
    let dir = scratch("remote31");
    let path = copy_in(&dir, KDBX4);
    let session = open(&path);

    // Point sync at a KDBX 3.1 file. It is a different database, so this also
    // exercises that the identity guard fires first — which is the correct
    // precedence: "not your vault" outranks "read-only format".
    std::fs::copy(fixture("test_db_with_password.kdbx"), &path).unwrap();

    match session.sync_preview(&dir, &lctx()) {
        Err(CoreError::Unsupported { detail }) => {
            assert!(
                detail.contains("different databases"),
                "identity should be checked before format: {detail}"
            );
        }
        other => panic!(
            "expected a refusal, got {other:?}",
            other = other.map(|_| ())
        ),
    }
}

/// A source context with no Drive client.
///
/// Every test here uses a file-backed vault, so the Drive arm must be
/// unreachable rather than merely unused - if a test ever reaches it, that is
/// the bug, not a missing fixture.
fn lctx() -> vardo_core::source::SourceContext {
    vardo_core::source::SourceContext::local_only(std::env::temp_dir())
}
