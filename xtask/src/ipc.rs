//! The `ipc-names` gate: every command the interface invokes is registered in
//! the app, and every registered command is invoked
//! (`inv:ipc-types-are-generated`). Types are generated; names are not, so
//! this closes the remaining hand-written half.

use std::collections::BTreeSet;

/// Names inside `generate_handler![...]`, without their module path.
pub fn registered(lib_rs: &str) -> BTreeSet<String> {
    let Some(start) = lib_rs.find("generate_handler![") else {
        return BTreeSet::new();
    };
    let body = &lib_rs[start..];
    let body = &body[..body.find(']').unwrap_or(body.len())];
    body.split(',')
        .filter_map(|item| item.trim().rsplit("::").next())
        .filter(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
        .map(str::to_owned)
        .collect()
}

/// The first string literal after each `invoke` call.
pub fn invoked(tauri_ts: &str) -> BTreeSet<String> {
    tauri_ts
        .match_indices("invoke")
        .filter(|(i, _)| matches!(tauri_ts.as_bytes().get(i + 6), Some(b'(' | b'<')))
        .filter_map(|(i, _)| {
            let rest = &tauri_ts[i..];
            let open = rest.find('"')? + 1;
            let close = rest[open..].find('"')?;
            Some(rest[open..open + close].to_owned())
        })
        .collect()
}

/// One line per mismatch; empty when both sides agree.
pub fn check(lib_rs: &str, tauri_ts: &str) -> Vec<String> {
    let reg = registered(lib_rs);
    let inv = invoked(tauri_ts);
    let mut broken: Vec<String> = inv
        .difference(&reg)
        .map(|n| format!("invoked but not registered: {n}"))
        .collect();
    broken.extend(
        reg.difference(&inv)
            .map(|n| format!("registered but never invoked: {n}")),
    );
    if reg.is_empty() {
        broken.push("found no generate_handler! list".into());
    }
    broken
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIB: &str =
        "tauri::generate_handler![\n commands::open_vault,\n commands::lock_vault,\n]";

    #[test]
    fn matching_names_pass() {
        let ts = r#"invoke<VaultInfo>("open_vault", { source }); invoke("lock_vault");"#;
        assert!(check(LIB, ts).is_empty());
    }

    #[test]
    fn a_renamed_command_fails_on_both_sides() {
        let ts = r#"invoke("open_vault"); invoke("lock")"#;
        assert_eq!(
            check(LIB, ts),
            [
                "invoked but not registered: lock",
                "registered but never invoked: lock_vault"
            ]
        );
    }

    #[test]
    fn the_import_line_is_not_a_call() {
        let ts = r#"import { invoke } from "@tauri-apps/api/core"; invoke("open_vault"); invoke("lock_vault");"#;
        assert!(check(LIB, ts).is_empty());
    }
}
