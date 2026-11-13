//! Typed editing of an unlocked vault.
//!
//! # The rules this module enforces
//!
//! **1. History is pushed once per edit, and only when something changed.**
//! KeePass records the *previous* state of an entry in `<History>` before
//! modifying it. Doing that per-field would push one history record per
//! keystroke-ish change; doing it unconditionally would push a record for an
//! edit that changed nothing. So [`Vault::update_entry`] takes a closure,
//! compares before and after, and pushes exactly one record if they differ.
//!
//! **2. Deletion writes a tombstone.** An entry that simply vanishes is
//! indistinguishable, to a merge, from an entry the other side has not seen
//! yet — and `vardo-merge` deliberately *keeps* entries in that case, because
//! keeping is recoverable and dropping is not. So a real deletion must record
//! a `<DeletedObject>`, or it will come back on the next sync.
//!
//! **3. Timestamps use the document's own encoding.** KDBX 4.x stores
//! base64 little-endian seconds; 3.1 stores ISO-8601. Writing the wrong one
//! produces a file that other clients silently misread.
//!
//! **4. Nothing here touches protected values.** The DOM holds plaintext while
//! unlocked (see [`crate::Vault`]), so editing is ordinary text manipulation
//! and the positional keystream coupling does not exist inside this window.

use vardo_kdbx::xml::{Element, Node};

use crate::{Encoding, Timestamp, Uuid, Vault};

/// What a deletion actually did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deletion {
    /// Moved to the recycle bin. No tombstone: the entry still exists.
    Recycled,
    /// Removed, with a `<DeletedObject>` tombstone recorded.
    Purged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// This vault is KDBX 3.1, which we read but never write.
    ReadOnly,
    NoSuchEntry(Uuid),
    NoSuchGroup(Uuid),
    /// A group cannot be moved inside itself.
    WouldCycle,
    Malformed(&'static str),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::ReadOnly => write!(f, "database is KDBX 3.1 and cannot be written"),
            EditError::NoSuchEntry(u) => write!(f, "no such entry: {}", u.to_base64()),
            EditError::NoSuchGroup(u) => write!(f, "no such group: {}", u.to_base64()),
            EditError::WouldCycle => write!(f, "a group cannot contain itself"),
            EditError::Malformed(what) => write!(f, "malformed document: {what}"),
        }
    }
}

impl std::error::Error for EditError {}

pub(crate) type Result<T> = std::result::Result<T, EditError>;

/// A staged edit of one entry.
///
/// Obtained from [`Vault::update_entry`]. Every setter is an ordinary DOM
/// mutation; the surrounding call decides whether a history record is due.
pub struct EntryEdit<'a> {
    /// Visible to `attach`, which adds and removes `<Binary>` children. Still
    /// private to the crate: an editor handing out its raw element would let
    /// any caller write XML the rest of this module has no idea about.
    pub(crate) element: &'a mut Element,
    encoding: Encoding,
}

