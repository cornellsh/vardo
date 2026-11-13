//! Applying a plan, then verifying the result by reopening the saved file.
//!
//! Every assertion is made against a vault that has been through
//! encode → decode, so a merge that produced an in-memory structure the writer
//! could not round-trip would fail here.

mod harness;

use harness::{Sealed, SeededRng, Variant, PW};
use vardo_kdbx::Credentials;
use vardo_merge::{apply, plan, ConflictPolicy};
use vardo_vault::Vault;

const FIXTURE: &str = "test_db_kdbx41_with_password_aes.kdbx";
const ENTRY: &str = "entry with no quality check";
const OTHER: &str = "entry that was moved";
const ICON: &str = "entry with named custom icon";

/// Applies a plan and returns the merged vault, re-read from bytes.
fn merge(
    base: &Sealed,
    local: &Sealed,
    remote: &Sealed,
    policy: ConflictPolicy,
) -> (Sealed, vardo_merge::Applied) {
    let bv = base.vault();
    let rv = remote.vault();
    let mut lv = local.vault();

    let p = plan(Some(&bv), &lv, &rv)
        .expect("harness vaults all derive from one fixture, so they share a root");
    let applied = apply(&p, &mut lv, &rv, policy).expect("apply");

    let bytes = lv
        .save_with(&Credentials::with_password(PW), &mut SeededRng(99))
        .expect("save merged");
    (Sealed::from_bytes(bytes), applied)
}

fn scenario(
    edit_local: impl FnOnce(&mut Variant),
    edit_remote: impl FnOnce(&mut Variant),
) -> (Sealed, Sealed, Sealed) {
    let base = Variant::open(FIXTURE).seal(1);
    let mut local = Variant::open(FIXTURE);
    edit_local(&mut local);
    let mut remote = Variant::open(FIXTURE);
    edit_remote(&mut remote);
    (base, local.seal(2), remote.seal(3))
}

fn titles(v: &Vault) -> Vec<String> {
    v.entries().iter().map(|e| e.title()).collect()
}

fn password_of(v: &Vault, title: &str) -> Option<String> {
    v.entries()
        .into_iter()
        .find(|e| e.title() == title)
        .and_then(|e| e.password())
        .map(|p| p.expose().to_string())
}

// ---------------------------------------------------------------------------

#[test]
fn applying_an_empty_plan_changes_nothing() {
    let (base, local, remote) = scenario(|_| {}, |_| {});
    let before = titles(&local.vault());
    let (merged, applied) = merge(&base, &local, &remote, ConflictPolicy::Duplicate);

    assert_eq!(applied, vardo_merge::Applied::default());
    assert_eq!(titles(&merged.vault()), before);
}

#[test]
fn a_remote_edit_is_taken_and_the_local_version_is_kept_in_history() {
    let (base, local, remote) = scenario(
        |v| v.set_time(ENTRY, "LastModificationTime", 1_000),
        |v| {
            v.set_field(ENTRY, "UserName", "from-remote");
            v.set_time(ENTRY, "LastModificationTime", 2_000);
        },
    );
    let (merged, applied) = merge(&base, &local, &remote, ConflictPolicy::Duplicate);
    assert_eq!(applied.entries_updated, 1);

    let v = merged.vault();
    let e = v
        .entries()
        .into_iter()
        .find(|e| e.title() == ENTRY)
        .expect("entry survived");
    assert_eq!(e.username(), "from-remote");
    assert!(
        !e.history().is_empty(),
        "the displaced local version must be preserved in history"
    );
}

#[test]
fn passwords_survive_a_merge_intact() {
    // The whole point. Merging must not disturb secrets it was not asked to
    // change — and the plaintext path means an edit no longer shifts keystream
    // positions.
    let (base, local, remote) = scenario(
        |v| v.set_time(ENTRY, "LastModificationTime", 1_000),
        |v| {
            v.set_field(ENTRY, "UserName", "x");
            v.set_time(ENTRY, "LastModificationTime", 2_000);
        },
    );
    let before: Vec<_> = {
        let v = local.vault();
        v.entries()
            .into_iter()
            .map(|e| (e.title(), e.password().map(|p| p.expose().to_string())))
            .collect()
    };

    let (merged, _) = merge(&base, &local, &remote, ConflictPolicy::Duplicate);
    let v = merged.vault();
    for (title, password) in before {
        assert_eq!(
            password_of(&v, &title),
            password,
            "password changed: {title}"
        );
    }
}

