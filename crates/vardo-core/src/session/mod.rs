//! An unlocked vault and everything you can do to it.
//!
//! Contract: [`Session`] is the only holder of a decrypted vault. Locking drops
//! it (`inv:lock-drops-plaintext`). Secrets leave only through
//! [`Session::field_value`] and [`Session::history_password`] as `Zeroizing`
//! buffers (`inv:secrets-stay-in-rust`). Saving goes through [`save`], which
//! verifies before replacing and refuses a moved remote
//! (`inv:atomic-verified-save`).
//!
//! Holds no Tauri types, so every path below is reachable from an ordinary
//! test. Reading and lifecycle live here, editing in `edit.rs`, and files
//! (icons, attachments out) in `files.rs`.

use std::path::{Path, PathBuf};

use serde::Serialize;
use vardo_kdbx::Credentials;
use vardo_vault::Vault;
use zeroize::Zeroizing;

use crate::model::{
    EntryDetail, EntrySummary, GroupNode, SaveResult, StorageKind, StorageWarning, SyncOutcome,
    SyncPreview, VaultInfo,
};
use crate::source::VaultSource;
use crate::CoreError;

mod edit;
pub mod entries;
mod files;
mod instance;
pub mod save;

/// Which stage of unlocking is running.
///
/// There is no fraction, because the KDF exposes no per-pass callback. An
/// indeterminate bar that is honest beats a determinate one that is invented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Reading,
    Deriving,
    Decrypting,
    Parsing,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Reading => "reading",
            Phase::Deriving => "deriving",
            Phase::Decrypting => "decrypting",
            Phase::Parsing => "parsing",
        }
    }
}

/// An unlocked vault.
pub struct Session {
    vault: Vault,
    info: VaultInfo,
    /// Held for the session because saving re-derives the key, and prompting on
    /// every save would be unusable. Lives in a `Zeroizing` buffer inside
    /// `Credentials` and dies with the session.
    creds: Credentials,
    dirty: bool,
    /// Where the bytes came from and where they go back to. Kept on the
    /// session rather than passed in per call, so there is no way to save a
    /// vault to somewhere it was not opened from.
    source: VaultSource,
    /// The stored copy's version when we read it.
    ///
    /// Saving compares against this and refuses if it has moved. Without it a
    /// save is a blind overwrite, which across two devices is not an edge case
    /// but the ordinary way a synced vault gets used.
    loaded_version: Option<String>,
    /// Tells a second window on this vault that it is open here.
    _claim: instance::Claim,
}

impl Session {
    /// Reads and unlocks a vault.
    ///
    /// `on_phase` is called as each stage begins. The caller decides how to
    /// report it; this function does not know what a progress bar is.
    pub fn open(
        source: &VaultSource,
        ctx: &crate::source::SourceContext,
        password: &str,
        mut on_phase: impl FnMut(Phase),
    ) -> Result<Session, CoreError> {
        on_phase(Phase::Reading);
        // Reading goes through the source's backend rather than the
        // filesystem, which is the entire reason a Drive vault can be opened
        // on a machine that has no Drive folder.
        // The version comes back with the bytes so a later save can tell
        // whether anything else has written in the meantime.
        let (bytes, loaded_version) = source.read_versioned(ctx)?;

        on_phase(Phase::Deriving);
        let creds = Credentials::with_password(password);
        let doc = vardo_kdbx::open(&bytes, &creds)?;

        on_phase(Phase::Decrypting);
        let vault = Vault::open(&doc)?;

        on_phase(Phase::Parsing);
        let (claim, open_elsewhere) = instance::Claim::take(&ctx.app_dir, source);
        let info = VaultInfo {
            name: vault.database_name(),
            path: source.short_name(),
            location: source.location(),
            source: source.clone(),
            entry_count: vault.entries().len(),
            format: format!("KDBX {}.{}", doc.version().major, doc.version().minor),
            writable: vault.is_writable(),
            // Only a local file can be on a risky mount. Running the check on a
            // Drive vault would either warn about nothing or, worse, warn about
            // whatever mount the config directory happens to be on.
            storage_warning: source.as_path().and_then(storage_warning),
            open_elsewhere,
        };

        Ok(Session {
            vault,
            info,
            creds,
            dirty: false,
            source: source.clone(),
            loaded_version,
            _claim: claim,
        })
    }

