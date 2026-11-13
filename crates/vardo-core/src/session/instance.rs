//! Noticing a vault that is already open in another Vardo window.
//!
//! Contract: an unlocked session holds an OS file lock named for its vault in
//! the app directory, and drops it with the session. A second session on the
//! same vault sees the lock and reports it, so the person learns at unlock
//! rather than at a refused save. It can still edit: the conditional save is
//! what prevents a lost update (`inv:atomic-verified-save`), and this only
//! warns earlier.
//!
//! The lock is advisory and per user. KeePassXC and other apps do not honour
//! it, and another device never sees it.

use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::source::VaultSource;

/// Held for as long as the session lives. Dropping it releases the lock.
pub(super) struct Claim {
    _file: Option<File>,
}

impl Claim {
    /// Locks `source` for this session. The flag is true when another session
    /// already holds it.
    ///
    /// Anything other than "already locked" counts as not open elsewhere: a
    /// filesystem without locks must not stop anyone opening a vault.
    pub(super) fn take(app_dir: &Path, source: &VaultSource) -> (Claim, bool) {
        let Ok(file) = open_lock_file(app_dir, source) else {
            return (Claim { _file: None }, false);
        };
        match file.try_lock() {
            Ok(()) => (Claim { _file: Some(file) }, false),
            Err(TryLockError::WouldBlock) => (Claim { _file: None }, true),
            Err(TryLockError::Error(_)) => (Claim { _file: None }, false),
        }
    }
}

fn open_lock_file(app_dir: &Path, source: &VaultSource) -> std::io::Result<File> {
    let dir = app_dir.join("open");
    std::fs::create_dir_all(&dir)?;
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(format!("{}.lock", key(source))))
}

/// Names a vault the same way however it was reached: a local file by its
/// canonical path, a Drive file by its id.
fn key(source: &VaultSource) -> String {
    let identity = match source {
        VaultSource::File { path } => {
            let real: PathBuf = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            format!("file:{}", real.display())
        }
        VaultSource::GoogleDrive { file_id, .. } => format!("drive:{file_id}"),
        VaultSource::WebDav { url, username } => format!("webdav:{username}@{url}"),
    };
    let digest = Sha256::digest(identity.as_bytes());
    digest[..16].iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vardo-instance-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_second_claim_on_the_same_vault_sees_the_first_until_it_drops() {
        let dir = scratch("same");
        let vault = VaultSource::file(dir.join("v.kdbx"));
        let (first, elsewhere) = Claim::take(&dir, &vault);
        assert!(!elsewhere);
        let (_second, elsewhere) = Claim::take(&dir, &vault);
        assert!(elsewhere, "the second window must learn the vault is open");
        drop(first);
        let (_third, elsewhere) = Claim::take(&dir, &vault);
        assert!(!elsewhere, "locking the first window frees the vault");
    }

    #[test]
    fn different_vaults_do_not_see_each_other() {
        let dir = scratch("different");
        let (_a, _) = Claim::take(&dir, &VaultSource::file(dir.join("a.kdbx")));
        let (_b, elsewhere) = Claim::take(&dir, &VaultSource::file(dir.join("b.kdbx")));
        assert!(!elsewhere);
    }

    #[test]
    fn one_file_reached_by_two_paths_is_one_vault() {
        let dir = scratch("paths");
        std::fs::write(dir.join("v.kdbx"), b"x").unwrap();
        let direct = VaultSource::file(dir.join("v.kdbx"));
        let roundabout = VaultSource::file(dir.join(".").join("v.kdbx"));
        assert_eq!(key(&direct), key(&roundabout));
        let drive = VaultSource::GoogleDrive {
            file_id: "abc".into(),
            name: "v.kdbx".into(),
        };
        assert_ne!(key(&direct), key(&drive));
    }
}
