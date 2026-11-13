//! Vaults on a WebDAV server: Nextcloud, ownCloud, Apache, nginx and others.
//!
//! Contract: the server password goes to the OS credential store, never into
//! recents or a vault (`dec:keyring-with-file-fallback`). Only `https` URLs are
//! accepted, except for this machine, because Basic authentication sends the
//! password with every request. Connecting reads the file once, so a typo or
//! a wrong password fails at once rather than at the first save.

use std::path::{Path, PathBuf};

use vardo_sync::StorageBackend;
use vardo_webdav::ureq_transport::UreqTransport;
use vardo_webdav::WebDavBackend;
use zeroize::Zeroizing;

use crate::source::VaultSource;
use crate::CoreError;

const SERVICE: &str = "app.vardo.webdav";

/// Checks the address and credentials by reading the file, then remembers
/// the password. Returns the source to open.
pub fn connect(
    app_dir: &Path,
    url: &str,
    username: &str,
    password: &str,
) -> Result<VaultSource, CoreError> {
    let url = url.trim();
    check_url(url)?;
    let mut http = UreqTransport::new();
    WebDavBackend::new(&mut http, url, username, password)
        .stat()
        .map_err(|e| crate::sync::map_sync_error(e, url))?;
    remember(app_dir, url, username, password)?;
    Ok(VaultSource::WebDav {
        url: url.to_string(),
        username: username.to_string(),
    })
}

/// Only `https`, or plain `http` to this machine. Anything else would send
/// the password where anyone on the path could read it.
pub fn check_url(url: &str) -> Result<(), CoreError> {
    let refuse = |why: &str| {
        Err(CoreError::Unsupported {
            detail: why.to_string(),
        })
    };
    let Some((scheme, rest)) = url.split_once("://") else {
        return refuse("Enter the full address of the vault file, starting with https://");
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let local = ["localhost", "127.0.0.1", "[::1]"]
        .iter()
        .any(|h| host == *h || host.starts_with(&format!("{h}:")));
    match scheme.to_ascii_lowercase().as_str() {
        "https" if !host.is_empty() => {}
        "http" if local => {}
        "http" => return refuse("Use https://. Plain http would send your password unencrypted."),
        _ => return refuse("Enter the full address of the vault file, starting with https://"),
    }
    if rest.contains('@') && rest.find('@') < rest.find('/') {
        return refuse("Put the username in its own box, not in the address.");
    }
    if rest.ends_with('/') {
        return refuse("Enter the address of the vault file itself, not its folder.");
    }
    Ok(())
}

/// The password saved for this server and username.
pub fn password(app_dir: &Path, url: &str, username: &str) -> Option<Zeroizing<String>> {
    let account = account(url, username);
    if let Some(secret) =
        crate::secrets::entry(SERVICE, &account).and_then(|e| e.get_password().ok())
    {
        return Some(Zeroizing::new(secret));
    }
    let text = Zeroizing::new(std::fs::read_to_string(file(app_dir, &account)).ok()?);
    Some(Zeroizing::new(text.trim_end_matches('\n').to_string()))
}

fn remember(app_dir: &Path, url: &str, username: &str, password: &str) -> Result<(), CoreError> {
    let account = account(url, username);
    if crate::secrets::entry(SERVICE, &account).is_some_and(|e| e.set_password(password).is_ok()) {
        let _ = std::fs::remove_file(file(app_dir, &account));
        return Ok(());
    }
    let path = file(app_dir, &account);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| CoreError::Io {
            detail: e.to_string(),
        })?;
    }
    crate::secrets::write_private(&path, password.as_bytes())
}

/// Forgets the saved password, for when the vault is removed from the list.
pub fn forget(app_dir: &Path, url: &str, username: &str) {
    let account = account(url, username);
    if let Some(e) = crate::secrets::entry(SERVICE, &account) {
        let _ = e.delete_credential();
    }
    let _ = std::fs::remove_file(file(app_dir, &account));
}

fn account(url: &str, username: &str) -> String {
    format!("{username} {url}")
}

fn file(app_dir: &Path, account: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(account.as_bytes());
    let name: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    app_dir.join("webdav").join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_or_this_machine_is_accepted() {
        for ok in [
            "https://cloud.example.com/remote.php/dav/files/alex/vault.kdbx",
            "http://localhost:8080/vault.kdbx",
            "http://127.0.0.1/dav/vault.kdbx",
        ] {
            assert!(check_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://cloud.example.com/vault.kdbx",
            "ftp://cloud.example.com/vault.kdbx",
            "cloud.example.com/vault.kdbx",
            "https://alex:secret@cloud.example.com/vault.kdbx",
            "https://cloud.example.com/folder/",
            "http://localhost.evil.example/vault.kdbx",
        ] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_password_is_remembered_per_server_and_username_and_forgotten() {
        let dir = std::env::temp_dir().join("vardo-webdav-secret");
        let _ = std::fs::remove_dir_all(&dir);
        let url = "https://cloud.example.com/vault.kdbx";
        remember(&dir, url, "alex", "dav-pass").unwrap();
        assert_eq!(
            password(&dir, url, "alex").as_deref().map(String::as_str),
            Some("dav-pass")
        );
        assert!(password(&dir, url, "sam").is_none());
        assert!(password(&dir, "https://other.example.com/vault.kdbx", "alex").is_none());
        forget(&dir, url, "alex");
        assert!(password(&dir, url, "alex").is_none());
    }
}
