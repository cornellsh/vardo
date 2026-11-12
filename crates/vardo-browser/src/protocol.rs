//! What the extension may ask, and what it may be told.
//!
//! JSON, because both ends speak it natively and every message is a few hundred
//! bytes. Versioned from the first message, so an older extension talking to a
//! newer Vardo is told so rather than misunderstood.
//!
//! The shape is the security policy written down. Only [`Response::Fill`]
//! carries a password, and only after Vardo has confirmed the login matches the
//! page it is being filled into. Listing and searching return titles and
//! usernames. Copying is done by Vardo itself, so the password never reaches
//! the browser at all.

use serde::{Deserialize, Serialize};

use crate::matching::Match;

/// Bumped when a change would make an older extension misbehave. Additions that
/// an older extension can safely ignore do not bump it.
pub const VERSION: u32 = 1;

/// The most logins one reply will list. Native messaging caps a reply to the
/// browser at 1 MB, and a search that matches half of a 20,000 entry vault
/// would otherwise be refused by Chrome with no explanation at all.
pub const MAX_LOGINS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub v: u32,
    /// Echoed back, so the extension can match a reply to its request.
    pub id: u64,
    #[serde(flatten)]
    pub body: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Request {
    /// Is Vardo unlocked, and which vault is open?
    Status,
    /// Logins that match this page. `url` is the tab's address as the browser
    /// reports it.
    Logins { url: String },
    /// Logins whose title, username or address contains `query`, for when the
    /// address saved in the vault does not match the page.
    Search { query: String },
    /// The username, password and current two-factor code for one login,
    /// to be typed into the page at `url`.
    Fill { uuid: String, url: String },
    /// Copy a field to the clipboard. Vardo does it and clears it afterwards.
    Copy { uuid: String, field: CopyField },
    /// Bring Vardo's window forward, usually so the user can unlock it.
    Show,
    /// Start Vardo. Answered by the native host when Vardo is not running, and
    /// never forwarded to Vardo itself.
    Launch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CopyField {
    Username,
    Password,
    Totp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Response {
    Status {
        state: State,
    },
    Logins {
        logins: Vec<Login>,
        /// True when there were more than [`MAX_LOGINS`] and the rest were
        /// left out, so the popup can say so.
        truncated: bool,
    },
    Fill {
        username: String,
        password: String,
        /// The current two-factor code, when the login has one.
        totp: Option<String>,
    },
    Copied {
        clear_after_seconds: u32,
    },
    Shown,
    Launched,
    Error {
        code: ErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum State {
    /// Vardo is running, and no vault is unlocked.
    ///
    /// One state rather than "locked" and "nothing open", because locking drops
    /// the vault entirely and Vardo cannot honestly tell the two apart. Either
    /// way the answer for the person is the same: unlock Vardo.
    Locked,
    Unlocked {
        vault: String,
    },
}

/// A login as the popup may see it: enough to choose one, and no secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Login {
    pub uuid: String,
    pub title: String,
    pub username: String,
    /// The host the login is saved for, to show beside the title.
    pub host: Option<String>,
    /// How well it fits the current page. `None` in search results that do not
    /// match the page.
    #[serde(rename = "match", skip_serializing_if = "Option::is_none", default)]
    pub fit: Option<MatchKind>,
    pub has_totp: bool,
}

/// [`Match`] as it appears on the wire. A separate type so the protocol can be
/// deserialised by the host without depending on matching rules changing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MatchKind {
    Exact,
    Subdomain,
}

impl From<Match> for MatchKind {
    fn from(m: Match) -> Self {
        match m {
            Match::Exact => MatchKind::Exact,
            Match::Subdomain => MatchKind::Subdomain,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    /// Vardo is not running. Produced by the host, which cannot reach it.
    NotRunning,
    Locked,
    /// The login does not belong to this page, so Vardo refused to fill it.
    NotAMatch,
    NotFound,
    /// The extension is newer or older than this Vardo understands.
    Version,
    /// The request could not be understood.
    BadRequest,
    Internal,
}

impl Response {
    pub fn error(code: ErrorCode, message: impl Into<String>) -> Self {
        Response::Error {
            code,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_reads_the_way_the_extension_writes_it() {
        let raw = r#"{"v":1,"id":7,"type":"fill","uuid":"abc","url":"https://github.com/"}"#;
        let parsed: Envelope<Request> = serde_json::from_str(raw).unwrap();
        assert_eq!(
            parsed,
            Envelope {
                v: 1,
                id: 7,
                body: Request::Fill {
                    uuid: "abc".into(),
                    url: "https://github.com/".into()
                }
            }
        );
    }

    #[test]
    fn a_reply_has_the_shape_the_extension_reads() {
        let reply = Envelope {
            v: VERSION,
            id: 7,
            body: Response::Status {
                state: State::Unlocked {
                    vault: "personal".into(),
                },
            },
        };
        assert_eq!(
            serde_json::to_string(&reply).unwrap(),
            r#"{"v":1,"id":7,"type":"status","state":{"kind":"unlocked","vault":"personal"}}"#
        );
    }

    #[test]
    fn a_login_listing_has_nowhere_to_put_a_password() {
        // The type is the guarantee: if someone adds a secret field to `Login`,
        // this is the test that has to be edited to allow it.
        let login = Login {
            uuid: "u".into(),
            title: "GitHub".into(),
            username: "alex".into(),
            host: Some("github.com".into()),
            fit: Some(MatchKind::Exact),
            has_totp: true,
        };
        let json = serde_json::to_value(&login).unwrap();
        let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["has_totp", "host", "match", "title", "username", "uuid"]
        );
    }

    #[test]
    fn an_unknown_request_is_an_error_not_a_crash() {
        let raw = r#"{"v":1,"id":1,"type":"export-everything"}"#;
        assert!(serde_json::from_str::<Envelope<Request>>(raw).is_err());
    }
}
