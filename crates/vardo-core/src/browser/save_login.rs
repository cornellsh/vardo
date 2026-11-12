//! Keeping a login the person typed into a page.
//!
//! Contract: runs only on a [`Request::Save`](vardo_browser::protocol::Request)
//! the person started from the popup. It never replaces a different saved
//! password unless the request says `replace`, which the popup sends only after
//! they confirm. A replaced password stays in the entry's history
//! (`inv:edits-keep-history`). Nothing here writes to storage: the app saves,
//! as it does after any edit.

use vardo_browser::matching::{self, Match};
use vardo_browser::protocol::{ErrorCode, Response, SaveOutcome};

use super::{addresses, error, visible_entries};
use crate::model::FieldEdit;
use crate::session::entries::uuid_text;
use crate::Session;

/// Saves the login, and says whether the vault changed.
pub(super) fn save_login(
    session: &mut Session,
    url: &str,
    username: &str,
    password: &str,
    replace: bool,
) -> (Response, bool) {
    if password.is_empty() {
        return (
            error(ErrorCode::BadRequest, "There is no password to save"),
            false,
        );
    }
    let Some((origin, host)) = matching::origin(url) else {
        return (
            error(
                ErrorCode::BadRequest,
                "Only logins for web pages can be saved",
            ),
            false,
        );
    };

    // The same site exactly, and the same username. A login saved for a
    // parent domain is a different account as often as not.
    let existing = visible_entries(session.vault())
        .into_iter()
        .find(|e| {
            e.username() == username
                && addresses(e)
                    .iter()
                    .any(|a| matching::classify(a, url) == Some(Match::Exact))
        })
        .and_then(|e| e.uuid().map(uuid_text));

    let field = |key: &str, value: &str, protected: bool| FieldEdit {
        key: key.into(),
        value: Some(value.into()),
        protected,
    };
    let done = match existing {
        Some(uuid) => {
            let same = session
                .field_value(&uuid, "Password")
                .is_ok_and(|saved| saved.as_str() == password);
            if same {
                Ok((uuid, SaveOutcome::Unchanged))
            } else if !replace {
                Ok((uuid, SaveOutcome::Differs))
            } else {
                session
                    .update_entry(&uuid, &[field("Password", password, true)])
                    .map(|_| (uuid, SaveOutcome::Updated))
            }
        }
        None => {
            let root = session.groups().uuid;
            session.create_entry(&root).and_then(|uuid| {
                let fields = [
                    field("Title", &host, false),
                    field("UserName", username, false),
                    field("Password", password, true),
                    field("URL", &origin, false),
                ];
                session
                    .update_entry(&uuid, &fields)
                    .map(|_| (uuid, SaveOutcome::Created))
            })
        }
    };
    match done {
        Ok((uuid, outcome)) => {
            let changed = matches!(outcome, SaveOutcome::Created | SaveOutcome::Updated);
            (Response::Saved { uuid, outcome }, changed)
        }
        Err(e) => (error(ErrorCode::Internal, &e.to_string()), false),
    }
}
