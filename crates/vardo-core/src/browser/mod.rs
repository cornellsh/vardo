//! Answers the browser extension.
//!
//! Contract: [`handle`] decides every reply to the extension. URL matching
//! happens here (`inv:match-in-rust`), a fill re-checks the match
//! (`inv:fill-rechecks-match`), a locked vault reveals nothing, and a secret
//! leaves only to fill a matching page
//! (`inv:browser-gets-secret-only-to-fill`). Replies stay under the native
//! messaging limit (`inv:reply-fits-native-messaging`).
//!
//! Everything the extension can ask is decided here, where it can be tested
//! without a window or a browser. The desktop app only moves bytes between
//! this and a socket, and performs the two things that need the toolkit:
//! copying to the clipboard and showing the window. See `dec:own-extension`.
//!
//! Two rules matter more than the rest:
//!
//! - **A password leaves only to be filled into a page it belongs to.** Fill
//!   checks the match itself, rather than trusting that the extension asked
//!   for a login it was shown for this page.
//! - **Deleted logins are gone.** Entries in the recycle bin are invisible to
//!   the browser, even though they are still in the file. Someone who deletes a
//!   login because it leaked must not find it filled into a page the next day.

mod save_login;
#[cfg(unix)]
pub mod server;

use serde::Serialize;
use vardo_browser::matching::{self, Match};
use vardo_browser::protocol::{
    CopyField, Envelope, ErrorCode, Login, MatchKind, Request, Response, State, MAX_LOGINS, VERSION,
};
use vardo_vault::{Entry, Group, Vault};
use zeroize::Zeroizing;

use crate::favicon::host_of;
use crate::session::entries::uuid_text;
use crate::Session;

/// What the app must do besides replying.
pub enum Effect {
    None,
    /// Put this on the clipboard with the usual automatic clearing. The reply
    /// already says it was copied; if copying fails, send an error instead.
    Copy(Zeroizing<String>),
    /// Bring the window forward.
    Show,
    /// The vault changed. The app refreshes what it shows and saves.
    Changed,
}

pub struct Outcome {
    pub reply: Envelope<Response>,
    pub effect: Effect,
}

impl Outcome {
    pub fn json(&self) -> String {
        serde_json::to_string(&self.reply).unwrap_or_else(|_| {
            // Every field is a plain string, number or enum, so this cannot
            // fail; the fallback exists so a socket never goes silent.
            format!(
                r#"{{"v":{VERSION},"id":{},"type":"error","code":"internal","message":"Could not encode the reply"}}"#,
                self.reply.id
            )
        })
    }
}

/// Answers one line from the socket. `session` is `None` when Vardo is locked.
pub fn handle(raw: &str, session: Option<&mut Session>) -> Outcome {
    let request: Envelope<Request> = match serde_json::from_str(raw) {
        Ok(r) => r,
        Err(_) => {
            // Echo the id if the line is JSON at all, so the extension can tie
            // the error to what it asked.
            let id = serde_json::from_str::<serde_json::Value>(raw)
                .ok()
                .and_then(|v| v.get("id").and_then(serde_json::Value::as_u64))
                .unwrap_or(0);
            return reply(
                id,
                error(
                    ErrorCode::BadRequest,
                    "Vardo did not understand that request",
                ),
            );
        }
    };
    let id = request.id;
    if request.v != VERSION {
        return reply(
            id,
            error(
                ErrorCode::Version,
                "The extension and Vardo are different versions. Update both.",
            ),
        );
    }
    answer(request.body, session, id)
}

fn answer(request: Request, session: Option<&mut Session>, id: u64) -> Outcome {
    // The two requests that make sense while locked.
    match &request {
        Request::Show => {
            return Outcome {
                reply: envelope(id, Response::Shown),
                effect: Effect::Show,
            }
        }
        Request::Status => {
            let state = match session {
                Some(s) => State::Unlocked {
                    vault: s.info().name.clone(),
                },
                None => State::Locked,
            };
            return reply(id, Response::Status { state });
        }
        _ => {}
    }

    let Some(session) = session else {
        return reply(id, error(ErrorCode::Locked, "Vardo is locked"));
    };

    match request {
        Request::Logins { url } => reply(id, logins_for(session.vault(), &url)),
        Request::Search { query } => reply(id, search(session.vault(), &query)),
        Request::Fill { uuid, url } => reply(id, fill(session, &uuid, &url)),
        Request::Copy { uuid, field } => copy(session, &uuid, field, id),
        Request::Launch => reply(id, error(ErrorCode::BadRequest, "Vardo is already running")),
        Request::Save {
            url,
            username,
            password,
            replace,
        } => {
            let password = Zeroizing::new(password);
            let (response, changed) =
                save_login::save_login(session, &url, &username, &password, replace);
            Outcome {
                reply: envelope(id, response),
                effect: if changed {
                    Effect::Changed
                } else {
                    Effect::None
                },
            }
        }
        Request::Show | Request::Status => unreachable!("answered above"),
    }
}

// --- the requests -------------------------------------------------------------

