//! UUID-keyed index of a vault, with parents and tombstones.
//!
//! Merge matches objects by UUID, never by name or position — a renamed entry
//! is still the same entry, and two entries called "Gmail" in different groups
//! are not.

use std::collections::HashMap;

use vardo_vault::{Entry, Group, Timestamp, Uuid, Vault};
use zeroize::Zeroizing;

/// An object plus where it sits in the tree.
pub struct Located<T> {
    pub item: T,
    /// `None` for the root group only.
    pub parent: Option<Uuid>,
    /// Slash-separated group names, for human-readable plan output.
    pub path: String,
}

pub struct Index<'a> {
    pub entries: HashMap<Uuid, Located<Entry<'a>>>,
    pub groups: HashMap<Uuid, Located<Group<'a>>>,
    /// UUID -> when it was deleted. The value is `None` if the writer recorded
    /// a tombstone without a time, which makes it unorderable against an edit.
    pub tombstones: HashMap<Uuid, Option<Timestamp>>,
    pub recycle_bin: Option<Uuid>,
}

impl<'a> Index<'a> {
    pub fn build(vault: &'a Vault) -> Self {
        let mut index = Self {
            entries: HashMap::new(),
            groups: HashMap::new(),
            tombstones: vault
                .deleted_objects()
                .into_iter()
                .map(|d| (d.uuid, d.deleted_at))
                .collect(),
            recycle_bin: vault.recycle_bin_uuid(),
        };
        if let Some(root) = vault.root() {
            index.walk(root, None, "");
        }
        index
    }

    fn walk(&mut self, group: Group<'a>, parent: Option<Uuid>, prefix: &str) {
        let name = group.name();
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };

        if let Some(uuid) = group.uuid() {
            self.groups.insert(
                uuid,
                Located {
                    item: group,
                    parent,
                    path: path.clone(),
                },
            );
            for entry in group.entries() {
                if let Some(entry_uuid) = entry.uuid() {
                    self.entries.insert(
                        entry_uuid,
                        Located {
                            item: entry,
                            parent: Some(uuid),
                            path: path.clone(),
                        },
                    );
                }
            }
            for child in group.groups() {
                self.walk(child, Some(uuid), &path);
            }
        }
    }

    /// True if `uuid` is inside the recycle bin.
    ///
    /// Structurally a move; to the user, a deletion. Merge has to know the
    /// difference or it will carry "deleted" entries back to the other device
    /// as ordinary moves.
    pub fn is_recycled(&self, uuid: Uuid) -> bool {
        let Some(bin) = self.recycle_bin else {
            return false;
        };
        let mut current = self
            .entries
            .get(&uuid)
            .map(|l| l.parent)
            .or_else(|| self.groups.get(&uuid).map(|l| l.parent))
            .flatten();

        // Bounded by the group count so a malformed parent cycle cannot hang.
        for _ in 0..=self.groups.len() {
            let Some(g) = current else { return false };
            if g == bin {
                return true;
            }
            current = self.groups.get(&g).and_then(|l| l.parent);
        }
        false
    }
}

/// The comparable content of an entry: every field, sorted, with secrets
/// decrypted.
///
/// Sorted because field order in the XML is a writer's choice, not user data;
/// two files with the same fields in a different order are the same entry, and
/// treating them as different would make every sync a conflict.
///
/// Includes decrypted secrets, because protected ciphertext differs on every
/// save by design (see `vardo_kdbx::write`). Comparing ciphertext would report
/// every password as changed, every time.
pub fn content(entry: &Entry<'_>) -> Zeroizing<Vec<(String, String, bool)>> {
    let mut fields: Vec<(String, String, bool)> = entry
        .fields()
        .into_iter()
        .map(|(k, v)| (k, v.expose().to_string(), v.is_protected()))
        .collect();
    fields.sort();
    Zeroizing::new(fields)
}

/// Whether two entries hold the same user-visible content.
pub fn same_content(a: &Entry<'_>, b: &Entry<'_>) -> bool {
    *content(a) == *content(b) && a.tags() == b.tags()
}

/// The fields that differ between two entries, by name.
pub fn changed_fields(a: &Entry<'_>, b: &Entry<'_>) -> Vec<String> {
    let (ca, cb) = (content(a), content(b));
    let lookup = |c: &[(String, String, bool)], key: &str| {
        c.iter()
            .find(|(k, _, _)| k == key)
            .map(|(_, v, _)| v.clone())
    };
    let mut keys: Vec<String> = ca
        .iter()
        .map(|(k, _, _)| k.clone())
        .chain(cb.iter().map(|(k, _, _)| k.clone()))
        .collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter(|k| lookup(&ca, k) != lookup(&cb, k))
        .collect()
}
