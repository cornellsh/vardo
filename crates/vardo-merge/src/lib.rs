//! Three-way merge for KDBX vaults.
//!
//! This is the product. Everything else exists so that this can be correct.
//!
//! # Why merge at all
//!
//! Google Drive has no compare-and-swap (`fact:drive-no-cas`): `etag` was
//! removed in API v3 and `If-Match` is silently ignored. Two devices can and
//! will write concurrently, and no amount of care in the sync layer prevents
//! it. The only thing standing between a race and lost passwords is a merge
//! that resolves it correctly — and, where it genuinely cannot, says so
//! instead of guessing.
//!
//! # The rule about "undecidable"
//!
//! KDBX timestamps have one-second resolution. Two edits inside the same
//! second are *indistinguishable*, not "simultaneous, so pick the left one".
//! Every comparison here treats equal timestamps as a [`Conflict`] to be
//! surfaced, never as a silent win.
//!
//! This is exactly where KeePassXC goes wrong. Its issue #3340 — "conflicting
//! changes on identical databases", open for years — is this failure mode.
//! Guessing here corrupts vaults quietly, which is worse than refusing.
//!
//! # Deletions
//!
//! A deletion is a tombstone in `<DeletedObjects>` with a `DeletionTime`, plus
//! the object's absence. Without tombstones a deletion is indistinguishable
//! from "the other side has something we lack", and every sync resurrects
//! everything the user has ever deleted.
//!
//! Delete-versus-edit resolves by time: the deletion wins only if it happened
//! strictly after the competing edit. Same second is a conflict.
//!
//! # Dry run first
//!
//! [`plan`] never mutates anything. It returns a description of what a merge
//! *would* do, so the user can see it before it happens. KeePassXC added a
//! merge preview dialog in PR #10173 and it is the right call; here it is the
//! only way in.

#![forbid(unsafe_code)]

pub mod apply;
pub mod dom;
pub mod index;

use std::collections::BTreeSet;

pub use apply::{apply, Applied, ConflictPolicy};

use index::{changed_fields, same_content, Index};
use vardo_vault::{Timestamp, Uuid, Vault};

/// Which side of the merge an object or decision came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Local,
    Remote,
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Side::Local => "local",
            Side::Remote => "remote",
        })
    }
}

/// Something the merge will do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    EntryAdded {
        uuid: Uuid,
        title: String,
        path: String,
        from: Side,
    },
    /// One side's edit is newer, so it becomes the live value. The other
    /// side's version is preserved in the entry's history rather than dropped.
    EntryUpdated {
        uuid: Uuid,
        title: String,
        winner: Side,
        fields: Vec<String>,
    },
    EntryDeleted {
        uuid: Uuid,
        title: String,
        by: Side,
    },
    /// Reappears because it was edited after the other side deleted it.
    EntryResurrected {
        uuid: Uuid,
        title: String,
        edited_by: Side,
    },
    EntryMoved {
        uuid: Uuid,
        title: String,
        to_path: String,
        by: Side,
    },
    GroupAdded {
        uuid: Uuid,
        path: String,
        from: Side,
    },
    GroupUpdated {
        uuid: Uuid,
        path: String,
        winner: Side,
    },
    GroupDeleted {
        uuid: Uuid,
        path: String,
        by: Side,
    },
}

/// Something the merge cannot decide, and will not guess about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conflict {
    /// Both sides changed the same entry, and their modification times are
    /// identical — so there is no basis for choosing. KDBX stores whole
    /// seconds, so this is a real occurrence, not a theoretical one.
    ConcurrentEdit {
        uuid: Uuid,
        title: String,
        fields: Vec<String>,
    },
    /// One side deleted an object in the same second the other side edited it.
    DeleteVersusEdit {
        uuid: Uuid,
        title: String,
        deleted_by: Side,
    },
    /// An edit exists but carries no usable modification time, so it cannot be
    /// ordered against anything.
    MissingTimestamp { uuid: Uuid, title: String },
    /// Both sides moved the same object to different groups at the same time.
    ConcurrentMove {
        uuid: Uuid,
        title: String,
        local_path: String,
        remote_path: String,
    },
}

/// What a merge would do, without having done any of it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MergePlan {
    pub changes: Vec<Change>,
    pub conflicts: Vec<Conflict>,
}