#[test]
fn a_remote_addition_appears_locally() {
    // Local deleted OTHER long ago (so it is absent from base and local);
    // remote still has it, which reads as an addition.
    let mut without = Variant::open(FIXTURE);
    without.delete_entry(OTHER, 500);
    let base = without.seal(1);
    let local = without.seal(2);
    let remote = Variant::open(FIXTURE).seal(3);

    let (merged, applied) = merge(&base, &local, &remote, ConflictPolicy::Duplicate);
    assert_eq!(applied.entries_added, 1);
    assert!(titles(&merged.vault()).contains(&OTHER.to_string()));
}

#[test]
fn a_remote_deletion_is_applied_and_tombstoned() {
    let (base, local, remote) = scenario(
        |v| v.set_time(ENTRY, "LastModificationTime", 1_000),
        |v| {
            v.set_time(ENTRY, "LastModificationTime", 1_000);
            v.delete_entry(ENTRY, 9_000);
        },
    );
    let (merged, applied) = merge(&base, &local, &remote, ConflictPolicy::Duplicate);
    assert_eq!(applied.entries_deleted, 1);

    let v = merged.vault();
    assert!(!titles(&v).contains(&ENTRY.to_string()));

    // The tombstone must carry across, or the next sync resurrects the entry.
    let uuid = base
        .vault()
        .entries()
        .into_iter()
        .find(|e| e.title() == ENTRY)
        .and_then(|e| e.uuid())
        .unwrap();
    assert!(
        v.deleted_objects().iter().any(|t| t.uuid == uuid),
        "deletion was applied without recording a tombstone"
    );
}

#[test]
fn a_resurrected_entry_loses_its_tombstone() {
    // Local deleted it; remote edited it afterwards. The entry comes back, and
    // the stale tombstone must go, or the next merge would delete it again.
    let (base, local, remote) = scenario(
        |v| {
            v.set_time(ENTRY, "LastModificationTime", 1_000);
            v.delete_entry(ENTRY, 2_000);
        },
        |v| {
            v.set_field(ENTRY, "UserName", "wanted-again");
            v.set_time(ENTRY, "LastModificationTime", 9_000);
        },
    );
    let (merged, applied) = merge(&base, &local, &remote, ConflictPolicy::Duplicate);
    assert_eq!(applied.entries_resurrected, 1);

    let v = merged.vault();
    let e = v
        .entries()
        .into_iter()
        .find(|e| e.title() == ENTRY)
        .expect("entry restored");
    assert_eq!(e.username(), "wanted-again");

    let uuid = e.uuid().unwrap();
    assert!(
        !v.deleted_objects().iter().any(|t| t.uuid == uuid),
        "a restored entry must not keep its tombstone"
    );
}

#[test]
fn a_remote_move_relocates_the_entry() {
    let (base, local, remote) = scenario(
        |v| v.set_time(ENTRY, "LocationChanged", 1_000),
        |v| v.move_entry(ENTRY, "Group with tags", 8_000),
    );
    let (merged, applied) = merge(&base, &local, &remote, ConflictPolicy::Duplicate);
    assert_eq!(applied.entries_moved, 1);

    let v = merged.vault();
    let group = v
        .root()
        .unwrap()
        .groups()
        .find(|g| g.name() == "Group with tags")
        .unwrap();
    assert!(group.entries().any(|e| e.title() == ENTRY));
}

// ---------------------------------------------------------------------------
// Conflict policies
// ---------------------------------------------------------------------------

fn conflicting() -> (Sealed, Sealed, Sealed) {
    scenario(
        |v| {
            v.set_field(ENTRY, "UserName", "alice");
            v.set_time(ENTRY, "LastModificationTime", 4_242);
        },
        |v| {
            v.set_field(ENTRY, "UserName", "bob");
            v.set_time(ENTRY, "LastModificationTime", 4_242);
        },
    )
}

