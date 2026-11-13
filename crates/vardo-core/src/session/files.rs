//! Files a session reads or writes outside the vault: site icons and
//! attachments saved to disk.
//!
//! Contract: an attachment written out is created 0600 and synced before the
//! call returns. Icons come from the site itself, never an aggregator
//! (`inv:no-phone-home`).

use std::path::Path;
use std::sync::Mutex;

use super::{entries, Session};
use crate::CoreError;

/// [`Session::fetch_icon`] for a session other threads share, without holding
/// the lock while the site answers.
///
/// A slow site would otherwise stall every command behind it, and a bulk fetch
/// would freeze the app for minutes. If the vault was locked or swapped while
/// the site answered, storing fails with `Locked` or `NotFound`, which is right.
pub fn fetch_icon_shared(
    shared: &Mutex<Option<Session>>,
    uuid: &str,
) -> Result<Option<String>, CoreError> {
    let mut http = vardo_drive::ureq_transport::UreqTransport::new();
    fetch_icon_shared_via(shared, uuid, &mut http)
}

/// [`fetch_icon_shared`] through a given transport, so a test can be the site.
pub fn fetch_icon_shared_via(
    shared: &Mutex<Option<Session>>,
    uuid: &str,
    http: &mut dyn vardo_drive::http::HttpTransport,
) -> Result<Option<String>, CoreError> {
    let url = {
        let guard = shared.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().ok_or(CoreError::Locked)?.icon_url(uuid)?
    };
    let Some(url) = url else { return Ok(None) };
    let Some(icon) = crate::favicon::fetch(http, &url)? else {
        return Ok(None);
    };
    let mut guard = shared.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .as_mut()
        .ok_or(CoreError::Locked)?
        .store_icon(uuid, &icon.png)
        .map(Some)
}

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
        let Some(url) = self.icon_url(uuid)? else {
            return Ok(None);
        };
        let mut http = vardo_drive::ureq_transport::UreqTransport::new();
        let Some(icon) = crate::favicon::fetch(&mut http, &url)? else {
            return Ok(None);
        };
        self.store_icon(uuid, &icon.png).map(Some)
    }

    /// The address to fetch this entry's icon from: `None` when it already has
    /// an icon or no web address, so a saved login never re-fetches one.
    fn icon_url(&self, uuid: &str) -> Result<Option<String>, CoreError> {
        let entry = entries::find(&self.vault, uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;
        let url = entry.url();
        Ok((entry.custom_icon_uuid().is_none() && !url.trim().is_empty()).then_some(url))
    }

    /// Stores a fetched icon and returns it as a data URL.
    fn store_icon(&mut self, uuid: &str, png: &[u8]) -> Result<String, CoreError> {
        let id = entries::parse_uuid(uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;
        self.vault.set_custom_icon(id, png)?;
        self.dirty = true;
        Ok(format!(
            "data:image/png;base64,{}",
            vardo_kdbx::write::base64_encode(png)
        ))
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
