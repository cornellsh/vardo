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
    /// Could not reach the remote at all. Kept apart from every other failure
    /// because it is the only one where the right advice is "try again later"
    /// and where nothing is wrong with the vault, the password, or the grant.
    Offline {
        detail: String,
    },
    /// The stored copy changed since we loaded it, so saving would destroy
    /// whatever changed it.
    ///
    /// Not a failure in any technical sense - the write would succeed. It is
    /// refused because succeeding is the harm: someone opens the vault on a
    /// laptop, adds a password on their phone, then saves on the laptop.
    /// Nothing in that sequence looks dangerous to the person doing it.
    RemoteChanged {
        detail: String,
    },
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Every arm here is one short line. These strings land in banners
            // and side panels, and anything that wraps to a paragraph there is
            // a wall of text nobody reads. Longer causes go in a trailing
            // parenthetical, which the UI folds behind a disclosure.
            CoreError::WrongPassword => write!(f, "Wrong password"),
            CoreError::NotFound { path } => write!(f, "Not found: {path}"),
            CoreError::Corrupt { detail }
            | CoreError::Unsupported { detail }
            | CoreError::Io { detail } => write!(f, "{detail}"),
            CoreError::Locked => write!(f, "The vault is locked"),
            // The path is deliberately dropped: whatever screen shows this
            // already names the vault, and interpolating a long path is what
            // pushed the sentence onto a second line.
            CoreError::RemoteMissing { .. } => write!(f, "The other copy is gone"),
            CoreError::RemotePassword => write!(f, "The other copy has a different password"),
            CoreError::OutOfSpace => write!(f, "Not enough space to save"),
            CoreError::DriveNotConnected => write!(f, "No Google account connected"),
            // Shows `detail` rather than a fixed sentence. The generic version
            // discarded the specific explanation the caller had already
            // written - including which file it was about and, for a replaced
            // file, why a vault plainly still sitting in Drive is unreachable.
            CoreError::DriveAuthExpired { detail } => write!(f, "{detail}"),
            CoreError::Offline { detail } => write!(f, "{detail}"),
            CoreError::RemoteChanged { .. } => write!(f, "This vault changed elsewhere"),
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
use crate::source::VaultSource;

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
        };

        Ok(Session {
            vault,
            info,
            creds,
            dirty: false,
            source: source.clone(),
            loaded_version,
        })
    }

    pub fn source(&self) -> &VaultSource {
        &self.source
    }

    pub fn info(&self) -> &VaultInfo {
        &self.info
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

        let saved = crate::save::save_to(&self.vault, &self.creds, &self.source, ctx)?;

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
            changes: crate::review::changes(&preview.plan),
            conflicts: crate::review::conflicts(&preview.plan),
            warnings: preview.warnings,
        })
    }

    pub fn sync_apply(
        &mut self,
        app_dir: &Path,
        ctx: &crate::source::SourceContext,
        policy: &str,
    ) -> Result<SyncOutcome, CoreError> {
        let point = crate::sync::load_point(app_dir, &self.source);
        let policy = parse_policy(policy)?;

        let (outcome, new_point) = crate::sync::apply(
            &self.source,
            ctx,
            &mut self.vault,
            &self.creds,
            point.as_ref(),
            policy,
        )?;

        if let Some(p) = &new_point {
            crate::sync::store_point(app_dir, &self.source, p)?;
        }

        // A sync writes too, so the version we are comparing against has moved
        // and it moved because of us. Without this the next ordinary save
        // refuses, and the safety check turns into a bug that only appears
        // after a successful merge.
        self.loaded_version = self.source.current_version(ctx).ok().flatten();

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
        why: risk.why().to_string(),
        provider_hint: risk.provider_hint().map(str::to_string),
    })
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
