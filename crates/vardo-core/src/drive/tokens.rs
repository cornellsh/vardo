//! Where the Google Drive refresh token is kept.
//!
//! Contract: the refresh token goes to the OS credential store (Keychain,
//! Windows Credential Manager, or the Secret Service) when one answers. Only
//! when none does is it written to a file readable by this user alone. Saving
//! to the store removes any old file, and disconnecting removes both. Access
//! tokens are never stored: [`super::auth::access`] refreshes on every use.
//!
//! A file readable by the user's own account is readable by anything running
//! as that user, so the file is the fallback for a desktop without a keyring,
//! not the design. Either way the `drive.file` scope bounds what a stolen token
//! reaches: files this app created or the user picked
//! (`fact:drive-refresh-token-persists`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use keyring_core::{CredentialStore, Entry};
use vardo_drive::oauth::Tokens;
use zeroize::Zeroizing;

use crate::CoreError;

/// The service name every Vardo credential is filed under.
const SERVICE: &str = "app.vardo.drive";

/// The refresh token, if this app directory is connected to Drive.
pub fn refresh_token(app_dir: &Path) -> Option<Zeroizing<String>> {
    read(entry(app_dir).as_ref(), app_dir)
}

pub fn is_connected(app_dir: &Path) -> bool {
    refresh_token(app_dir).is_some()
}

/// Keeps the refresh half of `tokens`. A token set without one, which is what
/// Google returns on an ordinary refresh, leaves the stored token alone.
pub fn store(app_dir: &Path, tokens: &Tokens) -> Result<(), CoreError> {
    match &tokens.refresh_token {
        Some(refresh) if !refresh.is_empty() => write(entry(app_dir).as_ref(), app_dir, refresh),
        _ => Ok(()),
    }
}

/// Forgets the connection. Disconnecting twice is not an error: the user
/// pressed the button.
pub fn disconnect(app_dir: &Path) -> Result<(), CoreError> {
    forget(entry(app_dir).as_ref(), app_dir)
}

// --- the two places a token can be -------------------------------------------

fn read(entry: Option<&Entry>, app_dir: &Path) -> Option<Zeroizing<String>> {
    if let Some(secret) = entry.and_then(|e| e.get_password().ok()) {
        if !secret.is_empty() {
            return Some(Zeroizing::new(secret));
        }
    }
    // Line 1 once held an access token. Files written before the credential
    // store existed still have one, so the refresh token stays on line 2.
    let text = Zeroizing::new(std::fs::read_to_string(file(app_dir)).ok()?);
    let refresh = text.lines().nth(1)?.trim();
    (!refresh.is_empty()).then(|| Zeroizing::new(refresh.to_string()))
}

fn write(entry: Option<&Entry>, app_dir: &Path, refresh: &str) -> Result<(), CoreError> {
    if entry.is_some_and(|e| e.set_password(refresh).is_ok()) {
        // The store has it, so a copy in a file is only a liability.
        return match std::fs::remove_file(file(app_dir)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
            _ => Ok(()),
        };
    }
    std::fs::create_dir_all(app_dir).map_err(io)?;
    let path = file(app_dir);
    write_private(&path, format!("\n{refresh}\n").as_bytes())
}

fn forget(entry: Option<&Entry>, app_dir: &Path) -> Result<(), CoreError> {
    if let Some(e) = entry {
        // A store that refuses the delete but still answers with the token
        // would leave the user connected while the app said otherwise.
        if e.delete_credential().is_err() && e.get_password().is_ok() {
            return Err(CoreError::Io {
                detail: "The system keyring would not forget the Google Drive connection".into(),
            });
        }
    }
    match std::fs::remove_file(file(app_dir)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
        _ => Ok(()),
    }
}

fn file(app_dir: &Path) -> PathBuf {
    app_dir.join("drive-tokens")
}

/// Created owner-only, so the token is never readable by others, not even for
/// the moment between writing and changing permissions.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), CoreError> {
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

// --- the credential store ----------------------------------------------------

/// This app directory's entry in the OS store, or `None` when there is no
/// store to ask.
///
/// Keyed by the app directory, so a test or a second profile never reads
/// another one's connection.
fn entry(app_dir: &Path) -> Option<Entry> {
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
    Entry::new(SERVICE, &app_dir.display().to_string()).ok()
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

fn io(e: std::io::Error) -> CoreError {
    CoreError::Io {
        detail: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vardo-drive-tokens-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn tokens(refresh: Option<&str>) -> Tokens {
        Tokens {
            access_token: "at".into(),
            refresh_token: refresh.map(String::from),
            expires_in: None,
        }
    }

    #[test]
    fn nothing_stored_reads_as_not_connected() {
        let dir = dir("absent");
        assert!(!is_connected(&dir));
        assert!(refresh_token(&dir).is_none());
    }

    #[test]
    fn the_token_goes_to_the_credential_store_and_not_to_a_file() {
        let dir = dir("store");
        store(&dir, &tokens(Some("rt"))).unwrap();
        assert_eq!(
            refresh_token(&dir).as_deref().map(String::as_str),
            Some("rt")
        );
        assert!(
            !file(&dir).exists(),
            "no copy on disk when the store took it"
        );
    }

    #[test]
    fn saving_to_the_store_removes_an_old_token_file() {
        let dir = dir("migrate");
        std::fs::write(file(&dir), "old-access\nold-refresh\n").unwrap();
        assert_eq!(
            refresh_token(&dir).as_deref().map(String::as_str),
            Some("old-refresh"),
            "a file from before the store still connects"
        );
        store(&dir, &tokens(Some("new-refresh"))).unwrap();
        assert!(!file(&dir).exists());
        assert_eq!(
            refresh_token(&dir).as_deref().map(String::as_str),
            Some("new-refresh")
        );
    }

    #[test]
    fn a_refresh_without_a_new_token_keeps_the_old_one() {
        // Google omits the refresh token on a refresh.
        let dir = dir("keep");
        store(&dir, &tokens(Some("rt"))).unwrap();
        store(&dir, &tokens(None)).unwrap();
        assert_eq!(
            refresh_token(&dir).as_deref().map(String::as_str),
            Some("rt")
        );
    }

    #[test]
    fn without_a_store_the_token_file_is_owner_only() {
        let dir = dir("fallback");
        write(None, &dir, "rt").unwrap();
        assert_eq!(read(None, &dir).as_deref().map(String::as_str), Some("rt"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(file(&dir)).unwrap().permissions().mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "the refresh token must not be world-readable"
            );
        }
    }

    #[test]
    fn a_file_without_a_refresh_half_is_not_a_connection() {
        // An access token alone expires within the hour, so treating it as a
        // connection would show "connected" and then fail on the next sync.
        let dir = dir("partial");
        std::fs::write(file(&dir), "access-only\n").unwrap();
        assert!(!is_connected(&dir));
    }

    #[test]
    fn disconnecting_forgets_both_places_and_is_idempotent() {
        let dir = dir("disconnect");
        store(&dir, &tokens(Some("rt"))).unwrap();
        write(None, &dir, "stale").unwrap();
        assert!(is_connected(&dir));
        disconnect(&dir).unwrap();
        assert!(!is_connected(&dir));
        assert!(!file(&dir).exists());
        disconnect(&dir).unwrap();
    }
}