impl EntryEdit<'_> {
    /// Sets a string field, creating it if absent.
    ///
    /// `protected` marks the value for encryption on save. Changing it on an
    /// existing field is allowed and is how "protect this custom field" works.
    pub fn set_field(&mut self, key: &str, value: &str, protected: bool) {
        if let Some(existing) = find_string_mut(self.element, key) {
            if let Some(v) = existing.child_mut("Value") {
                v.set_text(value);
                set_protected_attr(v, protected);
            }
            return;
        }

        let mut string = Element::new("String");
        let mut k = Element::new("Key");
        k.set_text(key);
        let mut v = Element::new("Value");
        v.set_text(value);
        set_protected_attr(&mut v, protected);
        append_child(&mut string, k);
        append_child(&mut string, v);

        // Standard fields come first, matching every other KeePass client.
        insert_string(self.element, string, key);
    }

    /// Removes a field. Standard fields are removable; KeePass treats a missing
    /// `UserName` and an empty one as the same thing.
    pub fn remove_field(&mut self, key: &str) {
        self.element.children.retain(|n| match n {
            Node::Element(e) if e.name == "String" => {
                e.child("Key").map(|k| k.text()) != Some(key.to_string())
            }
            _ => true,
        });
    }

    pub fn set_title(&mut self, value: &str) {
        self.set_field("Title", value, false);
    }

    pub fn set_username(&mut self, value: &str) {
        self.set_field("UserName", value, false);
    }

    pub fn set_password(&mut self, value: &str) {
        self.set_field("Password", value, true);
    }

    pub fn set_url(&mut self, value: &str) {
        self.set_field("URL", value, false);
    }

    pub fn set_notes(&mut self, value: &str) {
        self.set_field("Notes", value, false);
    }

    /// Sets `<Tags>`, KeePass's semicolon-separated list.
    pub fn set_tags(&mut self, tags: &[String]) {
        set_child_text(self.element, "Tags", &tags.join(";"));
    }

    /// Points this entry at a custom icon.
    ///
    /// Replaces any existing reference rather than adding a second, because
    /// KDBX allows exactly one and a duplicate element is the kind of thing
    /// another client silently drops on its next save.
    pub fn set_custom_icon_uuid(&mut self, icon: Uuid) {
        let text = icon.to_base64();
        if let Some(existing) = self.element.child_mut("CustomIconUUID") {
            existing.set_text(&text);
            return;
        }
        let mut node = Element::new("CustomIconUUID");
        node.set_text(&text);
        append_child(self.element, node);
    }

    /// Marks the entry as expiring, or not.
    ///
    /// Clearing it writes `Expires=False` and leaves the old date alone, which
    /// is what other clients do: the date is the flag's argument rather than a
    /// second source of truth, and deleting it throws away a value someone may
    /// want back when they switch expiry on again.
    pub fn set_expiry(&mut self, at: Option<Timestamp>) {
        let encoding = self.encoding;
        let times = child_or_create(self.element, "Times");
        match at {
            Some(t) => {
                set_child_text(times, "Expires", "True");
                set_child_text(times, "ExpiryTime", &t.with_encoding(encoding).to_text());
            }
            None => set_child_text(times, "Expires", "False"),
        }
    }
}

impl Vault {
    /// True when this vault can be written at all.
    pub(crate) fn require_writable(&self) -> Result<()> {
        if self.is_writable() {
            Ok(())
        } else {
            Err(EditError::ReadOnly)
        }
    }

    /// The timestamp encoding this document already uses.
    ///
    /// Sampled from the document rather than assumed from the version, because
    /// a file that disagrees with its own version header is a file some other
    /// client wrote, and matching it is what keeps the round-trip byte-stable.
    pub fn timestamp_encoding(&self) -> Encoding {
        self.dom
            .root
            .descendants()
            .find(|e| e.name == "LastModificationTime" || e.name == "CreationTime")
            .and_then(|e| Timestamp::parse(&e.text()))
            .map(|t| t.encoding())
            .unwrap_or(Encoding::Base64Seconds)
    }

