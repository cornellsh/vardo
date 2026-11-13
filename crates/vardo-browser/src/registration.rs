//! Registering the native host with Chromium browsers.
//!
//! Contract: writes `app.vardo.browser.json` only for browsers that have run at
//! least once, naming one host binary and allowing only Vardo's extension. On
//! Windows a per-user registry key points each browser at that file.
//! Disconnecting removes only what this wrote. Shared by `vardo-browser-host
//! --install` and the app's settings, so both register identically.

use std::path::{Path, PathBuf};

use serde_json::json;

/// The extension's ID, fixed by the public key in its manifest. Chrome only
/// starts the host for the IDs listed when it was registered.
pub const EXTENSION_ID: &str = "fkemoafedkbgljikdhjbglhlhfplamll";

/// The extension's ID in Firefox, from `browser_specific_settings` in the
/// Firefox build of its manifest (`extension/scripts/firefox.mjs`).
pub const GECKO_ID: &str = "browser@vardo.app";

/// The name the extension asks the browser for.
pub const HOST_NAME: &str = "app.vardo.browser";

/// Which family of browser, since the two read different manifests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Chromium,
    Firefox,
}

/// A browser, and where its registration lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browser {
    pub name: &'static str,
    pub kind: Kind,
    /// Exists once the browser has run, so it can be connected.
    pub dir: PathBuf,
    /// The registration file.
    pub manifest: PathBuf,
    /// Windows only: the key under `HKEY_CURRENT_USER` whose default value
    /// names `manifest`. Chrome looks there rather than in a directory.
    pub key: Option<String>,
}

/// Where one browser stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub name: &'static str,
    pub installed: bool,
    /// Its registration names `host`. One naming another copy of the host
    /// does not count: that copy may be gone.
    pub connected: bool,
}

/// The browsers checked to work, for this user. Others are added once
/// someone confirms them. `None` when the environment gives no home.
#[cfg(not(windows))]
pub fn browsers() -> Option<Vec<Browser>> {
    let home = PathBuf::from(std::env::var_os("HOME").filter(|h| !h.is_empty())?);
    let mac = cfg!(target_os = "macos");
    let base = if mac {
        home.join("Library/Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
    };
    let pick = |on_mac: &str, elsewhere: &str| base.join(if mac { on_mac } else { elsewhere });
    let browser = |name, dir: PathBuf| Browser {
        name,
        kind: Kind::Chromium,
        manifest: dir
            .join("NativeMessagingHosts")
            .join(format!("{HOST_NAME}.json")),
        dir,
        key: None,
    };
    // Firefox keeps hosts outside its profile folder.
    let (firefox, firefox_hosts) = if mac {
        (
            base.join("Firefox"),
            base.join("Mozilla/NativeMessagingHosts"),
        )
    } else {
        (
            home.join(".mozilla/firefox"),
            home.join(".mozilla/native-messaging-hosts"),
        )
    };
    Some(vec![
        browser("Google Chrome", pick("Google/Chrome", "google-chrome")),
        browser("Chromium", pick("Chromium", "chromium")),
        browser("Brave", base.join("BraveSoftware/Brave-Browser")),
        browser("Microsoft Edge", pick("Microsoft Edge", "microsoft-edge")),
        browser("Vivaldi", pick("Vivaldi", "vivaldi")),
        Browser {
            name: "Firefox",
            kind: Kind::Firefox,
            dir: firefox,
            manifest: firefox_hosts.join(format!("{HOST_NAME}.json")),
            key: None,
        },
    ])
}

/// On Windows every browser shares one manifest under the user's local app
/// data, and each has its own registry key pointing at it.
#[cfg(windows)]
pub fn browsers() -> Option<Vec<Browser>> {
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty())?);
    let manifest = local.join("Vardo").join(format!("{HOST_NAME}.json"));
    let browser = |name, data: &str, vendor: &str| Browser {
        name,
        kind: Kind::Chromium,
        dir: local.join(data),
        manifest: manifest.clone(),
        key: Some(format!(
            r"Software\{vendor}\NativeMessagingHosts\{HOST_NAME}"
        )),
    };
    Some(vec![
        browser(
            "Google Chrome",
            r"Google\Chrome\User Data",
            r"Google\Chrome",
        ),
        browser("Chromium", r"Chromium\User Data", "Chromium"),
        browser(
            "Brave",
            r"BraveSoftware\Brave-Browser\User Data",
            r"BraveSoftware\Brave-Browser",
        ),
        browser(
            "Microsoft Edge",
            r"Microsoft\Edge\User Data",
            r"Microsoft\Edge",
        ),
        // Its own manifest file, because Firefox reads a different shape.
        Browser {
            name: "Firefox",
            kind: Kind::Firefox,
            dir: PathBuf::from(std::env::var_os("APPDATA")?).join(r"Mozilla\Firefox"),
            manifest: local
                .join("Vardo")
                .join(format!("{HOST_NAME}.firefox.json")),
            key: Some(format!(
                r"Software\Mozilla\NativeMessagingHosts\{HOST_NAME}"
            )),
        },
    ])
}

