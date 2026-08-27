//! Which storage a vault syncs against.
//!
//! Until now `vardo-core` hard-coded [`vardo_sync::local::LocalFile`], so the
//! Drive backend and its OAuth were unreachable from the application even
//! though both were written and tested. This is the seam that fixes that: a
//! vault records where it syncs, and the session builds the right backend.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::session::CoreError;

/// Where a vault's remote copy lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Remote {
    /// The file itself. Suitable for a genuinely local disk, a USB key, or a
    /// network share you control, and explicitly **not** for a cloud-synced
    /// folder - see `vardo_sync::storage_risk`.
    LocalFile,
    /// Google Drive, addressed by file id.
    ///
    /// The id, never the path. Drive paths are not stable and not unique, and
    /// `files.update` against an id is what preserves the file's identity for
    /// every other client that has a grant on it.
    GoogleDrive { file_id: String, name: String },
}

impl Remote {
    pub fn describe(&self) -> String {
        match self {
            Remote::LocalFile => "this file".into(),
            Remote::GoogleDrive { name, .. } => format!("Google Drive: {name}"),
        }
    }
}

/// Per-vault settings, keyed by the vault's path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultConfig {
    pub remote: Remote,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            remote: Remote::LocalFile,
        }
    }
}

/// FNV-1a of the absolute path.
///
/// Not `DefaultHasher`, which is seeded per process: a per-run key would mean
/// a vault silently forgets its own configuration on every launch.
fn key_for(vault: &Path) -> String {
    let text = vault.to_string_lossy();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{hash:016x}")
}

fn config_path(app_dir: &Path, vault: &Path) -> PathBuf {
    app_dir
        .join("vaults")
        .join(format!("{}.json", key_for(vault)))
}

pub fn load(app_dir: &Path, vault: &Path) -> VaultConfig {
    std::fs::read_to_string(config_path(app_dir, vault))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn store(app_dir: &Path, vault: &Path, config: &VaultConfig) -> Result<(), CoreError> {
    let path = config_path(app_dir, vault);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CoreError::Io {
            detail: e.to_string(),
        })?;
    }
    let text = serde_json::to_string_pretty(config).map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })?;
    std::fs::write(&path, text).map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vault_defaults_to_syncing_against_itself() {
        let dir = std::env::temp_dir().join("vardo-cfg-default");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            load(&dir, Path::new("/x/vault.kdbx")).remote,
            Remote::LocalFile
        );
    }

    #[test]
    fn a_drive_remote_round_trips() {
        let dir = std::env::temp_dir().join("vardo-cfg-drive");
        let _ = std::fs::remove_dir_all(&dir);
        let vault = Path::new("/x/vault.kdbx");

        let config = VaultConfig {
            remote: Remote::GoogleDrive {
                file_id: "1AbC".into(),
                name: "passwords.kdbx".into(),
            },
        };
        store(&dir, vault, &config).unwrap();

        match load(&dir, vault).remote {
            Remote::GoogleDrive { file_id, name } => {
                assert_eq!(file_id, "1AbC");
                assert_eq!(name, "passwords.kdbx");
            }
            other => panic!("expected Drive, got {other:?}"),
        }
    }

    #[test]
    fn keys_are_stable_across_runs_and_distinct_per_path() {
        assert_eq!(
            key_for(Path::new("/a/v.kdbx")),
            key_for(Path::new("/a/v.kdbx"))
        );
        assert_ne!(
            key_for(Path::new("/a/v.kdbx")),
            key_for(Path::new("/b/v.kdbx"))
        );
    }

    #[test]
    fn corrupt_config_falls_back_to_local_rather_than_failing_to_open() {
        // A vault must still open if its settings file is damaged. Refusing
        // would lock the user out of their passwords over a preferences file.
        let dir = std::env::temp_dir().join("vardo-cfg-corrupt");
        let _ = std::fs::remove_dir_all(&dir);
        let vault = Path::new("/x/vault.kdbx");
        let path = config_path(&dir, vault);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();

        assert_eq!(load(&dir, vault).remote, Remote::LocalFile);
    }
}
