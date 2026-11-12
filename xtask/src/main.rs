//! `cargo xtask check`: every gate CI runs, one line each.
//!
//! Each gate prints `ok`, `FAIL` or `skip`, its name and its seconds. Its full
//! output goes to `target/check/<gate>.log`, so the reader opens one log
//! instead of scrolling cargo output. Any failure makes the exit code
//! non-zero. A gate that cannot run says why instead of passing.
//!
//! Usage: `cargo xtask check [--fast] [--only <gate>]... [--json]`

#![forbid(unsafe_code)]

mod doclinks;
mod ipc;
mod layers;

use std::{
    env,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    time::Instant,
};

enum Outcome {
    Ok,
    Fail,
    Skip(String),
}

struct Gate {
    name: &'static str,
    /// Part of `--fast`, which also narrows clippy and tests to changed crates.
    fast: bool,
    run: fn(&mut Ctx) -> Outcome,
}

const GATES: &[Gate] = &[
    Gate {
        name: "doc-links",
        fast: true,
        run: doc_links,
    },
    Gate {
        name: "layers",
        fast: true,
        run: layer_deps,
    },
    Gate {
        name: "ipc-names",
        fast: true,
        run: ipc_names,
    },
    Gate {
        name: "fmt",
        fast: true,
        run: |c| c.cargo(&["fmt", "--all", "--check"]),
    },
    Gate {
        name: "app-typecheck",
        fast: false,
        run: |c| c.npx("app", &["tsc", "--noEmit"]),
    },
    Gate {
        name: "app-lint",
        fast: false,
        run: |c| c.npx("app", &["eslint", "src", "--max-warnings", "0"]),
    },
    // Before clippy, because the app crate embeds the built interface.
    Gate {
        name: "app-build",
        fast: false,
        run: |c| c.npx("app", &["vite", "build"]),
    },
    Gate {
        name: "clippy",
        fast: true,
        run: clippy,
    },
    // The suite never enables `network`, so without this it could rot unseen.
    Gate {
        name: "network",
        fast: false,
        run: |c| {
            c.cargo(&[
                "build",
                "--locked",
                "-p",
                "vardo-drive",
                "--features",
                "network",
            ])
        },
    },
    Gate {
        name: "test",
        fast: true,
        run: test,
    },
    Gate {
        name: "app-test",
        fast: false,
        run: |c| c.npx("app", &["vitest", "run"]),
    },
    // The ported Telegram palette fails WCAG AA in places as shipped, so the
    // deviations in tokens.css are measured here, not asserted.
    Gate {
        name: "app-contrast",
        fast: false,
        run: |c| c.node("app", &["scripts/contrast.mjs"]),
    },
    Gate {
        name: "ext-typecheck",
        fast: false,
        run: |c| c.npx("extension", &["tsc", "--noEmit", "-p", "."]),
    },
    Gate {
        name: "ext-test",
        fast: false,
        run: |c| c.npx("extension", &["vitest", "run"]),
    },
    Gate {
        name: "e2e",
        fast: false,
        run: e2e,
    },
];

struct Ctx {
    root: PathBuf,
    log: File,
    /// `None` runs clippy and tests on the whole workspace.
    crates: Option<Vec<String>>,
}

impl Ctx {
    fn run(&mut self, dir: &str, program: &str, args: &[&str], envs: &[(&str, &Path)]) -> Outcome {
        let _ = writeln!(self.log, "$ (cd {dir} && {program} {})", args.join(" "));
        let (Ok(out), Ok(err)) = (self.log.try_clone(), self.log.try_clone()) else {
            return Outcome::Fail;
        };
        let status = Command::new(program)
            .args(args)
            .envs(envs.iter().copied())
            .current_dir(self.root.join(dir))
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .status();
        match status {
            Ok(s) if s.success() => Outcome::Ok,
            Ok(s) => {
                let _ = writeln!(self.log, "exited with {s}");
                Outcome::Fail
            }
            Err(e) => {
                let _ = writeln!(self.log, "could not start {program}: {e}");
                Outcome::Fail
            }
        }
    }

