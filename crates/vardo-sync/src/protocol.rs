//! The sync protocol.
//!
//! # Operating without compare-and-swap
//!
//! Drive offers no conditional write (`ADR-001-sync.md`), so between reading
//! the remote version and writing our own, another device can land a write
//! that we will silently overwrite. That window cannot be closed.
//!
//! It can, however, be made **detectable and repairable**, because every write
//! creates a revision and nothing is ever actually destroyed.
//!
//! The dangerous direction is subtle. It is *not* "did somebody overwrite us
//! afterwards" — that costs us nothing, since our write is still a revision and
//! they merged on top of it. It is **"did somebody write between our read and
//! our write, so that we clobbered a version we never saw?"**
//!
//! That is detectable after the fact: the revision directly beneath ours must
//! be the one we merged on top of. If it is something else, an unseen write
//! went under us.
//!
//! ```text
//!   read head  ────────────────────────────┐
//!        │                                 │ remember these bytes
//!        ▼                                 │
//!   merge, upload                          │
//!        │                                 │
//!        ▼                                 ▼
//!   what sits directly beneath our new revision?
//!        │                          │
//!   the bytes we read          something else ──▶ we clobbered an unseen
//!        │                                          write. It is still in the
//!        ▼                                          history: fold it in and
//!   sync point committed                            upload again.
//! ```
//!
//! This check is what every other KeePass client omits, and it is why they lose
//! writes. Nothing is ever actually destroyed, so a lost race costs a round
//! trip rather than a password.

use vardo_kdbx::{write::Rng, Credentials};
use vardo_merge::{apply, plan, Applied, ConflictPolicy, MergePlan};
use vardo_vault::Vault;

use crate::backend::{StorageBackend, SyncError, SyncResult};

/// What both sides agreed on at the last successful sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPoint {
    /// The remote version token as of that sync.
    pub version: String,
    /// The vault bytes as of that sync, which is the three-way merge base.
    ///
    /// Without it only a two-way merge is possible and nothing can ever be
    /// deleted, because an addition on one side is indistinguishable from a
    /// deletion on the other.
    pub base: Vec<u8>,
}

#[derive(Debug)]
pub enum Outcome {
    /// Remote matches our sync point and we had nothing to push.
    UpToDate,
    /// Remote was unchanged; our local edits were uploaded as-is.
    Pushed,
    /// Remote had changed; the two were merged and the result uploaded.
    Merged(Box<Applied>),
    /// The merge needs a human. Nothing was uploaded.
    NeedsReview(Box<MergePlan>),
    /// The remote file did not exist; the local vault was uploaded to create it.
    Created,
}

#[derive(Debug)]
pub struct SyncReport {
    pub outcome: Outcome,
    pub point: Option<SyncPoint>,
    /// How many times another writer beat us and we had to redo the merge.
    /// Non-zero means the race described above actually happened.
    pub races_lost: u32,
    /// Retryable transport or rate-limit failures we rode out.
    pub retries: u32,
}

#[derive(Debug)]
pub enum Error {
    Storage(SyncError),
    Vault(vardo_kdbx::Error),
    /// Too many concurrent writers; gave up rather than looping forever.
    RaceLimitExceeded(u32),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(e) => write!(f, "{e}"),
            Self::Vault(e) => write!(f, "{e}"),
            Self::RaceLimitExceeded(n) => write!(
                f,
                "another device overwrote our upload {n} times running; giving up rather than \
                 looping. Nothing was lost — every attempt is in the revision history."
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<SyncError> for Error {
    fn from(e: SyncError) -> Self {
        Self::Storage(e)
    }
}

impl From<vardo_kdbx::Error> for Error {
    fn from(e: vardo_kdbx::Error) -> Self {
        Self::Vault(e)
    }
}

pub struct Options {
    pub policy: ConflictPolicy,
    /// How many times to redo a merge after losing a race.
    pub max_race_retries: u32,
    /// How many times to retry a retryable transport failure.
    pub max_transport_retries: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            policy: ConflictPolicy::Duplicate,
            // Three concurrent writers on one vault is already pathological;
            // beyond that, stopping and telling the user beats spinning.
            max_race_retries: 3,
            max_transport_retries: 4,
        }
    }
}