impl MergePlan {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.conflicts.is_empty()
    }

    /// True if anything needs a human before the merge can proceed safely.
    pub fn needs_review(&self) -> bool {
        !self.conflicts.is_empty()
    }

    /// A short human summary, for the confirmation dialog.
    pub fn summary(&self) -> String {
        if self.is_empty() {
            return "No changes.".to_string();
        }
        let mut parts = Vec::new();
        let count = |f: fn(&Change) -> bool| self.changes.iter().filter(|c| f(c)).count();

        let added = count(|c| matches!(c, Change::EntryAdded { .. } | Change::GroupAdded { .. }));
        let updated =
            count(|c| matches!(c, Change::EntryUpdated { .. } | Change::GroupUpdated { .. }));
        let deleted =
            count(|c| matches!(c, Change::EntryDeleted { .. } | Change::GroupDeleted { .. }));
        let moved = count(|c| matches!(c, Change::EntryMoved { .. }));
        let resurrected = count(|c| matches!(c, Change::EntryResurrected { .. }));

        for (n, label) in [
            (added, "added"),
            (updated, "updated"),
            (deleted, "deleted"),
            (moved, "moved"),
            (resurrected, "restored"),
        ] {
            if n > 0 {
                parts.push(format!("{n} {label}"));
            }
        }
        if !self.conflicts.is_empty() {
            parts.push(format!("{} needing review", self.conflicts.len()));
        }
        parts.join(", ")
    }
}

/// Why a merge was refused before it began.
///
/// Distinct from a [`Conflict`], which is a disagreement between two versions
/// of the same thing. A refusal means the two documents are not versions of
/// one thing at all, so there is nothing meaningful to merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Different root groups: two different databases, not two copies of one.
    DifferentVaults { local_root: Uuid, remote_root: Uuid },
    /// A root group UUID is missing, so identity cannot be established.
    /// Refusing is the safe default — a merge that cannot verify what it is
    /// merging is precisely the case this check exists for.
    NoIdentity { local_has: bool, remote_has: bool },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::DifferentVaults { .. } => {
                write!(
                    f,
                    "these are two different databases, not two copies of one"
                )
            }
            Refusal::NoIdentity { .. } => {
                write!(f, "cannot establish whether these are the same database")
            }
        }
    }
}

impl std::error::Error for Refusal {}

/// The root group's UUID — this crate's notion of "which database".
///
/// Stable across re-encryption: the same vault written with Argon2d and with
/// Argon2id keeps the same root UUID, while an unrelated vault differs. That
/// makes it the right identity signal, and the only one KDBX offers.
fn vault_identity(vault: &Vault) -> Option<Uuid> {
    vault.root().and_then(|g| g.uuid())
}

/// Computes what merging `remote` into `local` would do.
///
/// Refuses unless both sides are the same database. Pointing sync at the wrong
/// `.kdbx` would otherwise fuse two unrelated password databases into one with
/// two histories and no automatic way back. No conflict rule would catch it,
/// because every entry from the other side looks like a legitimate addition.
///
/// Use [`plan_ignoring_identity`] for the one legitimate case: deliberately
/// importing a vault from somewhere else.
///
/// `base` is the last state both sides agreed on. Without it only a two-way
/// merge is possible: additions cannot be told apart from deletions, so
/// nothing is ever deleted and the union is taken instead. That is the correct
/// conservative behaviour for a first sync — losing a deletion is recoverable,
/// losing a password is not.
pub fn plan(base: Option<&Vault>, local: &Vault, remote: &Vault) -> Result<MergePlan, Refusal> {
    match (vault_identity(local), vault_identity(remote)) {
        (Some(l), Some(r)) if l == r => {}
        (Some(local_root), Some(remote_root)) => {
            return Err(Refusal::DifferentVaults {
                local_root,
                remote_root,
            })
        }
        (l, r) => {
            return Err(Refusal::NoIdentity {
                local_has: l.is_some(),
                remote_has: r.is_some(),
            })
        }
    }
    Ok(plan_ignoring_identity(base, local, remote))
}

/// As [`plan`], but skipping the same-database check.
///
/// Only for a deliberate import of a foreign vault, where the caller has
/// already told the user what is about to happen.
pub fn plan_ignoring_identity(base: Option<&Vault>, local: &Vault, remote: &Vault) -> MergePlan {
    let local_index = Index::build(local);
    let remote_index = Index::build(remote);
    let base_index = base.map(Index::build);

    let mut plan = MergePlan::default();
    plan_groups(&mut plan, base_index.as_ref(), &local_index, &remote_index);
    plan_entries(&mut plan, base_index.as_ref(), &local_index, &remote_index);
    plan
}

