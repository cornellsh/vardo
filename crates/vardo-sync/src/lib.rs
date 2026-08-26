//! Getting a vault safely between devices.
//!
//! The hard part is not transport. It is that Google Drive offers no
//! compare-and-swap, so two devices can and will overwrite each other, and the
//! only defences are a correct merge and the fact that every write leaves a
//! recoverable revision behind. See [`protocol`] for the shape of that, and
//! `ADR-001-sync.md` for why it has to be this way.
//!
//! The vault's canonical copy lives in app-private storage and this crate
//! talks to the provider over HTTPS. It is never a file inside a synced
//! folder: KeePassXC's issue tracker is a decade-long record of what happens
//! when a password database is handed to a sync daemon.

#![forbid(unsafe_code)]

pub mod backend;
pub mod fake;
pub mod local;
pub mod protocol;

pub use backend::{RemoteState, Revision, StorageBackend, SyncError, SyncResult};
pub use protocol::{sync, Options, Outcome, SyncPoint, SyncReport};