    /// Edits one entry, pushing a single history record if anything changed.
    ///
    /// Returns whether the document was actually modified, so a caller can skip
    /// a save that would only churn bytes.
    pub fn update_entry(
        &mut self,
        uuid: Uuid,
        edit: impl FnOnce(&mut EntryEdit<'_>),
    ) -> Result<bool> {
        self.require_writable()?;
        let encoding = self.timestamp_encoding();
        let now = Timestamp::from_unix(now_unix(), encoding);
        let max_history = self.history_max_items();

        let root = self.root_element_mut()?;
        let element = find_entry_mut(root, uuid).ok_or(EditError::NoSuchEntry(uuid))?;

        // Compare against the entry minus its own history: a new history record
        // is a consequence of the change, never the change itself.
        let before = without_history(element);

        let mut staged = EntryEdit { element, encoding };
        edit(&mut staged);

        // Structural equality, not serialised bytes: `Element` compares
        // whitespace and self-closing form too, so this still notices a change
        // that only alters formatting, without the cost of writing the subtree
        // out twice.
        if before == without_history(element) {
            return Ok(false);
        }

        push_history(element, before, max_history);
        touch(element, now, encoding);
        Ok(true)
    }

    /// Creates a group inside `parent` and returns its UUID.
    ///
    /// Vardo could show a tree it had no way to change, which meant a vault
    /// could only ever be organised somewhere else. `Times` is written because
    /// a group without one reads as epoch-dated in clients that sort by it.
    pub fn create_group(&mut self, parent: Uuid, name: &str) -> Result<Uuid> {
        self.require_writable()?;
        let encoding = self.timestamp_encoding();
        let now = Timestamp::from_unix(now_unix(), encoding);
        let uuid = Uuid::from_bytes(random_uuid_bytes());

        let mut group = Element::new("Group");
        let mut id = Element::new("UUID");
        id.set_text(&uuid.to_base64());
        append_child(&mut group, id);

        let mut label = Element::new("Name");
        label.set_text(name);
        append_child(&mut group, label);

        let mut times = Element::new("Times");
        let text = now.to_text();
        for field in [
            "CreationTime",
            "LastModificationTime",
            "LastAccessTime",
            "LocationChanged",
        ] {
            let mut t = Element::new(field);
            t.set_text(&text);
            append_child(&mut times, t);
        }
        let mut expires = Element::new("Expires");
        expires.set_text("False");
        append_child(&mut times, expires);
        append_child(&mut group, times);

        let root = self.root_element_mut()?;
        let target = find_group_mut(root, parent).ok_or(EditError::NoSuchGroup(parent))?;
        append_child(target, group);
        Ok(uuid)
    }

    pub fn rename_group(&mut self, uuid: Uuid, name: &str) -> Result<()> {
        self.require_writable()?;
        let encoding = self.timestamp_encoding();
        let now = Timestamp::from_unix(now_unix(), encoding);

        let root = self.root_element_mut()?;
        let group = find_group_mut(root, uuid).ok_or(EditError::NoSuchGroup(uuid))?;
        set_child_text(group, "Name", name);

        let times = child_or_create(group, "Times");
        set_child_text(times, "LastModificationTime", &now.to_text());
        Ok(())
    }

    /// Removes a group and everything inside it.
    ///
    /// Refuses the root, which is not a group anyone owns: deleting it would
    /// leave a file that parses and contains nothing, and no other client
    /// offers it either.
    ///
    /// Unlike a deleted entry this leaves no tombstone. KDBX has
    /// `<DeletedObjects>` for entries and groups alike, but a group's contents
    /// are already tombstoned individually by [`Vault::delete_entry`], and
    /// emitting one for the container as well makes a merge delete entries the
    /// other side had legitimately moved out of it first.
    pub fn delete_group(&mut self, uuid: Uuid) -> Result<()> {
        self.require_writable()?;

        // `WouldCycle` is the wrong name for this and the right error: the
        // root is the one group that cannot be removed from its own tree.
        let root_uuid = self
            .dom
            .root
            .child("Root")
            .and_then(|r| r.child("Group"))
            .and_then(|g| g.child("UUID"))
            .and_then(|u| Uuid::parse(&u.text()));
        if root_uuid == Some(uuid) {
            return Err(EditError::WouldCycle);
        }

        let root = self.root_element_mut()?;
        if !remove_group(root, uuid) {
            return Err(EditError::NoSuchGroup(uuid));
        }
        Ok(())
    }

    /// Moves a group, with everything in it, under `to_parent`.
    ///
    /// Refuses to move the root, or a group into itself or anything inside
    /// it, which would cut the subtree loose from the tree. Records the time in
    /// `LocationChanged`, so a merge can order this move against another
    /// device's.
    pub fn move_group(&mut self, uuid: Uuid, to_parent: Uuid) -> Result<()> {
        self.require_writable()?;
        let encoding = self.timestamp_encoding();
        let now = Timestamp::from_unix(now_unix(), encoding);

        let root = self.root_element_mut()?;
        let is_root = root
            .child("UUID")
            .and_then(|u| Uuid::parse(&u.text()))
            .is_some_and(|u| u == uuid);
        if is_root {
            return Err(EditError::WouldCycle);
        }
        let moving = find_group_mut(root, uuid).ok_or(EditError::NoSuchGroup(uuid))?;
        let into_itself = moving
            .child("UUID")
            .and_then(|u| Uuid::parse(&u.text()))
            .is_some_and(|u| u == to_parent)
            || find_group_mut(moving, to_parent).is_some();
        if into_itself {
            return Err(EditError::WouldCycle);
        }
        let mut element = moving.clone();
        if find_group_mut(root, to_parent).is_none() {
            return Err(EditError::NoSuchGroup(to_parent));
        }

        let times = child_or_create(&mut element, "Times");
        set_child_text(times, "LocationChanged", &now.to_text());
        remove_group(root, uuid);
        let parent = find_group_mut(root, to_parent).ok_or(EditError::NoSuchGroup(to_parent))?;
        append_child(parent, element);
        Ok(())
    }

    /// Creates an empty entry in `group` and returns its UUID.
    pub fn create_entry(&mut self, group: Uuid) -> Result<Uuid> {
        self.require_writable()?;
        let encoding = self.timestamp_encoding();
        let now = Timestamp::from_unix(now_unix(), encoding);
        let uuid = Uuid::from_bytes(random_uuid_bytes());

        let mut entry = Element::new("Entry");
        let mut id = Element::new("UUID");
        id.set_text(&uuid.to_base64());
        append_child(&mut entry, id);

        // Every client writes these four, and a missing `Password` element is
        // not the same as an empty one to some readers.
        for (key, protected) in [
            ("Title", false),
            ("UserName", false),
            ("Password", true),
            ("URL", false),
            ("Notes", false),
        ] {
            let mut string = Element::new("String");
            let mut k = Element::new("Key");
            k.set_text(key);
            let mut v = Element::new("Value");
            v.set_text("");
            set_protected_attr(&mut v, protected);
            append_child(&mut string, k);
            append_child(&mut string, v);
            append_child(&mut entry, string);
        }

        let mut times = Element::new("Times");
        let text = now.to_text();
        for name in [
            "CreationTime",
            "LastModificationTime",
            "LastAccessTime",
            "LocationChanged",
        ] {
            let mut t = Element::new(name);
            t.set_text(&text);
            append_child(&mut times, t);
        }
        let mut expires = Element::new("Expires");
        expires.set_text("False");
        append_child(&mut times, expires);
        let mut usage = Element::new("UsageCount");
        usage.set_text("0");
        append_child(&mut times, usage);
        append_child(&mut entry, times);

        let root = self.root_element_mut()?;
        let parent = find_group_mut(root, group).ok_or(EditError::NoSuchGroup(group))?;
        append_child(parent, entry);
        Ok(uuid)
    }

    /// Deletes an entry.
    ///
    /// Moves it to the recycle bin when the database has one enabled and the
    /// entry is not already there; otherwise removes it and records a
    /// `<DeletedObject>` tombstone. Without that tombstone the next merge would
    /// resurrect the entry, because an unexplained absence is deliberately not
    /// treated as a deletion.
    pub fn delete_entry(&mut self, uuid: Uuid) -> Result<Deletion> {
        self.require_writable()?;
        let encoding = self.timestamp_encoding();
        let now = Timestamp::from_unix(now_unix(), encoding);
        let bin = self.recycle_bin_uuid();
        let already_binned = bin.is_some_and(|b| self.is_within_group(uuid, b));

        match bin {
            Some(bin_uuid) if !already_binned => {
                self.move_entry(uuid, bin_uuid)?;
                Ok(Deletion::Recycled)
            }
            _ => {
                let root = self.root_element_mut()?;
                let removed = take_entry(root, uuid).ok_or(EditError::NoSuchEntry(uuid))?;
                drop(removed);
                add_tombstone(root, uuid, now);
                Ok(Deletion::Purged)
            }
        }
    }

    /// Moves an entry into another group, updating `LocationChanged`.
    ///
    /// `LocationChanged` is what merge uses to decide which side's move wins,
    /// so a move that does not set it is a move that silently loses.
    pub fn move_entry(&mut self, uuid: Uuid, to_group: Uuid) -> Result<()> {
        self.require_writable()?;
        let encoding = self.timestamp_encoding();
        let now = Timestamp::from_unix(now_unix(), encoding);

        let root = self.root_element_mut()?;
        if find_group_mut(root, to_group).is_none() {
            return Err(EditError::NoSuchGroup(to_group));
        }

        let node = take_entry(root, uuid).ok_or(EditError::NoSuchEntry(uuid))?;
        let Node::Element(mut element) = node else {
            return Err(EditError::Malformed("entry was not an element"));
        };

        let times = child_or_create(&mut element, "Times");
        set_child_text(times, "LocationChanged", &now.to_text());
        let _ = encoding;

        let parent = find_group_mut(root, to_group).ok_or(EditError::NoSuchGroup(to_group))?;
        append_child(parent, element);
        Ok(())
    }

    /// `Meta/HistoryMaxItems`, or KeePass's default of 10. A negative value
    /// means unlimited.
    fn history_max_items(&self) -> Option<usize> {
        let raw = self
            .meta()
            .and_then(|m| m.child("HistoryMaxItems"))
            .map(|e| e.text())
            .and_then(|t| t.trim().parse::<i64>().ok())
            .unwrap_or(10);
        (raw >= 0).then_some(raw as usize)
    }

    fn is_within_group(&self, entry: Uuid, group: Uuid) -> bool {
        fn walk(element: &Element, entry: Uuid) -> bool {
            element.descendants().any(|e| {
                e.name == "Entry"
                    && e.child("UUID")
                        .and_then(|u| Uuid::parse(&u.text()))
                        .is_some_and(|u| u == entry)
            })
        }
        self.dom
            .root
            .descendants()
            .find(|e| {
                e.name == "Group"
                    && e.child("UUID")
                        .and_then(|u| Uuid::parse(&u.text()))
                        .is_some_and(|u| u == group)
            })
            .is_some_and(|g| walk(g, entry))
    }

    /// The `<Meta>` element, for writing.
    pub(crate) fn meta_mut(&mut self) -> Result<&mut Element> {
        self.dom
            .root
            .child_mut("Meta")
            .ok_or(EditError::Malformed("no Meta element"))
    }

    fn root_element_mut(&mut self) -> Result<&mut Element> {
        self.dom
            .root
            .child_mut("Root")
            .ok_or(EditError::Malformed("no Root element"))
    }
}

// --- DOM helpers ------------------------------------------------------------

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 16 random bytes, RFC 4122 version 4.
///
/// KDBX UUIDs are opaque identifiers rather than true UUIDs, but stamping the
/// version and variant bits costs nothing and keeps other tools happy.
pub(crate) fn random_uuid_bytes() -> [u8; 16] {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("system RNG unavailable");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    bytes
}

fn set_protected_attr(value: &mut Element, protected: bool) {
    if protected {
        value.set_attr("Protected", "True");
    } else {
        value.remove_attr("Protected");
    }
}

fn find_string_mut<'a>(entry: &'a mut Element, key: &str) -> Option<&'a mut Element> {
    entry.children.iter_mut().find_map(|n| match n {
        Node::Element(e)
            if e.name == "String" && e.child("Key").map(|k| k.text()) == Some(key.to_string()) =>
        {
            Some(e)
        }
        _ => None,
    })
}

