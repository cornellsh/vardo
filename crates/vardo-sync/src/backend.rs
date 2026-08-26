//! The storage abstraction.
//!
//! Modelled on what Google Drive actually provides, not on what a storage API
//! ideally would — because the sync protocol has to survive the real one.
//!
//! Most importantly there is **no compare-and-swap**. Drive API v3 removed
//! `etag` from the file resource and ignores `If-Match` on the upload
//! endpoint, so a conditional write is not available at any price (see
//! `ADR-001-sync.md`). This trait therefore does not offer one, and no backend
//! may pretend to: a `write` can always be lost to a concurrent write, and the
//! protocol above must be built for that.
//!
//! What Drive *does* give, and what this trait exposes instead:
//!
//! * a **trustworthy change signal** ([`RemoteState`]), unlike a sync-daemon
//!   mount whose mtime can be weeks stale;
//! * **write confirmation** — [`StorageBackend::write`] returning means the
//!   bytes are stored, which `write()` to a FUSE mount does not;
//! * **revision history**, which is what makes a lost race recoverable rather
//!   than fatal.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteState {
    /// An opaque token that changes whenever the content changes.
    ///
    /// Drive supplies `headRevisionId`. Comparing tokens is the only reliable
    /// way to detect a change — file size and modification time both lie.
    pub version: String,
    pub size: u64,
    /// Seconds since the Unix epoch, for display only. Never for correctness:
    /// clocks disagree between devices and providers round differently.
    pub modified_unix: Option<i64>,
    /// Content hash when the backend offers one, for cheap equality checks.
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    pub id: String,
    pub modified_unix: Option<i64>,
    pub size: u64,
}

#[derive(Debug)]
pub enum SyncError {
    /// No file at this location yet.
    NotFound,
    /// The backend rejected the credentials, or they expired.
    Unauthorized(String),
    /// The file identity is gone — on Drive, because another client replaced
    /// the file rather than updating it, which mints a new ID and invalidates
    /// a per-file grant.
    IdentityLost(String),
    /// Transport failure. Retryable.
    Transport(String),
    /// The provider asked us to slow down.
    RateLimited {
        retry_after_secs: Option<u64>,
    },
    Other(String),
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "no file at that location"),
            Self::Unauthorized(d) => write!(f, "not authorised: {d}"),
            Self::IdentityLost(d) => write!(
                f,
                "the remote file was replaced rather than updated, so our reference is dead: {d}"
            ),
            Self::Transport(d) => write!(f, "transport failure: {d}"),
            Self::RateLimited { retry_after_secs } => match retry_after_secs {
                Some(s) => write!(f, "rate limited, retry in {s}s"),
                None => write!(f, "rate limited"),
            },
            Self::Other(d) => write!(f, "{d}"),
        }
    }
}

impl std::error::Error for SyncError {}

impl SyncError {
    /// Whether retrying the same request could plausibly succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Transport(_) | Self::RateLimited { .. })
    }
}

pub type SyncResult<T> = Result<T, SyncError>;

/// A place a vault file lives.
///
/// Intentionally small. Everything hard — merging, conflict recovery, retry —
/// belongs above this line so it is written once and tested against a fake,
/// rather than reimplemented per provider.
pub trait StorageBackend {
    /// Cheap metadata read. Must not transfer the file body.
    fn stat(&mut self) -> SyncResult<RemoteState>;

    fn read(&mut self) -> SyncResult<(Vec<u8>, RemoteState)>;

    /// Replaces the content **in place**, preserving the file's identity.
    ///
    /// On Drive this is `files.update` with a media upload against the existing
    /// `fileId` — never delete-and-recreate, which mints a new ID and breaks
    /// every other client's reference to the file. That is precisely how
    /// KeePassXC's "safe save" breaks mobile clients.
    ///
    /// There is no conditional form. The returned state describes what is
    /// stored *now*, which may already reflect somebody else's write.
    fn write(&mut self, bytes: &[u8]) -> SyncResult<RemoteState>;

    /// Prior versions, newest first.
    ///
    /// This is what turns a lost race into a recoverable event: the write that
    /// was overwritten is still there, so it can be fetched and merged after
    /// the fact.
    fn revisions(&mut self) -> SyncResult<Vec<Revision>>;

    fn read_revision(&mut self, id: &str) -> SyncResult<Vec<u8>>;

    /// A stable identifier for this location, for logs and state files.
    fn describe(&self) -> String;
}
