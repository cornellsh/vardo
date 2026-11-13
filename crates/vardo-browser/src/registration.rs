//! Registering the native host with Chromium browsers.
//!
//! Contract: writes `app.vardo.browser.json` only into browsers that have run
//! at least once, naming one host binary and allowing only Vardo's extension.
//! Disconnecting removes only that file. Shared by `vardo-browser-host
//! --install` and the app's settings, so both register identically.

use std::path::{Path, PathBuf};

use serde_json::json;

/// The extension's ID, fixed by the public key in its manifest. Chrome only
/// starts the host for the IDs listed when it was registered.
pub const EXTENSION_ID: &str = "fkemoafedkbgljikdhjbglhlhfplamll";

/// The name the extension asks Chrome for.
pub const HOST_NAME: &str = "app.vardo.browser";

/// A browser and the profile directory it looks for hosts under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browser {
    pub name: &'static str,
    pub dir: PathBuf,
}

/// Where one browser stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub name: &'static str,
    /// The browser has run on this machine, so it can be connected.
    pub installed: bool,
    /// Its registration names `host`. One naming another copy of the host
    /// does not count: that copy may be gone.
    pub connected: bool,
}

/// The browsers checked to work, for this user. Others are added once
/// someone confirms them.
pub fn browsers(home: &Path) -> Vec<Browser> {
    let mac = cfg!(target_os = "macos");
    let base = if mac {
        home.join("Library/Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
    };
    let pick = |on_mac: &str, elsewhere: &str| base.join(if mac { on_mac } else { elsewhere });
    vec![
        Browser {
            name: "Google Chrome",
            dir: pick("Google/Chrome", "google-chrome"),
        },
        Browser {
            name: "Chromium",
            dir: pick("Chromium", "chromium"),
        },
        Browser {
            name: "Brave",
            dir: base.join("BraveSoftware/Brave-Browser"),
        },
        Browser {
            name: "Microsoft Edge",
            dir: pick("Microsoft Edge", "microsoft-edge"),
        },
        Browser {
            name: "Vivaldi",
            dir: pick("Vivaldi", "vivaldi"),
        },
    ]
}

/// The registration file Chrome reads.
pub fn manifest(host: &Path) -> serde_json::Value {
    json!({
        "name": HOST_NAME,
        "description": "Vardo password manager",
        "path": host,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")],
    })
}

fn file(browser: &Browser) -> PathBuf {
    browser
        .dir
        .join("NativeMessagingHosts")
        .join(format!("{HOST_NAME}.json"))
}

pub fn status(browsers: &[Browser], host: &Path) -> Vec<Status> {
    browsers
        .iter()
        .map(|b| Status {
            name: b.name,
            installed: b.dir.is_dir(),
            connected: names(&file(b), host),
        })
        .collect()
}

fn names(file: &Path, host: &Path) -> bool {
    std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|m| m.get("path").and_then(|p| p.as_str()).map(PathBuf::from))
        .is_some_and(|p| p == host)
}

/// Registers `host` with every browser that has run. Returns their names, and
/// fails when there are none, since nothing would work.
pub fn install(browsers: &[Browser], host: &Path) -> Result<Vec<&'static str>, String> {
    let body = serde_json::to_vec_pretty(&manifest(host)).map_err(|e| e.to_string())?;
    let mut done = Vec::new();
    for browser in browsers {
        // A profile directory for a browser that is not installed would be litter.
        if !browser.dir.is_dir() {
            continue;
        }
        let target = file(browser);
        target
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&target, &body))
            .map_err(|e| {
                format!(
                    "{}: could not write {}: {e}",
                    browser.name,
                    target.display()
                )
            })?;
        done.push(browser.name);
    }
    if done.is_empty() {
        return Err("No supported browser found. Start Chrome, Chromium, Brave, Edge or Vivaldi once, then try again.".into());
    }
    Ok(done)
}

/// Removes the registration wherever it is. Returns the browsers it was in.
pub fn uninstall(browsers: &[Browser]) -> Result<Vec<&'static str>, String> {
    let mut done = Vec::new();
    for browser in browsers {
        let target = file(browser);
        match std::fs::remove_file(&target) {
            Ok(()) => done.push(browser.name),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("could not remove {}: {e}", target.display())),
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two(tag: &str) -> (PathBuf, Vec<Browser>) {
        let root = std::env::temp_dir().join(format!("vardo-registration-{tag}"));
        let _ = std::fs::remove_dir_all(&root);
        let present = root.join("chrome");
        std::fs::create_dir_all(&present).unwrap();
        let browsers = vec![
            Browser {
                name: "Present",
                dir: present,
            },
            Browser {
                name: "Absent",
                dir: root.join("never-run"),
            },
        ];
        (root, browsers)
    }

    #[test]
    fn only_browsers_that_have_run_are_registered() {
        let (root, browsers) = two("install");
        let host = root.join("vardo-browser-host");
        assert_eq!(install(&browsers, &host).unwrap(), ["Present"]);
        assert!(
            !browsers[1].dir.exists(),
            "no litter for a browser that never ran"
        );

        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(file(&browsers[0])).unwrap()).unwrap();
        assert_eq!(written, manifest(&host));
        assert_eq!(
            written["allowed_origins"],
            json!([format!("chrome-extension://{EXTENSION_ID}/")]),
            "only Vardo's extension may start the host"
        );
    }

    #[test]
    fn status_counts_only_a_registration_naming_this_host() {
        let (root, browsers) = two("status");
        let host = root.join("vardo-browser-host");
        install(&browsers, &root.join("an-old-copy")).unwrap();
        let before = status(&browsers, &host);
        assert!(before[0].installed && !before[0].connected);
        assert!(!before[1].installed && !before[1].connected);

        install(&browsers, &host).unwrap();
        assert!(status(&browsers, &host)[0].connected);
    }

    #[test]
    fn uninstall_removes_the_registration_and_is_idempotent() {
        let (root, browsers) = two("uninstall");
        let host = root.join("vardo-browser-host");
        install(&browsers, &host).unwrap();
        assert_eq!(uninstall(&browsers).unwrap(), ["Present"]);
        assert!(!status(&browsers, &host)[0].connected);
        assert!(uninstall(&browsers).unwrap().is_empty());
    }

    #[test]
    fn no_browser_at_all_is_an_error() {
        let (root, mut browsers) = two("none");
        browsers.remove(0);
        assert!(install(&browsers, &root.join("host")).is_err());
    }
}
