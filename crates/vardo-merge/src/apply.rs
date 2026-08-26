//! Applying a [`MergePlan`](crate::MergePlan) to a vault.
//!
//! Separate from planning on purpose: [`crate::plan`] is pure and can be shown
//! to the user, and only this module touches anything.
//!
//! # Conflicts are not resolved here by default
//!
//! A conflict means the merge genuinely cannot tell which side is right.
//! [`ConflictPolicy::Duplicate`] is the default because it is the only option
//! that cannot lose a password: both versions survive, one renamed, and the
//! user sorts it out with full information. Picking a side is available, but it
//! is a choice a human makes, not one this code makes on their behalf.

use vardo_kdbx::xml::{Element, Node};
use vardo_kdbx::Result;
use vardo_vault::{Timestamp, Uuid, Vault};

use crate::dom;
use crate::{Change, Conflict, MergePlan, Side};

/// What to do with entries the plan could not decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictPolicy {
    /// Keep both versions; the remote one is added alongside with a new UUID
    /// and a marked title. Never loses data, which is why it is the default.
    #[default]
    Duplicate,
    /// Keep the local version and discard the remote one.
    KeepLocal,
    /// Keep the remote version and discard the local one.
    KeepRemote,
    /// Refuse to apply while any conflict remains.
    Refuse,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    pub entries_added: usize,
    pub entries_updated: usize,
    pub entries_deleted: usize,
    pub entries_moved: usize,
    pub entries_resurrected: usize,
    pub groups_added: usize,
    pub conflicts_duplicated: usize,
    /// Changes the plan described that could no longer be carried out, usually
    /// because an earlier change removed the target. Reported rather than
    /// silently skipped.
    pub skipped: Vec<String>,
}

/// Applies `plan` to `local`, pulling content from `remote`.
///
/// `local` must be an unlocked vault holding plaintext; see
/// [`vardo_vault::Vault`].
pub fn apply(
    plan: &MergePlan,
    local: &mut Vault,
    remote: &Vault,
    policy: ConflictPolicy,
) -> Result<Applied> {
    if policy == ConflictPolicy::Refuse && plan.needs_review() {
        return Err(vardo_kdbx::Error::Malformed {
            what: "merge",
            detail: format!(
                "{} conflicts need resolving before this merge can be applied",
                plan.conflicts.len()
            ),
        });
    }

    let mut applied = Applied::default();
    let remote_root = remote
        .root()
        .map(|g| g.element().clone())
        .unwrap_or_else(|| Element::new("Group"));

    // Groups first: an added entry may need a group that does not exist yet.
    for change in &plan.changes {
        if let Change::GroupAdded { uuid, path, .. } = change {
            if add_group(local, &remote_root, *uuid) {
                applied.groups_added += 1;
            } else {
                applied.skipped.push(format!("add group {path}"));
            }
        }
    }

    for change in &plan.changes {
        match change {
            Change::GroupAdded { .. } => {}

            Change::EntryAdded { uuid, title, .. } => {
                if add_entry(local, remote, &remote_root, *uuid) {
                    applied.entries_added += 1;
                } else {
                    applied.skipped.push(format!("add entry {title:?}"));
                }
            }

            Change::EntryUpdated {
                uuid,
                title,
                winner,
                ..
            } => {
                let ok = match winner {
                    // Remote is newer: its version becomes live and the local
                    // one is preserved in history rather than discarded.
                    Side::Remote => replace_entry(local, &remote_root, *uuid, true),
                    // Local already holds the winner. Keep the remote version
                    // in history so the other device's edit is not lost.
                    Side::Local => archive_remote_version(local, &remote_root, *uuid),
                };
                if ok {
                    applied.entries_updated += 1;
                } else {
                    applied.skipped.push(format!("update entry {title:?}"));
                }
            }

            Change::EntryDeleted { uuid, title, .. } => {
                if delete_entry(local, *uuid, remote.deleted_objects()) {
                    applied.entries_deleted += 1;
                } else {
                    applied.skipped.push(format!("delete entry {title:?}"));
                }
            }

            Change::EntryResurrected { uuid, title, .. } => {
                // Already present locally? Then the local copy is the survivor
                // and there is nothing to restore.
                let present = local
                    .root()
                    .and_then(|r| dom::find_entry(r.element(), *uuid).map(|_| ()))
                    .is_some();
                if present {
                    continue;
                }
                if add_entry(local, remote, &remote_root, *uuid) {
                    applied.entries_resurrected += 1;
                    remove_tombstone(local, *uuid);
                } else {
                    applied.skipped.push(format!("restore entry {title:?}"));
                }
            }

            Change::EntryMoved { uuid, title, .. } => {
                if move_entry(local, &remote_root, *uuid) {
                    applied.entries_moved += 1;
                } else {
                    applied.skipped.push(format!("move entry {title:?}"));
                }
            }

            Change::GroupUpdated { uuid, path, winner } => {
                if *winner == Side::Remote && !update_group(local, &remote_root, *uuid) {
                    applied.skipped.push(format!("update group {path}"));
                }
            }

            Change::GroupDeleted { uuid, path, .. } => {
                let root = local.document_mut().root.child_mut("Root");
                let removed = root
                    .and_then(|r| r.child_mut("Group"))
                    .map(|g| dom::take_group(g, *uuid).is_some())
                    .unwrap_or(false);
                if !removed {
                    applied.skipped.push(format!("delete group {path}"));
                }
            }
        }
    }

    for conflict in &plan.conflicts {
        resolve_conflict(conflict, local, &remote_root, policy, &mut applied);
    }

    Ok(applied)
}

