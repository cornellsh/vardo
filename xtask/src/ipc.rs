//! The `ipc-names` gate: every command the interface invokes is registered in
//! the app, every registered command is invoked, and each call passes exactly
//! the arguments the Rust command takes (`inv:ipc-types-are-generated`). Types
//! are generated; names are not, so this closes the hand-written half.

use std::collections::{BTreeMap, BTreeSet};

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

/// Each `invoke` call: the command name, its first string literal, and the
/// keys of the argument object after it.
pub fn invoked(tauri_ts: &str) -> BTreeMap<String, BTreeSet<String>> {
    tauri_ts
        .match_indices("invoke")
        .filter(|(i, _)| matches!(tauri_ts.as_bytes().get(i + 6), Some(b'(' | b'<')))
        .filter_map(|(i, _)| {
            let rest = &tauri_ts[i..];
            let open = rest.find('"')? + 1;
            let close = open + rest[open..].find('"')?;
            let after = rest[close + 1..].trim_start();
            let keys = after
                .strip_prefix(',')
                .map(str::trim_start)
                .and_then(|a| a.strip_prefix('{'))
                .and_then(inside)
                .map(|body| {
                    split_top(body)
                        .into_iter()
                        .filter_map(|arg| arg.split(':').next())
                        .map(|key| key.trim().to_owned())
                        .filter(|key| !key.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            Some((rest[open..close].to_owned(), keys))
        })
        .collect()
}

/// Each `#[tauri::command]` in `rust`: its name and the camelCase keys Tauri
/// expects, leaving out the parameters Tauri injects itself.
pub fn parameters(rust: &str) -> BTreeMap<String, BTreeSet<String>> {
    rust.match_indices("#[tauri::command]")
        .filter_map(|(i, _)| {
            let rest = &rust[i..];
            let name_at = rest.find("fn ")? + 3;
            let open = name_at + rest[name_at..].find('(')?;
            let keys = split_top(inside(&rest[open + 1..])?)
                .into_iter()
                .filter_map(|param| param.split_once(':'))
                .filter(|(_, ty)| {
                    !["State<", "AppHandle", "Window"]
                        .iter()
                        .any(|injected| ty.contains(injected))
                })
                .map(|(name, _)| camel(name.trim()))
                .collect();
            Some((rest[name_at..open].trim().to_owned(), keys))
        })
        .collect()
}

/// Text up to the bracket that closes the one just before `s`.
fn inside(s: &str) -> Option<&str> {
    let mut depth = 0usize;
    for (i, b) in s.bytes().enumerate() {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth == 0 => return Some(&s[..i]),
            b')' | b']' | b'}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Splits on commas outside any brackets, generics included.
fn split_top(s: &str) -> Vec<&str> {
    let (mut depth, mut start, mut parts) = (0i32, 0, Vec::new());
    for (i, b) in s.bytes().enumerate() {
        match b {
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            b')' | b']' | b'}' | b'>' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

fn camel(snake: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in snake.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// One line per command whose call and definition disagree on arguments.
pub fn check_args(rust: &str, tauri_ts: &str) -> Vec<String> {
    let params = parameters(rust);
    invoked(tauri_ts)
        .into_iter()
        .filter_map(|(name, sent)| match params.get(&name) {
            None => Some(format!("{name}: no #[tauri::command] definition found")),
            Some(wanted) if sent != *wanted => {
                Some(format!("{name}: sends {sent:?}, Rust takes {wanted:?}"))
            }
            Some(_) => None,
        })
        .collect()
}

/// One line per mismatch; empty when both sides agree.
pub fn check(lib_rs: &str, tauri_ts: &str) -> Vec<String> {
    let reg = registered(lib_rs);
    let inv: BTreeSet<String> = invoked(tauri_ts).into_keys().collect();
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

    const RUST: &str = "#[tauri::command]\npub fn set_expiry(\n    state: tauri::State<'_, AppState>,\n    uuid: String,\n    at_unix: Option<i64>,\n) -> CmdResult<()> {}\n#[tauri::command]\npub fn drive_status(app: tauri::AppHandle) -> CmdResult<()> {}";

    #[test]
    fn matching_arguments_pass() {
        let ts = r#"invoke("set_expiry", { uuid, atUnix: at ?? null }); invoke("drive_status");"#;
        assert!(check_args(RUST, ts).is_empty());
    }

    #[test]
    fn a_renamed_parameter_fails() {
        let ts = r#"invoke("set_expiry", { uuid, at_unix }); invoke("drive_status", { app });"#;
        assert_eq!(
            check_args(RUST, ts),
            [
                r#"drive_status: sends {"app"}, Rust takes {}"#,
                r#"set_expiry: sends {"at_unix", "uuid"}, Rust takes {"atUnix", "uuid"}"#,
            ]
        );
    }
}