#[test]
fn duplicate_policy_keeps_both_versions() {
    let (base, local, remote) = conflicting();
    let (merged, applied) = merge(&base, &local, &remote, ConflictPolicy::Duplicate);
    assert_eq!(applied.conflicts_duplicated, 1);

    let v = merged.vault();
    let usernames: Vec<_> = v.entries().iter().map(|e| e.username()).collect();
    assert!(usernames.contains(&"alice".to_string()), "{usernames:?}");
    assert!(usernames.contains(&"bob".to_string()), "{usernames:?}");
    assert!(
        titles(&v).iter().any(|t| t.contains("conflicted copy")),
        "the duplicate must be labelled: {:?}",
        titles(&v)
    );
}

#[test]
fn duplicates_get_a_distinct_but_deterministic_uuid() {
    // Distinct, or the next sync collapses them back together and loses the
    // version this was meant to save. Deterministic, or two devices resolving
    // the same conflict breed a new copy on every sync.
    let (base, local, remote) = conflicting();
    let (a, _) = merge(&base, &local, &remote, ConflictPolicy::Duplicate);
    let (b, _) = merge(&base, &local, &remote, ConflictPolicy::Duplicate);

    let uuids = |s: &Sealed| {
        let mut u: Vec<_> = s
            .vault()
            .entries()
            .iter()
            .filter_map(|e| e.uuid())
            .collect();
        u.sort();
        u
    };
    let (ua, ub) = (uuids(&a), uuids(&b));
    assert_eq!(ua, ub, "duplicate resolution must be deterministic");

    let mut deduped = ua.clone();
    deduped.dedup();
    assert_eq!(
        deduped.len(),
        ua.len(),
        "duplicate produced a colliding UUID"
    );
}

#[test]
fn keep_local_policy_discards_the_remote_version() {
    let (base, local, remote) = conflicting();
    let (merged, applied) = merge(&base, &local, &remote, ConflictPolicy::KeepLocal);
    assert_eq!(applied.conflicts_duplicated, 0);

    let v = merged.vault();
    assert_eq!(
        v.entries()
            .into_iter()
            .find(|e| e.title() == ENTRY)
            .unwrap()
            .username(),
        "alice"
    );
    assert_eq!(v.entries().len(), 4, "nothing should have been added");
}

#[test]
fn keep_remote_policy_takes_the_remote_version() {
    let (base, local, remote) = conflicting();
    let (merged, _) = merge(&base, &local, &remote, ConflictPolicy::KeepRemote);

    let v = merged.vault();
    assert_eq!(
        v.entries()
            .into_iter()
            .find(|e| e.title() == ENTRY)
            .unwrap()
            .username(),
        "bob"
    );
}

/// After every time in the fixture, as a real edit would be.
const NOW: i64 = 2_000_000_000;

fn in_group(v: &Vault, group: &str, title: &str) -> bool {
    v.root()
        .unwrap()
        .groups()
        .find(|g| g.name() == group)
        .is_some_and(|g| g.entries().any(|e| e.title() == title))
}

fn edit(v: &mut Variant, title: &str, value: &str, at: i64) {
    v.set_field(title, "UserName", value);
    v.set_time(title, "LastModificationTime", at);
}

#[test]
fn keep_remote_restores_an_entry_deleted_here_and_edited_there() {
    // Found by tests/properties.rs: the remote edit was silently dropped.
    let (base, local, remote) = scenario(
        |v| v.delete_entry(ENTRY, NOW),
        |v| edit(v, ENTRY, "edited-there", NOW),
    );
    let (merged, applied) = merge(&base, &local, &remote, ConflictPolicy::KeepRemote);
    assert!(applied.skipped.is_empty(), "{:?}", applied.skipped);
    let v = merged.vault();
    let entry = v.entries().into_iter().find(|e| e.title() == ENTRY);
    assert_eq!(entry.map(|e| e.username()).as_deref(), Some("edited-there"));
}

#[test]
fn keep_remote_takes_a_delete_made_there() {
    // Found by tests/properties.rs: the local edit silently survived.
    let (base, local, remote) = scenario(
        |v| edit(v, ENTRY, "edited-here", NOW),
        |v| v.delete_entry(ENTRY, NOW),
    );
    let (merged, applied) = merge(&base, &local, &remote, ConflictPolicy::KeepRemote);
    assert!(applied.skipped.is_empty(), "{:?}", applied.skipped);
    assert!(!titles(&merged.vault()).contains(&ENTRY.to_string()));
}

