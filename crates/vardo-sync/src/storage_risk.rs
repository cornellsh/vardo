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

    /// One line, short enough not to wrap in the banner it appears in.
    ///
    /// The long version of this was a four-line paragraph, which in a banner is
    /// a wall of text that gets dismissed unread. The reasoning belongs in
    /// [`Self::why`], behind a disclosure, not in front of someone who only
    /// wanted to open their vault.
    pub fn explain(&self) -> String {
        match self {
            StorageRisk::Local => "Stored on this computer".into(),
            // Phrased to sidestep a/an: the filesystem name is interpolated and
            // "a rclone mount" reads as a bug in the app.
            StorageRisk::Fuse { fs } => format!("Saving here is not atomic ({fs} mount)"),
            StorageRisk::Network { fs } => {
                format!("Saving here is not atomic ({fs} network mount)")
            }
            StorageRisk::NameSuggestsSync { provider } => {
                format!("This looks like a {provider} folder")
            }
        }
    }

    /// The full reasoning, for whoever opens the disclosure.
    pub fn why(&self) -> &'static str {
        match self {
            StorageRisk::Local => "",
            StorageRisk::Fuse { .. } => {
                "A safe save replaces the file by renaming over it, which userspace mounts \
                 often cannot do atomically. The usual result is a forked copy such as \
                 \"vault (1).kdbx\", or a save that silently lands on a stale version."
            }
            StorageRisk::Network { .. } => {
                "An atomic rename is not guaranteed across a network, so a save interrupted \
                 partway can leave a partial file."
            }
            StorageRisk::NameSuggestsSync { .. } => {
                "A sync daemon rewriting the file underneath Vardo is how KeePass databases \
                 get forked into conflicted copies."
            }
        }
    }

    /// What to do about it. One line.
    pub fn remedy(&self) -> &'static str {
        match self {
            StorageRisk::Local => "",
            _ => "Keep it locally and let Vardo sync it",
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

    /// Wide enough for a banner in a narrow window, narrow enough that the
    /// text cannot wrap onto a second line. A warning that wraps is a wall,
    /// and a wall is dismissed unread.
    const ONE_LINE: usize = 48;

    #[test]
    fn every_risk_fits_on_one_line_and_offers_a_fix() {
        for risk in [
            StorageRisk::Fuse {
                fs: "rclone".into(),
            },
            StorageRisk::Network { fs: "NFS4".into() },
            StorageRisk::NameSuggestsSync {
                provider: "Dropbox".into(),
            },
        ] {
            let explain = risk.explain();
            assert!(
                explain.len() <= ONE_LINE,
                "must fit one line ({} chars): {explain:?}",
                explain.len()
            );
            assert!(
                risk.remedy().len() <= ONE_LINE,
                "remedy must fit one line: {:?}",
                risk.remedy()
            );
            assert!(!risk.remedy().is_empty(), "no remedy offered for {risk:?}");

            // The reasoning still has to exist - it just belongs behind a
            // disclosure rather than in front of someone opening their vault.
            assert!(
                risk.why().len() > 60,
                "the explanation must survive somewhere: {risk:?}"
            );
        }
    }
}