    pub fn source(&self) -> &VaultSource {
        &self.source
    }

    pub fn info(&self) -> &VaultInfo {
        &self.info
    }

    /// The open vault, for read-only views inside this crate.
    pub(crate) fn vault(&self) -> &Vault {
        &self.vault
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Present only for a vault that is a file on this machine. Callers that
    /// need somewhere to put a backup have to cope with `None`, which is the
    /// point: a Drive vault has no directory to write beside.
    pub fn path(&self) -> Option<PathBuf> {
        self.source.as_path().map(PathBuf::from)
    }

    /// Explicit rather than relying on drop order, so the intent is legible.
    pub fn lock(&mut self) {
        self.vault.lock();
    }

    // --- reading ---

    pub fn groups(&self) -> GroupNode {
        entries::group_tree(&self.vault)
    }

    pub fn list_entries(&self, group: Option<&str>, query: Option<&str>) -> Vec<EntrySummary> {
        entries::list(&self.vault, group, query)
    }

    pub fn list_entries_sorted(
        &self,
        group: Option<&str>,
        query: Option<&str>,
        sort: &str,
    ) -> Vec<EntrySummary> {
        entries::list_sorted(&self.vault, group, query, entries::Sort::parse(sort))
    }

    pub fn entry(&self, uuid: &str) -> Result<EntryDetail, CoreError> {
        entries::detail(&self.vault, uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })
    }

