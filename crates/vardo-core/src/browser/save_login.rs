//! Keeping a login the person typed into a page, or one Vardo made for it.
//!
//! Contract: runs only on a request the person started from the popup. A save
//! never replaces a different saved password unless the request says
//! `replace`, which the popup sends only after they confirm. A generated
//! password is kept before it is handed out, so it never exists only in a web
//! form. A replaced password stays in the entry's history
//! (`inv:edits-keep-history`). Nothing here writes to storage: the app saves,
//! as it does after any edit.

use vardo_browser::matching::{self, Match};
use vardo_browser::protocol::{ErrorCode, Response, SaveOutcome};
use zeroize::Zeroizing;

use super::{addresses, error, visible_entries};
use crate::model::{FieldEdit, GeneratorOptions};
use crate::session::entries::uuid_text;
use crate::Session;

/// Long enough that guessing is hopeless, short enough that sites accept it.
const GENERATED_LENGTH: usize = 20;
const GENERATED: GeneratorOptions = GeneratorOptions {
    lowercase: true,
    uppercase: true,
    digits: true,
    symbols: true,
    exclude_ambiguous: false,
};

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
    match keep(session, url, username, password, replace) {
        Ok((uuid, outcome)) => {
            let changed = matches!(outcome, SaveOutcome::Created | SaveOutcome::Updated);
            (Response::Saved { uuid, outcome }, changed)
        }
        Err(response) => (response, false),
    }
}

/// Makes a password for a sign-up or password-change form and keeps it,
/// replacing the one saved for exactly this site and username if there is
/// one. Its history keeps the old password, in case the form is never sent.
pub(super) fn generate_login(session: &mut Session, url: &str, username: &str) -> (Response, bool) {
    let password = match crate::generate::password(GENERATED_LENGTH, &GENERATED) {
        Ok(p) => Zeroizing::new(p),
        Err(e) => return (error(ErrorCode::Internal, &e), false),
    };
    match keep(session, url, username, &password, true) {
        Ok((uuid, _)) => (
            Response::Generated {
                uuid,
                password: password.to_string(),
            },
            true,
        ),
        Err(response) => (response, false),
    }
}

/// Creates the entry, or updates the one for exactly this site and username.
fn keep(
    session: &mut Session,
    url: &str,
    username: &str,
    password: &str,
    replace: bool,
) -> Result<(String, SaveOutcome), Response> {
    let Some((origin, host)) = matching::origin(url) else {
        return Err(error(
            ErrorCode::BadRequest,
            "Only logins for web pages can be saved",
        ));
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
    done.map_err(|e| error(ErrorCode::Internal, &e.to_string()))
}
