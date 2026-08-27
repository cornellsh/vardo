//! Working out whether a vault path is somewhere dangerous.
//!
//! Pointing a KeePass client at a cloud-synced folder is the single most common
//! way these deployments lose data: the daemon forks the file into
//! `vault (1).kdbx`, or reports a stale mtime for weeks, or holds a lock that
//! makes atomic rename impossible. KeePassXC issues #9140, #8790, #2066,
//! #11969 and #13050 are all this, and #13050 proposes exactly the warning
//! below without having shipped it.
//!
//! # Why not just match the path text
//!
//! Because it is wrong in both directions. A folder called `~/dropbox-backups`
//! that is an ordinary directory gets flagged, and an rclone mount at
//! `~/vaults` does not — even though the second is the one that eats files.
//!
//! So the mount table is consulted first, and the name is only a fallback for
//! platforms where that is unavailable. The name check still earns its place:
//! a Dropbox *daemon* syncing an ordinary local directory leaves no trace in
//! the mount table at all.

use std::path::Path;

/// What kind of storage a vault appears to live on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageRisk {
    /// An ordinary local filesystem. No warning.
    Local,
    /// A userspace filesystem — rclone, GVfs, sshfs, gocryptfs. The dangerous
    /// case: `rename` is frequently not atomic and mtime is often invented.
    Fuse { fs: String },
    /// A network filesystem. Atomic rename is not guaranteed across a network.
    Network { fs: String },
    /// The mount looks ordinary, but the path names a known sync provider, so a
    /// sync daemon is probably watching the directory.
    NameSuggestsSync { provider: String },
}

impl StorageRisk {
    pub fn is_risky(&self) -> bool {
        !matches!(self, StorageRisk::Local)
    }

    /// A sentence for the user, naming the specific hazard rather than saying
    /// "this may be unsafe", which tells them nothing they can act on.
    pub fn explain(&self) -> String {
        match self {
            StorageRisk::Local => "This vault is on ordinary local storage.".into(),
            // Phrased to sidestep a/an: the filesystem name is interpolated and
            // "a rclone mount" reads as a bug in the app.
            StorageRisk::Fuse { fs } => format!(
                "This vault is on a userspace mount ({fs}). Saving relies on an atomic \
                 rename, which userspace mounts often cannot provide — the usual result is \
                 a forked copy like \"vault (1).kdbx\" or a save that silently lands on a \
                 stale version."
            ),
            StorageRisk::Network { fs } => format!(
                "This vault is on a network mount ({fs}). An atomic rename is not guaranteed \
                 across a network, so an interrupted save can leave a partial file."
            ),
            StorageRisk::NameSuggestsSync { provider } => format!(
                "This vault looks like it is inside a {provider} folder. A sync daemon \
                 rewriting the file underneath us is how KeePass databases get forked into \
                 conflicted copies."
            ),
        }
    }

    /// What to do about it.
    pub fn remedy(&self) -> &'static str {
        match self {
            StorageRisk::Local => "",
            _ => {
                "Move the vault to ordinary local storage and let Vardo sync it through the \
                  provider's API instead. Vardo can then merge properly rather than racing a \
                  background daemon."
            }
        }
    }
}

/// Filesystem types that are userspace mounts.
fn is_fuse(fs: &str) -> bool {
    fs.starts_with("fuse") || matches!(fs, "rclone" | "gocryptfs" | "sshfs" | "gvfsd-fuse")
}

/// Filesystem types that live across a network.
fn is_network(fs: &str) -> bool {
    matches!(
        fs,
        "nfs" | "nfs4" | "cifs" | "smb3" | "smbfs" | "afpfs" | "davfs" | "webdav" | "ncpfs"
    )
}

const PROVIDERS: &[(&str, &str)] = &[
    ("google drive", "Google Drive"),
    ("googledrive", "Google Drive"),
    ("/gdrive", "Google Drive"),
    ("dropbox", "Dropbox"),
    ("onedrive", "OneDrive"),
    ("cloudstorage", "iCloud or another provider"),
    ("nextcloud", "Nextcloud"),
    ("owncloud", "ownCloud"),
    ("pcloud", "pCloud"),
    ("yandex.disk", "Yandex.Disk"),
    ("icloud", "iCloud"),
    ("mega", "MEGA"),
];

pub fn assess(path: &Path) -> StorageRisk {
    assess_with(path, &read_mount_table())
}

/// Split out so the mount table can be supplied in tests. Reading `/proc/mounts`
/// in a test would assert something about the machine rather than the code.
pub fn assess_with(path: &Path, mounts: &[(String, String)]) -> StorageRisk {
    if let Some(fs) = filesystem_for(path, mounts) {
        if is_fuse(&fs) {
            return StorageRisk::Fuse { fs: pretty_fs(&fs) };
        }
        if is_network(&fs) {
            return StorageRisk::Network {
                fs: fs.to_uppercase(),
            };
        }
    }

    let text = path.to_string_lossy().to_lowercase();
    for (needle, provider) in PROVIDERS {
        if text.contains(needle) {
            return StorageRisk::NameSuggestsSync {
                provider: (*provider).to_string(),
            };
        }
    }

    StorageRisk::Local
}

