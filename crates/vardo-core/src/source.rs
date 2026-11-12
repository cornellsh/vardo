//! Where a vault lives.
//!
//! This exists because the first version of this program was written around a
//! wrong idea: that a vault *is* a filesystem path. `Session::open` took a
//! `&Path` and called `std::fs::read` on it. That worked on the machine it was
//! written on, where Google Drive happens to be mounted as a folder by rclone,
//! and it could not have worked anywhere else. There was no way to express "the
//! vault lives in Drive" at all, because the type said a vault is a path and
//! Drive files do not have one.
//!
//! So a vault is a [`VaultSource`], and reading and writing go through
//! [`vardo_sync::StorageBackend`], which already models storage that is not a
//! filesystem. Adding WebDAV later is a variant and a `with_backend` arm, not a
//! redesign.
//!
//! The closure shape of [`VaultSource::with_backend`] is not decoration.
//! `DriveBackend` borrows its HTTP transport, so it cannot outlive the call
//! that made it, and a refreshed access token has to be written back
//! afterwards. Scoping both to one function means no caller can forget the
//! second part.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vardo_sync::backend::StorageBackend;
use vardo_sync::local::LocalFile;

use crate::drive_auth::{self, DriveClient};
use crate::session::CoreError;

/// A place a vault can be.
///
/// Serialised into recents and per-vault config, so the variant names are a
/// compatibility surface. `tag`/`content` rather than the default so the JSON
/// stays readable and a new variant does not disturb the old ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum VaultSource {
    File { path: PathBuf },
    GoogleDrive { file_id: String, name: String },
}

