//! Refusing to merge two databases that are not the same database.
//!
//! This is the guard against the worst outcome this crate can produce. Point
//! sync at the wrong `.kdbx` and, without it, every entry from one database is
//! added to the other as a perfectly legitimate-looking addition. No conflict
//! rule catches that, because nothing is in conflict — the two histories simply
//! fuse, and there is no automatic way back.
//!
//! Uses real fixture files rather than the synthetic harness, because the
//! property under test is about genuine vault identity.

use vardo_kdbx::{open, Credentials};
use vardo_merge::{plan, plan_ignoring_identity, Refusal};
use vardo_vault::Vault;

fn vault(name: &str) -> Vault {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let doc = open(&bytes, &Credentials::with_password("demopass")).expect("open");
    Vault::open(&doc).expect("unlock")
}

/// Same vault, saved with Argon2d.
fn demo_argon2() -> Vault {
    vault("test_db_kdbx4_with_password_argon2.kdbx")
}

/// The same vault again, saved with Argon2id. Different KDF, different bytes,
/// same database.
fn demo_argon2id() -> Vault {
    vault("test_db_kdbx4_with_password_argon2id.kdbx")
}

/// An unrelated database.
fn unrelated() -> Vault {
    vault("test_db_kdbx41_with_password_aes.kdbx")
}

#[test]
fn merging_two_different_databases_is_refused() {
    let local = demo_argon2id();
    let remote = unrelated();

    match plan(None, &local, &remote) {
        Err(Refusal::DifferentVaults {
            local_root,
            remote_root,
        }) => {
            assert_ne!(local_root, remote_root);
        }
        Err(other) => panic!("wrong refusal: {other:?}"),
        Ok(p) => panic!(
            "merged two unrelated databases instead of refusing: {}",
            p.summary()
        ),
    }
}

#[test]
fn the_same_vault_under_a_different_kdf_is_still_the_same_vault() {
    // The check must key on identity, not on bytes. Re-encrypting a vault
    // changes every byte — including every protected value, by design — so a
    // content-based check would refuse to merge a vault with itself.
    let plan = plan(None, &demo_argon2id(), &demo_argon2())
        .expect("the same database saved with a different KDF must still merge");
    assert!(
        plan.conflicts.is_empty(),
        "same vault should not conflict with itself: {:?}",
        plan.conflicts
    );
}

#[test]
fn a_vault_merges_with_itself() {
    let v = demo_argon2id();
    let plan = plan(None, &v, &v).expect("a vault is the same database as itself");
    assert!(
        plan.is_empty(),
        "expected no changes, got: {}",
        plan.summary()
    );
}

#[test]
fn the_refusal_names_both_databases() {
    // The message has to be actionable. "Merge failed" would leave the user
    // with no way to tell which file they pointed at by mistake.
    let err = plan(None, &demo_argon2id(), &unrelated()).unwrap_err();
    let Refusal::DifferentVaults {
        local_root,
        remote_root,
    } = err
    else {
        panic!("expected DifferentVaults");
    };
    assert_ne!(local_root.to_base64(), remote_root.to_base64());
    assert!(!local_root.to_base64().is_empty());
}

#[test]
fn the_override_exists_for_a_deliberate_import() {
    // Merging a vault someone sent you is legitimate; it just must not be the
    // default, and must not happen without the caller having said so.
    let merged = plan_ignoring_identity(None, &demo_argon2id(), &unrelated());
    assert!(
        !merged.is_empty(),
        "the override should actually merge, otherwise it is not an override"
    );
}

#[test]
fn refusal_renders_as_something_a_person_can_act_on() {
    let err = plan(None, &demo_argon2id(), &unrelated()).unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("different databases"),
        "unhelpful refusal text: {text}"
    );
}