    fn cargo(&mut self, args: &[&str]) -> Outcome {
        let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        self.run(".", &cargo, args, &[])
    }

    fn npx(&mut self, dir: &str, args: &[&str]) -> Outcome {
        if let Some(skip) = missing_node_modules(&self.root, dir) {
            return skip;
        }
        // Windows only runs npx through its .cmd shim.
        let npx = if cfg!(windows) { "npx.cmd" } else { "npx" };
        self.run(dir, npx, args, &[])
    }

    fn node(&mut self, dir: &str, args: &[&str]) -> Outcome {
        if let Some(skip) = missing_node_modules(&self.root, dir) {
            return skip;
        }
        self.run(dir, "node", args, &[])
    }

    /// `-p` flags for the selected crates, or `None` when there is nothing to do.
    fn package_args(&self) -> Option<Vec<String>> {
        match &self.crates {
            None => Some(vec!["--workspace".into()]),
            Some(c) if c.is_empty() => None,
            Some(c) => Some(c.iter().flat_map(|p| ["-p".into(), p.clone()]).collect()),
        }
    }
}

fn missing_node_modules(root: &Path, dir: &str) -> Option<Outcome> {
    (!root.join(dir).join("node_modules").is_dir())
        .then(|| Outcome::Skip(format!("no {dir}/node_modules (run: cd {dir} && npm ci)")))
}

fn doc_links(c: &mut Ctx) -> Outcome {
    let listed = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(&c.root)
        .output();
    // Fails closed: no file list must never read as no broken links.
    let files = match listed {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .split('\0')
            .filter(|f| !f.is_empty())
            .map(String::from)
            .collect::<Vec<_>>(),
        other => {
            let _ = writeln!(c.log, "git ls-files failed: {other:?}");
            return Outcome::Fail;
        }
    };
    match doclinks::check(&c.root, &files) {
        Ok(broken) if broken.is_empty() => Outcome::Ok,
        Ok(broken) => {
            for line in broken {
                let _ = writeln!(c.log, "{line}");
            }
            Outcome::Fail
        }
        Err(e) => {
            let _ = writeln!(c.log, "{e}");
            Outcome::Fail
        }
    }
}

fn layer_deps(c: &mut Ctx) -> Outcome {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1"])
        .current_dir(&c.root)
        .output();
    let metadata = match out.map(|o| (o.status.success(), o)) {
        Ok((true, o)) => serde_json::from_slice(&o.stdout),
        other => {
            let _ = writeln!(c.log, "cargo metadata failed: {other:?}");
            return Outcome::Fail;
        }
    };
    let broken = match metadata {
        Ok(m) => layers::check(&m),
        Err(e) => vec![format!("cargo metadata is not JSON: {e}")],
    };
    for line in &broken {
        let _ = writeln!(c.log, "{line}");
    }
    if broken.is_empty() {
        Outcome::Ok
    } else {
        Outcome::Fail
    }
}

fn ipc_names(c: &mut Ctx) -> Outcome {
    let read = |p: &str| std::fs::read_to_string(c.root.join(p));
    // Commands may live in any shell module, so the arguments check reads them all.
    let shell: std::io::Result<String> = std::fs::read_dir(c.root.join("app/src-tauri/src"))
        .and_then(|dir| dir.map(|e| std::fs::read_to_string(e?.path())).collect());
    let broken = match (
        read("app/src-tauri/src/lib.rs"),
        read("app/src/ipc/tauri.ts"),
        shell,
    ) {
        (Ok(lib), Ok(ts), Ok(shell)) => {
            let mut broken = ipc::check(&lib, &ts);
            broken.extend(ipc::check_args(&shell, &ts));
            broken
        }
        (lib, ts, shell) => vec![format!(
            "cannot read the command lists: {:?} {:?} {:?}",
            lib.err(),
            ts.err(),
            shell.err()
        )],
    };
    for line in &broken {
        let _ = writeln!(c.log, "{line}");
    }
    if broken.is_empty() {
        Outcome::Ok
    } else {
        Outcome::Fail
    }
}

