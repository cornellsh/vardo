//! A typed view over a KDBX document.
//!
//! # The rule this crate obeys
//!
//! The XML DOM is the source of truth. This crate is a *lens* over it, never a
//! replacement. Nothing here copies the document into a parallel model, because
//! the moment two representations exist, one of them starts losing fields —
//! which is exactly the failure that disqualified the `keepass` crate (see
//! `PHASE0-FINDINGS.md`).
//!
//! So `Group` and `Entry` are borrowed views holding a reference to an
//! `xml::Element`. Reading walks the tree; writing (later) will be a surgical
//! mutation of it. An element nobody understands is an element nobody touched.
//!
//! # Protected values
//!
//! Passwords are not in the DOM as plaintext — they are base64 ciphertext, and
//! decrypting them requires knowing each value's *position* in the document,
//! because a single keystream covers all of them in order.
//!
//! [`Vault`] resolves that once, at open, and indexes the result by element
//! identity. Callers get plaintext by asking an [`Entry`], never by counting
//! anything themselves.

#![forbid(unsafe_code)]

pub mod time;
pub mod uuid;

use vardo_kdbx::protected;
use vardo_kdbx::xml;
use zeroize::Zeroizing;

pub use time::{Encoding, Timestamp};
pub use uuid::Uuid;

/// An unlocked vault.
///
/// # Why the DOM holds plaintext
///
/// Protected values are decrypted by **position** in one shared keystream. A
/// document that still holds ciphertext therefore cannot be edited: removing
/// one entry shifts every later value's position, and the bytes after it
/// decrypt to garbage. Deleting a single entry corrupts the password of every
/// entry below it.
///
/// So unlocking decrypts *into* the DOM, and encryption happens only at
/// [`Vault::save_with`]. Inside this window there is no positional coupling at
/// all, which makes arbitrary mutation safe.
///
/// The cost is that plaintext passwords live in an in-memory XML tree rather
/// than in individually zeroized buffers. That is the same exposure every
/// KeePass client has while unlocked, it is bounded by [`Vault::lock`], and it
/// buys the elimination of a silent-corruption class. Given the choice between
/// a slightly wider memory-disclosure window and a vault that quietly destroys
/// passwords on save, this is the right trade.
pub struct Vault {
    /// Boxed so the tree never moves. [`Vault::ordinals`] keys on element
    /// addresses, which is only sound while those addresses are stable.
    /// Addresses are used purely as identity and are never dereferenced.
    dom: Box<xml::Document>,
    /// Format settings to preserve on save.
    save_options: vardo_kdbx::write::SaveOptions,
    binaries: Vec<vardo_kdbx::header::Binary>,
    limits: vardo_kdbx::Limits,
}

/// Redacting: the DOM is plaintext while unlocked.
impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("database_name", &self.database_name())
            .field("contents", &format_args!("<redacted>"))
            .finish()
    }
}

impl Vault {
    /// Unlocks a parsed document, decrypting protected values into the DOM.
    pub fn open(doc: &vardo_kdbx::Document) -> vardo_kdbx::Result<Self> {
        let ciphertext_dom = doc.parse_xml()?;
        let dom = Box::new(protected::plaintext_document(
            &ciphertext_dom,
            doc.inner.stream_cipher,
            &doc.inner.stream_key,
        )?);
        Ok(Self {
            dom,
            save_options: vardo_kdbx::write::SaveOptions::preserving(doc)?,
            binaries: doc.inner.binaries.clone(),
            limits: vardo_kdbx::Limits::default(),
        })
    }

    /// The plaintext DOM. Contains every password in the clear.
    pub fn document(&self) -> &xml::Document {
        &self.dom
    }

    /// Mutable access to the plaintext DOM.
    ///
    /// Safe to restructure freely: while unlocked there is no positional
    /// coupling between protected values.
    pub fn document_mut(&mut self) -> &mut xml::Document {
        &mut self.dom
    }

    /// Re-encrypts and encodes, preserving the original format settings.
    pub fn save_with(
        &self,
        creds: &vardo_kdbx::Credentials,
        rng: &mut dyn vardo_kdbx::write::Rng,
    ) -> vardo_kdbx::Result<Vec<u8>> {
        vardo_kdbx::write::save_plaintext(
            &self.dom,
            &self.binaries,
            creds,
            &self.save_options,
            &self.limits,
            rng,
        )
    }

    /// Overwrites every plaintext secret in the DOM.
    ///
    /// Bounds how long decrypted passwords sit in memory. The vault is unusable
    /// afterwards and must be reopened.
    pub fn lock(&mut self) {
        fn scrub(e: &mut xml::Element) {
            if protected::is_protected(e) {
                let len = e.text().len();
                e.set_text(&"\0".repeat(len));
                e.set_text("");
                return;
            }
            for child in e.children.iter_mut() {
                if let xml::Node::Element(child) = child {
                    scrub(child);
                }
            }
        }
        scrub(&mut self.dom.root);
    }

