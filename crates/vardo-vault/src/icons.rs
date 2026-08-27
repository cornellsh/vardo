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