fn clippy(c: &mut Ctx) -> Outcome {
    let Some(packages) = c.package_args() else {
        return Outcome::Skip("no crate changed".into());
    };
    let mut args = vec!["clippy", "--locked", "--all-targets", "--all-features"];
    args.extend(packages.iter().map(String::as_str));
    args.extend(["--", "-D", "warnings"]);
    c.cargo(&args)
}

fn test(c: &mut Ctx) -> Outcome {
    let Some(packages) = c.package_args() else {
        return Outcome::Skip("no crate changed".into());
    };
    let mut args = vec!["test", "--locked"];
    args.extend(packages.iter().map(String::as_str));
    c.cargo(&args)
}

fn e2e(c: &mut Ctx) -> Outcome {
    if let Some(skip) = missing_node_modules(&c.root, "extension") {
        return skip;
    }
    let Some(chromium) = find_chromium() else {
        return Outcome::Skip(
            "no Chromium: set CHROMIUM, or run npx @puppeteer/browsers install chrome@stable \
             --path ~/.cache/chrome-for-testing"
                .into(),
        );
    };
    c.run(
        "extension",
        "node",
        &["e2e/run.mjs"],
        &[("CHROMIUM", &chromium)],
    )
}

/// `$CHROMIUM`, else the newest Chrome for Testing in the usual caches, else a
/// Chromium on the PATH. Branded Chrome is never picked: since version 137 it
/// ignores `--load-extension`, which the end-to-end check needs.
fn find_chromium() -> Option<PathBuf> {
    if let Some(set) = env::var_os("CHROMIUM").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(set));
    }
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let mut found = Vec::new();
    for cache in [".cache/chrome-for-testing", ".cache/puppeteer"] {
        if let Some(home) = &home {
            find_files(&home.join(cache), 8, &mut found);
        }
    }
    found.sort();
    if let Some(newest) = found.pop() {
        return Some(newest);
    }
    let names = ["chromium", "chromium-browser", "google-chrome-for-testing"];
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .flat_map(|dir| names.map(|n| dir.join(n)))
        .chain([PathBuf::from(
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        )])
        .find(|p| p.is_file())
}

fn find_files(dir: &Path, depth: u32, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let is_binary = ["Google Chrome for Testing", "chrome", "chrome.exe"]
            .iter()
            .any(|n| name == *n);
        if is_binary && path.is_file() {
            found.push(path);
        } else if depth > 0 && path.is_dir() {
            find_files(&path, depth - 1, found);
        }
    }
}

/// Workspace crates touched by uncommitted or untracked changes. `None` when a
/// workspace-wide file changed, so everything must be checked.
fn changed_crates(root: &Path) -> Option<Vec<String>> {
    let mut paths = String::new();
    for args in [
        &["diff", "--name-only", "HEAD"][..],
        &["ls-files", "--others", "--exclude-standard"],
    ] {
        let out = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        paths.push_str(&String::from_utf8_lossy(&out.stdout));
    }
    let mut crates = Vec::new();
    for path in paths.lines() {
        let name = if let Some(rest) = path.strip_prefix("crates/") {
            rest.split('/').next().unwrap_or_default().to_string()
        } else if path.starts_with("app/src-tauri/") {
            "vardo-app".into()
        } else if path.starts_with("xtask/") {
            "xtask".into()
        } else if matches!(path, "Cargo.toml" | "Cargo.lock") || path.starts_with(".cargo/") {
            return None;
        } else {
            continue;
        };
        if !crates.contains(&name) {
            crates.push(name);
        }
    }
    Some(crates)
}

