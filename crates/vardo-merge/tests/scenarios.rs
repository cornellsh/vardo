//! The merge decision table, exercised through real encode/decode cycles.

mod harness;

use harness::{Sealed, Variant};
use vardo_merge::{plan, Change, Conflict, Side};

const FIXTURE: &str = "test_db_kdbx41_with_password_aes.kdbx";
const ENTRY: &str = "entry with no quality check";
const OTHER: &str = "entry that was moved";

/// Base, plus two edited variants.
fn scenario(
    edit_local: impl FnOnce(&mut Variant),
    edit_remote: impl FnOnce(&mut Variant),
) -> (Sealed, Sealed, Sealed) {
    let base = Variant::open(FIXTURE);
    let base_sealed = base.seal(1);

    let mut local = Variant::open(FIXTURE);
    edit_local(&mut local);
    let mut remote = Variant::open(FIXTURE);
    edit_remote(&mut remote);

    (base_sealed, local.seal(2), remote.seal(3))
}

// ---------------------------------------------------------------------------
// The property everything else rests on
// ---------------------------------------------------------------------------

#[test]
fn identical_vaults_produce_no_changes() {
    // Two saves of the same data have completely different ciphertext: fresh
    // master seed, fresh IV, fresh inner key, so every protected value differs
    // byte for byte. If merge compared ciphertext it would report every entry
    // as changed on every sync.
    //
    // This is KeePassXC issue #3340 — "conflicting changes on identical
    // databases" — and it is the single most important thing not to get wrong.
    let (base, local, remote) = scenario(|_| {}, |_| {});
    let (bv, lv, rv) = (base.vault(), local.vault(), remote.vault());

    let p = plan(Some(&bv), &lv, &rv);
    assert!(p.is_empty(), "identical vaults produced changes: {:#?}", p);
    assert_eq!(p.summary(), "No changes.");
}

#[test]
fn a_vault_merged_with_itself_is_a_no_op() {
    let sealed = Variant::open(FIXTURE).seal(7);
    let v = sealed.vault();
    assert!(plan(Some(&v), &v, &v).is_empty());
}

// ---------------------------------------------------------------------------
// Edits
// ---------------------------------------------------------------------------

#[test]
fn newer_remote_edit_wins() {
    let (base, local, remote) = scenario(
        |v| v.set_time(ENTRY, "LastModificationTime", 1_000),
        |v| {
            v.set_field(ENTRY, "UserName", "changed-remotely");
            v.set_time(ENTRY, "LastModificationTime", 2_000);
        },
    );
    let (bv, lv, rv) = (base.vault(), local.vault(), remote.vault());
    let p = plan(Some(&bv), &lv, &rv);

    assert!(p.conflicts.is_empty(), "{:#?}", p.conflicts);
    let updated = p
        .changes
        .iter()
        .find(|c| matches!(c, Change::EntryUpdated { .. }))
        .expect("expected an update");
    match updated {
        Change::EntryUpdated { winner, fields, .. } => {
            assert_eq!(*winner, Side::Remote);
            assert_eq!(fields, &["UserName"]);
        }
        _ => unreachable!(),
    }
}

#[test]
fn newer_local_edit_wins() {
    let (base, local, remote) = scenario(
        |v| {
            v.set_field(ENTRY, "UserName", "changed-locally");
            v.set_time(ENTRY, "LastModificationTime", 5_000);
        },
        |v| v.set_time(ENTRY, "LastModificationTime", 1_000),
    );
    let (bv, lv, rv) = (base.vault(), local.vault(), remote.vault());
    let p = plan(Some(&bv), &lv, &rv);

    assert!(p.conflicts.is_empty());
    assert!(p.changes.iter().any(|c| matches!(
        c,
        Change::EntryUpdated {
            winner: Side::Local,
            ..
        }
    )));
}