fn logins_for(vault: &Vault, page: &str) -> Response {
    let mut found: Vec<(Match, bool, Login)> = Vec::new();
    for entry in visible_entries(vault) {
        let mut best: Option<(Match, bool)> = None;
        for saved in addresses(&entry) {
            if let Some(m) = matching::classify(&saved, page) {
                let candidate = (m, matching::path_fits(&saved, page));
                // Exact beats subdomain; among equals, a fitting path wins.
                if best.is_none_or(|b| (candidate.0, !candidate.1) < (b.0, !b.1)) {
                    best = Some(candidate);
                }
            }
        }
        if let Some((m, path)) = best {
            found.push((m, path, login_of(&entry, Some(m.into()))));
        }
    }

    found.sort_by(|a, b| {
        (a.0, !a.1, a.2.title.to_lowercase()).cmp(&(b.0, !b.1, b.2.title.to_lowercase()))
    });
    listing(found.into_iter().map(|(_, _, login)| login).collect())
}

fn search(vault: &Vault, query: &str) -> Response {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return listing(Vec::new());
    }
    let mut found: Vec<Login> = visible_entries(vault)
        .into_iter()
        .map(|e| login_of(&e, None))
        .filter(|l| {
            l.title.to_lowercase().contains(&needle)
                || l.username.to_lowercase().contains(&needle)
                || l.host.as_deref().is_some_and(|h| h.contains(&needle))
        })
        .collect();
    found.sort_by_key(|l| l.title.to_lowercase());
    listing(found)
}

fn fill(session: &Session, uuid: &str, page: &str) -> Response {
    let Some(entry) = find_visible(session.vault(), uuid) else {
        return error(ErrorCode::NotFound, "That login is no longer in the vault");
    };
    // Checked again here, whatever the extension believes. A compromised or
    // buggy extension asking for the bank's login on another site gets nothing.
    let belongs = addresses(&entry)
        .iter()
        .any(|saved| matching::classify(saved, page).is_some());
    if !belongs {
        return error(
            ErrorCode::NotAMatch,
            "This login is not saved for this site",
        );
    }

    let password = match session.field_value(uuid, "Password") {
        Ok(p) => p,
        Err(_) => Zeroizing::new(String::new()),
    };
    let totp = session.totp(uuid).ok().flatten().map(|t| t.code);
    Response::Fill {
        username: entry.username(),
        password: password.to_string(),
        totp,
    }
}

fn copy(session: &Session, uuid: &str, field: CopyField, id: u64) -> Outcome {
    let Some(entry) = find_visible(session.vault(), uuid) else {
        return reply(
            id,
            error(ErrorCode::NotFound, "That login is no longer in the vault"),
        );
    };
    let value = match field {
        CopyField::Username => Some(Zeroizing::new(entry.username())),
        CopyField::Password => session.field_value(uuid, "Password").ok(),
        CopyField::Totp => session
            .totp(uuid)
            .ok()
            .flatten()
            .map(|t| Zeroizing::new(t.code)),
    };
    match value.filter(|v| !v.is_empty()) {
        Some(value) => Outcome {
            reply: envelope(
                id,
                Response::Copied {
                    clear_after_seconds: crate::CLIPBOARD_CLEAR_SECONDS,
                },
            ),
            effect: Effect::Copy(value),
        },
        None => reply(
            id,
            error(ErrorCode::NotFound, "That login has nothing to copy there"),
        ),
    }
}

// --- the vault, as the browser sees it ----------------------------------------

/// Every entry outside the recycle bin.
fn visible_entries(vault: &Vault) -> Vec<Entry<'_>> {
    let bin = vault.recycle_bin_uuid();
    let mut out = Vec::new();
    if let Some(root) = vault.root() {
        collect(root, bin, &mut out);
    }
    out
}

fn collect<'a>(group: Group<'a>, bin: Option<vardo_vault::Uuid>, out: &mut Vec<Entry<'a>>) {
    if bin.is_some() && group.uuid() == bin {
        return;
    }
    out.extend(group.entries());
    for child in group.groups() {
        collect(child, bin, out);
    }
}

fn find_visible<'a>(vault: &'a Vault, uuid: &str) -> Option<Entry<'a>> {
    visible_entries(vault)
        .into_iter()
        .find(|e| e.uuid().map(uuid_text).as_deref() == Some(uuid))
}

/// The login's address, plus any extra ones.
///
/// KeePassXC, KeePassDX and Keepass2Android keep additional addresses in fields
/// named `KP2A_URL`, `KP2A_URL_1` and so on. Honouring them means a vault set
/// up in those apps fills here without being edited.
fn addresses(entry: &Entry<'_>) -> Vec<String> {
    let mut out = vec![entry.url()];
    for (key, value) in entry.fields() {
        if key == "KP2A_URL" || key.starts_with("KP2A_URL_") {
            out.push(value.expose().to_string());
        }
    }
    out.retain(|u| !u.trim().is_empty());
    out
}

fn login_of(entry: &Entry<'_>, fit: Option<MatchKind>) -> Login {
    Login {
        uuid: entry.uuid().map(uuid_text).unwrap_or_default(),
        title: entry.title(),
        username: entry.username(),
        host: host_of(&entry.url()),
        fit,
        has_totp: entry.field("otp").is_some() || entry.field("TOTP Seed").is_some(),
    }
}

// --- replies -------------------------------------------------------------------

fn listing(mut logins: Vec<Login>) -> Response {
    let truncated = logins.len() > MAX_LOGINS;
    logins.truncate(MAX_LOGINS);
    Response::Logins { logins, truncated }
}

fn error(code: ErrorCode, message: &str) -> Response {
    Response::error(code, message)
}

fn envelope<T: Serialize>(id: u64, body: T) -> Envelope<T> {
    Envelope {
        v: VERSION,
        id,
        body,
    }
}

fn reply(id: u64, body: Response) -> Outcome {
    Outcome {
        reply: envelope(id, body),
        effect: Effect::None,
    }
}
