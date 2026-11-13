//! Connecting the browser extension from the app, without a terminal.
//!
//! Contract: registers the `vardo-browser-host` that sits beside the running
//! app, and only that one, through the same code as `vardo-browser-host
//! --install` (`vardo_browser::registration`). Nothing here talks to a
//! browser; Chrome reads the files the next time the extension asks.

use std::path::{Path, PathBuf};

use vardo_browser::registration;

use crate::model::{BrowserHost, BrowserHosts};
use crate::CoreError;

const HELPER: &str = if cfg!(windows) {
    "vardo-browser-host.exe"
} else {
    "vardo-browser-host"
};

/// The host binary shipped beside this app, if it is there.
pub fn helper() -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?.parent()?.join(HELPER);
    path.is_file().then_some(path)
}

/// Where each supported browser stands.
pub fn status() -> Result<BrowserHosts, CoreError> {
    let (home, helper) = (home()?, helper());
    Ok(report(&registration::browsers(&home), helper.as_deref()))
}

/// Registers the helper with every browser that has run.
pub fn connect() -> Result<BrowserHosts, CoreError> {
    let home = home()?;
    let helper = helper().ok_or_else(|| CoreError::Unsupported {
        detail: format!("{HELPER} is not beside Vardo, so there is nothing to connect"),
    })?;
    let browsers = registration::browsers(&home);
    registration::install(&browsers, &helper).map_err(|detail| CoreError::Io { detail })?;
    Ok(report(&browsers, Some(&helper)))
}

/// Removes the registration from every browser.
pub fn disconnect() -> Result<BrowserHosts, CoreError> {
    let browsers = registration::browsers(&home()?);
    registration::uninstall(&browsers).map_err(|detail| CoreError::Io { detail })?;
    Ok(report(&browsers, helper().as_deref()))
}

fn report(browsers: &[registration::Browser], helper: Option<&Path>) -> BrowserHosts {
    // Without a helper nothing can be connected to this copy of Vardo.
    let probe = helper.unwrap_or(Path::new(""));
    BrowserHosts {
        helper_found: helper.is_some(),
        browsers: registration::status(browsers, probe)
            .into_iter()
            .map(|s| BrowserHost {
                name: s.name.to_string(),
                installed: s.installed,
                connected: s.connected && helper.is_some(),
            })
            .collect(),
    }
}

fn home() -> Result<PathBuf, CoreError> {
    // Windows registers hosts in the registry instead (`plan:windows-pipe`).
    if cfg!(windows) {
        return Err(CoreError::Unsupported {
            detail: "Connecting the browser extension is not available on Windows yet".into(),
        });
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| CoreError::Io {
            detail: "HOME is not set".into(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_helper_nothing_reads_as_connected() {
        let root = std::env::temp_dir().join("vardo-core-host-report");
        let _ = std::fs::remove_dir_all(&root);
        let chrome = root.join("chrome");
        std::fs::create_dir_all(&chrome).unwrap();
        let browsers = vec![registration::Browser {
            name: "Chrome",
            dir: chrome,
        }];
        let helper = root.join(HELPER);
        registration::install(&browsers, &helper).unwrap();

        let with = report(&browsers, Some(&helper));
        assert!(with.helper_found && with.browsers[0].connected);
        let without = report(&browsers, None);
        assert!(!without.helper_found && !without.browsers[0].connected);
        assert!(without.browsers[0].installed);
    }
}