impl VaultSource {
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::File { path: path.into() }
    }

    /// The name to show as the vault's title. Never the full path: it is
    /// usually too long for the space and the interesting part is at the end.
    pub fn short_name(&self) -> String {
        match self {
            Self::File { path } => path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            Self::GoogleDrive { name, .. } => {
                name.strip_suffix(".kdbx").unwrap_or(name).to_string()
            }
        }
    }

    /// Where it is, in words, for a second line under the name.
    pub fn location(&self) -> String {
        match self {
            Self::File { path } => path
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "this computer".into()),
            Self::GoogleDrive { .. } => "Google Drive".into(),
        }
    }

    /// True when the bytes are on this machine, so a mount-related warning
    /// could be meaningful. For a Drive vault it never is, and showing "this
    /// vault is on a userspace mount" for a file that is not on any mount would
    /// be noise that teaches people to ignore warnings.
    pub fn is_local_file(&self) -> bool {
        matches!(self, Self::File { .. })
    }

    pub fn as_path(&self) -> Option<&Path> {
        match self {
            Self::File { path } => Some(path),
            Self::GoogleDrive { .. } => None,
        }
    }

    /// A stable identity for keying sync state and per-vault settings.
    ///
    /// Must not change between launches, and must differ between two vaults
    /// that share a filename. For Drive the file id is already exactly this;
    /// for a file it is the absolute path.
    pub fn stable_key(&self) -> String {
        match self {
            Self::File { path } => format!("file:{}", path.display()),
            Self::GoogleDrive { file_id, .. } => format!("gdrive:{file_id}"),
        }
    }

    /// Run `f` against this source's storage.
    ///
    /// Any token refresh that happens inside is persisted before returning, so
    /// a long-lived session does not re-prompt on next launch.
    pub fn with_backend<R>(
        &self,
        ctx: &SourceContext,
        f: impl FnOnce(&mut dyn StorageBackend) -> Result<R, CoreError>,
    ) -> Result<R, CoreError> {
        match self {
            Self::File { path } => {
                let mut backend = LocalFile::new(path);
                f(&mut backend)
            }
            Self::GoogleDrive { file_id, .. } => {
                let client = ctx.client.as_ref().ok_or_else(|| CoreError::Unsupported {
                    detail: "this build has no Google client id, so a Drive vault cannot be opened"
                        .into(),
                })?;

                // Fails cleanly when the grant was revoked from the Google
                // account page, which is a thing users do and then forget.
                let tokens = drive_auth::access(client, &ctx.app_dir)?;

                let mut http = vardo_drive::ureq_transport::UreqTransport::new();
                let mut backend = vardo_drive::drive::DriveBackend::new(
                    &mut http,
                    client.config(),
                    tokens,
                    file_id.clone(),
                );

                let out = f(&mut backend);

                // Deliberately after `f`, and not conditional on its success: a
                // refresh that happened during a failed read is still worth
                // keeping, and dropping it would make the next attempt fail the
                // same way.
                if backend.tokens_updated {
                    let _ = drive_auth::store_tokens(&ctx.app_dir, backend.tokens());
                }

                out
            }
        }
    }

    /// The vault's current bytes.
    ///
    /// A missing file is [`CoreError::NotFound`] here, not `RemoteMissing`,
    /// even though the underlying storage error is identical. The difference is
    /// which question the user asked: opening a vault that is not there means
    /// "that vault is gone", while syncing against a file that is not there
    /// means "the copy you sync with is gone, your vault is fine". Collapsing
    /// the two produced a message telling someone their intact vault had
    /// vanished.
    pub fn read(&self, ctx: &SourceContext) -> Result<Vec<u8>, CoreError> {
        self.read_versioned(ctx).map(|(bytes, _)| bytes)
    }

    /// The bytes, and the stored copy's version at the moment they were read.
    ///
    /// The two come back together deliberately. Asking for the version in a
    /// second call would leave a gap in which exactly the write we care about
    /// could land unseen, and the check would then report a safety it had not
    /// established, which is worse than not checking.
    pub fn read_versioned(
        &self,
        ctx: &SourceContext,
    ) -> Result<(Vec<u8>, Option<String>), CoreError> {
        let where_it_is = self.describe_for_error();
        self.with_backend(ctx, |backend| {
            backend
                .read()
                .map(|(bytes, state)| (bytes, Some(state.version)))
                .map_err(|e| match e {
                    vardo_sync::SyncError::NotFound => CoreError::NotFound {
                        path: where_it_is.clone(),
                    },
                    other => crate::sync::map_sync_error(other, &where_it_is),
                })
        })
    }

    /// The stored copy's current version, without downloading it.
    ///
    /// Cheap on Drive (one metadata call) and near-free on a file, which is
    /// what makes it reasonable to check on every save.
    pub fn current_version(&self, ctx: &SourceContext) -> Result<Option<String>, CoreError> {
        let where_it_is = self.describe_for_error();
        self.with_backend(ctx, |backend| match backend.stat() {
            Ok(state) => Ok(Some(state.version)),
            // A vault that is not there yet cannot have been changed by anyone
            // else, so this is not a reason to block a save - it is the first
            // one.
            Err(vardo_sync::SyncError::NotFound) => Ok(None),
            Err(e) => Err(crate::sync::map_sync_error(e, &where_it_is)),
        })
    }

    /// How to name the other copy on the merge review screen.
    ///
    /// Not `StorageBackend::describe`, which returns `drive:1mUbZAIGW3ec2ok...`.
    /// That is the correct identifier and exactly the wrong thing to show a
    /// person deciding which version of their passwords to keep.
    pub fn describe_for_display(&self) -> String {
        match self {
            Self::File { path } => path.display().to_string(),
            Self::GoogleDrive { name, .. } => format!("{name} in Google Drive"),
        }
    }

    /// How to refer to this source inside an error sentence.
    pub fn describe_for_error(&self) -> String {
        match self {
            Self::File { path } => path.display().to_string(),
            Self::GoogleDrive { name, .. } => format!("{name} in Google Drive"),
        }
    }
}

/// What the process needs on hand to reach any source.
///
/// Passed rather than looked up globally so tests can build one pointing at a
/// temporary directory, and so the Drive path is unreachable by accident.
pub struct SourceContext {
    pub app_dir: PathBuf,
    pub client: Option<DriveClient>,
}

