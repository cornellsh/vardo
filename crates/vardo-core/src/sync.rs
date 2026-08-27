//! Sync orchestration and sync-point persistence.
//!
//! # Why preview and apply are separate
//!
//! `vardo_sync::sync` will merge and upload in one call. That is right for an
//! unattended background sync, and wrong for the first time a user sees what a
//! merge is about to do to their passwords. `vardo_merge::plan` is pure by
//! design so the result can be shown before anything is touched — KeePassXC
//! only added a preview dialog in PR #10173, and here it is the only way in.
//!
//! So [`preview`] reads the remote and returns the plan without mutating
//! anything, and [`apply`] performs the real sync afterwards.
//!
//! # The base is not optional
//!
//! [`SyncPoint::base`] is the three-way merge base. Without it only a two-way
//! merge is possible, and then an addition on one side is indistinguishable
//! from a deletion on the other — so nothing can ever be deleted safely. It is
//! stored beside the app config, encrypted exactly as it came off the wire,
//! because it *is* a vault.

use std::fs;
use std::path::{Path, PathBuf};

use vardo_kdbx::Credentials;
use vardo_sync::backend::StorageBackend;
use vardo_sync::local::LocalFile;
use vardo_sync::protocol::{self, Outcome, SyncPoint};
use vardo_vault::Vault;

use crate::session::CoreError as UiError;

/// Where the sync point for a given vault lives.
///
/// Keyed by a hash of the absolute path so two vaults with the same file name
/// do not share a base, which would merge one vault against the other's
/// history.
fn point_path(app_dir: &Path, vault: &Path) -> PathBuf {
    let key = stable_key(&vault.to_string_lossy());
    app_dir.join("syncpoints").join(format!("{key}.base"))
}

/// FNV-1a. Not a security boundary — just a filename, and it must be stable
/// across runs, which `DefaultHasher` explicitly is not.
fn stable_key(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn load_point(app_dir: &Path, vault: &Path) -> Option<SyncPoint> {
    let path = point_path(app_dir, vault);
    let bytes = fs::read(&path).ok()?;
    let version = fs::read_to_string(path.with_extension("version")).ok()?;
    Some(SyncPoint {
        version: version.trim().to_string(),
        base: bytes,
    })
}

pub fn store_point(app_dir: &Path, vault: &Path, point: &SyncPoint) -> Result<(), UiError> {
    let path = point_path(app_dir, vault);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io)?;
    }
    fs::write(&path, &point.base).map_err(io)?;
    fs::write(path.with_extension("version"), &point.version).map_err(io)?;
    Ok(())
}

/// A merge plan, rendered for review, plus what it is a plan against.
pub struct Preview {
    pub plan: vardo_merge::MergePlan,
    pub remote: String,
    /// True when the remote is byte-identical to our sync point, so there is
    /// nothing to fold in and the only question is whether to push.
    pub remote_unchanged: bool,
    /// Things that do not stop the sync but change what it will do.
    pub warnings: Vec<crate::model::SyncWarning>,
}

/// Reads the remote and plans a merge, mutating nothing.
pub fn preview(
    vault_path: &Path,
    local: &Vault,
    creds: &Credentials,
    point: Option<&SyncPoint>,
) -> Result<Preview, UiError> {
    let mut backend = LocalFile::new(vault_path);
    let remote_description = backend.describe();

    let mut warnings = Vec::new();

    let state = match backend.stat() {
        Ok(state) => state,
        Err(vardo_sync::SyncError::NotFound) => {
            return Err(UiError::RemoteMissing {
                path: vault_path.display().to_string(),
            })
        }
        Err(e) => {
            return Err(UiError::Io {
                detail: format!("cannot reach the other copy: {e}"),
            })
        }
    };

    if let Some(w) = clock_skew_warning(&state) {
        warnings.push(w);
    }

    let unchanged = point.is_some_and(|p| p.version == state.version);
    if unchanged {
        return Ok(Preview {
            plan: vardo_merge::MergePlan {
                changes: Vec::new(),
                conflicts: Vec::new(),
            },
            remote: remote_description,
            remote_unchanged: true,
            warnings,
        });
    }

    let (bytes, _) = backend.read().map_err(|e| UiError::Io {
        detail: e.to_string(),
    })?;

    // We are already unlocked, so our own password is right by construction.
    // An authentication failure here therefore means the *other copy* uses a
    // different password — reporting that as "wrong password" would send the
    // user off to re-check the one that is working fine.
    let remote_doc = vardo_kdbx::open(&bytes, creds).map_err(|e| match UiError::from(e) {
        UiError::WrongPassword => UiError::RemotePassword,
        other => other,
    })?;
    let remote_vault = Vault::open(&remote_doc)?;

    if !remote_vault.is_writable() {
        warnings.push(crate::model::SyncWarning {
            kind: "remote-read-only".into(),
            detail: "The other copy is KDBX 3.1. Vardo can merge it but cannot write \
                     that format, so the merged result will be saved as KDBX 4."
                .into(),
        });
    }

    // A sync point recorded under a password that has since changed cannot
    // serve as a merge base. Falling back to a two-way merge is safe — nothing
    // is ever deleted without a base — but doing it *silently* means deletions
    // stop propagating and nobody is told why. So it is a warning, not a shrug.
    let base_vault = match point {
        Some(p) => match vardo_kdbx::open(&p.base, creds).and_then(|d| Vault::open(&d)) {
            Ok(v) => Some(v),
            Err(e) => {
                warnings.push(crate::model::SyncWarning {
                    kind: "no-base".into(),
                    detail: format!(
                        "The record of the last sync could not be read ({e}), so this is a \
                         two-way merge: entries will be combined, and nothing will be \
                         deleted even if it was deleted on the other device."
                    ),
                });
                None
            }
        },
        None => None,
    };

    // A refusal means the remote is a different database, not a stale copy of
    // this one. Surfaced as its own error rather than an empty plan, because
    // "nothing to merge" and "that is not your vault" must not look alike.
    let plan = vardo_merge::plan(base_vault.as_ref(), local, &remote_vault).map_err(|r| {
        UiError::Unsupported {
            detail: format!("{r}. Check that the sync target is the right file."),
        }
    })?;

    Ok(Preview {
        plan,
        remote: remote_description,
        remote_unchanged: false,
        warnings,
    })
}

