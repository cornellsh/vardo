//! An in-process backend that models Google Drive **including its flaws**.
//!
//! # Why a fake rather than the real API
//!
//! You cannot chaos-test a network service. It is too slow for the thousands
//! of iterations a property test needs, it is non-deterministic, it is flaky in
//! CI, and — fatally — **you cannot force the interesting races.** There is no
//! way to ask Google to land two writes three milliseconds apart on demand.
//!
//! So the sync protocol is developed against this, where a race is a method
//! call, and a small suite of contract tests runs against real Drive purely to
//! prove this fake does not lie.
//!
//! # It must model the flaws, not an ideal store
//!
//! A fake that offered atomic compare-and-swap would let a protocol pass here
//! and lose data in production. So this one reproduces Drive's actual
//! behaviour:
//!
//! * writes are **unconditional** and silently clobber concurrent ones;
//! * every write creates a **revision**, so a clobbered write is recoverable;
//! * the file **identity can be lost** if something replaces rather than
//!   updates it;
//! * transport failures, rate limits and expired credentials can be injected
//!   at any call.

use std::sync::{Arc, Mutex};

use crate::backend::{RemoteState, Revision, StorageBackend, SyncError, SyncResult};

#[derive(Default)]
struct Stored {
    /// Newest last. Index doubles as the revision id.
    revisions: Vec<Vec<u8>>,
    /// Bumped on every write so `version` changes even if content repeats.
    counter: u64,
    /// Set when the file is replaced rather than updated; every existing
    /// handle then fails, as a `drive.file` grant does.
    identity: u64,
    exists: bool,
}

/// Shared server state. Two [`FakeDrive`] handles onto the same `Server` are
/// two devices talking to one file.
#[derive(Clone, Default)]
pub struct Server(Arc<Mutex<Stored>>);

impl Server {
    pub fn new() -> Self {
        Self::default()
    }

    /// A file that already exists with this content.
    pub fn with_file(bytes: &[u8]) -> Self {
        let server = Self::new();
        {
            let mut s = server.0.lock().unwrap();
            s.exists = true;
            s.counter = 1;
            s.revisions.push(bytes.to_vec());
        }
        server
    }

    pub fn connect(&self) -> FakeDrive {
        FakeDrive {
            server: self.clone(),
            identity: self.0.lock().unwrap().identity,
            faults: Faults::default(),
            calls: 0,
        }
    }

    /// Simulates another application replacing the file instead of updating
    /// it — a new file ID, which kills every existing per-file grant.
    ///
    /// This is what KeePassXC's "safe save" does through a cloud mount, and it
    /// is why mobile clients lose their reference to the vault.
    pub fn replace_file(&self, bytes: &[u8]) {
        let mut s = self.0.lock().unwrap();
        s.identity += 1;
        s.counter += 1;
        s.revisions.clear();
        s.revisions.push(bytes.to_vec());
        s.exists = true;
    }

    pub fn current(&self) -> Option<Vec<u8>> {
        let s = self.0.lock().unwrap();
        s.revisions.last().cloned()
    }

    pub fn revision_count(&self) -> usize {
        self.0.lock().unwrap().revisions.len()
    }
}

/// Faults to inject, consumed one call at a time.
#[derive(Default, Clone)]
pub struct Faults {
    /// Fail the next N calls with a transport error.
    pub transport_failures: usize,
    /// Fail the next N calls with a rate limit.
    pub rate_limits: usize,
    /// Fail every call with an authorisation error.
    pub unauthorized: bool,
    /// Run this just before each `write` lands, to interleave another device.
    #[allow(clippy::type_complexity)]
    pub before_write: Option<Arc<dyn Fn() + Send + Sync>>,
}

pub struct FakeDrive {
    server: Server,
    /// The file identity this handle was opened against.
    identity: u64,
    pub faults: Faults,
    calls: u64,
}

