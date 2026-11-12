//! Files a session reads or writes outside the vault: site icons and
//! attachments saved to disk.
//!
//! Contract: an attachment written out is created 0600 and synced before the
//! call returns. Icons come from the site itself, never an aggregator
//! (`inv:no-phone-home`).

use std::path::Path;

use super::{entries, Session};
use crate::CoreError;

impl Session {
    /// Fetches this entry's site icon and stores it in the vault.
    ///
    /// Direct from the site, never an aggregator, and cached in the vault so it
    /// happens once ever rather than once per device. Returns the data URL when
    /// something was found; `None` when the site has no usable icon, which is
    /// common and not worth interrupting anyone about.
    ///
    /// Marks the vault dirty, because it genuinely changed. An icon that lived
    /// only in memory would be re-fetched on every device forever.
    pub fn fetch_icon(&mut self, uuid: &str) -> Result<Option<String>, CoreError> {
        let id = entries::parse_uuid(uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;

        let url = entries::find(&self.vault, uuid)
            .map(|e| e.url())
            .unwrap_or_default();
        if url.trim().is_empty() {
            return Ok(None);
        }

        let mut http = vardo_drive::ureq_transport::UreqTransport::new();
        let Some(icon) = crate::favicon::fetch(&mut http, &url)? else {
            return Ok(None);
        };

        self.vault.set_custom_icon(id, &icon.png)?;
        self.dirty = true;

        Ok(Some(format!(
            "data:image/png;base64,{}",
            vardo_kdbx::write::base64_encode(&icon.png)
        )))
    }

    /// Entries that have a web address and no icon yet.
    ///
    /// Exposed so the UI can say how many requests a bulk fetch would make
    /// before making them, rather than after.
    pub fn entries_missing_icons(&self) -> Vec<String> {
        self.vault
            .entries()
            .into_iter()
            .filter(|e| e.custom_icon_uuid().is_none())
            .filter(|e| crate::favicon::host_of(&e.url()).is_some())
            .filter_map(|e| e.uuid().map(entries::uuid_text))
            .collect()
    }
}

impl Session {
    /// Writes an attachment out to a file the user chose.
    ///
    /// Attachments are the things people put in a vault precisely because they
    /// matter: recovery codes, an SSH key, a scan of a passport. Being able to
    /// see the filename and never the contents makes a vault somewhere data
    /// goes to be lost.
    ///
    /// Returns how many bytes were written, so the caller can confirm the file
    /// is really there rather than assuming.
    pub fn save_attachment(&self, uuid: &str, name: &str, dest: &Path) -> Result<usize, CoreError> {
        let entry = entries::find(&self.vault, uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;

        let bytes = entry
            .attachment_bytes(name)
            .ok_or_else(|| CoreError::NotFound {
                path: name.to_string(),
            })?;

        // Owner-only from the moment it exists, rather than written first and
        // tightened after. The gap between those two is short and is exactly
        // when another process on a shared machine could read it.
        write_private(dest, &bytes)?;
        Ok(bytes.len())
    }
}

fn write_private(dest: &Path, bytes: &[u8]) -> Result<(), CoreError> {
    use std::io::Write as _;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }

    let mut file = options.open(dest).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => CoreError::Io {
            detail: format!("Cannot write to {}", dest.display()),
        },
        _ => CoreError::Io {
            detail: e.to_string(),
        },
    })?;
    file.write_all(bytes).map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })?;
    file.sync_all().map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })
}
