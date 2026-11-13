//! Golden files for every message in the protocol.
//!
//! The extension is plain JavaScript and repeats the protocol by hand. These
//! files are its copy of the truth: `extension/src/native.test.js` reads each
//! one, so renaming a variant here fails the extension's tests.
//!
//! When this test fails because a message changed on purpose, rewrite the files
//! with `UPDATE_FIXTURES=1 cargo test -p vardo-browser --test protocol_fixtures`.

use std::{collections::BTreeMap, fs, path::PathBuf};
use vardo_browser::protocol::{
    CopyField, Envelope, ErrorCode, Login, MatchKind, Request, Response, SaveOutcome, State,
};

fn requests() -> Vec<Request> {
    vec![
        Request::Status,
        Request::Logins {
            url: "https://github.com/login".into(),
        },
        Request::Search {
            query: "git".into(),
        },
        Request::Fill {
            uuid: "4f0b9a51c3c94c8f9b0f2c2d7e1a6b3c".into(),
            url: "https://github.com/login".into(),
        },
        Request::Copy {
            uuid: "4f0b9a51c3c94c8f9b0f2c2d7e1a6b3c".into(),
            field: CopyField::Password,
        },
        Request::Show,
        Request::Launch,
        Request::Save {
            url: "https://example.com/signup".into(),
            username: "alex@example.com".into(),
            password: "correct-horse".into(),
            replace: false,
        },
        Request::Generate {
            url: "https://example.com/signup".into(),
            username: "alex@example.com".into(),
        },
    ]
}

fn responses() -> Vec<Response> {
    vec![
        Response::Status {
            state: State::Unlocked {
                vault: "Personal".into(),
            },
        },
        Response::Logins {
            logins: vec![Login {
                uuid: "4f0b9a51c3c94c8f9b0f2c2d7e1a6b3c".into(),
                title: "GitHub".into(),
                username: "octocat".into(),
                host: Some("github.com".into()),
                fit: Some(MatchKind::Exact),
                has_totp: true,
            }],
            truncated: false,
        },
        Response::Fill {
            username: "octocat".into(),
            password: "correct horse battery staple".into(),
            totp: Some("123456".into()),
        },
        Response::Copied {
            clear_after_seconds: 15,
        },
        Response::Shown,
        Response::Launched,
        Response::Saved {
            uuid: "4f9c2a7e5b3d4c1a9e8f7a6b5c4d3e2f".into(),
            outcome: SaveOutcome::Created,
        },
        Response::Generated {
            uuid: "4f9c2a7e5b3d4c1a9e8f7a6b5c4d3e2f".into(),
            password: "kD7#vQ2!pX9mW4zR8sT1".into(),
        },
        Response::error(ErrorCode::NotAMatch, "That login is not for this page"),
    ]
}

/// Fails to compile when a variant is added, so the lists above get a sample
/// for it. Values in nested enums are listed in full in `enums.json`.
fn name_request(r: &Request) -> &'static str {
    match r {
        Request::Status => "status",
        Request::Logins { .. } => "logins",
        Request::Search { .. } => "search",
        Request::Fill { .. } => "fill",
        Request::Copy { .. } => "copy",
        Request::Show => "show",
        Request::Launch => "launch",
        Request::Save { .. } => "save",
        Request::Generate { .. } => "generate",
    }
}

fn name_response(r: &Response) -> &'static str {
    match r {
        Response::Status { .. } => "status",
        Response::Logins { .. } => "logins",
        Response::Fill { .. } => "fill",
        Response::Copied { .. } => "copied",
        Response::Shown => "shown",
        Response::Launched => "launched",
        Response::Saved { .. } => "saved",
        Response::Generated { .. } => "generated",
        Response::Error { .. } => "error",
    }
}

fn enums() -> serde_json::Value {
    serde_json::json!({
        "copy-field": [CopyField::Username, CopyField::Password, CopyField::Totp],
        "error-code": [
            ErrorCode::NotRunning,
            ErrorCode::Locked,
            ErrorCode::NotAMatch,
            ErrorCode::NotFound,
            ErrorCode::Version,
            ErrorCode::BadRequest,
            ErrorCode::Internal,
        ],
        "match": [MatchKind::Exact, MatchKind::Subdomain],
        "save-outcome": [
            SaveOutcome::Created,
            SaveOutcome::Updated,
            SaveOutcome::Unchanged,
            SaveOutcome::Differs,
        ],
        "state": [State::Locked, State::Unlocked { vault: "Personal".into() }],
    })
}

fn pretty(value: &impl serde::Serialize) -> String {
    serde_json::to_string_pretty(value).expect("protocol types serialise") + "\n"
}

#[test]
fn the_extensions_protocol_files_are_current() {
    let mut expected = BTreeMap::new();
    for (i, body) in requests().into_iter().enumerate() {
        let name = format!("requests/{}.json", name_request(&body));
        let envelope = Envelope {
            v: vardo_browser::protocol::VERSION,
            id: i as u64 + 1,
            body,
        };
        expected.insert(name, pretty(&envelope));
    }
    for (i, body) in responses().into_iter().enumerate() {
        let name = format!("responses/{}.json", name_response(&body));
        let envelope = Envelope {
            v: vardo_browser::protocol::VERSION,
            id: i as u64 + 1,
            body,
        };
        expected.insert(name, pretty(&envelope));
    }
    expected.insert("enums.json".into(), pretty(&enums()));

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extension/protocol");
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        let _ = fs::remove_dir_all(&dir);
        for (name, text) in &expected {
            let path = dir.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }

    let mut actual = BTreeMap::new();
    for sub in ["", "requests", "responses"] {
        for entry in fs::read_dir(dir.join(sub)).into_iter().flatten().flatten() {
            if entry.path().is_file() {
                let name = if sub.is_empty() {
                    entry.file_name().to_string_lossy().into_owned()
                } else {
                    format!("{sub}/{}", entry.file_name().to_string_lossy())
                };
                actual.insert(name, fs::read_to_string(entry.path()).unwrap());
            }
        }
    }
    assert!(
        actual == expected,
        "extension/protocol/ is stale. If the protocol changed on purpose, run \
         UPDATE_FIXTURES=1 cargo test -p vardo-browser --test protocol_fixtures \
         and update extension/src/native.js to match."
    );
}

#[test]
fn every_sample_reads_back_as_itself() {
    for body in requests() {
        let text = serde_json::to_string(&body).unwrap();
        assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), body);
    }
    for body in responses() {
        let text = serde_json::to_string(&body).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&text).unwrap(), body);
    }
}