fn usage() -> ExitCode {
    let names: Vec<_> = GATES.iter().map(|g| g.name).collect();
    eprintln!("usage: cargo xtask check [--fast] [--only <gate>]... [--json]");
    eprintln!("gates: {}", names.join(", "));
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("check") {
        return usage();
    }
    let (mut fast, mut json, mut only) = (false, false, Vec::new());
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--fast" => fast = true,
            "--json" => json = true,
            "--only" => match rest.next() {
                Some(g) if GATES.iter().any(|x| x.name == g) => only.push(g.clone()),
                _ => return usage(),
            },
            _ => return usage(),
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits in the workspace")
        .to_path_buf();
    let logs = root.join("target/check");
    let _ = fs::remove_dir_all(&logs);
    if let Err(e) = fs::create_dir_all(&logs) {
        eprintln!("cannot create {}: {e}", logs.display());
        return ExitCode::FAILURE;
    }
    // The app crate embeds app/dist at compile time. A placeholder lets
    // clippy and tests compile it when the interface was never built;
    // app-build replaces it with the real thing.
    let dist = root.join("app/dist/index.html");
    if !dist.exists() {
        let _ = fs::create_dir_all(root.join("app/dist"));
        let _ = fs::write(&dist, "<!doctype html>\n");
    }
    let crates = if fast { changed_crates(&root) } else { None };

    let mut results = Vec::new();
    for gate in GATES {
        let selected = if only.is_empty() {
            !fast || gate.fast
        } else {
            only.iter().any(|g| g == gate.name)
        };
        if !selected {
            continue;
        }
        let path = logs.join(format!("{}.log", gate.name));
        let log = match File::create(&path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("cannot create {}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        };
        let mut ctx = Ctx {
            root: root.clone(),
            log,
            crates: crates.clone(),
        };
        let started = Instant::now();
        let outcome = (gate.run)(&mut ctx);
        let secs = started.elapsed().as_secs_f64();
        let line = match &outcome {
            Outcome::Ok => format!("ok   {:<14} {secs:>6.1}s", gate.name),
            Outcome::Fail => format!(
                "FAIL {:<14} {secs:>6.1}s  target/check/{}.log",
                gate.name, gate.name
            ),
            Outcome::Skip(why) => format!("skip {:<14} {secs:>6.1}s  {why}", gate.name),
        };
        if json {
            eprintln!("{line}")
        } else {
            println!("{line}")
        }
        // In CI nobody can open the log afterwards, so it goes in the job output.
        if matches!(outcome, Outcome::Fail) && env::var_os("GITHUB_ACTIONS").is_some() {
            println!("{}", fs::read_to_string(&path).unwrap_or_default());
        }
        results.push((gate.name, outcome, secs));
    }

    let failed = results
        .iter()
        .filter(|r| matches!(r.1, Outcome::Fail))
        .count();
    let skipped: Vec<_> = results
        .iter()
        .filter_map(|(name, o, _)| match o {
            Outcome::Skip(why) => Some((*name, why.as_str())),
            _ => None,
        })
        .collect();
    if json {
        let gates: Vec<_> = results
            .iter()
            .map(|(name, o, secs)| {
                let (status, reason) = match o {
                    Outcome::Ok => ("ok", None),
                    Outcome::Fail => ("fail", None),
                    Outcome::Skip(why) => ("skip", Some(why.as_str())),
                };
                serde_json::json!({
                    "gate": name,
                    "status": status,
                    "seconds": (secs * 10.0).round() / 10.0,
                    "reason": reason,
                    "log": format!("target/check/{name}.log"),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({ "ok": failed == 0, "gates": gates })
        );
    } else {
        let ok = results.len() - failed - skipped.len();
        println!("\n{ok} ok, {failed} failed, {} skipped", skipped.len());
        for (name, why) in &skipped {
            println!("  skipped {name}: {why}");
        }
    }
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