    /// The `<Meta>` element, or `None` in a structurally invalid document.
    pub fn meta(&self) -> Option<&xml::Element> {
        self.dom.root.child("Meta")
    }

    pub fn database_name(&self) -> String {
        self.meta()
            .and_then(|m| m.child("DatabaseName"))
            .map(|e| e.text())
            .unwrap_or_default()
    }

    pub fn generator(&self) -> String {
        self.meta()
            .and_then(|m| m.child("Generator"))
            .map(|e| e.text())
            .unwrap_or_default()
    }

    /// The recycle bin's UUID, if one is configured and enabled.
    ///
    /// Merge needs this: an entry moved to the recycle bin is a *deletion* to
    /// the user, but structurally it is just a move, and treating it as an
    /// ordinary move would resurrect deleted entries on the other device.
    pub fn recycle_bin_uuid(&self) -> Option<Uuid> {
        let meta = self.meta()?;
        let enabled = meta
            .child("RecycleBinEnabled")
            .map(|e| e.text().eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if !enabled {
            return None;
        }
        let uuid = Uuid::parse(&meta.child("RecycleBinUUID")?.text())?;
        (!uuid.is_nil()).then_some(uuid)
    }

    /// The top-level group. `None` only in a structurally invalid document.
    pub fn root(&self) -> Option<Group<'_>> {
        let element = self.dom.root.child("Root")?.child("Group")?;
        Some(Group {
            element,
            vault: self,
        })
    }

    /// Tombstones, which record that an object was deleted and when.
    ///
    /// Load-bearing for merge: without them a deletion is indistinguishable
    /// from "the other side has an entry we lack", and every sync would
    /// resurrect everything the user has ever deleted.
    pub fn deleted_objects(&self) -> Vec<DeletedObject> {
        let Some(root) = self.dom.root.child("Root") else {
            return Vec::new();
        };
        let Some(list) = root.child("DeletedObjects") else {
            return Vec::new();
        };
        list.children_named("DeletedObject")
            .filter_map(|e| {
                Some(DeletedObject {
                    uuid: Uuid::parse(&e.child("UUID")?.text())?,
                    deleted_at: e
                        .child("DeletionTime")
                        .and_then(|t| Timestamp::parse(&t.text())),
                })
            })
            .collect()
    }

    /// Every entry in the tree, excluding history, in document order.
    pub fn entries(&self) -> Vec<Entry<'_>> {
        let mut out = Vec::new();
        if let Some(root) = self.root() {
            root.walk_entries(&mut out);
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedObject {
    pub uuid: Uuid,
    pub deleted_at: Option<Timestamp>,
}

/// A field value. Protected and plain values are the same type so that callers
/// cannot accidentally treat a password as ordinary text.
pub struct FieldValue {
    value: Zeroizing<String>,
    protected: bool,
}

impl FieldValue {
    pub fn is_protected(&self) -> bool {
        self.protected
    }

    /// Borrows the value. Named `expose` so that reading a secret is visible
    /// at the call site rather than looking like an ordinary getter.
    pub fn expose(&self) -> &str {
        &self.value
    }
}

/// Redacting: a protected value here is a password in plaintext.
impl std::fmt::Debug for FieldValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.protected {
            write!(f, "FieldValue(<protected, {} chars>)", self.value.len())
        } else {
            write!(f, "FieldValue({:?})", &*self.value)
        }
    }
}

/// The `<Times>` block shared by groups and entries.
///
/// Every field is optional because a partially-written or foreign file may
/// omit any of them, and defaulting a missing timestamp to the epoch would let
/// merge silently discard the newer side.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Times {
    pub created: Option<Timestamp>,
    pub last_modified: Option<Timestamp>,
    pub last_accessed: Option<Timestamp>,
    pub expires_at: Option<Timestamp>,
    pub location_changed: Option<Timestamp>,
    pub expires: bool,
    pub usage_count: Option<u64>,
}

impl Times {
    fn parse(parent: &xml::Element) -> Self {
        let Some(t) = parent.child("Times") else {
            return Self::default();
        };
        let stamp = |name: &str| t.child(name).and_then(|e| Timestamp::parse(&e.text()));
        Self {
            created: stamp("CreationTime"),
            last_modified: stamp("LastModificationTime"),
            last_accessed: stamp("LastAccessTime"),
            expires_at: stamp("ExpiryTime"),
            location_changed: stamp("LocationChanged"),
            expires: t
                .child("Expires")
                .map(|e| e.text().eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            usage_count: t.child("UsageCount").and_then(|e| e.text().parse().ok()),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Group<'a> {
    element: &'a xml::Element,
    vault: &'a Vault,
}

impl std::fmt::Debug for Group<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Group")
            .field("name", &self.name())
            .field("uuid", &self.uuid())
            .finish()
    }
}

impl<'a> Group<'a> {
    pub fn element(&self) -> &'a xml::Element {
        self.element
    }