#[test]
fn same_second_edits_are_a_conflict_not_a_coin_flip() {
    // KDBX stores whole seconds, so this happens for real. Picking a side here
    // silently discards one user's work.
    let (base, local, remote) = scenario(
        |v| {
            v.set_field(ENTRY, "UserName", "alice");
            v.set_time(ENTRY, "LastModificationTime", 4_242);
        },
        |v| {
            v.set_field(ENTRY, "UserName", "bob");
            v.set_time(ENTRY, "LastModificationTime", 4_242);
        },
    );
    let (bv, lv, rv) = (base.vault(), local.vault(), remote.vault());
    let p = plan(Some(&bv), &lv, &rv);

    assert!(
        p.needs_review(),
        "same-second divergent edits must need review"
    );
    assert!(p.conflicts.iter().any(|c| matches!(
        c,
        Conflict::ConcurrentEdit { fields, .. } if fields == &["UserName"]
    )));
    // And crucially: no silent winner was recorded.
    assert!(!p
        .changes
        .iter()
        .any(|c| matches!(c, Change::EntryUpdated { .. })));
}

#[test]
fn same_content_at_the_same_second_is_not_a_conflict() {
    // Both sides made the same edit. There is nothing to resolve.
    let (base, local, remote) = scenario(
        |v| {
            v.set_field(ENTRY, "UserName", "agreed");
            v.set_time(ENTRY, "LastModificationTime", 99);
        },
        |v| {
            v.set_field(ENTRY, "UserName", "agreed");
            v.set_time(ENTRY, "LastModificationTime", 99);
        },
    );
    let (bv, lv, rv) = (base.vault(), local.vault(), remote.vault());
    assert!(plan(Some(&bv), &lv, &rv).is_empty());
}

#[test]
fn field_order_is_not_a_change() {
    // Field order in the XML is a writer's choice, not user data.
    let (base, local, remote) = scenario(
        |_| {},
        |v| {
            let entry = {
                let root = v
                    .dom_mut()
                    .root
                    .child_mut("Root")
                    .unwrap()
                    .child_mut("Group")
                    .unwrap();
                fn find(
                    g: &mut vardo_kdbx::xml::Element,
                ) -> Option<&mut vardo_kdbx::xml::Element> {
                    for c in g.children.iter_mut() {
                        if let vardo_kdbx::xml::Node::Element(e) = c {
                            if e.name == "Entry" {
                                return Some(e);
                            }
                            if e.name == "Group" {
                                if let Some(f) = find(e) {
                                    return Some(f);
                                }
                            }
                        }
                    }
                    None
                }
                find(root).expect("an entry")
            };
            // Reverse the <String> children in place.
            let mut strings: Vec<_> = entry
                .children
                .iter()
                .enumerate()
                .filter(
                    |(_, c)| matches!(c, vardo_kdbx::xml::Node::Element(e) if e.name == "String"),
                )
                .map(|(i, _)| i)
                .collect();
            strings.reverse();
            let taken: Vec<_> = strings.iter().map(|&i| entry.children[i].clone()).collect();
            for (slot, node) in strings.iter().zip(taken) {
                entry.children[*slot] = node;
            }
        },
    );
    let (bv, lv, rv) = (base.vault(), local.vault(), remote.vault());
    assert!(
        plan(Some(&bv), &lv, &rv).is_empty(),
        "reordering fields must not look like an edit"
    );
}

// ---------------------------------------------------------------------------
// Additions
// ---------------------------------------------------------------------------

#[test]
fn an_entry_only_on_the_remote_side_is_added() {
    // Local deleted nothing; remote simply has one we lack because base lacks
    // it too.
    let base = Variant::open(FIXTURE);
    let mut base_edited = Variant::open(FIXTURE);
    base_edited.delete_entry(OTHER, 500);
    let base_sealed = base_edited.seal(1);

    let local = base_edited.seal(2);
    let remote = base.seal(3);

    let (bv, lv, rv) = (base_sealed.vault(), local.vault(), remote.vault());
    let p = plan(Some(&bv), &lv, &rv);

    assert!(
        p.changes.iter().any(|c| matches!(
            c,
            Change::EntryAdded { title, from: Side::Remote, .. } if title == OTHER
        )),
        "{:#?}",
        p
    );
}