#[test]
fn keep_remote_follows_a_move_made_there_in_the_same_second() {
    let (base, local, remote) = scenario(
        |v| v.move_entry(ENTRY, "Group that was moved", NOW),
        |v| v.move_entry(ENTRY, "Group with tags", NOW),
    );
    let (merged, _) = merge(&base, &local, &remote, ConflictPolicy::KeepRemote);
    assert!(in_group(&merged.vault(), "Group with tags", ENTRY));
}

#[test]
fn a_local_move_survives_merging_a_remote_edit_twice() {
    // Found by tests/properties.rs: taking the remote content also took its
    // move time, so the second merge moved the entry back.
    let (base, local, remote) = scenario(
        |v| v.move_entry(ICON, "Group that was moved", NOW + 1),
        |v| edit(v, ICON, "edited-there", NOW + 2),
    );
    let (once, _) = merge(&base, &local, &remote, ConflictPolicy::KeepRemote);
    assert!(in_group(&once.vault(), "Group that was moved", ICON));
    let (twice, _) = merge(&base, &once, &remote, ConflictPolicy::KeepRemote);
    assert!(in_group(&twice.vault(), "Group that was moved", ICON));
}

#[test]
fn refuse_policy_declines_to_apply_anything() {
    let (base, local, remote) = conflicting();
    let bv = base.vault();
    let rv = remote.vault();
    let mut lv = local.vault();
    let p = plan(Some(&bv), &lv, &rv)
        .expect("harness vaults all derive from one fixture, so they share a root");

    let err = apply(&p, &mut lv, &rv, ConflictPolicy::Refuse).unwrap_err();
    assert!(err.to_string().contains("conflicts"), "{err}");
}

// ---------------------------------------------------------------------------
// Convergence
// ---------------------------------------------------------------------------

#[test]
fn merging_twice_changes_nothing_the_second_time() {
    // Idempotence. If a merge is not a fixed point, every sync cycle keeps
    // rewriting the vault and two devices never settle.
    let (base, local, remote) = scenario(
        |v| v.set_time(ENTRY, "LastModificationTime", 1_000),
        |v| {
            v.set_field(ENTRY, "UserName", "settled");
            v.set_time(ENTRY, "LastModificationTime", 2_000);
        },
    );
    let (merged, _) = merge(&base, &local, &remote, ConflictPolicy::Duplicate);

    let mv = merged.vault();
    let rv = remote.vault();
    let second = plan(Some(&rv), &mv, &rv)
        .expect("harness vaults all derive from one fixture, so they share a root");
    assert!(
        second.changes.is_empty(),
        "second merge still wants to change things: {:#?}",
        second.changes
    );
}

#[test]
fn both_devices_reach_the_same_content() {
    // Apply the merge on each side and confirm the two vaults agree on every
    // entry. Divergence here means the devices would ping-pong forever.
    let (base, local, remote) = scenario(
        |v| {
            v.set_field(ENTRY, "UserName", "left");
            v.set_time(ENTRY, "LastModificationTime", 1_000);
        },
        |v| {
            v.set_field(OTHER, "UserName", "right");
            v.set_time(OTHER, "LastModificationTime", 2_000);
        },
    );

    let (a, _) = merge(&base, &local, &remote, ConflictPolicy::Duplicate);
    let (b, _) = merge(&base, &remote, &local, ConflictPolicy::Duplicate);

    let describe = |s: &Sealed| {
        let v = s.vault();
        let mut rows: Vec<_> = v
            .entries()
            .into_iter()
            .map(|e| {
                (
                    e.uuid().map(|u| u.to_string()).unwrap_or_default(),
                    e.username(),
                    e.password().map(|p| p.expose().to_string()),
                )
            })
            .collect();
        rows.sort();
        rows
    };
    assert_eq!(describe(&a), describe(&b), "the two devices disagree");
}

