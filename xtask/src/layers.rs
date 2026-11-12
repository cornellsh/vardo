//! The layer gate: the tower in `docs/ARCHITECTURE.md`, enforced.
//!
//! Each workspace crate may depend only on the crates listed for it here. A
//! new crate fails until it is placed in the tower. `vardo-core` must also
//! reach no UI toolkit through any normal dependency (`inv:core-has-no-toolkit`).

use serde_json::Value;
use std::collections::{HashMap, HashSet};

const STORAGE: &[&str] = &["vardo-kdbx", "vardo-vault", "vardo-merge", "vardo-sync"];

/// Workspace crate, then the workspace crates it may depend on.
const TOWER: &[(&str, &[&str])] = &[
    ("vardo-kdbx", &[]),
    ("vardo-vault", &["vardo-kdbx"]),
    ("vardo-merge", &["vardo-kdbx", "vardo-vault"]),
    ("vardo-sync", &["vardo-kdbx", "vardo-vault", "vardo-merge"]),
    ("vardo-drive", STORAGE),
    // The contract between core and the native host depends on nothing.
    ("vardo-browser", &[]),
    (
        "vardo-core",
        &[
            "vardo-kdbx",
            "vardo-vault",
            "vardo-merge",
            "vardo-sync",
            "vardo-drive",
            "vardo-browser",
        ],
    ),
    // A shell talks to core only, plus the shared browser contract.
    ("vardo-app", &["vardo-core", "vardo-browser"]),
    ("vardo-conformance", &["vardo-kdbx"]),
    ("xtask", &[]),
];

const TOOLKITS: &[&str] = &["tauri", "tao", "wry", "gtk", "webkit2gtk", "objc2-app-kit"];

/// Reads `cargo metadata` output. Returns one line per broken rule.
pub fn check(metadata: &Value) -> Vec<String> {
    let mut broken = Vec::new();
    let packages = metadata["packages"].as_array().cloned().unwrap_or_default();
    let members: HashSet<&str> = metadata["workspace_members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let name_of: HashMap<&str, &str> = packages
        .iter()
        .filter_map(|p| Some((p["id"].as_str()?, p["name"].as_str()?)))
        .collect();
    let workspace: HashSet<&str> = members
        .iter()
        .filter_map(|id| name_of.get(id).copied())
        .collect();

    for package in packages
        .iter()
        .filter(|p| p["id"].as_str().is_some_and(|id| members.contains(id)))
    {
        let name = package["name"].as_str().unwrap_or_default();
        let Some((_, allowed)) = TOWER.iter().find(|(n, _)| *n == name) else {
            broken.push(format!("{name} is not in the tower: add it to xtask/src/layers.rs and docs/ARCHITECTURE.md"));
            continue;
        };
        for dep in package["dependencies"].as_array().into_iter().flatten() {
            let dep_name = dep["name"].as_str().unwrap_or_default();
            if workspace.contains(dep_name) && !allowed.contains(&dep_name) {
                broken.push(format!("forbidden edge: {name} -> {dep_name}"));
            }
        }
    }

    // Toolkits are external crates, so this walks the resolved graph.
    let nodes: HashMap<&str, &Value> = metadata["resolve"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| Some((n["id"].as_str()?, n)))
        .collect();
    let core = name_of
        .iter()
        .find(|(id, n)| **n == "vardo-core" && members.contains(**id));
    if let Some((core_id, _)) = core {
        let mut seen = HashSet::new();
        let mut stack = vec![*core_id];
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let name = name_of.get(id).copied().unwrap_or_default();
            if TOOLKITS.contains(&name) {
                broken.push(format!("vardo-core reaches the UI toolkit crate {name}"));
            }
            let deps = nodes
                .get(id)
                .and_then(|n| n["deps"].as_array())
                .into_iter()
                .flatten();
            for dep in deps {
                let normal = dep["dep_kinds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|k| k["kind"].is_null());
                if let (true, Some(pkg)) = (normal, dep["pkg"].as_str()) {
                    stack.push(pkg);
                }
            }
        }
    }
    broken
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn metadata(edges: &[(&str, &[&str])], resolve: Value) -> Value {
        let packages: Vec<Value> = edges
            .iter()
            .map(|(name, deps)| {
                let deps: Vec<Value> = deps.iter().map(|d| json!({ "name": d })).collect();
                json!({ "id": format!("{name}-id"), "name": name, "dependencies": deps })
            })
            .chain([json!({ "id": "tauri-id", "name": "tauri", "dependencies": [] })])
            .collect();
        let members: Vec<String> = edges.iter().map(|(n, _)| format!("{n}-id")).collect();
        json!({ "packages": packages, "workspace_members": members, "resolve": resolve })
    }

    #[test]
    fn the_current_tower_passes() {
        let edges = [
            ("vardo-kdbx", &[][..]),
            ("vardo-vault", &["vardo-kdbx"]),
            ("vardo-app", &["vardo-core", "vardo-browser", "tauri"]),
            ("vardo-core", &["vardo-vault"]),
            ("vardo-browser", &[]),
        ];
        assert_eq!(
            check(&metadata(&edges, json!({ "nodes": [] }))),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_upward_or_skipping_edge_fails() {
        let edges = [
            ("vardo-kdbx", &["vardo-vault"][..]),
            ("vardo-vault", &[]),
            ("vardo-app", &["vardo-kdbx"]),
        ];
        assert_eq!(
            check(&metadata(&edges, json!({ "nodes": [] }))),
            [
                "forbidden edge: vardo-kdbx -> vardo-vault",
                "forbidden edge: vardo-app -> vardo-kdbx"
            ]
        );
    }

    #[test]
    fn an_unplaced_crate_fails() {
        let broken = check(&metadata(&[("vardo-new", &[])], json!({ "nodes": [] })));
        assert!(broken[0].starts_with("vardo-new is not in the tower"));
    }

    #[test]
    fn core_reaching_a_toolkit_fails_but_a_dev_dependency_does_not() {
        let normal = json!([{ "kind": null }]);
        let dev = json!([{ "kind": "dev" }]);
        let resolve = |kinds: &Value| {
            json!({ "nodes": [
                { "id": "vardo-core-id", "deps": [{ "pkg": "vardo-vault-id", "dep_kinds": normal }] },
                { "id": "vardo-vault-id", "deps": [{ "pkg": "tauri-id", "dep_kinds": kinds }] },
            ]})
        };
        let edges = [("vardo-core", &["vardo-vault"][..]), ("vardo-vault", &[])];
        assert_eq!(
            check(&metadata(&edges, resolve(&normal))),
            ["vardo-core reaches the UI toolkit crate tauri"]
        );
        assert!(check(&metadata(&edges, resolve(&dev))).is_empty());
    }
}
