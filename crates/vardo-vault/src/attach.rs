//! Adding and removing attachments.
//!
//! KDBX 4 keeps attachment bytes in the inner header's binary pool and has each
//! entry reference one by index. That indirection is the whole difficulty:
//! indices are positional, so removing a pool entry renumbers every reference
//! after it, in every entry in the file.
//!
//! So nothing is ever removed from the pool. Detaching drops the entry's
//! reference and leaves the bytes where they are. That wastes space in a file
//! whose attachments are usually a few kilobytes, and it cannot silently
//! repoint some unrelated entry at the wrong document, which is the trade worth
//! making in a password vault.

use vardo_kdbx::header::Binary;
use vardo_kdbx::xml::Element;

use crate::edit::{append_child, EditError, Result};
use crate::uuid::Uuid;
use crate::Vault;

impl Vault {
    /// Attaches `data` to an entry under `name`.
    ///
    /// Replaces an attachment of the same name, because two `<Binary>` children
    /// sharing a key is a state KDBX permits and no client displays sensibly.
    pub fn add_attachment(&mut self, entry: Uuid, name: &str, data: &[u8]) -> Result<()> {
        self.require_writable()?;

        // Appended, never inserted. Every existing reference is an index into
        // this vector and inserting anywhere else moves them all.
        let index = self.binaries.len();
        self.binaries.push(Binary {
            // Protected attachments exist, but the flag governs the inner
            // stream cipher rather than anything Vardo does here, and writing
            // it without implementing that would produce a file whose bytes
            // are unreadable to clients that honour it.
            flags: 0,
            data: data.to_vec(),
        });

        let changed = self.update_entry(entry, |e| {
            e.remove_attachment_ref(name);
            e.add_attachment_ref(name, index);
        })?;

        if !changed {
            return Err(EditError::NoSuchEntry(entry));
        }
        Ok(())
    }

    /// Removes an entry's reference to an attachment.
    ///
    /// The bytes stay in the pool. See the module note: indices are positional
    /// and compacting them would renumber every other reference in the file.
    pub fn remove_attachment(&mut self, entry: Uuid, name: &str) -> Result<()> {
        self.require_writable()?;
        let changed = self.update_entry(entry, |e| e.remove_attachment_ref(name))?;
        if !changed {
            return Err(EditError::NoSuchEntry(entry));
        }
        Ok(())
    }
}

impl crate::edit::EntryEdit<'_> {
    pub(crate) fn add_attachment_ref(&mut self, name: &str, index: usize) {
        let mut binary = Element::new("Binary");

        let mut key = Element::new("Key");
        key.set_text(name);
        append_child(&mut binary, key);

        let mut value = Element::new("Value");
        value.set_attr("Ref", &index.to_string());
        append_child(&mut binary, value);

        append_child(self.element, binary);
    }

    pub(crate) fn remove_attachment_ref(&mut self, name: &str) {
        self.element.children.retain(|n| match n {
            vardo_kdbx::xml::Node::Element(e) if e.name == "Binary" => {
                e.child("Key").map(|k| k.text()).as_deref() != Some(name)
            }
            _ => true,
        });
    }
}