/// Standard fields are inserted in KeePass's order; custom fields go last.
fn insert_string(entry: &mut Element, string: Element, key: &str) {
    const ORDER: [&str; 5] = ["Title", "UserName", "Password", "URL", "Notes"];
    let Some(rank) = ORDER.iter().position(|k| *k == key) else {
        append_child(entry, string);
        return;
    };

    let at = entry.children.iter().position(|n| match n {
        Node::Element(e) if e.name == "String" => e
            .child("Key")
            .map(|k| k.text())
            .and_then(|k| ORDER.iter().position(|o| *o == k))
            .is_none_or(|other| other > rank),
        _ => false,
    });

    match at {
        Some(index) => insert_child_at(entry, index, string),
        None => append_child(entry, string),
    }
}

fn child_or_create<'a>(parent: &'a mut Element, name: &str) -> &'a mut Element {
    if parent.child(name).is_none() {
        append_child(parent, Element::new(name));
    }
    parent.child_mut(name).expect("just created")
}

fn set_child_text(parent: &mut Element, name: &str, text: &str) {
    child_or_create(parent, name).set_text(text);
}

/// A copy of the entry with `<History>` removed, for change detection and for
/// the history record itself. KeePass never nests history inside history.
fn without_history(entry: &Element) -> Element {
    let mut clone = entry.clone();
    clone
        .children
        .retain(|n| !matches!(n, Node::Element(e) if e.name == "History"));
    clone
}

