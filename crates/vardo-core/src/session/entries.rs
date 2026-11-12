//! Turning a vault into the view models the UI renders.
//!
//! Contract: pure functions over a borrowed vault. Only [`history_password`]
//! returns a secret, and its caller wraps it in `Zeroizing`
//! (`inv:secrets-stay-in-rust`).
//!
//! Pure functions over a borrowed [`Vault`], so they are testable without a
//! window, a session, or a Tauri handle.

use std::collections::HashMap;

use vardo_vault::{Entry, Group, Timestamp, Uuid, Vault};

use crate::model::{
    Attachment, EntryDetail, EntrySummary, Field, GroupNode, HistoryVersion, Times,
};

/// The single string form of a UUID on the wire.
///
/// Base64, deliberately: it is the form KDBX itself stores and the only one
/// [`Uuid::parse`] accepts. `Display` renders hyphenated hex, which has no
/// parser at all — emitting that meant every read handed the UI an identifier
/// no write could resolve, and every edit failed with "no such entry" while the
/// entry was plainly on screen.
pub fn uuid_text(uuid: Uuid) -> String {
    uuid.to_base64()
}

pub fn parse_uuid(text: &str) -> Option<Uuid> {
    Uuid::parse(text)
}

pub fn millis(t: Option<Timestamp>) -> i64 {
    t.map(|t| t.unix_seconds() * 1000).unwrap_or(0)
}

/// Host only, not the full URL: enough to disambiguate two entries with the
/// same title, less to leak into a screenshot.
pub fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    host.split_once('@')
        .map(|(_, h)| h)
        .unwrap_or(host)
        .to_string()
}

pub fn group_tree(vault: &Vault) -> GroupNode {
    vault.root().map(node_of).unwrap_or_else(|| GroupNode {
        uuid: String::new(),
        name: String::new(),
        entry_count: 0,
        children: Vec::new(),
    })
}

fn node_of(g: Group<'_>) -> GroupNode {
    GroupNode {
        uuid: g.uuid().map(uuid_text).unwrap_or_default(),
        name: g.name(),
        // Direct children only; the rail shows per-group counts, not rollups.
        entry_count: g.entries().count(),
        children: g.groups().map(node_of).collect(),
    }
}

/// How to order a listing.
///
/// Two orders, not five. Alphabetical is how you find something you already
/// know the name of; recent is how you find what you were just doing. Every
/// other axis anyone proposes turns out to be one of those two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Name,
    Recent,
}

impl Sort {
    pub fn parse(text: &str) -> Self {
        match text {
            "recent" => Self::Recent,
            _ => Self::Name,
        }
    }
}

pub fn list(vault: &Vault, group: Option<&str>, query: Option<&str>) -> Vec<EntrySummary> {
    list_sorted(vault, group, query, Sort::Name)
}

pub fn list_sorted(
    vault: &Vault,
    group: Option<&str>,
    query: Option<&str>,
    sort: Sort,
) -> Vec<EntrySummary> {
    let needle = query
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(str::to_lowercase);

    let icons = icon_index(vault);
    let mut rows: Vec<EntrySummary> = collect(vault, group)
        .into_iter()
        .map(|e| summary_of(e, &icons))
        .filter(|s| match &needle {
            None => true,
            Some(q) => {
                s.title.to_lowercase().contains(q)
                    || s.username.to_lowercase().contains(q)
                    || s.url_host.to_lowercase().contains(q)
            }
        })
        .collect();

    match sort {
        // Newest within a name, so repeated entries for one site read as a
        // history rather than an arbitrary shuffle.
        Sort::Name => rows.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then(b.modified.cmp(&a.modified))
        }),
        // Name breaks the tie, because a vault imported in one go gives every
        // entry the same timestamp and an unstable order would then reshuffle
        // the list on every keystroke.
        Sort::Recent => rows.sort_by(|a, b| {
            b.modified
                .cmp(&a.modified)
                .then(a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        }),
    }
    rows
}

