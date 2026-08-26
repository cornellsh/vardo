//! Builds base/local/remote variants of a real vault by mutating its DOM.
//!
//! Deliberately goes through the real writer and the real reader rather than
//! constructing `Vault`s directly, so every scenario also exercises encode →
//! decode. A merge test that used hand-built objects could pass while the file
//! on disk said something else.

use vardo_kdbx::xml::{self, Element, Node};
use vardo_kdbx::{open, Credentials, Document};
use vardo_vault::{Encoding, Timestamp, Vault};

pub const PW: &str = "demopass";

/// Deterministic RNG so a failing scenario reproduces exactly.
pub struct SeededRng(pub u64);

impl vardo_kdbx::write::Rng for SeededRng {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), vardo_kdbx::Error> {
        for b in out.iter_mut() {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            *b = (self.0 >> 33) as u8;
        }
        Ok(())
    }
}

pub fn fixture_bytes(name: &str) -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// An in-progress variant: a decrypted document plus its editable DOM.
pub struct Variant {
    vault: Vault,
}

impl Variant {
    pub fn open(fixture: &str) -> Self {
        let bytes = fixture_bytes(fixture);
        let doc = open(&bytes, &Credentials::with_password(PW)).expect("open fixture");
        // Through Vault, so the DOM we edit holds plaintext. Editing a
        // ciphertext DOM shifts keystream positions and corrupts every
        // protected value after the edit.
        let vault = Vault::open(&doc).expect("unlock");
        Self { vault }
    }

    pub fn dom_mut(&mut self) -> &mut xml::Document {
        self.vault.document_mut()
    }

    /// Re-encodes and reopens, so the result has been through a real save.
    pub fn seal(&self, seed: u64) -> Sealed {
        let bytes = self
            .vault
            .save_with(&Credentials::with_password(PW), &mut SeededRng(seed))
            .expect("save");
        Sealed::from_bytes(bytes)
    }

    fn root_group(&mut self) -> &mut Element {
        self.vault
            .document_mut()
            .root
            .child_mut("Root")
            .expect("Root")
            .child_mut("Group")
            .expect("root Group")
    }

    /// Sets an unprotected field on the entry with the given title.
    pub fn set_field(&mut self, title: &str, key: &str, value: &str) {
        let entry = find_entry_mut(self.root_group(), title).expect("entry not found");
        for child in entry.children.iter_mut() {
            let Node::Element(s) = child else { continue };
            if s.name != "String" {
                continue;
            }
            if s.child("Key").map(|k| k.text()).as_deref() == Some(key) {
                s.child_mut("Value").expect("Value").set_text(value);
                return;
            }
        }
        panic!("field {key:?} not found on {title:?}");
    }

    /// Sets one of the entry's `<Times>` values.
    pub fn set_time(&mut self, title: &str, which: &str, unix_seconds: i64) {
        let entry = find_entry_mut(self.root_group(), title).expect("entry not found");
        let text = Timestamp::from_unix(unix_seconds, Encoding::Base64Seconds).to_text();
        entry
            .child_mut("Times")
            .expect("Times")
            .child_mut(which)
            .unwrap_or_else(|| panic!("no <{which}>"))
            .set_text(&text);
    }

    /// Removes an entry and records a tombstone, as a real client would.
    pub fn delete_entry(&mut self, title: &str, deleted_at_unix: i64) {
        let uuid = {
            let entry = find_entry_mut(self.root_group(), title).expect("entry not found");
            entry.child("UUID").expect("UUID").text()
        };
        let removed = remove_entry(self.root_group(), title);
        assert!(removed, "failed to remove {title:?}");
        self.add_tombstone(&uuid, deleted_at_unix);
    }

    /// Removes an entry *without* a tombstone, as a writer that does not keep
    /// them would.
    pub fn delete_entry_untracked(&mut self, title: &str) {
        assert!(remove_entry(self.root_group(), title), "remove {title:?}");
    }

