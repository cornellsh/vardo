//! `cargo xtask release <patch|minor|major|X.Y.Z>`: raise the version in every
//! file that states it, commit, sign a tag and push both. The tag starts
//! `.github/workflows/release.yml`, which builds and publishes
//! (`dec:unsigned-releases`).

use std::{
    env, fs,
    path::Path,
    process::{Command, ExitCode},
};

/// Every file the release commit touches. Each must be clean beforehand, so
/// the commit carries the version and nothing else.
const FILES: [&str; 8] = [
    "Cargo.toml",
    "Cargo.lock",
    "app/src-tauri/tauri.conf.json",
    "extension/manifest.json",
    "app/package.json",
    "app/package-lock.json",
    "extension/package.json",
    "extension/package-lock.json",
];

const CARGO_KEY: &str = "[workspace.package]\nversion = \"";

/// The version after `current`, or `None` for a bad request or one that
/// would not go forward.
pub fn next(current: &str, how: &str) -> Option<String> {
    let parse = |s: &str| -> Option<[u64; 3]> {
        let parts: Vec<u64> = s
            .split('.')
            .map(|p| p.parse().ok())
            .collect::<Option<_>>()?;
        parts.try_into().ok()
    };
    let [a, b, c] = parse(current)?;
    let n = match how {
        "patch" => [a, b, c + 1],
        "minor" => [a, b + 1, 0],
        "major" => [a + 1, 0, 0],
        exact => Some(parse(exact)?).filter(|n| *n > [a, b, c])?,
    };
    Some(format!("{}.{}.{}", n[0], n[1], n[2]))
}

fn replace_once(root: &Path, file: &str, from: &str, to: &str) -> Result<(), String> {
    let path = root.join(file);
    let text = fs::read_to_string(&path).map_err(|e| format!("{file}: {e}"))?;
    if text.matches(from).count() != 1 {
        return Err(format!("{file}: expected `{from}` exactly once"));
    }
    fs::write(&path, text.replacen(from, to, 1)).map_err(|e| format!("{file}: {e}"))
}

fn run(root: &Path, program: &str, args: &[&str], dir: &str) -> Result<String, String> {
    let out = Command::new(program)
        .args(args)
        .current_dir(root.join(dir))
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{program} {} failed:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn release(root: &Path, how: &str) -> Result<String, String> {
    let mut status = vec!["status", "--porcelain", "--"];
    status.extend(FILES);
    let dirty = run(root, "git", &status, ".")?;
    if !dirty.is_empty() {
        return Err(format!("commit or discard these first:\n{dirty}"));
    }

    let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).map_err(|e| e.to_string())?;
    let current = cargo_toml
        .split_once(CARGO_KEY)
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(v, _)| v.to_owned())
        .ok_or("Cargo.toml has no [workspace.package] version")?;
    let new = next(&current, how).ok_or(format!("cannot go from {current} to `{how}`"))?;
    let tag = format!("v{new}");

    replace_once(
        root,
        "Cargo.toml",
        &format!("{CARGO_KEY}{current}\""),
        &format!("{CARGO_KEY}{new}\""),
    )?;
    for file in ["app/src-tauri/tauri.conf.json", "extension/manifest.json"] {
        replace_once(
            root,
            file,
            &format!("\"version\": \"{current}\""),
            &format!("\"version\": \"{new}\""),
        )?;
    }
    // Windows only runs npm through its .cmd shim.
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    for dir in ["app", "extension"] {
        run(root, npm, &["version", &new, "--no-git-tag-version"], dir)?;
    }
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    run(root, &cargo, &["update", "--workspace", "--quiet"], ".")?;

    let message = format!("Release {tag}");
    let mut commit = vec!["commit", "--quiet", "-m", &message, "--"];
    commit.extend(FILES);
    run(root, "git", &commit, ".")?;
    run(root, "git", &["tag", "-s", &tag, "-m", &tag], ".")?;
    // Atomic, so main never gets the commit without the tag or the reverse.
    run(
        root,
        "git",
        &["push", "--atomic", "--quiet", "origin", "HEAD:main", &tag],
        ".",
    )?;
    Ok(tag)
}

pub fn main(root: &Path, how: &str) -> ExitCode {
    match release(root, how) {
        Ok(tag) => {
            println!("pushed {tag}; the Release workflow builds and publishes it");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::next;

    #[test]
    fn versions_only_go_forward() {
        assert_eq!(next("0.0.1", "patch").as_deref(), Some("0.0.2"));
        assert_eq!(next("0.9.3", "minor").as_deref(), Some("0.10.0"));
        assert_eq!(next("1.2.3", "major").as_deref(), Some("2.0.0"));
        assert_eq!(next("0.0.1", "0.3.0").as_deref(), Some("0.3.0"));
        assert_eq!(next("0.3.0", "0.2.9"), None);
        assert_eq!(next("0.3.0", "0.3.0"), None);
        assert_eq!(next("0.3.0", "1.0"), None);
        assert_eq!(next("0.3.0", "huge"), None);
    }
}