/// The registration file the browser reads. Chromium names the extension by
/// origin, Firefox by add-on ID.
pub fn manifest(host: &Path, kind: Kind) -> serde_json::Value {
    let mut m = json!({
        "name": HOST_NAME,
        "description": "Vardo password manager",
        "path": host,
        "type": "stdio",
    });
    m[match kind {
        Kind::Chromium => "allowed_origins",
        Kind::Firefox => "allowed_extensions",
    }] = match kind {
        Kind::Chromium => json!([format!("chrome-extension://{EXTENSION_ID}/")]),
        Kind::Firefox => json!([GECKO_ID]),
    };
    m
}

pub fn status(browsers: &[Browser], host: &Path) -> Vec<Status> {
    browsers
        .iter()
        .map(|b| Status {
            name: b.name,
            installed: b.dir.is_dir(),
            connected: names(&b.manifest, host)
                && b.key
                    .as_deref()
                    .is_none_or(|k| registry::get(k).as_deref() == Some(&b.manifest)),
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
    let mut done = Vec::new();
    for browser in browsers {
        // Registering a browser that is not installed would be litter.
        if !browser.dir.is_dir() {
            continue;
        }
        let fail = |e: std::io::Error| format!("{}: could not register: {e}", browser.name);
        if let Some(parent) = browser.manifest.parent() {
            std::fs::create_dir_all(parent).map_err(fail)?;
        }
        let body =
            serde_json::to_vec_pretty(&manifest(host, browser.kind)).map_err(|e| e.to_string())?;
        std::fs::write(&browser.manifest, &body).map_err(fail)?;
        if let Some(key) = &browser.key {
            registry::set(key, &browser.manifest).map_err(fail)?;
        }
        done.push(browser.name);
    }
    if done.is_empty() {
        return Err("No supported browser found. Start Chrome, Chromium, Brave, Edge, Vivaldi or Firefox once, then try again.".into());
    }
    Ok(done)
}

/// Removes the registration wherever it is. Returns the browsers it was in.
pub fn uninstall(browsers: &[Browser]) -> Result<Vec<&'static str>, String> {
    let mut done = Vec::new();
    for browser in browsers {
        let mut found = false;
        if let Some(key) = &browser.key {
            found |= registry::delete(key).map_err(|e| format!("{}: {e}", browser.name))?;
        }
        match std::fs::remove_file(&browser.manifest) {
            Ok(()) => found = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(format!(
                    "could not remove {}: {e}",
                    browser.manifest.display()
                ))
            }
        }
        if found {
            done.push(browser.name);
        }
    }
    Ok(done)
}

/// The per-user registry, through `winreg`'s safe API.
#[cfg(windows)]
mod registry {
    use std::path::{Path, PathBuf};

    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    pub fn get(key: &str) -> Option<PathBuf> {
        let value: String = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(key)
            .ok()?
            .get_value("")
            .ok()?;
        Some(PathBuf::from(value))
    }