fn resolve_conflict(
    conflict: &Conflict,
    local: &mut Vault,
    remote_root: &Element,
    policy: ConflictPolicy,
    applied: &mut Applied,
) {
    let uuid = match conflict {
        Conflict::ConcurrentEdit { uuid, .. }
        | Conflict::DeleteVersusEdit { uuid, .. }
        | Conflict::MissingTimestamp { uuid, .. }
        | Conflict::ConcurrentMove { uuid, .. } => *uuid,
    };

    match policy {
        // Leave local untouched; the remote version is simply not taken.
        ConflictPolicy::KeepLocal | ConflictPolicy::Refuse => {}
        ConflictPolicy::KeepRemote => {
            replace_entry(local, remote_root, uuid, false);
        }
        ConflictPolicy::Duplicate => {
            if duplicate_remote(local, remote_root, uuid) {
                applied.conflicts_duplicated += 1;
            }
        }
    }
}

fn local_root_mut(local: &mut Vault) -> Option<&mut Element> {
    local
        .document_mut()
        .root
        .child_mut("Root")?
        .child_mut("Group")
}

fn add_group(local: &mut Vault, remote_root: &Element, uuid: Uuid) -> bool {
    let Some(source) = dom::find_group(remote_root, uuid) else {
        return false;
    };
    // Copy without descendants; their own changes place them individually.
    let mut copy = source.clone();
    copy.children
        .retain(|n| !matches!(n, Node::Element(e) if e.name == "Entry" || e.name == "Group"));

    let parent_uuid = dom::parent_of(remote_root, uuid);
    let Some(root) = local_root_mut(local) else {
        return false;
    };
    let target = match parent_uuid {
        Some(p) => dom::group_mut(root, p),
        None => Some(root),
    };
    match target {
        Some(target) => {
            dom::append_child(target, copy);
            true
        }
        None => false,
    }
}

fn add_entry(local: &mut Vault, remote: &Vault, remote_root: &Element, uuid: Uuid) -> bool {
    let Some(source) = dom::find_entry(remote_root, uuid) else {
        return false;
    };
    let copy = source.clone();
    let parent_uuid = remote
        .root()
        .and_then(|r| dom::parent_of(r.element(), uuid));
    insert_into_group(local, parent_uuid, copy)
}