/// Warn when the other copy's timestamp is implausibly far from our clock.
///
/// Merge resolves by modification time, so a device with a badly wrong clock
/// wins every conflict on both machines — silently, and in a way that looks
/// like the merge is malfunctioning rather than like a clock problem.
fn clock_skew_warning(state: &vardo_sync::RemoteState) -> Option<crate::model::SyncWarning> {
    /// An hour. Small enough to catch a wrong timezone written into a
    /// timestamp, large enough not to fire on ordinary drift.
    const TOLERANCE: i64 = 3600;

    let remote = state.modified_unix?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;

    let skew = remote - now;
    if skew <= TOLERANCE {
        return None;
    }
    Some(crate::model::SyncWarning {
        kind: "clock-skew".into(),
        detail: format!(
            "The other copy is dated {} in the future. Merges are decided by \
             modification time, so a device with a wrong clock wins every conflict.",
            humanise(skew)
        ),
    })
}

fn humanise(seconds: i64) -> String {
    match seconds {
        s if s < 7200 => format!("{} minutes", s / 60),
        s if s < 172_800 => format!("{} hours", s / 3600),
        s => format!("{} days", s / 86_400),
    }
}

/// Performs the sync for real, mutating `local` to the merged result.
pub fn apply(
    vault_path: &Path,
    local: &mut Vault,
    creds: &Credentials,
    point: Option<&SyncPoint>,
    policy: vardo_merge::ConflictPolicy,
) -> Result<(Outcome, Option<SyncPoint>), UiError> {
    let mut backend = LocalFile::new(vault_path);
    let options = protocol::Options {
        policy,
        ..Default::default()
    };

    let report = protocol::sync(
        &mut backend,
        local,
        creds,
        point,
        &options,
        &mut vardo_kdbx::write::OsRng,
    )
    .map_err(|e| UiError::Io {
        detail: e.to_string(),
    })?;

    Ok((report.outcome, report.point))
}

fn io(e: std::io::Error) -> UiError {
    UiError::Io {
        detail: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_point_keys_are_stable_across_runs() {
        // DefaultHasher is seeded per-process, so using it here would give a
        // vault a different base file on every launch — which silently means
        // "no base", which silently means nothing can ever be deleted.
        assert_eq!(
            stable_key("/home/a/passwords.kdbx"),
            stable_key("/home/a/passwords.kdbx")
        );
        assert_ne!(
            stable_key("/home/a/passwords.kdbx"),
            stable_key("/home/b/passwords.kdbx")
        );
    }

    #[test]
    fn same_filename_in_different_directories_does_not_collide() {
        let dir = Path::new("/tmp/app");
        assert_ne!(
            point_path(dir, Path::new("/one/passwords.kdbx")),
            point_path(dir, Path::new("/two/passwords.kdbx")),
        );
    }

    #[test]
    fn a_stored_point_round_trips() {
        let tmp = std::env::temp_dir().join("vardo-syncpoint-test");
        let _ = fs::remove_dir_all(&tmp);
        let vault = Path::new("/somewhere/passwords.kdbx");

        let point = SyncPoint {
            version: "rev-42".into(),
            base: vec![1, 2, 3, 4],
        };
        store_point(&tmp, vault, &point).unwrap();

        let loaded = load_point(&tmp, vault).expect("a stored point must load");
        assert_eq!(loaded.version, "rev-42");
        assert_eq!(loaded.base, vec![1, 2, 3, 4]);
    }

    #[test]
    fn a_missing_point_is_none_rather_than_an_error() {
        // A first sync has no base, and that must be an ordinary state.
        let tmp = std::env::temp_dir().join("vardo-syncpoint-absent");
        let _ = fs::remove_dir_all(&tmp);
        assert!(load_point(&tmp, Path::new("/nope.kdbx")).is_none());
    }
}