// ---------------------------------------------------------------------------
// Deletions
// ---------------------------------------------------------------------------

#[test]
fn a_deletion_newer_than_the_edit_is_applied() {
    let (base, local, remote) = scenario(
        |v| v.set_time(ENTRY, "LastModificationTime", 1_000),
        |v| v.delete_entry(ENTRY, 9_000),
    );
    let (bv, lv, rv) = (base.vault(), local.vault(), remote.vault());
    let p = plan(Some(&bv), &lv, &rv);

    assert!(p.conflicts.is_empty(), "{:#?}", p.conflicts);
    assert!(
        p.changes.iter().any(|c| matches!(
            c,
            Change::EntryDeleted { by: Side::Remote, title, .. } if title == ENTRY
        )),
        "{:#?}",
        p
    );
}

#[test]
fn an_edit_newer_than_the_deletion_resurrects_the_entry() {
    // The user deleted it on one device, then changed their mind and edited it
    // on the other. The later intent wins.
    let (base, local, remote) = scenario(
        |v| {
            v.set_field(ENTRY, "UserName", "still wanted");
            v.set_time(ENTRY, "LastModificationTime", 9_000);
        },
        |v| v.delete_entry(ENTRY, 1_000),
    );
    let (bv, lv, rv) = (base.vault(), local.vault(), remote.vault());
    let p = plan(Some(&bv), &lv, &rv);

    assert!(p.conflicts.is_empty(), "{:#?}", p.conflicts);
    assert!(
        p.changes.iter().any(|c| matches!(
            c,
            Change::EntryResurrected { title, .. } if title == ENTRY
        )),
        "{:#?}",
        p
    );
}

#[test]
fn delete_and_edit_in_the_same_second_is_a_conflict() {
    let (base, local, remote) = scenario(
        |v| {
            v.set_field(ENTRY, "UserName", "edited");
            v.set_time(ENTRY, "LastModificationTime", 7_777);
        },
        |v| v.delete_entry(ENTRY, 7_777),
    );
    let (bv, lv, rv) = (base.vault(), local.vault(), remote.vault());
    let p = plan(Some(&bv), &lv, &rv);

    assert!(
        p.conflicts.iter().any(|c| matches!(
            c,
            Conflict::DeleteVersusEdit {
                deleted_by: Side::Remote,
                ..
            }
        )),
        "{:#?}",
        p
    );
}

#[test]
fn an_untracked_disappearance_keeps_the_entry() {
    // Gone with no tombstone is an absence we cannot explain, not a recorded
    // deletion. Keeping it is recoverable; dropping it is not.
    let (base, local, remote) = scenario(
        |v| v.set_time(ENTRY, "LastModificationTime", 1_000),
        |v| v.delete_entry_untracked(ENTRY),
    );
    let (bv, lv, rv) = (base.vault(), local.vault(), remote.vault());
    let p = plan(Some(&bv), &lv, &rv);

    assert!(
        p.changes.iter().any(|c| matches!(
            c,
            Change::EntryResurrected { title, .. } if title == ENTRY
        )),
        "{:#?}",
        p
    );
    assert!(!p
        .changes
        .iter()
        .any(|c| matches!(c, Change::EntryDeleted { .. })));
}

#[test]
fn without_a_base_nothing_is_ever_deleted() {
    // A first sync cannot distinguish "they added it" from "we deleted it".
    // Taking the union loses a deletion; guessing loses a password.
    let (_, local, remote) = scenario(|_| {}, |v| v.delete_entry(ENTRY, 9_000));
    let (lv, rv) = (local.vault(), remote.vault());
    let p = plan(None, &lv, &rv);

    assert!(
        !p.changes
            .iter()
            .any(|c| matches!(c, Change::EntryDeleted { .. })),
        "two-way merge must not delete: {:#?}",
        p
    );
}

// ---------------------------------------------------------------------------
// Moves
// ---------------------------------------------------------------------------

