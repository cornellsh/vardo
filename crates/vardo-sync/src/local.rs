//! A plain-filesystem backend.
//!
//! For genuinely local storage: a real disk, a USB key, a network share you
//! control. **Not** for a cloud-synced folder.
//!
//! Pointing this at `~/Google Drive`, `~/Dropbox`, `~/OneDrive`,
//! `~/Library/CloudStorage`, or an rclone/GVfs mount is the single most common
//! way KeePass deployments lose data: the daemon forks the file into
//! `vault (1).kdbx`, or reports a stale mtime for weeks, or holds a lock that
//! makes atomic rename impossible. KeePassXC issues #9140, #8790, #2066,
//! #11969 and #13050 are all this. Use the provider's API instead.
//!
//! Revision support is a small local rotation, so the race-recovery path in
//! [`crate::protocol`] behaves the same way here as against a real provider.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::backend::{RemoteState, Revision, StorageBackend, SyncError, SyncResult};

pub struct LocalFile {
    path: PathBuf,
    /// Where prior versions are kept, alongside the vault.
    history: PathBuf,
    max_revisions: usize,
}

impl LocalFile {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let history = path.with_extension("kdbx.history");
        Self {
            path,
            history,
            max_revisions: 32,
        }
    }

    /// True if this path looks like it is inside a cloud-synced folder.
    ///
    /// Advisory, not a guarantee — a user can mount anything anywhere. The
    /// point is to catch the common case at open time and offer to migrate,
    /// which KeePassXC has an open proposal for (#13050) and has not shipped.
    pub fn looks_cloud_synced(path: &Path) -> bool {
        let text = path.to_string_lossy().to_lowercase();
        [
            "google drive",
            "googledrive",
            "/gdrive",
            "dropbox",
            "onedrive",
            "cloudstorage",
            "nextcloud",
            "owncloud",
            "pcloud",
            "mega",
            "yandex.disk",
            "icloud",
        ]
        .iter()
        .any(|needle| text.contains(needle))
    }

    fn io(e: std::io::Error) -> SyncError {
        match e.kind() {
            std::io::ErrorKind::NotFound => SyncError::NotFound,
            std::io::ErrorKind::PermissionDenied => SyncError::Unauthorized(e.to_string()),
            _ => SyncError::Transport(e.to_string()),
        }
    }

    fn state_of(bytes: &[u8], modified: Option<i64>) -> RemoteState {
        RemoteState {
            // Content-derived, because a local filesystem has no version token
            // and mtime granularity is too coarse to detect a fast rewrite.
            version: format!("{:016x}", fnv1a(bytes)),
            size: bytes.len() as u64,
            modified_unix: modified,
            content_hash: Some(format!("{:016x}", fnv1a(bytes))),
        }
    }

    fn modified(&self) -> Option<i64> {
        let meta = fs::metadata(&self.path).ok()?;
        let mtime = meta.modified().ok()?;
        mtime
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs() as i64)
    }

    fn rotate(&self, previous: &[u8]) -> SyncResult<()> {
        fs::create_dir_all(&self.history).map_err(Self::io)?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        fs::write(self.history.join(format!("{stamp:039}.kdbx")), previous).map_err(Self::io)?;

        let mut existing: Vec<_> = fs::read_dir(&self.history)
            .map_err(Self::io)?
            .flatten()
            .map(|e| e.path())
            .collect();
        existing.sort();
        while existing.len() > self.max_revisions {
            let _ = fs::remove_file(existing.remove(0));
        }
        Ok(())
    }
}

impl StorageBackend for LocalFile {
    fn stat(&mut self) -> SyncResult<RemoteState> {
        let bytes = fs::read(&self.path).map_err(Self::io)?;
        Ok(Self::state_of(&bytes, self.modified()))
    }

    fn read(&mut self) -> SyncResult<(Vec<u8>, RemoteState)> {
        let bytes = fs::read(&self.path).map_err(Self::io)?;
        let state = Self::state_of(&bytes, self.modified());
        Ok((bytes, state))
    }

    fn write(&mut self, bytes: &[u8]) -> SyncResult<RemoteState> {
        if let Ok(previous) = fs::read(&self.path) {
            self.rotate(&previous)?;
        }
        // Write-then-rename, which is atomic on a real filesystem. It is
        // exactly what breaks on a cloud mount, which is why this backend is
        // documented as local-only.
        //
        // The fsync is not ceremony. `rename` being atomic guarantees only that
        // readers see the old file or the new one — not that the new one's
        // *contents* reached the disk. Without flushing first, a crash shortly
        // after can leave the name pointing at a zero-length or partially
        // written file, and the original is already gone. That is a destroyed
        // vault, and it is a well-known way to lose data on ext4.
        let temp = self.path.with_extension("kdbx.tmp");
        {
            let mut file = fs::File::create(&temp).map_err(Self::io)?;
            file.write_all(bytes).map_err(Self::io)?;
            file.sync_all().map_err(Self::io)?;
        }
        fs::rename(&temp, &self.path).map_err(Self::io)?;

        // And fsync the directory, so the rename itself is durable. Best
        // effort: some filesystems refuse to open a directory for this, and
        // failing the save over it would be worse than the risk.
        if let Some(parent) = self.path.parent() {
            if let Ok(dir) = fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }

        Ok(Self::state_of(bytes, self.modified()))
    }

    /// Refuses rather than buries.
    ///
    /// The default emulation writes first and discovers afterwards that it
    /// destroyed somebody's change, then recovers it from the revision
    /// history. On a local file we can simply look before writing, so the other
    /// write is never destroyed at all — which is the whole reason
    /// `write_if_unchanged` exists as a trait method rather than the protocol
    /// hard-coding Drive's shape.
    ///
    /// Honest about what this is not: comparing and then writing is not atomic.
    /// Another process can still slip in between, and closing that properly
    /// needs an advisory lock. What it does remove is the guaranteed-destroy
    /// step, turning a certainty into a narrow race.
    fn write_if_unchanged(
        &mut self,
        bytes: &[u8],
        expected: &[u8],
    ) -> SyncResult<crate::backend::WriteOutcome> {
        let current = fs::read(&self.path).unwrap_or_default();

        // An empty `expected` means "we believe nothing is there yet".
        let matches = if expected.is_empty() {
            current.is_empty()
        } else {
            current == expected
        };

        if !matches {
            return Ok(crate::backend::WriteOutcome::Superseded { buried: current });
        }

        let state = self.write(bytes)?;
        Ok(crate::backend::WriteOutcome::Written(state))
    }

    fn revisions(&mut self) -> SyncResult<Vec<Revision>> {
        let Ok(dir) = fs::read_dir(&self.history) else {
            return Ok(Vec::new());
        };
        let mut out: Vec<_> = dir
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                let id = path.file_stem()?.to_string_lossy().to_string();
                let size = e.metadata().ok()?.len();
                Some(Revision {
                    id,
                    modified_unix: None,
                    size,
                })
            })
            .collect();
        out.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(out)
    }

    fn read_revision(&mut self, id: &str) -> SyncResult<Vec<u8>> {
        fs::read(self.history.join(format!("{id}.kdbx"))).map_err(Self::io)
    }

    fn describe(&self) -> String {
        self.path.display().to_string()
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