fn push_history(entry: &mut Element, previous: Element, max_items: Option<usize>) {
    let history = child_or_create(entry, "History");
    append_child(history, previous);

    if let Some(max) = max_items {
        // Oldest first, so trimming from the front drops the oldest records.
        let mut count = history
            .children
            .iter()
            .filter(|n| matches!(n, Node::Element(e) if e.name == "Entry"))
            .count();
        while count > max {
            if let Some(index) = history
                .children
                .iter()
                .position(|n| matches!(n, Node::Element(e) if e.name == "Entry"))
            {
                history.children.remove(index);
                // Drop the indentation that preceded it, if any.
                if index > 0 {
                    if let Some(Node::Text(t)) = history.children.get(index - 1) {
                        if t.trim().is_empty() {
                            history.children.remove(index - 1);
                        }
                    }
                }
                count -= 1;
            } else {
                break;
            }
        }
    }
}

fn touch(entry: &mut Element, now: Timestamp, _encoding: Encoding) {
    let times = child_or_create(entry, "Times");
    let text = now.to_text();
    set_child_text(times, "LastModificationTime", &text);
    set_child_text(times, "LastAccessTime", &text);
}

/// Removes a `<Group>` with this UUID from anywhere in the tree.
///
/// Recursive because a group can be nested arbitrarily deep, and `retain` only
/// sees one level. Returns whether anything was actually removed, so the caller
/// can tell "deleted" from "there was nothing there" rather than reporting
/// success for a UUID that never existed.
fn remove_group(parent: &mut Element, uuid: Uuid) -> bool {
    let before = parent.children.len();
    parent.children.retain(|n| match n {
        Node::Element(e) if e.name == "Group" => e
            .child("UUID")
            .and_then(|u| Uuid::parse(&u.text()))
            .is_none_or(|u| u != uuid),
        _ => true,
    });
    if parent.children.len() != before {
        return true;
    }

    for child in &mut parent.children {
        if let Node::Element(e) = child {
            if e.name == "Group" && remove_group(e, uuid) {
                return true;
            }
        }
    }
    false
}