/// Inserts `element` under `parent_uuid`, falling back to the root group.
///
/// Landing an entry in the wrong group is recoverable by the user; dropping it
/// because its group is missing is not.
fn insert_into_group(local: &mut Vault, parent_uuid: Option<Uuid>, element: Element) -> bool {
    let Some(root) = local_root_mut(local) else {
        return false;
    };
    // Resolve to a path first: holding a `&mut` to the target while also
    // needing the root as a fallback is what the borrow checker forbids.
    let target = match parent_uuid {
        Some(p) => match dom::group_mut(root, p) {
            Some(g) => g,
            None => root,
        },
        None => root,
    };
    dom::append_child(target, element);
    true
}

/// Replaces the local entry's content with the remote version.
fn replace_entry(
    local: &mut Vault,
    remote_root: &Element,
    uuid: Uuid,
    archive_local: bool,
) -> bool {
    let Some(source) = dom::find_entry(remote_root, uuid) else {
        return false;
    };
    let mut winner = source.clone();

    let Some(root) = local_root_mut(local) else {
        return false;
    };
    let Some(existing) = dom::entry_mut(root, uuid) else {
        return false;
    };

    // Merge the two history lists so neither device's past is lost, then
    // optionally add the version being displaced.
    let mut history = collect_history(existing);
    history.extend(collect_history(&winner));
    if archive_local {
        history.push(strip_history(existing.clone()));
    }
    dedupe_history(&mut history);

    set_history(&mut winner, history);
    *existing = winner;
    true
}

/// Local won, but the remote's version still deserves to be recorded.
fn archive_remote_version(local: &mut Vault, remote_root: &Element, uuid: Uuid) -> bool {
    let Some(source) = dom::find_entry(remote_root, uuid) else {
        return false;
    };
    let remote_version = strip_history(source.clone());
    let remote_history = collect_history(source);

    let Some(root) = local_root_mut(local) else {
        return false;
    };
    let Some(existing) = dom::entry_mut(root, uuid) else {
        return false;
    };

    let mut history = collect_history(existing);
    history.extend(remote_history);
    history.push(remote_version);
    dedupe_history(&mut history);
    set_history(existing, history);
    true
}

fn delete_entry(
    local: &mut Vault,
    uuid: Uuid,
    remote_tombstones: Vec<vardo_vault::DeletedObject>,
) -> bool {
    let Some(root) = local_root_mut(local) else {
        return false;
    };
    if dom::take_entry(root, uuid).is_none() {
        return false;
    }
    let when = remote_tombstones
        .into_iter()
        .find(|t| t.uuid == uuid)
        .and_then(|t| t.deleted_at);
    add_tombstone(local, uuid, when);
    true
}

fn move_entry(local: &mut Vault, remote_root: &Element, uuid: Uuid) -> bool {
    let Some(target_parent) = dom::parent_of(remote_root, uuid) else {
        return false;
    };
    let Some(root) = local_root_mut(local) else {
        return false;
    };
    let Some(node) = dom::take_entry(root, uuid) else {
        return false;
    };
    let Node::Element(element) = node else {
        return false;
    };
    // `insert_into_group` falls back to the root group, so a missing target
    // relocates the entry rather than losing it.
    insert_into_group(local, Some(target_parent), element)
}

fn update_group(local: &mut Vault, remote_root: &Element, uuid: Uuid) -> bool {
    let Some(source) = dom::find_group(remote_root, uuid) else {
        return false;
    };
    let name = source.child("Name").map(|e| e.text());
    let notes = source.child("Notes").map(|e| e.text());

    let Some(root) = local_root_mut(local) else {
        return false;
    };
    let Some(target) = dom::group_mut(root, uuid) else {
        return false;
    };
    if let (Some(name), Some(e)) = (name, target.child_mut("Name")) {
        e.set_text(&name);
    }
    if let (Some(notes), Some(e)) = (notes, target.child_mut("Notes")) {
        e.set_text(&notes);
    }
    true
}