impl FakeDrive {
    /// Injects a callback that runs immediately before each write commits.
    ///
    /// This is how a race is made deterministic: the other device's write
    /// happens *inside* ours, at exactly the moment that is hardest to handle.
    pub fn race_before_write(&mut self, f: impl Fn() + Send + Sync + 'static) {
        self.faults.before_write = Some(Arc::new(f));
    }

    pub fn fail_next_calls(&mut self, n: usize) {
        self.faults.transport_failures = n;
    }

    pub fn rate_limit_next_calls(&mut self, n: usize) {
        self.faults.rate_limits = n;
    }

    pub fn call_count(&self) -> u64 {
        self.calls
    }

    fn precheck(&mut self) -> SyncResult<()> {
        self.calls += 1;
        if self.faults.unauthorized {
            return Err(SyncError::Unauthorized("token rejected".into()));
        }
        if self.faults.transport_failures > 0 {
            self.faults.transport_failures -= 1;
            return Err(SyncError::Transport("connection reset".into()));
        }
        if self.faults.rate_limits > 0 {
            self.faults.rate_limits -= 1;
            return Err(SyncError::RateLimited {
                retry_after_secs: Some(1),
            });
        }
        let stored = self.server.0.lock().unwrap();
        if stored.identity != self.identity {
            return Err(SyncError::IdentityLost(
                "the file was replaced by another client".into(),
            ));
        }
        Ok(())
    }

    fn state(stored: &Stored) -> RemoteState {
        let bytes = stored.revisions.last();
        RemoteState {
            version: format!("rev-{}", stored.counter),
            size: bytes.map(|b| b.len() as u64).unwrap_or(0),
            modified_unix: Some(1_700_000_000 + stored.counter as i64),
            content_hash: bytes.map(|b| format!("{:016x}", fnv1a(b))),
        }
    }
}

impl StorageBackend for FakeDrive {
    fn stat(&mut self) -> SyncResult<RemoteState> {
        self.precheck()?;
        let stored = self.server.0.lock().unwrap();
        if !stored.exists {
            return Err(SyncError::NotFound);
        }
        Ok(Self::state(&stored))
    }

    fn read(&mut self) -> SyncResult<(Vec<u8>, RemoteState)> {
        self.precheck()?;
        let stored = self.server.0.lock().unwrap();
        if !stored.exists {
            return Err(SyncError::NotFound);
        }
        let bytes = stored.revisions.last().cloned().unwrap_or_default();
        Ok((bytes, Self::state(&stored)))
    }

    fn write(&mut self, bytes: &[u8]) -> SyncResult<RemoteState> {
        self.precheck()?;
        // The injected race runs here: after our checks, before our write
        // lands. Exactly the window a real concurrent writer occupies, and
        // exactly what no conditional-write primitive exists to close.
        if let Some(f) = self.faults.before_write.clone() {
            f();
        }
        let mut stored = self.server.0.lock().unwrap();
        if stored.identity != self.identity {
            return Err(SyncError::IdentityLost(
                "the file was replaced mid-write".into(),
            ));
        }
        stored.exists = true;
        stored.counter += 1;
        stored.revisions.push(bytes.to_vec());
        Ok(Self::state(&stored))
    }

    fn revisions(&mut self) -> SyncResult<Vec<Revision>> {
        self.precheck()?;
        let stored = self.server.0.lock().unwrap();
        Ok(stored
            .revisions
            .iter()
            .enumerate()
            .rev()
            .map(|(i, b)| Revision {
                id: i.to_string(),
                modified_unix: Some(1_700_000_000 + i as i64),
                size: b.len() as u64,
            })
            .collect())
    }

    fn read_revision(&mut self, id: &str) -> SyncResult<Vec<u8>> {
        self.precheck()?;
        let stored = self.server.0.lock().unwrap();
        let index: usize = id
            .parse()
            .map_err(|_| SyncError::Other(format!("bad revision id {id:?}")))?;
        stored
            .revisions
            .get(index)
            .cloned()
            .ok_or(SyncError::NotFound)
    }

    fn describe(&self) -> String {
        format!("fake-drive#{}", self.identity)
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}