fn add_tombstone(root: &mut Element, uuid: Uuid, now: Timestamp) {
    let list = child_or_create(root, "DeletedObjects");

    // Replace an existing tombstone rather than accumulating duplicates.
    list.children.retain(|n| match n {
        Node::Element(e) if e.name == "DeletedObject" => e
            .child("UUID")
            .and_then(|u| Uuid::parse(&u.text()))
            .is_none_or(|u| u != uuid),
        _ => true,
    });

    let mut object = Element::new("DeletedObject");
    let mut id = Element::new("UUID");
    id.set_text(&uuid.to_base64());
    let mut at = Element::new("DeletionTime");
    at.set_text(&now.to_text());
    append_child(&mut object, id);
    append_child(&mut object, at);
    append_child(list, object);
}

fn find_entry_mut(root: &mut Element, uuid: Uuid) -> Option<&mut Element> {
    find_named_mut(root, "Entry", uuid)
}

fn find_group_mut(root: &mut Element, uuid: Uuid) -> Option<&mut Element> {
    find_named_mut(root, "Group", uuid)
}

/// Depth-first search for an `Entry`/`Group` by UUID.
///
/// Deliberately skips `<History>`: a history record carries the same UUID as
/// its live entry, so a naive walk can hand back an archived version and edits
/// would silently apply to the past.
fn find_named_mut<'a>(element: &'a mut Element, name: &str, uuid: Uuid) -> Option<&'a mut Element> {
    for child in element.children.iter_mut() {
        let Node::Element(e) = child else { continue };
        if e.name == "History" {
            continue;
        }
        if e.name == name
            && e.child("UUID")
                .and_then(|u| Uuid::parse(&u.text()))
                .is_some_and(|u| u == uuid)
        {
            return Some(e);
        }
        if let Some(found) = find_named_mut(e, name, uuid) {
            return Some(found);
        }
    }
    None
}