fn plan_entries(
    plan: &mut MergePlan,
    base: Option<&Index<'_>>,
    local: &Index<'_>,
    remote: &Index<'_>,
) {
    let uuids: BTreeSet<Uuid> = local
        .entries
        .keys()
        .chain(remote.entries.keys())
        .copied()
        .collect();

    for uuid in uuids {
        match (local.entries.get(&uuid), remote.entries.get(&uuid)) {
            // Present on both sides: compare content, then times.
            (Some(l), Some(r)) => {
                let title = l.item.title();
                if !same_content(&l.item, &r.item) {
                    resolve_edit(plan, uuid, &title, l, r);
                }
                resolve_move(plan, uuid, &title, l, r);
            }

            // Only local. Either remote deleted it, or we added it.
            (Some(l), None) => {
                let title = l.item.title();
                let existed_in_base = base.is_some_and(|b| b.entries.contains_key(&uuid));
                if !existed_in_base {
                    // Our addition; nothing to do, it is already here.
                    continue;
                }
                resolve_deletion(plan, uuid, &title, Side::Remote, remote, l);
            }

            // Only remote. Either we deleted it, or remote added it.
            (None, Some(r)) => {
                let title = r.item.title();
                let existed_in_base = base.is_some_and(|b| b.entries.contains_key(&uuid));
                if !existed_in_base {
                    plan.changes.push(Change::EntryAdded {
                        uuid,
                        title,
                        path: r.path.clone(),
                        from: Side::Remote,
                    });
                    continue;
                }
                resolve_deletion(plan, uuid, &title, Side::Local, local, r);
            }

            (None, None) => unreachable!("uuid came from one of the two maps"),
        }
    }
}

/// Both sides still have the entry and its content differs.
fn resolve_edit(
    plan: &mut MergePlan,
    uuid: Uuid,
    title: &str,
    l: &index::Located<vardo_vault::Entry<'_>>,
    r: &index::Located<vardo_vault::Entry<'_>>,
) {
    let fields = changed_fields(&l.item, &r.item);
    let lt = l.item.times().last_modified;
    let rt = r.item.times().last_modified;

    match (lt, rt) {
        (Some(lt), Some(rt)) => match lt.cmp_time(rt) {
            std::cmp::Ordering::Greater => plan.changes.push(Change::EntryUpdated {
                uuid,
                title: title.to_string(),
                winner: Side::Local,
                fields,
            }),
            std::cmp::Ordering::Less => plan.changes.push(Change::EntryUpdated {
                uuid,
                title: title.to_string(),
                winner: Side::Remote,
                fields,
            }),
            // Same second, different content. There is no basis for a choice,
            // and inventing one is how vaults get quietly corrupted.
            std::cmp::Ordering::Equal => plan.conflicts.push(Conflict::ConcurrentEdit {
                uuid,
                title: title.to_string(),
                fields,
            }),
        },
        // An edit we cannot place in time cannot be resolved against anything.
        _ => plan.conflicts.push(Conflict::MissingTimestamp {
            uuid,
            title: title.to_string(),
        }),
    }
}

/// One side no longer has the entry; decide whether that is a deletion to
/// apply or an edit that outlived it.
fn resolve_deletion(
    plan: &mut MergePlan,
    uuid: Uuid,
    title: &str,
    deleted_by: Side,
    deleter: &Index<'_>,
    survivor: &index::Located<vardo_vault::Entry<'_>>,
) {
    let edited_at = survivor.item.times().last_modified;

    let Some(tombstone) = deleter.tombstones.get(&uuid) else {
        // Gone with no tombstone. That is not a recorded deletion, it is an
        // absence we cannot explain — possibly a writer that does not keep
        // tombstones. Keeping the entry is the recoverable choice.
        plan.changes.push(Change::EntryResurrected {
            uuid,
            title: title.to_string(),
            edited_by: opposite(deleted_by),
        });
        return;
    };

    match (tombstone, edited_at) {
        (Some(deleted_at), Some(edited_at)) => match deleted_at.cmp_time(edited_at) {
            // Deleted after the last edit: the deletion is the newer intent.
            std::cmp::Ordering::Greater => plan.changes.push(Change::EntryDeleted {
                uuid,
                title: title.to_string(),
                by: deleted_by,
            }),
            // Edited after the deletion: the user brought it back.
            std::cmp::Ordering::Less => plan.changes.push(Change::EntryResurrected {
                uuid,
                title: title.to_string(),
                edited_by: opposite(deleted_by),
            }),
            std::cmp::Ordering::Equal => plan.conflicts.push(Conflict::DeleteVersusEdit {
                uuid,
                title: title.to_string(),
                deleted_by,
            }),
        },
        // Either time is missing, so the two cannot be ordered. Ask.
        _ => plan.conflicts.push(Conflict::DeleteVersusEdit {
            uuid,
            title: title.to_string(),
            deleted_by,
        }),
    }
}

