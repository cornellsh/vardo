//! The OS credential store, and the owner-only file used when there is none.
//!
//! Contract: secrets that must outlive a session (the Drive refresh token, a
//! WebDAV password) go to the macOS Keychain, Windows Credential Manager or the
//! Secret Service. Unit tests get an in-memory store, so no test touches a
//! real keychain. A fallback file is created owner-only before any byte is
//! written (`dec:keyring-with-file-fallback`).

use std::path::Path;
use std::sync::{Arc, OnceLock};

use keyring_core::{CredentialStore, Entry};

use crate::CoreError;

/// The entry for `account` under `service` in the OS store, or `None`
/// when there is no store to ask.
pub(crate) fn entry(service: &str, account: &str) -> Option<Entry> {
    static READY: OnceLock<bool> = OnceLock::new();
    let ready = *READY.get_or_init(|| match os_store() {
        Ok(store) => {
            keyring_core::set_default_store(store);
            true
        }
        Err(_) => false,
    });
    if !ready {
        return None;
    }
    Entry::new(service, account).ok()
}

/// Unit tests get an in-memory store, so no test ever touches the person's
/// real keychain.
#[cfg(test)]
fn os_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(keyring_core::sample::Store::new()?)
}

#[cfg(all(not(test), target_os = "macos"))]
fn os_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(apple_native_keyring_store::keychain::Store::new()?)
}

#[cfg(all(not(test), windows))]
fn os_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(windows_native_keyring_store::Store::new()?)
}

#[cfg(all(not(test), unix, not(target_os = "macos")))]
fn os_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(zbus_secret_service_keyring_store::Store::new()?)
}

/// Created owner-only, so the token is never readable by others, not even for
/// the moment between writing and changing permissions.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), CoreError> {
    let tmp = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut out = options.open(&tmp).map_err(io)?;
    std::io::Write::write_all(&mut out, bytes).map_err(io)?;
    out.sync_all().map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

fn io(e: std::io::Error) -> CoreError {
    CoreError::Io {
        detail: e.to_string(),
    }
}