    pub fn uuid(&self) -> Option<Uuid> {
        Uuid::parse(&self.element.child("UUID")?.text())
    }

    pub fn name(&self) -> String {
        self.element
            .child("Name")
            .map(|e| e.text())
            .unwrap_or_default()
    }

    pub fn notes(&self) -> String {
        self.element
            .child("Notes")
            .map(|e| e.text())
            .unwrap_or_default()
    }

    pub fn times(&self) -> Times {
        Times::parse(self.element)
    }

    /// Tags, `;`-separated. A KDBX 4.1 addition; absent in older files.
    pub fn tags(&self) -> Vec<String> {
        split_tags(self.element.child("Tags").map(|e| e.text()))
    }

    pub fn groups(&self) -> impl Iterator<Item = Group<'a>> + 'a {
        let vault = self.vault;
        self.element
            .children_named("Group")
            .map(move |element| Group { element, vault })
    }

    /// Direct child entries, excluding history.
    ///
    /// History entries live inside `<Entry><History>`, so they are never direct
    /// children of a group and cannot leak in here.
    pub fn entries(&self) -> impl Iterator<Item = Entry<'a>> + 'a {
        let vault = self.vault;
        self.element
            .children_named("Entry")
            .map(move |element| Entry { element, vault })
    }

    fn walk_entries(&self, out: &mut Vec<Entry<'a>>) {
        out.extend(self.entries());
        for child in self.groups() {
            child.walk_entries(out);
        }
    }
}

#[derive(Clone, Copy)]
pub struct Entry<'a> {
    element: &'a xml::Element,
    vault: &'a Vault,
}

/// Redacting: never render an entry's field values.
impl std::fmt::Debug for Entry<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry")
            .field("uuid", &self.uuid())
            .field("title", &self.title())
            .finish_non_exhaustive()
    }
}

impl<'a> Entry<'a> {
    pub fn element(&self) -> &'a xml::Element {
        self.element
    }

    pub fn uuid(&self) -> Option<Uuid> {
        Uuid::parse(&self.element.child("UUID")?.text())
    }

    pub fn times(&self) -> Times {
        Times::parse(self.element)
    }

    pub fn tags(&self) -> Vec<String> {
        split_tags(self.element.child("Tags").map(|e| e.text()))
    }

    /// A named field, decrypting it if the writer marked it protected.
    pub fn field(&self, key: &str) -> Option<FieldValue> {
        let value = self
            .element
            .children_named("String")
            .find(|s| s.child("Key").map(|k| k.text()).as_deref() == Some(key))?
            .child("Value")?;

        // The DOM holds plaintext while unlocked, so this is a plain read for
        // both kinds of field; only the flag differs.
        Some(FieldValue {
            value: Zeroizing::new(value.text()),
            protected: protected::is_protected(value),
        })
    }

    /// Every field, in document order.
    pub fn fields(&self) -> Vec<(String, FieldValue)> {
        self.element
            .children_named("String")
            .filter_map(|s| {
                let key = s.child("Key")?.text();
                let value = self.field(&key)?;
                Some((key, value))
            })
            .collect()
    }

    pub fn title(&self) -> String {
        self.plain("Title")
    }

    pub fn username(&self) -> String {
        self.plain("UserName")
    }

    pub fn url(&self) -> String {
        self.plain("URL")
    }

    pub fn notes(&self) -> String {
        self.plain("Notes")
    }

    /// The password, decrypted.
    pub fn password(&self) -> Option<FieldValue> {
        self.field("Password")
    }

    /// Convenience for fields conventionally stored unprotected.
    ///
    /// Returns an empty string rather than the ciphertext if a writer has
    /// protected the field, so a title can never leak a secret into a log.
    fn plain(&self, key: &str) -> String {
        match self.field(key) {
            Some(v) if !v.is_protected() => v.expose().to_string(),
            _ => String::new(),
        }
    }

    /// Previous versions of this entry, oldest first.
    pub fn history(&self) -> Vec<Entry<'a>> {
        let vault = self.vault;
        self.element
            .child("History")
            .map(|h| {
                h.children_named("Entry")
                    .map(|element| Entry { element, vault })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Attachment names paired with their index into the binary pool.
    pub fn attachments(&self) -> Vec<(String, Option<usize>)> {
        self.element
            .children_named("Binary")
            .filter_map(|b| {
                let name = b.child("Key")?.text();
                let index = b
                    .child("Value")
                    .and_then(|v| v.attr_value("Ref"))
                    .and_then(|r| r.parse().ok());
                Some((name, index))
            })
            .collect()
    }

    /// The group this entry was in before it was last moved, if recorded.
    /// KDBX 4.1 only.
    pub fn previous_parent(&self) -> Option<Uuid> {
        Uuid::parse(&self.element.child("PreviousParentGroup")?.text())
    }
}

fn split_tags(raw: Option<String>) -> Vec<String> {
    raw.unwrap_or_default()
        .split([';', ','])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}