#[test]
fn a_merged_vault_still_opens_in_keepassxc() {
    let Ok(out) = std::process::Command::new("keepassxc-cli")
        .arg("--version")
        .output()
    else {
        eprintln!("keepassxc-cli not installed; skipping interop check");
        return;
    };
    assert!(out.status.success());

    let (base, local, remote) = scenario(
        |v| v.set_time(ENTRY, "LastModificationTime", 1_000),
        |v| {
            v.set_field(ENTRY, "UserName", "interop");
            v.set_time(ENTRY, "LastModificationTime", 2_000);
            v.delete_entry(OTHER, 9_000);
        },
    );
    let (merged, _) = merge(&base, &local, &remote, ConflictPolicy::Duplicate);

    let dir = std::env::temp_dir().join("vardo-merge-interop");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("merged.kdbx");
    let bytes = merged
        .vault()
        .save_with(&Credentials::with_password(PW), &mut SeededRng(5))
        .unwrap();
    std::fs::write(&path, &bytes).unwrap();

    let out = std::process::Command::new("keepassxc-cli")
        .args(["ls", "-R", path.to_str().unwrap()])
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
        "keepassxc-cli rejected the merged file: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ---------------------------------------------------------------------------
// Group moves
// ---------------------------------------------------------------------------

const TAGS: &str = "Group with tags";
const MOVED: &str = "Group that was moved";

/// The names of the groups directly under `parent` (the root if `None`).
fn subgroups(v: &Vault, parent: Option<&str>) -> Vec<String> {
    fn find<'a>(g: vardo_vault::Group<'a>, name: &str) -> Option<vardo_vault::Group<'a>> {
        if g.name() == name {
            return Some(g);
        }
        g.groups().find_map(|c| find(c, name))
    }
    let root = v.root().unwrap();
    let group = match parent {
        Some(name) => find(root, name).expect("parent group"),
        None => root,
    };
    group.groups().map(|g| g.name()).collect()
}

#[test]
fn a_remote_group_move_carries_the_group_and_its_contents() {
    let (base, local, remote) = scenario(|_| {}, |v| v.move_group(MOVED, TAGS, NOW + 8));
    let before = titles(&local.vault()).len();
    let (merged, applied) = merge(&base, &local, &remote, ConflictPolicy::Duplicate);
    assert!(applied.skipped.is_empty(), "{:?}", applied.skipped);

    let v = merged.vault();
    assert_eq!(subgroups(&v, None), vec![TAGS.to_string()]);
    assert_eq!(subgroups(&v, Some(TAGS)), vec![MOVED.to_string()]);
    assert_eq!(titles(&v).len(), before, "no entry is lost on the way");
}

#[test]
fn two_groups_moved_into_each_other_do_not_close_a_loop() {
    // Here: MOVED went into TAGS. There, later: TAGS went into MOVED.
    let (base, local, remote) = scenario(
        |v| v.move_group(MOVED, TAGS, NOW + 8),
        |v| v.move_group(TAGS, MOVED, NOW + 9),
    );
    let before = titles(&local.vault()).len();
    let (merged, applied) = merge(&base, &local, &remote, ConflictPolicy::Duplicate);
    assert_eq!(applied.skipped.len(), 1, "the loop is refused, and said so");

    let v = merged.vault();
    assert_eq!(
        subgroups(&v, None),
        vec![TAGS.to_string()],
        "the local tree stands"
    );
    assert_eq!(subgroups(&v, Some(TAGS)), vec![MOVED.to_string()]);
    assert_eq!(titles(&v).len(), before);
}

#[test]
fn group_moves_in_the_same_second_are_a_conflict_the_person_settles() {
    let (base, local, remote) = scenario(
        |v| v.move_group(MOVED, TAGS, NOW + 8),
        |v| v.set_group_time(MOVED, "LocationChanged", NOW + 8),
    );
    let p = plan(Some(&base.vault()), &local.vault(), &remote.vault()).unwrap();
    assert_eq!(p.conflicts.len(), 1, "{:?}", p.conflicts);

    let (kept, _) = merge(&base, &local, &remote, ConflictPolicy::KeepLocal);
    assert_eq!(
        subgroups(&kept.vault(), Some(TAGS)),
        vec![MOVED.to_string()]
    );

    let (taken, applied) = merge(&base, &local, &remote, ConflictPolicy::KeepRemote);
    assert!(applied.skipped.is_empty(), "{:?}", applied.skipped);
    let mut top = subgroups(&taken.vault(), None);
    top.sort();
    assert_eq!(
        top,
        vec![MOVED.to_string(), TAGS.to_string()],
        "back at the root"
    );
}