/// Synchronises `local` with the remote, mutating `local` to the merged result.
///
/// Returns a new [`SyncPoint`] to persist. If the outcome is
/// [`Outcome::NeedsReview`] nothing was uploaded and the previous sync point
/// still stands.
pub fn sync(
    backend: &mut dyn StorageBackend,
    local: &mut Vault,
    creds: &Credentials,
    point: Option<&SyncPoint>,
    options: &Options,
    rng: &mut dyn Rng,
) -> Result<SyncReport, Error> {
    let mut report = SyncReport {
        outcome: Outcome::UpToDate,
        point: None,
        races_lost: 0,
        retries: 0,
    };

    // The remote may not exist yet.
    let remote_state = match retry(backend, options, &mut report.retries, |b| b.stat()) {
        Ok(state) => Some(state),
        Err(SyncError::NotFound) => None,
        Err(e) => return Err(e.into()),
    };

    let Some(remote_state) = remote_state else {
        let bytes = local.save_with(creds, rng)?;
        let state = retry(backend, options, &mut report.retries, |b| b.write(&bytes))?;
        report.outcome = Outcome::Created;
        report.point = Some(SyncPoint {
            version: state.version,
            base: bytes,
        });
        return Ok(report);
    };

    // Remote unchanged since our sync point: nothing to merge, just push.
    // Still verified, because "unchanged when we looked" does not mean
    // "unchanged when we wrote".
    let unchanged = point.is_some_and(|p| p.version == remote_state.version);

    // Two distinct things, and conflating them is a bug:
    //   `to_fold`       - content that still needs merging into `local`
    //   `expected_prev` - what should sit directly beneath our next write
    // After a retry these differ: we must merge the clobbered bytes, but the
    // revision beneath us will be our own previous attempt.
    let mut to_fold: Option<Vec<u8>> = if unchanged {
        None
    } else {
        Some(retry(backend, options, &mut report.retries, |b| b.read())?.0)
    };
    let mut expected_prev: Vec<u8> = match (&to_fold, point) {
        (Some(head), _) => head.clone(),
        (None, Some(p)) => p.base.clone(),
        (None, None) => Vec::new(),
    };

    let mut applied_summary: Option<Applied> = None;
    let mut attempt = 0u32;

    loop {
        if let Some(remote_bytes) = to_fold.take() {
            let remote_doc = vardo_kdbx::open(&remote_bytes, creds)?;
            let remote_vault = Vault::open(&remote_doc)?;

            let base_doc = match point {
                Some(p) => Some(vardo_kdbx::open(&p.base, creds)?),
                None => None,
            };
            let base_vault = match &base_doc {
                Some(doc) => Some(Vault::open(doc)?),
                None => None,
            };

            let merge_plan = plan(base_vault.as_ref(), local, &remote_vault);
            if merge_plan.needs_review() && options.policy == ConflictPolicy::Refuse {
                report.outcome = Outcome::NeedsReview(Box::new(merge_plan));
                return Ok(report);
            }
            let applied = apply(&merge_plan, local, &remote_vault, options.policy)?;
            applied_summary = Some(match applied_summary.take() {
                Some(previous) => combine(previous, applied),
                None => applied,
            });
        }

        let bytes = local.save_with(creds, rng)?;
        let version = retry(backend, options, &mut report.retries, |b| b.write(&bytes))?.version;

        // What ended up directly beneath our write? It must be what was head
        // when we wrote; anything else is a write we never saw and just buried.
        match clobbered(backend, options, &mut report.retries, &expected_prev)? {
            None => {
                report.outcome = match applied_summary {
                    Some(applied) => Outcome::Merged(Box::new(applied)),
                    None => Outcome::Pushed,
                };
                report.point = Some(SyncPoint {
                    version,
                    base: bytes,
                });
                return Ok(report);
            }
            Some(unseen) => {
                // Recoverable precisely because the write we buried is still a
                // revision. Fold it in and go again — and next time expect to
                // be sitting on top of the attempt we just made.
                report.races_lost += 1;
                attempt += 1;
                if attempt > options.max_race_retries {
                    return Err(Error::RaceLimitExceeded(attempt));
                }
                to_fold = Some(unseen);
                expected_prev = bytes;
            }
        }
    }
}

/// Returns the bytes of a write we overwrote without having seen it.
///
/// The revision directly beneath ours should be exactly what we merged on top
/// of. If it is not, somebody wrote in the gap between our read and our write.
fn clobbered(
    backend: &mut dyn StorageBackend,
    options: &Options,
    retries: &mut u32,
    expected: &[u8],
) -> Result<Option<Vec<u8>>, Error> {
    let revisions = retry(backend, options, retries, |b| b.revisions())?;
    // [0] is the write we just made; [1] is what it replaced.
    let Some(previous) = revisions.get(1) else {
        return Ok(None);
    };
    let id = previous.id.clone();
    let bytes = retry(backend, options, retries, |b| b.read_revision(&id))?;
    if bytes == expected {
        Ok(None)
    } else {
        Ok(Some(bytes))
    }
}

fn combine(a: Applied, b: Applied) -> Applied {
    Applied {
        entries_added: a.entries_added + b.entries_added,
        entries_updated: a.entries_updated + b.entries_updated,
        entries_deleted: a.entries_deleted + b.entries_deleted,
        entries_moved: a.entries_moved + b.entries_moved,
        entries_resurrected: a.entries_resurrected + b.entries_resurrected,
        groups_added: a.groups_added + b.groups_added,
        conflicts_duplicated: a.conflicts_duplicated + b.conflicts_duplicated,
        skipped: a.skipped.into_iter().chain(b.skipped).collect(),
    }
}

/// Retries a retryable failure a bounded number of times.
///
/// Deliberately does not sleep: the delay policy belongs to the caller, and a
/// test suite that slept would take minutes.
fn retry<T>(
    backend: &mut dyn StorageBackend,
    options: &Options,
    counter: &mut u32,
    mut op: impl FnMut(&mut dyn StorageBackend) -> SyncResult<T>,
) -> SyncResult<T> {
    let mut attempts = 0;
    loop {
        match op(backend) {
            Ok(value) => return Ok(value),
            Err(e) if e.is_retryable() && attempts < options.max_transport_retries => {
                attempts += 1;
                *counter += 1;
            }
            Err(e) => return Err(e),
        }
    }
}