/// Adds the remote version alongside the local one, under a fresh UUID.
///
/// The new UUID matters: reusing the original would make the duplicate look
/// like the same object on the next sync and the two would collapse back
/// together, losing exactly the version this was meant to preserve.
fn duplicate_remote(local: &mut Vault, remote_root: &Element, uuid: Uuid) -> bool {
    let Some(source) = dom::find_entry(remote_root, uuid) else {
        return false;
    };
    let mut copy = strip_history(source.clone());

    let fresh = derive_conflict_uuid(uuid);
    if let Some(e) = copy.child_mut("UUID") {
        e.set_text(&fresh.to_base64());
    }
    for s in copy.children.iter_mut() {
        let Node::Element(s) = s else { continue };
        if s.name != "String" {
            continue;
        }
        if s.child("Key").map(|k| k.text()).as_deref() == Some("Title") {
            if let Some(v) = s.child_mut("Value") {
                let title = v.text();
                v.set_text(&format!("{title} (conflicted copy)"));
            }
        }
    }

    let parent = dom::parent_of(remote_root, uuid);
    insert_into_group(local, parent, copy)
}

/// A deterministic UUID derived from the original.
///
/// Deterministic so that two devices resolving the same conflict independently
/// produce the *same* duplicate rather than two, which would otherwise breed a
/// new copy on every sync.
fn derive_conflict_uuid(original: Uuid) -> Uuid {
    let mut bytes = *original.as_bytes();
    // Flip the variant bits and invert the first byte: distinct from the
    // original, stable, and derived only from it.
    bytes[0] ^= 0xFF;
    bytes[8] ^= 0x40;
    Uuid::from_bytes(bytes)
}

fn collect_history(entry: &Element) -> Vec<Element> {
    entry
        .child("History")
        .map(|h| h.children_named("Entry").cloned().collect())
        .unwrap_or_default()
}

/// History entries never nest, so a version being archived must lose its own.
fn strip_history(mut entry: Element) -> Element {
    entry
        .children
        .retain(|n| !matches!(n, Node::Element(e) if e.name == "History"));
    entry
}

fn dedupe_history(history: &mut Vec<Element>) {
    let key = |e: &Element| {
        e.child("Times")
            .and_then(|t| t.child("LastModificationTime"))
            .map(|t| t.text())
            .unwrap_or_default()
    };
    history.sort_by_key(|e| {
        Timestamp::parse(&key(e))
            .map(|t| t.unix_seconds())
            .unwrap_or(i64::MIN)
    });
    history.dedup_by(|a, b| key(a) == key(b) && a.text() == b.text());
}

fn set_history(entry: &mut Element, history: Vec<Element>) {
    entry
        .children
        .retain(|n| !matches!(n, Node::Element(e) if e.name == "History"));
    if history.is_empty() {
        return;
    }
    let mut block = Element::new("History");
    block.self_closing = false;
    for h in history {
        dom::append_child(&mut block, h);
    }
    dom::append_child(entry, block);
}

fn add_tombstone(local: &mut Vault, uuid: Uuid, when: Option<Timestamp>) {
    let mut object = Element::new("DeletedObject");
    object.self_closing = false;
    let mut id = Element::new("UUID");
    id.set_text(&uuid.to_base64());
    dom::append_child(&mut object, id);
    if let Some(when) = when {
        let mut t = Element::new("DeletionTime");
        t.set_text(&when.to_text());
        dom::append_child(&mut object, t);
    }

    let Some(root) = local.document_mut().root.child_mut("Root") else {
        return;
    };
    if root.child("DeletedObjects").is_none() {
        let mut list = Element::new("DeletedObjects");
        list.self_closing = false;
        dom::append_child(root, list);
    }
    if let Some(list) = root.child_mut("DeletedObjects") {
        list.self_closing = false;
        dom::append_child(list, object);
    }
}

fn remove_tombstone(local: &mut Vault, uuid: Uuid) {
    let Some(root) = local.document_mut().root.child_mut("Root") else {
        return;
    };
    let Some(list) = root.child_mut("DeletedObjects") else {
        return;
    };
    list.children.retain(|n| match n {
        Node::Element(e) => {
            e.name != "DeletedObject"
                || e.child("UUID")
                    .and_then(|u| Uuid::parse(&u.text()))
                    .is_none_or(|u| u != uuid)
        }
        _ => true,
    });
    dom::tidy(list);
}
