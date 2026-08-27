//! An unlocked vault and everything you can do to it.
//!
//! Holds no Tauri types, so every path below is reachable from an ordinary
//! test.

use std::path::{Path, PathBuf};

use serde::Serialize;
use vardo_kdbx::Credentials;
use vardo_vault::Vault;
use zeroize::Zeroizing;

use crate::entries;
use crate::model::{
    EntryDetail, EntrySummary, FieldEdit, GroupNode, SaveResult, StorageWarning, SyncOutcome,
    SyncPreview, VaultInfo,
};

/// Errors as the UI models them.
///
/// Deliberately coarse. A caller must not be able to distinguish a wrong
/// password from a corrupt file any more finely than KDBX itself does.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CoreError {
    WrongPassword,
    NotFound {
        path: String,
    },
    Corrupt {
        detail: String,
    },
    Unsupported {
        detail: String,
    },
    Io {
        detail: String,
    },
    Locked,
    /// The file we sync against is gone. Distinct from [`CoreError::NotFound`]
    /// so the UI can offer to re-create or re-point rather than saying the
    /// vault is missing, which it is not.
    RemoteMissing {
        path: String,
    },
    /// The other copy opens with a different password than this one. Reporting
    /// it as a wrong password would send the user to re-check the one that is
    /// working.
    RemotePassword,
    /// Ran out of space. Called out separately because the answer is different
    /// from every other IO failure, and because the vault on disk is intact.
    OutOfSpace,
    /// This vault syncs to Drive but no account is connected.
    DriveNotConnected,
    /// The Drive grant was revoked or expired. Not a transport failure, so
    /// "check your connection" would send the user somewhere useless.
    DriveAuthExpired {
        detail: String,
    },
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoreError::WrongPassword => write!(f, "wrong password"),
            CoreError::NotFound { path } => write!(f, "not found: {path}"),
            CoreError::Corrupt { detail }
            | CoreError::Unsupported { detail }
            | CoreError::Io { detail } => write!(f, "{detail}"),
            CoreError::Locked => write!(f, "the vault is locked"),
            CoreError::RemoteMissing { path } => {
                write!(f, "the copy at {path} is no longer there")
            }
            CoreError::RemotePassword => {
                write!(f, "the other copy uses a different password than this one")
            }
            CoreError::OutOfSpace => write!(
                f,
                "not enough space to save. The vault on disk is unchanged."
            ),
            CoreError::DriveNotConnected => {
                write!(f, "no Google account is connected")
            }
            CoreError::DriveAuthExpired { .. } => write!(
                f,
                "the Google Drive connection expired or was revoked. Reconnect to continue."
            ),
        }
    }
}

impl std::error::Error for CoreError {}

impl From<vardo_kdbx::Error> for CoreError {
    fn from(e: vardo_kdbx::Error) -> Self {
        use vardo_kdbx::Error as E;
        match e {
            // A wrong password and a tampered file are the same event here, and
            // that is a property of KDBX rather than a shortcut: the header MAC
            // is computed from the derived key, so a bad key and corrupt bytes
            // fail identically. Reporting them separately would invent a signal
            // the format deliberately withholds.
            E::HeaderAuthFailed | E::BlockAuthFailed { .. } => CoreError::WrongPassword,
            E::UnsupportedVersion { major, minor } => CoreError::Unsupported {
                detail: format!("KDBX {major}.{minor}"),
            },
            E::UnknownCipher(d) => CoreError::Unsupported {
                detail: format!("cipher {d}"),
            },
            E::UnknownKdf(d) => CoreError::Unsupported {
                detail: format!("KDF {d}"),
            },
            E::ResourceLimit(d) => CoreError::Unsupported { detail: d },
            other => CoreError::Corrupt {
                detail: other.to_string(),
            },
        }
    }
}

impl From<vardo_vault::EditError> for CoreError {
    fn from(e: vardo_vault::EditError) -> Self {
        use vardo_vault::EditError as E;
        match e {
            E::ReadOnly => CoreError::Unsupported {
                detail: "KDBX 3.1 databases are read-only in Vardo".into(),
            },
            E::NoSuchEntry(u) | E::NoSuchGroup(u) => CoreError::NotFound {
                path: u.to_base64(),
            },
            other => CoreError::Corrupt {
                detail: other.to_string(),
            },
        }
    }
}

/// Which stage of unlocking is running.
///
/// There is no fraction, because the KDF exposes no per-pass callback. An
/// indeterminate bar that is honest beats a determinate one that is invented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

impl Session {
    /// Reads and unlocks a vault.
    ///
    /// `on_phase` is called as each stage begins. The caller decides how to
    /// report it; this function does not know what a progress bar is.
    pub fn open(
        path: &Path,
        password: &str,
        mut on_phase: impl FnMut(Phase),
    ) -> Result<Session, CoreError> {
        on_phase(Phase::Reading);
        let bytes = std::fs::read(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => CoreError::NotFound {
                path: path.display().to_string(),
            },
            _ => CoreError::Io {
                detail: e.to_string(),
            },
        })?;

        on_phase(Phase::Deriving);
        let creds = Credentials::with_password(password);
        let doc = vardo_kdbx::open(&bytes, &creds)?;

