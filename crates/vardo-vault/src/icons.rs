//! Icons the vault already carries.
//!
//! Two sources, both already inside the file and both currently thrown away:
//!
//! * `<Meta><CustomIcons>` holds base64 PNGs. This is where KeePassXC's
//!   "download favicons" feature puts what it fetched, so a vault that has been
//!   through KeePassXC often already has a real site icon for most entries.
//! * `<IconID>` selects one of KeePass's 47 built-in icons.
//!
//! Reading these costs nothing and leaks nothing. It is strictly better than
//! fetching anything from the network, and it has to come first: fetching an
//! icon we already have would be absurd.

use crate::uuid::Uuid;
use vardo_kdbx::xml::Element;

use crate::{Entry, Vault};

/// A PNG stored in the database, keyed by the UUID entries reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomIcon {
    pub uuid: Uuid,
    /// Raw base64 exactly as stored, so it can go straight into a data URL
    /// without a decode-and-re-encode round trip that could only lose fidelity.
    pub png_base64: String,
}

impl Vault {
    /// Every custom icon in `<Meta><CustomIcons>`.
    pub fn custom_icons(&self) -> Vec<CustomIcon> {
        let Some(icons) = self.meta().and_then(|m| m.child("CustomIcons")) else {
            return Vec::new();
        };

        icons
            .children_named("Icon")
            .filter_map(|icon| {
                let uuid = Uuid::parse(&icon.child("UUID")?.text())?;
                let data = icon.child("Data")?.text();
                if data.is_empty() {
                    return None;
                }
                Some(CustomIcon {
                    uuid,
                    // KDBX writers wrap long base64 across lines. Left in, it
                    // produces a data URL browsers reject, and the failure looks
                    // like a corrupt icon rather than stray whitespace.
                    png_base64: data.chars().filter(|c| !c.is_whitespace()).collect(),
                })
            })
            .collect()
    }
}

impl Entry<'_> {
    /// The custom icon this entry points at, if any.
    ///
    /// `<CustomIconUUID>` wins over `<IconID>` when both are present, matching
    /// every other KeePass client: a downloaded site icon is more specific than
    /// a pick from the built-in set.
    pub fn custom_icon_uuid(&self) -> Option<Uuid> {
        Uuid::parse(&self.element().child("CustomIconUUID")?.text())
    }

    /// The built-in KeePass icon number. Zero (a key) when unset, which is what
    /// the format's own default is.
    pub fn icon_id(&self) -> u32 {
        self.element()
            .child("IconID")
            .and_then(|e| e.text().parse().ok())
            .unwrap_or(0)
    }
}

impl Vault {
    /// Stores a PNG in `<Meta><CustomIcons>` and points `entry` at it.
    ///
    /// Writing into the vault rather than a private cache is the whole point:
    /// the icon then travels with the file, so it is fetched once ever rather
    /// than once per device, and KeePassXC and KeePassium render it too. It is
    /// also exactly where those clients put the icons they download, so this
    /// is not a Vardo-specific convention being smuggled into the format.
    pub fn set_custom_icon(&mut self, entry: Uuid, png: &[u8]) -> crate::edit::Result<Uuid> {
        self.require_writable()?;

        let icon_uuid = Uuid::from_bytes(crate::edit::random_uuid_bytes());
        let encoded = vardo_kdbx::write::base64_encode(png);

        {
            let meta = self.meta_mut()?;
            let icons = child_or_insert(meta, "CustomIcons");

            let mut icon = Element::new("Icon");
            let mut id = Element::new("UUID");
            id.set_text(&icon_uuid.to_base64());
            crate::edit::append_child(&mut icon, id);

            let mut data = Element::new("Data");
            data.set_text(&encoded);
            crate::edit::append_child(&mut icon, data);

            crate::edit::append_child(icons, icon);
        }

        // Done as an ordinary edit so it gets a history record and a modified
        // timestamp like any other change. An icon appearing without either
        // would make the entry look untouched to the merge, and a sync could
        // then discard it in favour of a copy that never had one.
        let changed = self.update_entry(entry, |e| {
            e.set_custom_icon_uuid(icon_uuid);
        })?;

        if !changed {
            return Err(crate::edit::EditError::NoSuchEntry(entry));
        }
        Ok(icon_uuid)
    }
}

fn child_or_insert<'a>(parent: &'a mut Element, name: &str) -> &'a mut Element {
    if parent.child(name).is_none() {
        crate::edit::append_child(parent, Element::new(name));
    }
    parent
        .child_mut(name)
        .expect("just inserted when it was absent")
}

#[cfg(test)]
mod tests {
    use crate::Vault;

    fn open(bytes: &[u8], password: &str) -> vardo_kdbx::Document {
        vardo_kdbx::open(bytes, &vardo_kdbx::Credentials::with_password(password)).unwrap()
    }

    #[test]
    fn a_vault_without_custom_icons_yields_none() {
        // Most vaults have none, and that must be an empty list rather than a
        // structural failure.
        let bytes =
            std::fs::read("../../fixtures/test_db_kdbx4_with_password_argon2.kdbx").unwrap();
        let doc = open(&bytes, "demopass");
        let vault = Vault::open(&doc).unwrap();
        assert!(vault.custom_icons().is_empty());
    }
}
