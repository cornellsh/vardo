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

use crate::source::{SourceContext, VaultSource};
use vardo_sync::protocol::{self, Outcome, SyncPoint};
use vardo_vault::Vault;

use crate::session::CoreError as UiError;

/// Where the sync point for a given vault lives.
///
/// Keyed by a hash of the absolute path so two vaults with the same file name
/// do not share a base, which would merge one vault against the other's
/// history.
fn point_path(app_dir: &Path, vault: &VaultSource) -> PathBuf {
    // Keyed on the source's identity, not its name, so a Drive file that gets
    // renamed keeps its sync point and two vaults called passwords.kdbx from
    // different places never share one.
    let key = stable_key(&vault.stable_key());
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

pub fn load_point(app_dir: &Path, vault: &VaultSource) -> Option<SyncPoint> {
    let path = point_path(app_dir, vault);
    let bytes = fs::read(&path).ok()?;
    let version = fs::read_to_string(path.with_extension("version")).ok()?;
    Some(SyncPoint {
        version: version.trim().to_string(),
        base: bytes,
    })
}

pub fn store_point(app_dir: &Path, vault: &VaultSource, point: &SyncPoint) -> Result<(), UiError> {
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

/// Turns a storage failure into something a person can act on.
///
/// Every arm here is a real thing that happens to a remote vault, and the
/// distinctions matter more than they look. "Not authorised" and "no network"
/// both surface as a failed read, but one is fixed by reconnecting and the
/// other by waiting; telling them apart is the whole job of this function.
///
/// `where_it_is` is the source described in words, so the sentence names the
/// actual file rather than saying "the remote".
pub fn map_sync_error(e: vardo_sync::SyncError, where_it_is: &str) -> UiError {
    use vardo_sync::SyncError as S;
    match e {
        S::NotFound => UiError::RemoteMissing {
            path: where_it_is.to_string(),
        },

        // The grant was revoked, or the refresh token was invalidated by a
        // password change on the Google account. Neither is recoverable by
        // retrying, and both are fixed in the same place, so say where.
        // Every message here fits one line in the panel it appears in.
        // A message that wraps to four lines is a wall, and a wall is not read
        // at the exact moment someone is worried about their passwords. The
        // cause goes in the parenthetical, which the UI folds away.
        S::Unauthorized(detail) => UiError::DriveAuthExpired {
            detail: format!("Drive access was withdrawn ({detail})"),
        },

        // Another client replaced the file instead of updating it, minting a
        // new id. Our per-file grant points at something that no longer
        // exists, and no amount of retrying will bring it back.
        S::IdentityLost(detail) => UiError::DriveAuthExpired {
            detail: format!("That file was replaced by another app ({detail})"),
        },

        // The overwhelmingly common cause is no network. Saying so beats
        // "transport failure", which reads as a bug in the program.
        S::Transport(detail) => UiError::Offline {
            detail: format!("Could not reach Google Drive ({detail})"),
        },

        S::RateLimited { retry_after_secs } => UiError::Offline {
            detail: match retry_after_secs {
                // The wait goes in the headline, not the parenthetical: the UI
                // folds parentheses away, and "how long" is the only thing the
                // user actually wants from this message.
                Some(s) => format!("Rate limited by Google, retry in {s}s"),
                None => "Google is rate limiting Vardo".into(),
            },
        },

        S::Other(detail) => UiError::Io {
            detail: format!("Could not read {where_it_is} ({detail})"),
        },
    }
}

/// Reads the remote and plans a merge, mutating nothing.
pub fn preview(
    source: &VaultSource,
    ctx: &SourceContext,
    local: &Vault,
    creds: &Credentials,
    point: Option<&SyncPoint>,
) -> Result<Preview, UiError> {
    source.with_backend(ctx, |backend| {
        preview_with(backend, source, local, creds, point)
    })
}

fn preview_with(
    backend: &mut dyn vardo_sync::StorageBackend,
    source: &VaultSource,
    local: &Vault,
    creds: &Credentials,
    point: Option<&SyncPoint>,
) -> Result<Preview, UiError> {
    let remote_description = source.describe_for_display();
    let where_it_is = source.describe_for_error();

    let mut warnings = Vec::new();

    let state = match backend.stat() {
        Ok(state) => state,
        Err(e) => return Err(map_sync_error(e, &where_it_is)),
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

    let (bytes, _) = backend
        .read()
        .map_err(|e| map_sync_error(e, &where_it_is))?;

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
    source: &VaultSource,
    ctx: &SourceContext,
    local: &mut Vault,
    creds: &Credentials,
    point: Option<&SyncPoint>,
    policy: vardo_merge::ConflictPolicy,
) -> Result<(Outcome, Option<SyncPoint>), UiError> {
    let where_it_is = source.describe_for_error();
    let options = protocol::Options {
        policy,
        ..Default::default()
    };

    source.with_backend(ctx, |backend| {
        let report = protocol::sync(
            backend,
            local,
            creds,
            point,
            &options,
            &mut vardo_kdbx::write::OsRng,
        )
        .map_err(|e| match e {
            // The protocol's own failures are already written for a person.
            // Only storage failures need translating, and losing that
            // distinction is how "you are offline" became "transport failure".
            protocol::Error::Storage(inner) => map_sync_error(inner, &where_it_is),
            other => UiError::Io {
                detail: other.to_string(),
            },
        })?;

        Ok((report.outcome, report.point))
    })
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
            point_path(dir, &VaultSource::file("/one/passwords.kdbx")),
            point_path(dir, &VaultSource::file("/two/passwords.kdbx")),
        );
    }

    #[test]
    fn a_stored_point_round_trips() {
        let tmp = std::env::temp_dir().join("vardo-syncpoint-test");
        let _ = fs::remove_dir_all(&tmp);
        let vault = VaultSource::file("/somewhere/passwords.kdbx");

        let point = SyncPoint {
            version: "rev-42".into(),
            base: vec![1, 2, 3, 4],
        };
        store_point(&tmp, &vault, &point).unwrap();

        let loaded = load_point(&tmp, &vault).expect("a stored point must load");
        assert_eq!(loaded.version, "rev-42");
        assert_eq!(loaded.base, vec![1, 2, 3, 4]);
    }

    #[test]
    fn a_missing_point_is_none_rather_than_an_error() {
        // A first sync has no base, and that must be an ordinary state.
        let tmp = std::env::temp_dir().join("vardo-syncpoint-absent");
        let _ = fs::remove_dir_all(&tmp);
        assert!(load_point(&tmp, &VaultSource::file("/nope.kdbx")).is_none());
    }
}

#[cfg(test)]
mod remote_failures {
    use super::*;
    use vardo_sync::SyncError as S;

    const WHERE: &str = "passwords.kdbx in Google Drive";

    /// The visible part of a message must fit one line in a narrow panel.
    ///
    /// This is the rule these messages kept breaking. A four-line paragraph in
    /// a side panel is a wall, and a wall is not read at the exact moment
    /// someone is worried about their passwords. Anything longer belongs behind
    /// the disclosure, which is what the parenthetical becomes in the UI.
    const ONE_LINE: usize = 48;

    /// What the user actually sees: everything before the technical cause.
    fn headline(e: S) -> String {
        let full = map_sync_error(e, WHERE).to_string();
        match full.find(" (") {
            Some(i) => full[..i].to_string(),
            None => full,
        }
    }

    #[test]
    fn every_message_fits_on_one_line() {
        for e in [
            S::Transport("dns failure".into()),
            S::Unauthorized("invalid_grant".into()),
            S::IdentityLost("new id".into()),
            S::Other("something odd".into()),
            S::NotFound,
            S::RateLimited {
                retry_after_secs: Some(30),
            },
            S::RateLimited {
                retry_after_secs: None,
            },
        ] {
            let line = headline(e);
            assert!(
                line.len() <= ONE_LINE,
                "must fit one line, got {} chars: {line:?}",
                line.len()
            );
            assert!(!line.contains('\n'), "must be a single line: {line:?}");
        }
    }

    #[test]
    fn the_technical_cause_survives_behind_the_headline() {
        // Shortening the message must not throw the cause away: it is the only
        // thing that makes a bug report useful, and it is what told us a space
        // in a URL was the real problem.
        let full = map_sync_error(S::Transport("invalid uri character".into()), WHERE).to_string();
        assert!(full.contains("invalid uri character"), "got: {full}");
    }

    #[test]
    fn being_offline_is_never_reported_as_a_password_problem() {
        // The worst failure this screen can produce. Someone whose network is
        // down retypes a correct password, gets the same error, and concludes
        // their vault is corrupt.
        let err = map_sync_error(S::Transport("dns failure".into()), WHERE);
        assert!(matches!(err, UiError::Offline { .. }));

        // Checking for the bare word "password" would be wrong: the vault is
        // *called* passwords.kdbx. What must not appear is a claim about it.
        let text = err.to_string().to_lowercase();
        for accusation in [
            "wrong password",
            "incorrect password",
            "check your password",
        ] {
            assert!(
                !text.contains(accusation),
                "must not blame the password: {text}"
            );
        }
    }

    #[test]
    fn a_revoked_grant_points_at_access_not_the_network() {
        let err = map_sync_error(S::Unauthorized("invalid_grant".into()), WHERE);
        assert!(matches!(err, UiError::DriveAuthExpired { .. }));
        assert!(
            !err.to_string().to_lowercase().contains("connection"),
            "must not send the user to check their network: {err}"
        );
    }

    #[test]
    fn a_replaced_file_says_it_was_replaced() {
        // `fact:drive-safe-save-kills-id`. If it ever happens, the
        // message has to explain why a file plainly still in Drive is dead.
        assert!(headline(S::IdentityLost("new id".into())).contains("replaced"));
    }

    #[test]
    fn rate_limiting_gives_the_wait_when_google_supplies_one() {
        assert!(headline(S::RateLimited {
            retry_after_secs: Some(30)
        })
        .contains("30"));
    }
}