        on_phase(Phase::Decrypting);
        let vault = Vault::open(&doc)?;

        on_phase(Phase::Parsing);
        let info = VaultInfo {
            name: vault.database_name(),
            path: path.display().to_string(),
            entry_count: vault.entries().len(),
            format: format!("KDBX {}.{}", doc.version().major, doc.version().minor),
            writable: vault.is_writable(),
            storage_warning: storage_warning(path),
        };

        Ok(Session {
            vault,
            info,
            creds,
            dirty: false,
        })
    }

    pub fn info(&self) -> &VaultInfo {
        &self.info
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn path(&self) -> PathBuf {
        PathBuf::from(&self.info.path)
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

    // --- writing ---

    pub fn update_entry(&mut self, uuid: &str, fields: &[FieldEdit]) -> Result<bool, CoreError> {
        let id = entries::parse_uuid(uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;
        let changed = self.vault.update_entry(id, |e| {
            for f in fields {
                match &f.value {
                    Some(value) => e.set_field(&f.key, value, f.protected),
                    None => e.remove_field(&f.key),
                }
            }
        })?;
        self.dirty |= changed;
        Ok(changed)
    }

    pub fn create_entry(&mut self, group: &str) -> Result<String, CoreError> {
        let id = entries::parse_uuid(group).ok_or(CoreError::NotFound {
            path: group.to_string(),
        })?;
        let uuid = self.vault.create_entry(id)?;
        self.dirty = true;
        self.info.entry_count = self.vault.entries().len();
        Ok(entries::uuid_text(uuid))
    }

    pub fn delete_entry(&mut self, uuid: &str) -> Result<&'static str, CoreError> {
        let id = entries::parse_uuid(uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;
        let outcome = self.vault.delete_entry(id)?;
        self.dirty = true;
        self.info.entry_count = self.vault.entries().len();
        Ok(match outcome {
            vardo_vault::Deletion::Recycled => "recycled",
            vardo_vault::Deletion::Purged => "purged",
        })
    }

    pub fn move_entry(&mut self, uuid: &str, group: &str) -> Result<(), CoreError> {
        let id = entries::parse_uuid(uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;
        let to = entries::parse_uuid(group).ok_or(CoreError::NotFound {
            path: group.to_string(),
        })?;
        self.vault.move_entry(id, to)?;
        self.dirty = true;
        Ok(())
    }

    /// Encodes, verifies the result re-opens, then replaces the file.
    pub fn save(&mut self) -> Result<SaveResult, CoreError> {
        let saved = crate::save::save(&self.vault, &self.creds, &self.path())?;
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

    pub fn sync_preview(&self, app_dir: &Path) -> Result<SyncPreview, CoreError> {
        let path = self.path();
        let point = crate::sync::load_point(app_dir, &path);
        let preview = crate::sync::preview(&path, &self.vault, &self.creds, point.as_ref())?;

        Ok(SyncPreview {
            remote: preview.remote,
            remote_unchanged: preview.remote_unchanged,
            first_sync: point.is_none(),
            changes: crate::review::changes(&preview.plan),
            conflicts: crate::review::conflicts(&preview.plan),
            warnings: preview.warnings,
        })
    }

    pub fn sync_apply(&mut self, app_dir: &Path, policy: &str) -> Result<SyncOutcome, CoreError> {
        let path = self.path();
        let point = crate::sync::load_point(app_dir, &path);
        let policy = parse_policy(policy)?;

        let (outcome, new_point) =
            crate::sync::apply(&path, &mut self.vault, &self.creds, point.as_ref(), policy)?;

        if let Some(p) = &new_point {
            crate::sync::store_point(app_dir, &path, p)?;
        }

        let (kind, detail) = crate::review::describe_outcome(&outcome);
        // A merge rewrites the in-memory vault, so anything on screen is stale
        // — including the entry count in the rail.
        self.dirty = false;
        self.info.entry_count = self.vault.entries().len();

        Ok(SyncOutcome {
            kind: kind.into(),
            detail,
            races_lost: 0,
            retries: 0,
        })
    }
}

fn parse_policy(policy: &str) -> Result<vardo_merge::ConflictPolicy, CoreError> {
    match policy {
        // The default. Keeps both versions rather than choosing, because a
        // password manager that silently discards one of two passwords is worse
        // than one that leaves you two to look at.
        "duplicate" => Ok(vardo_merge::ConflictPolicy::Duplicate),
        "keep-local" => Ok(vardo_merge::ConflictPolicy::KeepLocal),
        "keep-remote" => Ok(vardo_merge::ConflictPolicy::KeepRemote),
        "refuse" => Ok(vardo_merge::ConflictPolicy::Refuse),
        other => Err(CoreError::Unsupported {
            detail: format!("unknown conflict policy: {other}"),
        }),
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
        StorageRisk::Fuse { .. } => "fuse",
        StorageRisk::Network { .. } => "network",
        StorageRisk::NameSuggestsSync { .. } => "name",
    };
    Some(StorageWarning {
        kind: kind.into(),
        explanation: risk.explain(),
        remedy: risk.remedy().to_string(),
    })
}