fn collect<'a>(vault: &'a Vault, group: Option<&str>) -> Vec<Entry<'a>> {
    match group {
        None => vault.entries(),
        Some(want) => {
            let mut out = Vec::new();
            if let Some(root) = vault.root() {
                walk(root, &mut |g| {
                    if g.uuid().map(uuid_text).as_deref() == Some(want) {
                        out.extend(g.entries());
                    }
                });
            }
            out
        }
    }
}

fn walk<'a>(g: Group<'a>, f: &mut impl FnMut(&Group<'a>)) {
    f(&g);
    for child in g.groups() {
        walk(child, f);
    }
}

/// Resolves each entry's custom icon to a data URL.
///
/// Built once per listing rather than per row: `<CustomIcons>` is a flat list,
/// so a naive lookup would be quadratic, and a vault with a few hundred entries
/// and a few hundred icons is exactly the case where that is felt.
fn icon_index(vault: &Vault) -> HashMap<String, String> {
    vault
        .custom_icons()
        .into_iter()
        .map(|icon| {
            (
                uuid_text(icon.uuid),
                format!("data:image/png;base64,{}", icon.png_base64),
            )
        })
        .collect()
}

fn summary_of(e: Entry<'_>, icons: &HashMap<String, String>) -> EntrySummary {
    let url = e.url();
    EntrySummary {
        // A site icon the user already has beats anything we could draw, and
        // beats anything we could fetch. This is where KeePassXC's downloaded
        // favicons live, so a vault that has been through it is already done.
        icon_png: e
            .custom_icon_uuid()
            .map(uuid_text)
            .and_then(|u| icons.get(&u).cloned()),
        uuid: e.uuid().map(uuid_text).unwrap_or_default(),
        title: e.title(),
        username: e.username(),
        url_host: host_of(&url),
        modified: millis(e.times().last_modified),
        icon_id: e
            .element()
            .child("IconID")
            .and_then(|c| c.text().parse::<u32>().ok())
            .unwrap_or(0),
        has_totp: e.field("otp").is_some() || e.field("TOTP Seed").is_some(),
        has_attachments: !e.attachments().is_empty(),
    }
}

pub fn find<'a>(vault: &'a Vault, uuid: &str) -> Option<Entry<'a>> {
    vault
        .entries()
        .into_iter()
        .find(|e| e.uuid().map(uuid_text).as_deref() == Some(uuid))
}

pub fn detail(vault: &Vault, uuid: &str) -> Option<EntryDetail> {
    let icons = icon_index(vault);
    let entry = find(vault, uuid)?;
    let times = entry.times();

    Some(EntryDetail {
        uuid: uuid.to_string(),
        icon_png: entry
            .custom_icon_uuid()
            .map(uuid_text)
            .and_then(|u| icons.get(&u).cloned()),
        title: entry.title(),
        fields: ordered_fields(&entry),
        tags: entry.tags(),
        times: Times {
            created: millis(times.created),
            modified: millis(times.last_modified),
            accessed: millis(times.last_accessed),
            // Only meaningful when the entry actually expires; KDBX stores a
            // date regardless and it is garbage when `expires` is false.
            expires: if times.expires {
                times.expires_at.map(|t| t.unix_seconds() * 1000)
            } else {
                None
            },
        },
        attachments: entry
            .attachments()
            .into_iter()
            .map(|(name, size)| Attachment { name, size })
            .collect(),
        history_count: entry.history().len(),
        group_path: path_to(vault, uuid),
    })
}

/// KDBX stores `String` elements in whatever order the writing client used, so
/// reading them verbatim produces orders like Password, Title, UserName. Sort
/// into the order every KeePass client displays, with custom fields after the
/// standard ones in their original relative order.
///
/// `Title` is dropped: it is already the pane heading, and repeating it as a
/// field is noise.
fn ordered_fields(entry: &Entry<'_>) -> Vec<Field> {
    const STANDARD: [&str; 4] = ["UserName", "Password", "URL", "Notes"];
    let rank = |key: &str| {
        STANDARD
            .iter()
            .position(|s| *s == key)
            .unwrap_or(STANDARD.len())
    };

    let mut fields: Vec<(usize, Field)> = entry
        .fields()
        .into_iter()
        .filter(|(key, _)| key != "Title")
        .map(|(key, v)| {
            let r = rank(&key);
            (
                r,
                Field {
                    key,
                    // A protected field carries NO value. This is the whole
                    // contract: there is no shape of this struct that can
                    // leak one.
                    value: if v.is_protected() {
                        None
                    } else {
                        Some(v.expose().to_string())
                    },
                    empty: v.expose().is_empty(),
                    protected: v.is_protected(),
                },
            )
        })
        .collect();

    // Stable, so custom fields keep the document's own ordering among
    // themselves.
    fields.sort_by_key(|(r, _)| *r);
    fields.into_iter().map(|(_, f)| f).collect()
}