impl SourceContext {
    pub fn local_only(app_dir: impl Into<PathBuf>) -> Self {
        Self {
            app_dir: app_dir.into(),
            client: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_key_distinguishes_sources_that_share_a_name() {
        // The failure this prevents: two vaults both called passwords.kdbx,
        // one local and one in Drive, silently sharing one sync point and so
        // each convincing the other it had already synced.
        let a = VaultSource::file("/home/a/passwords.kdbx");
        let b = VaultSource::GoogleDrive {
            file_id: "1AbC".into(),
            name: "passwords.kdbx".into(),
        };
        assert_ne!(a.stable_key(), b.stable_key());
    }

    #[test]
    fn stable_key_survives_a_rename_in_drive() {
        // Drive lets a file be renamed without changing its id. Keying on the
        // name would lose the sync point and force a full re-merge.
        let before = VaultSource::GoogleDrive {
            file_id: "1AbC".into(),
            name: "passwords.kdbx".into(),
        };
        let after = VaultSource::GoogleDrive {
            file_id: "1AbC".into(),
            name: "family-passwords.kdbx".into(),
        };
        assert_eq!(before.stable_key(), after.stable_key());
    }

    #[test]
    fn short_name_drops_the_extension_on_both_kinds() {
        assert_eq!(
            VaultSource::file("/x/passwords.kdbx").short_name(),
            "passwords"
        );
        assert_eq!(
            VaultSource::GoogleDrive {
                file_id: "1".into(),
                name: "passwords.kdbx".into()
            }
            .short_name(),
            "passwords"
        );
    }

    #[test]
    fn a_drive_vault_is_not_a_local_file() {
        // Guards the storage warning: a Drive vault is on no mount at all, and
        // warning about a userspace mount there would be nonsense.
        assert!(!VaultSource::GoogleDrive {
            file_id: "1".into(),
            name: "v.kdbx".into()
        }
        .is_local_file());
        assert!(VaultSource::file("/x/v.kdbx").is_local_file());
    }

    #[test]
    fn round_trips_through_json() {
        for source in [
            VaultSource::file("/home/a/passwords.kdbx"),
            VaultSource::GoogleDrive {
                file_id: "1AbC".into(),
                name: "v.kdbx".into(),
            },
        ] {
            let json = serde_json::to_string(&source).unwrap();
            let back: VaultSource = serde_json::from_str(&json).unwrap();
            assert_eq!(source, back);
        }
    }

    #[test]
    fn opening_a_drive_vault_without_a_client_id_says_so() {
        // Rather than a generic auth failure, which would send someone looking
        // at their Google account for a problem that is in the build.
        let ctx = SourceContext::local_only("/tmp/nonexistent-vardo-test");
        let err = VaultSource::GoogleDrive {
            file_id: "1".into(),
            name: "v.kdbx".into(),
        }
        .read(&ctx)
        .unwrap_err();
        assert!(
            matches!(err, CoreError::Unsupported { .. }),
            "expected Unsupported, got {err:?}"
        );
    }
}

#[cfg(test)]
mod display_tests {
    use super::*;

    #[test]
    fn a_drive_vault_is_never_described_by_its_file_id() {
        // Caught by running the live round trip: the merge review screen was
        // about to label the other copy `drive:1mUbZAIGW3ec2okLFEU6B9FT8zDoR3T6C`
        // while asking which version of someone's passwords to keep.
        let source = VaultSource::GoogleDrive {
            file_id: "1mUbZAIGW3ec2okLFEU6B9FT8zDoR3T6C".into(),
            name: "passwords.kdbx".into(),
        };
        let shown = source.describe_for_display();
        assert!(!shown.contains("1mUbZ"), "must not show a file id: {shown}");
        assert!(shown.contains("passwords.kdbx"), "got: {shown}");
        assert!(shown.contains("Google Drive"), "got: {shown}");
    }

    #[test]
    fn a_local_vault_is_described_by_its_path() {
        // The full path, not just the name: someone choosing between two
        // copies needs to know which directory each is in.
        let shown = VaultSource::file("/home/me/vaults/passwords.kdbx").describe_for_display();
        assert!(shown.contains("/home/me/vaults"), "got: {shown}");
    }
}
