//! Saving a vault to disk without ever being able to destroy it.
//!
//! Three things have to hold, and each one has burned a real password manager:
//!
//! **1. Verify before replacing.** The encoded bytes are re-opened with the
//! same credentials and compared against what we meant to write, *before*
//! anything on disk is touched. A save that silently produced an unopenable
//! file would otherwise overwrite the only copy. This costs one extra KDF pass,
//! which is the best money this project spends.
//!
//! **2. Write, flush, then rename.** `rename` being atomic guarantees only that
//! a reader sees the old file or the new one — not that the new one's contents
//! reached the disk. Without `sync_all` first, a crash shortly after can leave
//! the name pointing at a zero-length file while the original is already gone.
//!
//! **3. Keep the previous version.** The prior file is rotated to a timestamped
//! backup before being replaced, because "verified correct" still assumes our
//! own reader is right.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use vardo_kdbx::Credentials;
use vardo_vault::Vault;

use crate::UiError;

/// How many previous versions to keep beside the vault.
const KEEP_BACKUPS: usize = 5;

pub struct Saved {
    pub bytes_written: usize,
    pub backup: Option<PathBuf>,
}

pub fn save(vault: &Vault, creds: &Credentials, path: &Path) -> Result<Saved, UiError> {
    let encoded = vault
        .save_with(creds, &mut vardo_kdbx::write::OsRng)
        .map_err(UiError::from)?;

    verify(vault, creds, &encoded)?;

    let backup = rotate_backups(path)?;
    write_atomically(path, &encoded)?;

    Ok(Saved {
        bytes_written: encoded.len(),
        backup,
    })
}

/// Re-open what we just encoded and confirm it holds the same data.
///
/// Compares the *canonical* payload — the XML with protected values as
/// plaintext — because every save re-encrypts under a fresh inner key, so two
/// files holding identical data differ in every password field by design.
/// Comparing raw bytes here would always fail.
fn verify(vault: &Vault, creds: &Credentials, encoded: &[u8]) -> Result<(), UiError> {
    let reopened = vardo_kdbx::open(encoded, creds).map_err(|e| UiError::Corrupt {
        detail: format!("refusing to save: the file we produced does not open ({e})"),
    })?;

    let reopened_dom = reopened.parse_xml().map_err(|e| UiError::Corrupt {
        detail: format!("refusing to save: produced unparseable XML ({e})"),
    })?;

    let actual = reopened
        .canonical_payload(&reopened_dom)
        .map_err(|e| UiError::Corrupt {
            detail: format!("refusing to save: cannot canonicalise our own output ({e})"),
        })?;

    // The in-memory DOM is already plaintext, so its serialisation is the
    // canonical form by construction.
    let expected = vault.document().to_bytes();

    if actual.as_slice() != expected.as_slice() {
        return Err(UiError::Corrupt {
            detail: "refusing to save: the encoded file does not round-trip to \
                     the data in memory"
                .into(),
        });
    }
    Ok(())
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), UiError> {
    let temp = path.with_extension("kdbx.saving");

    {
        let mut file = fs::File::create(&temp).map_err(io)?;
        file.write_all(bytes).map_err(io)?;
        // Contents durable before the name points at them.
        file.sync_all().map_err(io)?;
    }

    fs::rename(&temp, path).map_err(|e| {
        // Clean up rather than leaving a stray .saving file next to the vault.
        let _ = fs::remove_file(&temp);
        io(e)
    })?;

    // Make the rename itself durable. Best effort: some filesystems refuse to
    // open a directory this way, and failing the save over it would be worse
    // than the residual risk.
    if let Some(parent) = path.parent() {
        if let Ok(dir) = fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }

    Ok(())
}

/// Copies the current file aside before it is replaced, keeping the most recent
/// [`KEEP_BACKUPS`].
fn rotate_backups(path: &Path) -> Result<Option<PathBuf>, UiError> {
    let Ok(previous) = fs::read(path) else {
        // No existing file: a first save has nothing to preserve.
        return Ok(None);
    };

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let backup = path.with_file_name(format!("{name}.{stamp}.bak"));
    fs::write(&backup, &previous).map_err(io)?;

    prune_backups(path, &name);
    Ok(Some(backup))
}

fn prune_backups(path: &Path, name: &str) {
    let Some(dir) = path.parent() else { return };
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    let prefix = format!("{name}.");
    let mut backups: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy())
                .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".bak"))
        })
        .collect();

    // Lexicographic order matches chronological order: the stamp is a
    // fixed-width-enough unix second count for the next few centuries.
    backups.sort();
    while backups.len() > KEEP_BACKUPS {
        if let Some(oldest) = backups.first().cloned() {
            let _ = fs::remove_file(&oldest);
            backups.remove(0);
        }
    }
}

fn io(e: std::io::Error) -> UiError {
    UiError::Io {
        detail: e.to_string(),
    }
}