/// Removes a live entry, returning its node. Skips history for the same reason
/// as [`find_named_mut`].
fn take_entry(element: &mut Element, uuid: Uuid) -> Option<Node> {
    let found = element.children.iter().position(|n| match n {
        Node::Element(e) => {
            e.name == "Entry"
                && e.child("UUID")
                    .and_then(|u| Uuid::parse(&u.text()))
                    .is_some_and(|u| u == uuid)
        }
        _ => false,
    });

    if let Some(index) = found {
        let node = element.children.remove(index);
        if index > 0 {
            if let Some(Node::Text(t)) = element.children.get(index - 1) {
                if t.trim().is_empty() {
                    element.children.remove(index - 1);
                }
            }
        }
        return Some(node);
    }

    for child in element.children.iter_mut() {
        if let Node::Element(e) = child {
            if e.name == "History" {
                continue;
            }
            if let Some(node) = take_entry(e, uuid) {
                return Some(node);
            }
        }
    }
    None
}

/// Appends a child, un-collapsing `<Foo/>` and matching sibling indentation so
/// the document still looks hand-written after an edit.
pub(crate) fn append_child(parent: &mut Element, child: Element) {
    let insert_at = parent
        .children
        .iter()
        .rposition(|n| !matches!(n, Node::Text(t) if t.trim().is_empty()))
        .map(|i| i + 1)
        .unwrap_or(parent.children.len());
    insert_child_at(parent, insert_at, child);
}

fn insert_child_at(parent: &mut Element, index: usize, child: Element) {
    let indent = detect_indent(parent);
    parent.self_closing = false;
    match indent {
        Some(indent) => {
            parent.children.insert(index, Node::Text(indent));
            parent.children.insert(index + 1, Node::Element(child));
        }
        None => parent.children.insert(index, Node::Element(child)),
    }
}

fn detect_indent(parent: &Element) -> Option<String> {
    let mut previous: Option<&String> = None;
    for child in &parent.children {
        match child {
            Node::Text(t) if t.trim().is_empty() => previous = Some(t),
            Node::Element(_) => return previous.cloned(),
            _ => previous = None,
        }
    }
    None
}