    pub fn set(key: &str, manifest: &Path) -> std::io::Result<()> {
        let (k, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(key)?;
        k.set_value("", &manifest.as_os_str())
    }

    /// True when there was a key to delete.
    pub fn delete(key: &str) -> std::io::Result<bool> {
        match RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(key) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// No registry elsewhere: no browser carries a key, so these are never reached
/// with one.
#[cfg(not(windows))]
mod registry {
    use std::path::{Path, PathBuf};

    pub fn get(_key: &str) -> Option<PathBuf> {
        None
    }

    pub fn set(_key: &str, _manifest: &Path) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }

    pub fn delete(_key: &str) -> std::io::Result<bool> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One browser that has run and one that never did. On Windows both carry
    /// a registry key under a test-only path.
    fn two(tag: &str) -> (PathBuf, Vec<Browser>) {
        let root = std::env::temp_dir().join(format!("vardo-registration-{tag}"));
        let _ = std::fs::remove_dir_all(&root);
        let present = root.join("chrome");
        std::fs::create_dir_all(&present).unwrap();
        let key = |name: &str| {
            cfg!(windows).then(|| {
                format!(r"Software\VardoTest\{tag}\{name}\NativeMessagingHosts\{HOST_NAME}")
            })
        };
        let browsers = vec![
            Browser {
                name: "Present",
                kind: Kind::Chromium,
                manifest: root.join("present").join(format!("{HOST_NAME}.json")),
                dir: present,
                key: key("present"),
            },
            Browser {
                name: "Absent",
                kind: Kind::Chromium,
                dir: root.join("never-run"),
                manifest: root.join("absent").join(format!("{HOST_NAME}.json")),
                key: key("absent"),
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
            !browsers[1].manifest.exists(),
            "no litter for a browser that never ran"
        );
        if let Some(key) = &browsers[1].key {
            assert_eq!(registry::get(key), None);
        }

        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&browsers[0].manifest).unwrap()).unwrap();
        assert_eq!(written, manifest(&host, Kind::Chromium));
        assert_eq!(
            written["allowed_origins"],
            json!([format!("chrome-extension://{EXTENSION_ID}/")]),
            "only Vardo's extension may start the host"
        );
        if let Some(key) = &browsers[0].key {
            assert_eq!(registry::get(key).as_ref(), Some(&browsers[0].manifest));
        }
        uninstall(&browsers).unwrap();
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
        uninstall(&browsers).unwrap();
    }

    #[test]
    fn uninstall_removes_the_registration_and_is_idempotent() {
        let (root, browsers) = two("uninstall");
        let host = root.join("vardo-browser-host");
        install(&browsers, &host).unwrap();
        assert_eq!(uninstall(&browsers).unwrap(), ["Present"]);
        assert!(!status(&browsers, &host)[0].connected);
        if let Some(key) = &browsers[0].key {
            assert_eq!(registry::get(key), None, "the registry key is gone too");
        }
        assert!(uninstall(&browsers).unwrap().is_empty());
    }

    #[test]
    fn firefox_is_told_the_add_on_id_and_nothing_else() {
        let (root, mut browsers) = two("firefox");
        browsers.truncate(1);
        browsers[0].kind = Kind::Firefox;
        let host = root.join("vardo-browser-host");
        install(&browsers, &host).unwrap();
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&browsers[0].manifest).unwrap()).unwrap();
        assert_eq!(written["allowed_extensions"], json!([GECKO_ID]));
        assert!(written.get("allowed_origins").is_none());
        assert!(status(&browsers, &host)[0].connected);
        uninstall(&browsers).unwrap();
    }

    #[test]
    fn no_browser_at_all_is_an_error() {
        let (root, mut browsers) = two("none");
        browsers.remove(0);
        assert!(install(&browsers, &root.join("host")).is_err());
    }
}