/// `fuse.rclone` reads better as `rclone`.
fn pretty_fs(fs: &str) -> String {
    fs.strip_prefix("fuse.").unwrap_or(fs).to_string()
}

/// The filesystem type of the longest mount point that is a prefix of `path`.
///
/// Longest wins because mount points nest: `/` always matches, so a shorter
/// match would report `ext4` for everything.
fn filesystem_for(path: &Path, mounts: &[(String, String)]) -> Option<String> {
    let text = path.to_string_lossy().to_string();
    let mut best: Option<(usize, String)> = None;

    for (point, fs) in mounts {
        let matches = text == *point
            || text.starts_with(&format!("{}/", point.trim_end_matches('/')))
            || point == "/";
        if matches {
            let len = point.len();
            if best.as_ref().is_none_or(|(best_len, _)| len > *best_len) {
                best = Some((len, fs.clone()));
            }
        }
    }

    best.map(|(_, fs)| fs)
}

/// `(mount point, filesystem type)` pairs. Empty on platforms without
/// `/proc/mounts`, which degrades to the name check rather than failing.
fn read_mount_table() -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string("/proc/mounts") else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let _device = fields.next()?;
            let point = fields.next()?;
            let fs = fields.next()?;
            // Mount points are escaped octal in /proc/mounts.
            Some((point.replace("\\040", " "), fs.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mounts() -> Vec<(String, String)> {
        vec![
            ("/".into(), "ext4".into()),
            ("/home".into(), "ext4".into()),
            ("/home/user/GoogleDrive".into(), "fuse.rclone".into()),
            ("/mnt/nas".into(), "nfs4".into()),
            ("/home/user/Dropbox".into(), "ext4".into()),
        ]
    }

    #[test]
    fn an_rclone_mount_is_detected_by_filesystem_not_by_name() {
        let risk = assess_with(
            Path::new("/home/user/GoogleDrive/passwords.kdbx"),
            &mounts(),
        );
        assert_eq!(
            risk,
            StorageRisk::Fuse {
                fs: "rclone".into()
            }
        );
        assert!(risk.explain().contains("rclone"));
    }

    #[test]
    fn an_rclone_mount_at_an_innocent_path_is_still_caught() {
        // The case pure name-matching misses entirely.
        let mut m = mounts();
        m.push(("/home/user/vaults".into(), "fuse.rclone".into()));
        assert!(assess_with(Path::new("/home/user/vaults/passwords.kdbx"), &m).is_risky());
    }

    #[test]
    fn a_network_mount_is_flagged_separately() {
        assert_eq!(
            assess_with(Path::new("/mnt/nas/passwords.kdbx"), &mounts()),
            StorageRisk::Network { fs: "NFS4".into() }
        );
    }

    #[test]
    fn a_sync_daemon_on_an_ordinary_directory_is_still_flagged() {
        // Dropbox syncs a normal ext4 directory, so the mount table says
        // nothing. The name check exists for exactly this.
        let risk = assess_with(Path::new("/home/user/Dropbox/passwords.kdbx"), &mounts());
        assert_eq!(
            risk,
            StorageRisk::NameSuggestsSync {
                provider: "Dropbox".into()
            }
        );
    }

    #[test]
    fn ordinary_local_storage_is_not_flagged() {
        let risk = assess_with(Path::new("/home/user/passwords.kdbx"), &mounts());
        assert_eq!(risk, StorageRisk::Local);
        assert!(!risk.is_risky());
    }

    #[test]
    fn the_longest_matching_mount_point_wins() {
        // "/" matches everything, so a naive first-match reports ext4 for the
        // rclone mount and the warning never fires.
        assert_eq!(
            filesystem_for(Path::new("/home/user/GoogleDrive/x.kdbx"), &mounts()).as_deref(),
            Some("fuse.rclone")
        );
    }

    #[test]
    fn a_prefix_that_is_not_a_path_component_does_not_match() {
        // /home/user/GoogleDriveBackup is not inside the /home/user/GoogleDrive
        // mount, so it must resolve to the enclosing /home mount instead of
        // inheriting rclone's risk from a bare string prefix.
        let fs = filesystem_for(Path::new("/home/user/GoogleDriveBackup/x.kdbx"), &mounts());
        assert_eq!(fs.as_deref(), Some("ext4"));

        // It is still flagged, but by name rather than by mount — a different
        // and weaker claim, which is the point of keeping them distinct.
        assert_eq!(
            assess_with(Path::new("/home/user/GoogleDriveBackup/x.kdbx"), &mounts()),
            StorageRisk::NameSuggestsSync {
                provider: "Google Drive".into()
            }
        );
    }

    #[test]
    fn every_risk_names_the_hazard_and_the_fix() {
        for risk in [
            StorageRisk::Fuse {
                fs: "rclone".into(),
            },
            StorageRisk::Network { fs: "NFS4".into() },
            StorageRisk::NameSuggestsSync {
                provider: "Dropbox".into(),
            },
        ] {
            assert!(risk.explain().len() > 40, "explanation too vague: {risk:?}");
            assert!(!risk.remedy().is_empty(), "no remedy offered for {risk:?}");
        }
    }
}
