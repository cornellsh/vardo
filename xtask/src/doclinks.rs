//! The doc-link gate.
//!
//! Docs, code and tests link through tags (`inv:`, `dec:`, `fact:`, `plan:`)
//! and through repo paths written in backticks in `AGENTS.md` and `docs/`. A
//! link to nothing fails here, so the map cannot drift from the code it
//! describes.

use std::{fs, io, path::Path};

const KINDS: [(&str, &str); 4] = [
    ("inv", "docs/INVARIANTS.md"),
    ("dec", "docs/DECISIONS.md"),
    ("fact", "docs/FACTS.md"),
    ("plan", "docs/PLAN.md"),
];
const REF_ROOTS: [&str; 7] = [
    "crates",
    "app",
    "extension",
    "fixtures",
    "fuzz",
    "docs",
    "xtask",
];

/// Checks every tracked or untracked-but-not-ignored file. Returns one line
/// per broken link; empty means the gate passes.
pub fn check(root: &Path, files: &[String]) -> io::Result<Vec<String>> {
    let mut tags = Vec::new();
    let mut refs = Vec::new();
    for file in files {
        // Binary files and files deleted from the work tree have nothing to say.
        let Ok(text) = fs::read_to_string(root.join(file)) else {
            continue;
        };
        tags.extend(tags_in(&text));
        if file == "AGENTS.md" || file == "README.md" || file.starts_with("docs/") {
            refs.extend(refs_in(&text));
        }
    }
    tags.sort();
    tags.dedup();
    refs.sort();
    refs.dedup();

    let mut broken = Vec::new();
    for tag in tags {
        let kind = &tag[..tag.find(':').unwrap_or(0)];
        let doc = KINDS.iter().find(|(k, _)| *k == kind).map(|(_, d)| *d);
        let defined = doc
            .and_then(|d| fs::read_to_string(root.join(d)).ok())
            .is_some_and(|text| text.lines().any(|l| l == format!("### {tag}")));
        if !defined {
            broken.push(format!("undefined tag: {tag}"));
        }
    }
    for r in refs {
        let (path, func) = match r.split_once("::") {
            Some((p, f)) => (p, Some(f)),
            None => (r.as_str(), None),
        };
        let full = root.join(path);
        if !full.exists() {
            broken.push(format!("missing path: {path}"));
        } else if let Some(func) = func {
            let text = fs::read_to_string(&full).unwrap_or_default();
            if !text.contains(&format!("fn {func}(")) && !text.contains(&format!("fn {func}<")) {
                broken.push(format!("missing test: {r}"));
            }
        }
    }
    Ok(broken)
}

/// Every `kind:name` tag, where name is lowercase words joined by hyphens.
fn tags_in(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    for (kind, _) in KINDS {
        let needle = format!("{kind}:");
        let mut from = 0;
        while let Some(at) = text[from..].find(&needle) {
            let start = from + at;
            from = start + needle.len();
            // `codec:x` is not a tag.
            if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
                continue;
            }
            let name = tag_name(&text[from..]);
            if !name.is_empty() {
                found.push(format!("{kind}:{name}"));
            }
        }
    }
    found
}

fn tag_name(rest: &str) -> &str {
    let word = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let bytes = rest.as_bytes();
    let mut end = 0;
    loop {
        let len = bytes[end..].iter().take_while(|b| word(b)).count();
        if len == 0 {
            break;
        }
        end += len;
        if bytes.get(end) == Some(&b'-') && bytes.get(end + 1).is_some_and(word) {
            end += 1;
        } else {
            break;
        }
    }
    &rest[..end]
}

/// Every backticked span that names a repo path, optionally `path::function`.
fn refs_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        let ticks: Vec<usize> = line.match_indices('`').map(|(i, _)| i).collect();
        for pair in ticks.windows(2) {
            let span = &line[pair[0] + 1..pair[1]];
            if is_ref(span) {
                found.push(span.to_string());
            }
        }
    }
    found
}

fn is_ref(span: &str) -> bool {
    let (path, func) = match span.split_once("::") {
        Some((p, f)) => (p, Some(f)),
        None => (span, None),
    };
    let rooted = REF_ROOTS.iter().any(|r| {
        path.strip_prefix(r)
            .is_some_and(|rest| rest.starts_with('/'))
    });
    let path_ok = path
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_./-".contains(&b));
    let func_ok = func
        .is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
    rooted && path_ok && func_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    // Built at run time so this file does not cite tags that do not exist.
    fn tag(kind: &str, name: &str) -> String {
        format!("{kind}:{name}")
    }

    #[test]
    fn finds_tags_and_trims_trailing_hyphens() {
        let text = format!(
            "see {} and ({}). codec:{} {}-* and `{}`",
            tag("inv", "a-b"),
            tag("dec", "c2"),
            "nope",
            tag("fact", "drive"),
            "inv:"
        );
        assert_eq!(
            tags_in(&text),
            [tag("inv", "a-b"), tag("dec", "c2"), tag("fact", "drive")]
        );
    }

    #[test]
    fn finds_path_refs_only_under_known_roots() {
        let text = "`crates/x/src/a.rs::some_test` `cargo test` `crates` `app/src/b.ts`\n\
                    `docs/a b.md` `crates/x.rs::` `elsewhere/c.rs`";
        assert_eq!(
            refs_in(text),
            ["crates/x/src/a.rs::some_test", "app/src/b.ts"]
        );
    }

    #[test]
    fn reports_each_kind_of_broken_link() {
        let root = std::env::temp_dir().join(format!("xtask-doclinks-{}", std::process::id()));
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(
            root.join("docs/INVARIANTS.md"),
            format!("### {}\n", tag("inv", "known")),
        )
        .unwrap();
        fs::write(root.join("docs/x.rs"), "fn real_test() {}\n").unwrap();
        fs::write(
            root.join("docs/note.md"),
            format!(
                "{} {} `docs/x.rs::real_test` `docs/x.rs::gone` `docs/none.md`\n",
                tag("inv", "known"),
                tag("inv", "unknown")
            ),
        )
        .unwrap();
        let files = ["docs/INVARIANTS.md", "docs/x.rs", "docs/note.md"].map(String::from);
        let broken = check(&root, &files).unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            broken,
            [
                format!("undefined tag: {}", tag("inv", "unknown")),
                "missing path: docs/none.md".to_string(),
                "missing test: docs/x.rs::gone".to_string(),
            ]
        );
    }
}
