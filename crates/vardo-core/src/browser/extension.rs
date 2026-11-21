//! The Chromium extension folder that updates with the app.
//!
//! Contract: [`refresh`] leaves `to` holding exactly the extension files of
//! `from`, test files left out. It rewrites nothing when the two already
//! match, and builds the new copy beside `to` before swapping it in, so a
//! browser never loads a folder with half the files.
//!
//! The app bundles the extension and copies it to a folder that keeps its path
//! across updates. A browser loads an unpacked extension from disk at every
//! start, so after an app update the next browser restart runs the matching
//! extension (`dec:auto-update`). The manifest's `key` keeps the extension id
//! the same whatever the folder.

use std::path::{Path, PathBuf};

use crate::CoreError;

/// Makes `to` match `from`. Returns whether anything changed.
pub fn refresh(from: &Path, to: &Path) -> Result<bool, CoreError> {
    let wanted = files(from)?;
    if wanted.is_empty() {
        return Err(CoreError::NotFound {
            path: from.display().to_string(),
        });
    }
    if files(to).is_ok_and(|have| have == wanted) {
        return Ok(false);
    }
    let fresh = to.with_extension("new");
    let _ = std::fs::remove_dir_all(&fresh);
    for (rel, bytes) in &wanted {
        let path = fresh.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(path, bytes).map_err(io)?;
    }
    let _ = std::fs::remove_dir_all(to);
    std::fs::rename(&fresh, to).map_err(io)?;
    Ok(true)
}

/// Every file under `root` by relative path, sorted, tests left out.
fn files(root: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, CoreError> {
    let mut out = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for item in std::fs::read_dir(&dir).map_err(io)? {
            let path = item.map_err(io)?.path();
            if path.is_dir() {
                dirs.push(path);
            } else if !path.to_string_lossy().ends_with(".test.js") {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                out.push((rel, std::fs::read(&path).map_err(io)?));
            }
        }
    }
    out.sort();
    Ok(out)
}

fn io(e: std::io::Error) -> CoreError {
    CoreError::Io {
        detail: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_follows_the_bundle_and_leaves_tests_out() {
        let base = std::env::temp_dir().join("vardo-extension-refresh");
        let _ = std::fs::remove_dir_all(&base);
        let (from, to) = (base.join("bundle"), base.join("extension"));
        std::fs::create_dir_all(from.join("src")).unwrap();
        std::fs::write(from.join("manifest.json"), "1").unwrap();
        std::fs::write(from.join("src/fill.js"), "a").unwrap();
        std::fs::write(from.join("src/fill.test.js"), "t").unwrap();

        assert!(refresh(&from, &to).unwrap());
        assert!(!to.join("src/fill.test.js").exists());
        assert!(
            !refresh(&from, &to).unwrap(),
            "nothing changed, nothing written"
        );

        std::fs::write(from.join("manifest.json"), "2").unwrap();
        std::fs::write(to.join("stray.js"), "x").unwrap();
        assert!(refresh(&from, &to).unwrap());
        assert_eq!(
            std::fs::read_to_string(to.join("manifest.json")).unwrap(),
            "2"
        );
        assert!(!to.join("stray.js").exists());
        assert!(!to.with_extension("new").exists());

        assert!(refresh(&base.join("missing"), &to).is_err());
    }
}