#[test]
fn a_newer_remote_move_is_applied() {
    let (base, local, remote) = scenario(
        |v| v.set_time(ENTRY, "LocationChanged", 1_000),
        |v| v.move_entry(ENTRY, "Group with tags", 8_000),
    );
    let (bv, lv, rv) = (base.vault(), local.vault(), remote.vault());
    let p = plan(Some(&bv), &lv, &rv);

    assert!(p.changes.iter().any(|c| matches!(
        c,
        Change::EntryMoved { to_path, by: Side::Remote, .. } if to_path.ends_with("Group with tags")
    )), "{:#?}", p);
}

#[test]
fn a_newer_local_move_needs_no_action() {
    let (base, local, remote) = scenario(
        |v| v.move_entry(ENTRY, "Group with tags", 8_000),
        |v| v.set_time(ENTRY, "LocationChanged", 1_000),
    );
    let (bv, lv, rv) = (base.vault(), local.vault(), remote.vault());
    let p = plan(Some(&bv), &lv, &rv);

    assert!(!p
        .changes
        .iter()
        .any(|c| matches!(c, Change::EntryMoved { .. })));
    assert!(p.conflicts.is_empty(), "{:#?}", p.conflicts);
}

#[test]
fn simultaneous_moves_to_different_groups_conflict() {
    let (base, local, remote) = scenario(
        |v| v.move_entry(ENTRY, "Group with tags", 3_000),
        |v| v.move_entry(ENTRY, "Group that was moved", 3_000),
    );
    let (bv, lv, rv) = (base.vault(), local.vault(), remote.vault());
    let p = plan(Some(&bv), &lv, &rv);

    assert!(
        p.conflicts
            .iter()
            .any(|c| matches!(c, Conflict::ConcurrentMove { .. })),
        "{:#?}",
        p
    );
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------

#[test]
fn summary_is_human_readable() {
    let (base, local, remote) = scenario(
        |v| {
            v.set_time(ENTRY, "LastModificationTime", 1_000);
            // The tombstone below must be newer than this entry's last edit,
            // and the fixture's own timestamps are from 2026.
            v.set_time(OTHER, "LastModificationTime", 1_000);
        },
        |v| {
            v.set_field(ENTRY, "UserName", "x");
            v.set_time(ENTRY, "LastModificationTime", 2_000);
            v.set_time(OTHER, "LastModificationTime", 1_000);
            v.delete_entry(OTHER, 9_000);
        },
    );
    let (bv, lv, rv) = (base.vault(), local.vault(), remote.vault());
    let p = plan(Some(&bv), &lv, &rv);

    let summary = p.summary();
    assert!(summary.contains("updated"), "{summary}");
    assert!(summary.contains("deleted"), "{summary}");
}

#[test]
fn merging_is_symmetric_in_what_it_notices() {
    // Swapping the sides must find the same objects, only with the winner
    // reversed. An asymmetry would mean the result depends on which device
    // happened to sync first.
    let (base, local, remote) = scenario(
        |v| {
            v.set_field(ENTRY, "UserName", "left");
            v.set_time(ENTRY, "LastModificationTime", 1_000);
        },
        |v| {
            v.set_field(ENTRY, "UserName", "right");
            v.set_time(ENTRY, "LastModificationTime", 2_000);
        },
    );
    let (bv, lv, rv) = (base.vault(), local.vault(), remote.vault());

    let forward = plan(Some(&bv), &lv, &rv);
    let backward = plan(Some(&bv), &rv, &lv);

    assert_eq!(forward.changes.len(), backward.changes.len());
    assert_eq!(forward.conflicts.len(), backward.conflicts.len());
    match (&forward.changes[0], &backward.changes[0]) {
        (Change::EntryUpdated { winner: a, .. }, Change::EntryUpdated { winner: b, .. }) => {
            assert_ne!(a, b, "the winner must flip when the sides swap")
        }
        other => panic!("unexpected changes: {other:#?}"),
    }
}