    fn add_tombstone(&mut self, uuid_b64: &str, deleted_at_unix: i64) {
        let time = Timestamp::from_unix(deleted_at_unix, Encoding::Base64Seconds).to_text();
        let mut object = Element::new("DeletedObject");
        object.self_closing = false;
        object
            .children
            .push(Node::Element(text_element("UUID", uuid_b64)));
        object
            .children
            .push(Node::Element(text_element("DeletionTime", &time)));

        let root = self
            .vault
            .document_mut()
            .root
            .child_mut("Root")
            .expect("Root");
        let list = match root.child_mut("DeletedObjects") {
            Some(list) => list,
            None => {
                let mut fresh = Element::new("DeletedObjects");
                fresh.self_closing = false;
                root.children.push(Node::Element(fresh));
                root.child_mut("DeletedObjects").unwrap()
            }
        };
        list.self_closing = false;
        list.children.push(Node::Element(object));
    }

    /// Moves an entry into the named sibling group, updating LocationChanged.
    pub fn move_entry(&mut self, title: &str, to_group: &str, moved_at_unix: i64) {
        self.set_time(title, "LocationChanged", moved_at_unix);
        let node = take_entry(self.root_group(), title).expect("entry not found");
        let target = find_group_mut(self.root_group(), to_group).expect("group not found");
        target.self_closing = false;
        target.children.push(node);
    }
}

/// A saved, reopened variant. Holds the `Document` so the `Vault` can borrow.
pub struct Sealed {
    pub doc: Document,
}

impl Sealed {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let doc = open(&bytes, &Credentials::with_password(PW)).expect("reopen");
        Self { doc }
    }

    pub fn vault(&self) -> Vault {
        Vault::open(&self.doc).expect("unlock")
    }
}

fn text_element(name: &str, text: &str) -> Element {
    let mut e = Element::new(name);
    e.set_text(text);
    e
}

fn entry_title(e: &Element) -> Option<String> {
    e.children_named("String")
        .find(|s| s.child("Key").map(|k| k.text()).as_deref() == Some("Title"))
        .and_then(|s| s.child("Value"))
        .map(|v| v.text())
}

/// Locates a matching descendant and returns the child indices to reach it.
///
/// Searching immutably and navigating afterwards sidesteps the borrow
/// checker's inability to see that a recursive search returning `&mut` only
/// borrows one branch.
fn find_path(group: &Element, matches: &dyn Fn(&Element) -> bool) -> Option<Vec<usize>> {
    for (i, child) in group.children.iter().enumerate() {
        if let Node::Element(e) = child {
            if matches(e) {
                return Some(vec![i]);
            }
        }
    }
    for (i, child) in group.children.iter().enumerate() {
        if let Node::Element(g) = child {
            if g.name == "Group" {
                if let Some(mut rest) = find_path(g, matches) {
                    rest.insert(0, i);
                    return Some(rest);
                }
            }
        }
    }
    None
}

fn nav_mut<'a>(group: &'a mut Element, path: &[usize]) -> &'a mut Element {
    let mut current = group;
    for &i in path {
        current = match &mut current.children[i] {
            Node::Element(e) => e,
            _ => panic!("path points at a non-element"),
        };
    }
    current
}

fn find_entry_mut<'a>(group: &'a mut Element, title: &str) -> Option<&'a mut Element> {
    let path = find_path(group, &|e: &Element| {
        e.name == "Entry" && entry_title(e).as_deref() == Some(title)
    })?;
    Some(nav_mut(group, &path))
}

fn find_group_mut<'a>(group: &'a mut Element, name: &str) -> Option<&'a mut Element> {
    let path = find_path(group, &|e: &Element| {
        e.name == "Group" && e.child("Name").map(|n| n.text()).as_deref() == Some(name)
    })?;
    Some(nav_mut(group, &path))
}

fn take_entry(group: &mut Element, title: &str) -> Option<Node> {
    let path = find_path(group, &|e: &Element| {
        e.name == "Entry" && entry_title(e).as_deref() == Some(title)
    })?;
    let (last, parents) = path.split_last().expect("path is never empty");
    let parent = nav_mut(group, parents);
    Some(parent.children.remove(*last))
}

fn remove_entry(group: &mut Element, title: &str) -> bool {
    take_entry(group, title).is_some()
}