fn resolve_move(
    plan: &mut MergePlan,
    uuid: Uuid,
    title: &str,
    l: &index::Located<vardo_vault::Entry<'_>>,
    r: &index::Located<vardo_vault::Entry<'_>>,
) {
    if l.parent == r.parent {
        return;
    }
    let lt = l.item.times().location_changed;
    let rt = r.item.times().location_changed;
    match (lt, rt) {
        (Some(lt), Some(rt)) => match lt.cmp_time(rt) {
            // Local moved it more recently; nothing to apply.
            std::cmp::Ordering::Greater => {}
            std::cmp::Ordering::Less => plan.changes.push(Change::EntryMoved {
                uuid,
                title: title.to_string(),
                to_path: r.path.clone(),
                by: Side::Remote,
            }),
            std::cmp::Ordering::Equal => plan.conflicts.push(Conflict::ConcurrentMove {
                uuid,
                title: title.to_string(),
                local_path: l.path.clone(),
                remote_path: r.path.clone(),
            }),
        },
        _ => plan.conflicts.push(Conflict::ConcurrentMove {
            uuid,
            title: title.to_string(),
            local_path: l.path.clone(),
            remote_path: r.path.clone(),
        }),
    }
}

fn plan_groups(
    plan: &mut MergePlan,
    base: Option<&Index<'_>>,
    local: &Index<'_>,
    remote: &Index<'_>,
) {
    let uuids: BTreeSet<Uuid> = local
        .groups
        .keys()
        .chain(remote.groups.keys())
        .copied()
        .collect();

    for uuid in uuids {
        match (local.groups.get(&uuid), remote.groups.get(&uuid)) {
            (Some(l), Some(r)) => {
                if l.item.name() != r.item.name() || l.item.notes() != r.item.notes() {
                    let lt = l.item.times().last_modified;
                    let rt = r.item.times().last_modified;
                    if let (Some(lt), Some(rt)) = (lt, rt) {
                        if rt.is_after(lt) {
                            plan.changes.push(Change::GroupUpdated {
                                uuid,
                                path: r.path.clone(),
                                winner: Side::Remote,
                            });
                        } else if lt.is_after(rt) {
                            plan.changes.push(Change::GroupUpdated {
                                uuid,
                                path: l.path.clone(),
                                winner: Side::Local,
                            });
                        } else {
                            plan.conflicts.push(Conflict::ConcurrentEdit {
                                uuid,
                                title: l.item.name(),
                                fields: vec!["Name".into()],
                            });
                        }
                    }
                }
            }
            (Some(_), None) => {
                if base.is_some_and(|b| b.groups.contains_key(&uuid))
                    && remote.tombstones.contains_key(&uuid)
                {
                    plan.changes.push(Change::GroupDeleted {
                        uuid,
                        path: local.groups[&uuid].path.clone(),
                        by: Side::Remote,
                    });
                }
            }
            (None, Some(r)) => {
                if !base.is_some_and(|b| b.groups.contains_key(&uuid)) {
                    plan.changes.push(Change::GroupAdded {
                        uuid,
                        path: r.path.clone(),
                        from: Side::Remote,
                    });
                } else if local.tombstones.contains_key(&uuid) {
                    plan.changes.push(Change::GroupDeleted {
                        uuid,
                        path: r.path.clone(),
                        by: Side::Local,
                    });
                }
            }
            (None, None) => unreachable!("uuid came from one of the two maps"),
        }
    }
}

fn opposite(side: Side) -> Side {
    match side {
        Side::Local => Side::Remote,
        Side::Remote => Side::Local,
    }
}

/// Ordering helper used by callers that want the newest of two optional times.
pub fn newer(a: Option<Timestamp>, b: Option<Timestamp>) -> Option<Timestamp> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if a.is_after(b) { a } else { b }),
        (Some(a), None) => Some(a),
        (None, b) => b,
    }
}
