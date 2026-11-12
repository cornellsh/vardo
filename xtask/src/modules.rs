//! The `core-modules` gate: every core module states its contract in its
//! first lines and stays small enough to read whole.

use std::path::Path;

/// Longest core source file allowed, in lines.
pub const MAX_LINES: usize = 500;

/// Reads every `.rs` file under `dir`, as (path relative to `dir`, text).
pub fn read_tree(dir: &Path) -> std::io::Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let rel = path.strip_prefix(dir).unwrap_or(&path);
                out.push((rel.display().to_string(), std::fs::read_to_string(&path)?));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// One line per broken rule.
pub fn check(files: &[(String, String)]) -> Vec<String> {
    if files.is_empty() {
        return vec!["found no core modules".into()];
    }
    let mut broken = Vec::new();
    for (path, text) in files {
        let lines = text.lines().count();
        if lines > MAX_LINES {
            broken.push(format!("{path}: {lines} lines, over {MAX_LINES}; split it"));
        }
        let header: Vec<&str> = text.lines().take_while(|l| l.starts_with("//!")).collect();
        if header.is_empty() {
            broken.push(format!("{path}: does not open with a //! header"));
        } else if path != "lib.rs" && !header.iter().any(|l| l.contains("Contract:")) {
            broken.push(format!("{path}: header has no \"Contract:\" line"));
        }
    }
    broken
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, text: &str) -> (String, String) {
        (path.into(), text.into())
    }

    #[test]
    fn a_module_with_a_contract_passes() {
        let ok = file("a.rs", "//! A.\n//!\n//! Contract: does a.\n\nfn a() {}\n");
        assert!(check(&[ok]).is_empty());
    }

    #[test]
    fn a_missing_header_or_contract_fails() {
        let bare = file("bare.rs", "fn a() {}\n");
        let vague = file("vague.rs", "//! Does things.\nfn a() {}\n");
        assert_eq!(
            check(&[bare, vague]),
            [
                "bare.rs: does not open with a //! header",
                "vague.rs: header has no \"Contract:\" line",
            ]
        );
    }

    #[test]
    fn a_long_module_fails() {
        let long = format!("//! Contract: x.\n{}", "fn a() {}\n".repeat(MAX_LINES));
        assert_eq!(
            check(&[file("long.rs", &long)]),
            ["long.rs: 501 lines, over 500; split it"]
        );
    }

    #[test]
    fn no_modules_fails_closed() {
        assert_eq!(check(&[]), ["found no core modules"]);
    }
}