fn path_to(vault: &Vault, uuid: &str) -> Vec<String> {
    fn descend(g: Group<'_>, uuid: &str, trail: &mut Vec<String>) -> bool {
        trail.push(g.name());
        if g.entries()
            .any(|e| e.uuid().map(uuid_text).as_deref() == Some(uuid))
        {
            return true;
        }
        for child in g.groups() {
            if descend(child, uuid, trail) {
                return true;
            }
        }
        trail.pop();
        false
    }
    let mut trail = Vec::new();
    if let Some(root) = vault.root() {
        descend(root, uuid, &mut trail);
    }
    trail
}

/// Earlier versions of an entry, newest first.
///
/// KDBX keeps a full copy of the entry each time it changes, and Vardo has
/// been writing them faithfully and showing a count. A count you cannot open
/// is a tease: the reason anyone wants this is that they changed a password,
/// the site did not accept it, and the old one is now the only way back in.
///
/// Newest first because that is the one people want. Reverse chronological is
/// also what "2 earlier versions" implies you will get.
pub fn history(vault: &Vault, uuid: &str) -> Option<Vec<HistoryVersion>> {
    let entry = find(vault, uuid)?;
    let icons = icon_index(vault);

    let mut versions: Vec<HistoryVersion> = entry
        .history()
        .into_iter()
        .enumerate()
        .map(|(index, old)| {
            let summary = summary_of(old, &icons);
            HistoryVersion {
                index,
                title: summary.title,
                username: summary.username,
                url: old.url(),
                modified: summary.modified,
                // Whether there is anything under the dots. An empty password
                // in an old version is normal - a note that later gained one -
                // and offering to reveal nothing is worse than saying so.
                has_password: old
                    .password()
                    .map(|p| !p.expose().is_empty())
                    .unwrap_or(false),
            }
        })
        .collect();

    versions.reverse();
    Some(versions)
}

/// One earlier version's password, in the clear.
///
/// Separate from [`history`] for the same reason the current password is:
/// listing an entry must never carry its secret, or every list becomes a leak
/// waiting for a log line.
pub fn history_password(vault: &Vault, uuid: &str, index: usize) -> Option<String> {
    let entry = find(vault, uuid)?;
    let old = entry.history().into_iter().nth(index)?;
    Some(old.password()?.expose().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_text_round_trips_through_parse() {
        // Regression guard for the two-representations bug: whatever a read
        // emits, a write must be able to parse back.
        let uuid = Uuid::from_bytes([
            0x17, 0x81, 0x93, 0x0d, 0x6f, 0xf7, 0xf9, 0x4b, 0xbe, 0xb1, 0x1c, 0x23, 0x5f, 0xaa,
            0x1d, 0xf0,
        ]);
        assert_eq!(parse_uuid(&uuid_text(uuid)), Some(uuid));
    }

    #[test]
    fn the_display_form_is_not_what_we_send() {
        let uuid = Uuid::from_bytes([1; 16]);
        assert!(Uuid::parse(&uuid.to_string()).is_none());
        assert_ne!(uuid_text(uuid), uuid.to_string());
    }

    #[test]
    fn host_extraction_drops_scheme_path_and_credentials() {
        assert_eq!(host_of("https://github.com/foo/bar"), "github.com");
        assert_eq!(host_of("http://user:pw@example.org/x?y#z"), "example.org");
        assert_eq!(host_of("example.net"), "example.net");
        assert_eq!(host_of(""), "");
    }
}