    /// The plaintext of one field.
    ///
    /// The only way a secret leaves this crate. Returns a `Zeroizing` buffer so
    /// the caller has to be deliberate about how long it keeps it, and is named
    /// so that reading a secret is visible at the call site.
    pub fn field_value(&self, uuid: &str, key: &str) -> Result<Zeroizing<String>, CoreError> {
        let entry = entries::find(&self.vault, uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;
        let value = entry.field(key).ok_or(CoreError::NotFound {
            path: key.to_string(),
        })?;
        Ok(Zeroizing::new(value.expose().to_string()))
    }

    /// The current one-time code, if the entry carries a seed.
    ///
    /// Only the derived code leaves this crate. The seed itself is a
    /// long-lived shared secret — strictly more valuable than any single code,
    /// because it generates every future one — so it is read here and never
    /// returned.
    pub fn totp(&self, uuid: &str) -> Result<Option<crate::model::TotpCode>, CoreError> {
        let entry = entries::find(&self.vault, uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;

        let read = |key: &str| entry.field(key).map(|f| f.expose().to_string());
        let otp = read("otp");
        let seed = read("TOTP Seed");
        let settings = read("TOTP Settings");

        let Some(totp) =
            crate::totp::Totp::from_fields(otp.as_deref(), seed.as_deref(), settings.as_deref())
        else {
            return Ok(None);
        };

        let code = totp.now();
        Ok(Some(crate::model::TotpCode {
            code: code.code,
            seconds_remaining: code.seconds_remaining,
            period: code.period,
        }))
    }

    /// Encodes, verifies the result re-opens, then writes it back to wherever
    /// the vault came from.
    pub fn save(&mut self, ctx: &crate::source::SourceContext) -> Result<SaveResult, CoreError> {
        // Refuse rather than overwrite when the stored copy has moved. The
        // write would succeed; that is exactly the problem.
        if let Some(loaded) = &self.loaded_version {
            if let Ok(Some(current)) = self.source.current_version(ctx) {
                if &current != loaded {
                    return Err(CoreError::RemoteChanged {
                        detail: self.source.describe_for_display(),
                    });
                }
            }
            // A failure to *check* is deliberately not a failure to save. Being
            // unable to reach the remote is common, and refusing to write
            // someone's password change because of it would be its own kind of
            // data loss. The write below reports its own failure honestly.
        }

        let saved = crate::session::save::save_to(&self.vault, &self.creds, &self.source, ctx)?;

        // Our own write moved the version, so the next save must compare
        // against the new one. Leaving this stale made every save after the
        // first refuse, which is how a safety check becomes a bug.
        self.loaded_version = self.source.current_version(ctx).ok().flatten();
        self.dirty = false;
        Ok(SaveResult {
            bytes_written: saved.bytes_written,
            backup: saved.backup.map(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            }),
        })
    }

    // --- sync ---

    pub fn sync_preview(
        &self,
        app_dir: &Path,
        ctx: &crate::source::SourceContext,
    ) -> Result<SyncPreview, CoreError> {
        let point = crate::sync::load_point(app_dir, &self.source);
        let preview =
            crate::sync::preview(&self.source, ctx, &self.vault, &self.creds, point.as_ref())?;

        Ok(SyncPreview {
            remote: preview.remote,
            remote_unchanged: preview.remote_unchanged,
            first_sync: point.is_none(),
            changes: crate::sync::review::changes(&preview.plan),
            conflicts: crate::sync::review::conflicts(&preview.plan),
            warnings: preview.warnings,
        })
    }

    pub fn sync_apply(
        &mut self,
        app_dir: &Path,
        ctx: &crate::source::SourceContext,
        policy: crate::model::ConflictPolicy,
    ) -> Result<SyncOutcome, CoreError> {
        let point = crate::sync::load_point(app_dir, &self.source);

        let (outcome, new_point) = crate::sync::apply(
            &self.source,
            ctx,
            &mut self.vault,
            &self.creds,
            point.as_ref(),
            policy.into(),
        )?;

        if let Some(p) = &new_point {
            crate::sync::store_point(app_dir, &self.source, p)?;
        }

        // A sync writes too, so the version we are comparing against has moved
        // and it moved because of us. Without this the next ordinary save
        // refuses, and the safety check turns into a bug that only appears
        // after a successful merge.
        self.loaded_version = self.source.current_version(ctx).ok().flatten();

        let (kind, detail) = crate::sync::review::describe_outcome(&outcome);
        // A merge rewrites the in-memory vault, so anything on screen is stale
        // — including the entry count in the rail.
        self.dirty = false;
        self.info.entry_count = self.vault.entries().len();

        Ok(SyncOutcome {
            kind,
            detail,
            races_lost: 0,
            retries: 0,
        })
    }
}

/// Checked at unlock rather than at save, because the first save is already too
/// late: by then the user has typed a password into a file the sync daemon may
/// be about to fork.
fn storage_warning(path: &Path) -> Option<StorageWarning> {
    use vardo_sync::StorageRisk;
    let risk = vardo_sync::storage_risk::assess(path);
    let kind = match &risk {
        StorageRisk::Local => return None,
        StorageRisk::Fuse { .. } => StorageKind::Fuse,
        StorageRisk::Network { .. } => StorageKind::Network,
        StorageRisk::NameSuggestsSync { .. } => StorageKind::Name,
    };
    Some(StorageWarning {
        kind,
        explanation: risk.explain(),
        remedy: risk.remedy().to_string(),
        why: risk.why().to_string(),
        provider_hint: risk.provider_hint().map(str::to_string),
    })
}

impl Session {
    /// What is wrong with the passwords in this vault.
    ///
    /// Computed on demand rather than cached: it is a single pass over entries
    /// already in memory, and a cache would need invalidating on every edit,
    /// which is exactly when the answer has changed and matters most.
    pub fn health(&self) -> crate::health::Report {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        crate::health::review(&self.vault, now)
    }
}

impl Session {
    pub fn history(&self, uuid: &str) -> Result<Vec<crate::model::HistoryVersion>, CoreError> {
        entries::history(&self.vault, uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })
    }

    /// An old password, for putting on the clipboard.
    ///
    /// Returns `Zeroizing` so it does not outlive the call by accident, the
    /// same as the current password. An old password is not a lesser secret:
    /// it is very often still in use somewhere the user has forgotten about.
    pub fn history_password(
        &self,
        uuid: &str,
        index: usize,
    ) -> Result<zeroize::Zeroizing<String>, CoreError> {
        entries::history_password(&self.vault, uuid, index)
            .map(zeroize::Zeroizing::new)
            .ok_or(CoreError::NotFound {
                path: uuid.to_string(),
            })
    }
}
