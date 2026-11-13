//! The browser extension's requests, answered against a real vault.
//!
//! Each test is one `inv:` rule under "Browser" in `docs/INVARIANTS.md`.
//! The ones that matter most are the
//! refusals: a password for the wrong site, a deleted login, a locked vault.

use std::path::PathBuf;

use serde_json::{json, Value};
use vardo_core::browser::{handle, Effect};
use vardo_core::model::FieldEdit;
use vardo_core::source::{SourceContext, VaultSource};
use vardo_core::Session;

const PW: &str = "demopass";
// This one, because it has a recycle bin. The first version used a fixture
// without one, where deleting purges, and the precondition below caught that
// the deleted-login test would have passed without testing anything.
const FIXTURE: &str = "../../fixtures/test_db_kdbx4_with_password_deleted_entry.kdbx";
const SECRET: &str = "correct-horse-battery-staple";

struct Vault {
    session: Session,
    github: String,
    google: String,
    deleted: String,
    extra_url: String,
}

/// A vault with a few logins whose addresses exercise the rules.
fn vault(tag: &str) -> Vault {
    let dir = std::env::temp_dir().join(format!("vardo-browser-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path: PathBuf = dir.join("v.kdbx");
    std::fs::copy(FIXTURE, &path).unwrap();
    let mut session = Session::open(
        &VaultSource::file(&path),
        &SourceContext::local_only(&dir),
        PW,
        |_| {},
    )
    .expect("unlock");

    let root = session.groups().uuid.clone();
    let mut add = |fields: &[(&str, &str)]| {
        let uuid = session.create_entry(&root).unwrap();
        let edits: Vec<FieldEdit> = fields
            .iter()
            .map(|(k, v)| FieldEdit {
                key: (*k).into(),
                value: Some((*v).into()),
                protected: *k == "Password",
            })
            .collect();
        session.update_entry(&uuid, &edits).unwrap();
        uuid
    };

    let github = add(&[
        ("Title", "GitHub"),
        ("UserName", "alex"),
        ("Password", SECRET),
        ("URL", "https://github.com/login"),
        // RFC 6238's test secret, so the code is a real one.
        (
            "otp",
            "otpauth://totp/GitHub:alex?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=GitHub",
        ),
    ]);
    let google = add(&[
        ("Title", "Google"),
        ("UserName", "alex@gmail.com"),
        ("Password", "google-password"),
        ("URL", "google.com"),
    ]);
    let deleted = add(&[
        ("Title", "Old GitHub"),
        ("UserName", "alex-old"),
        ("Password", "leaked-password"),
        ("URL", "https://github.com"),
    ]);
    let extra_url = add(&[
        ("Title", "Work SSO"),
        ("UserName", "a.mercer"),
        ("Password", "sso-password"),
        ("URL", "https://sso.example.org"),
        ("KP2A_URL", "https://intranet.example.com"),
    ]);

    let outcome = session.delete_entry(&deleted).unwrap();
    assert_eq!(
        outcome, "recycled",
        "precondition: the fixture has a recycle bin, or the deleted-login tests prove nothing"
    );

    Vault {
        session,
        github,
        google,
        deleted,
        extra_url,
    }
}

fn ask(session: Option<&mut Session>, request: Value) -> (Value, Effect) {
    let mut message = json!({ "v": 1, "id": 42 });
    message
        .as_object_mut()
        .unwrap()
        .extend(request.as_object().unwrap().clone());
    let outcome = handle(&message.to_string(), session);
    let reply: Value = serde_json::from_str(&outcome.json()).unwrap();
    assert_eq!(reply["id"], 42, "every reply carries the request's id");
    (reply, outcome.effect)
}

fn titles(reply: &Value) -> Vec<String> {
    reply["logins"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["title"].as_str().unwrap().to_string())
        .collect()
}

// --- locked ---------------------------------------------------------------------

#[test]
fn a_locked_vault_says_so_and_gives_nothing_away() {
    let (status, _) = ask(None, json!({ "type": "status" }));
    assert_eq!(status["state"]["kind"], "locked");

    for request in [
        json!({ "type": "logins", "url": "https://github.com/" }),
        json!({ "type": "fill", "uuid": "anything", "url": "https://github.com/" }),
        json!({ "type": "search", "query": "git" }),
    ] {
        let (reply, _) = ask(None, request);
        assert_eq!(reply["code"], "locked");
    }
}

#[test]
fn show_works_while_locked_because_that_is_how_you_unlock() {
    let (reply, effect) = ask(None, json!({ "type": "show" }));
    assert_eq!(reply["type"], "shown");
    assert!(matches!(effect, Effect::Show));
}

// --- listing ---------------------------------------------------------------------

#[test]
fn the_logins_for_a_page_are_listed_without_their_passwords() {
    let mut v = vault("list");
    let (reply, _) = ask(
        Some(&mut v.session),
        json!({ "type": "logins", "url": "https://github.com/session" }),
    );
    assert_eq!(titles(&reply), ["GitHub"]);
    assert_eq!(reply["logins"][0]["match"], "exact");
    assert_eq!(reply["logins"][0]["username"], "alex");
    assert_eq!(reply["logins"][0]["has_totp"], true);
    assert!(
        !reply.to_string().contains(SECRET),
        "a listing must never carry a password"
    );
}

#[test]
fn a_deleted_login_is_never_offered() {
    // It is still in the file, in the recycle bin. Someone who deleted it
    // because it leaked must not see it filled into GitHub tomorrow.
    let mut v = vault("deleted");
    let (reply, _) = ask(
        Some(&mut v.session),
        json!({ "type": "logins", "url": "https://github.com/" }),
    );
    assert!(!titles(&reply).contains(&"Old GitHub".to_string()));

    let (search, _) = ask(
        Some(&mut v.session),
        json!({ "type": "search", "query": "github" }),
    );
    assert!(!titles(&search).contains(&"Old GitHub".to_string()));

    let (fill, _) = ask(
        Some(&mut v.session),
        json!({ "type": "fill", "uuid": v.deleted, "url": "https://github.com/" }),
    );
    assert_eq!(fill["code"], "not-found");
    assert!(!fill.to_string().contains("leaked-password"));
}

#[test]
fn a_subdomain_is_offered_but_marked() {
    let mut v = vault("subdomain");
    let (reply, _) = ask(
        Some(&mut v.session),
        json!({ "type": "logins", "url": "https://accounts.google.com/signin" }),
    );
    assert_eq!(titles(&reply), ["Google"]);
    assert_eq!(
        reply["logins"][0]["match"], "subdomain",
        "the extension's shortcut must be able to tell this apart from an exact match"
    );
}

#[test]
fn extra_addresses_from_other_keepass_apps_count() {
    let mut v = vault("extra");
    let (reply, _) = ask(
        Some(&mut v.session),
        json!({ "type": "logins", "url": "https://intranet.example.com/" }),
    );
    assert_eq!(titles(&reply), ["Work SSO"]);

    let (fill, _) = ask(
        Some(&mut v.session),
        json!({ "type": "fill", "uuid": v.extra_url, "url": "https://intranet.example.com/" }),
    );
    assert_eq!(fill["password"], "sso-password");
}

#[test]
fn a_page_with_no_logins_gets_an_empty_list_not_an_error() {
    let mut v = vault("none");
    let (reply, _) = ask(
        Some(&mut v.session),
        json!({ "type": "logins", "url": "https://nowhere.example/" }),
    );
    assert_eq!(reply["type"], "logins");
    assert!(titles(&reply).is_empty());
    assert_eq!(reply["truncated"], false);
}

// --- filling ---------------------------------------------------------------------

#[test]
fn filling_a_matching_login_returns_what_the_page_needs() {
    let mut v = vault("fill");
    let (reply, effect) = ask(
        Some(&mut v.session),
        json!({ "type": "fill", "uuid": v.github, "url": "https://github.com/login" }),
    );
    assert_eq!(reply["type"], "fill");
    assert_eq!(reply["username"], "alex");
    assert_eq!(reply["password"], SECRET);
    let code = reply["totp"]
        .as_str()
        .expect("the login has a two-factor code");
    assert_eq!(code.len(), 6);
    assert!(code.chars().all(|c| c.is_ascii_digit()));
    assert!(matches!(effect, Effect::None));
}

#[test]
fn a_login_is_never_filled_into_a_site_it_does_not_belong_to() {
    // The extension asking does not make it a match. This is the request a
    // compromised extension would send.
    let mut v = vault("wrongsite");
    for url in [
        "https://evil.example/",
        "https://github.com.evil.example/",
        "https://github.com@evil.example/",
        "http://github.com/login",
    ] {
        let (reply, _) = ask(
            Some(&mut v.session),
            json!({ "type": "fill", "uuid": v.github, "url": url }),
        );
        assert_eq!(reply["code"], "not-a-match", "{url}");
        assert!(!reply.to_string().contains(SECRET), "{url}");
    }
}

#[test]
fn a_chosen_subdomain_login_can_be_filled() {
    // Listed as a subdomain match, the user picked it in the popup.
    let mut v = vault("subfill");
    let (reply, _) = ask(
        Some(&mut v.session),
        json!({ "type": "fill", "uuid": v.google, "url": "https://accounts.google.com/" }),
    );
    assert_eq!(reply["password"], "google-password");
}

// --- copying ---------------------------------------------------------------------

#[test]
fn copying_hands_the_password_to_vardo_not_the_browser() {
    let mut v = vault("copy");
    let (reply, effect) = ask(
        Some(&mut v.session),
        json!({ "type": "copy", "uuid": v.github, "field": "password" }),
    );
    assert_eq!(reply["type"], "copied");
    assert_eq!(reply["clear_after_seconds"], 15);
    assert!(
        !reply.to_string().contains(SECRET),
        "the reply goes to the browser"
    );
    match effect {
        Effect::Copy(value) => assert_eq!(value.as_str(), SECRET),
        _ => panic!("copying must ask the app to copy"),
    }
}

// --- the protocol itself -------------------------------------------------------------

#[test]
fn a_different_version_is_told_so_rather_than_misunderstood() {
    let outcome = handle(r#"{"v":2,"id":5,"type":"status"}"#, None);
    let reply: Value = serde_json::from_str(&outcome.json()).unwrap();
    assert_eq!(reply["code"], "version");
    assert_eq!(reply["id"], 5);
}

#[test]
fn nonsense_is_an_error_with_the_id_echoed_when_there_is_one() {
    for (raw, id) in [
        ("not json", 0),
        (r#"{"v":1,"id":9,"type":"export-everything"}"#, 9),
        (r#"{"v":1,"id":3,"type":"fill"}"#, 3),
    ] {
        let reply: Value = serde_json::from_str(&handle(raw, None).json()).unwrap();
        assert_eq!(reply["code"], "bad-request", "{raw}");
        assert_eq!(reply["id"], id, "{raw}");
    }
}

#[test]
fn a_huge_vault_does_not_produce_a_reply_chrome_would_refuse() {
    let v = vault("many");
    let mut session = v.session;
    let root = session.groups().uuid.clone();
    for i in 0..120 {
        let uuid = session.create_entry(&root).unwrap();
        session
            .update_entry(
                &uuid,
                &[
                    FieldEdit {
                        key: "Title".into(),
                        value: Some(format!("Mirror {i}")),
                        protected: false,
                    },
                    FieldEdit {
                        key: "URL".into(),
                        value: Some("https://mirror.example/".into()),
                        protected: false,
                    },
                ],
            )
            .unwrap();
    }
    let (reply, _) = ask(
        Some(&mut session),
        json!({ "type": "logins", "url": "https://mirror.example/" }),
    );
    assert_eq!(titles(&reply).len(), vardo_browser::protocol::MAX_LOGINS);
    assert_eq!(
        reply["truncated"], true,
        "the popup must be able to say some were left out"
    );
}

// --- saving ----------------------------------------------------------------------

fn save(
    v: &mut Vault,
    url: &str,
    username: &str,
    password: &str,
    replace: bool,
) -> (Value, Effect) {
    ask(
        Some(&mut v.session),
        json!({ "type": "save", "url": url, "username": username, "password": password, "replace": replace }),
    )
}

#[test]
fn a_new_login_becomes_an_entry_for_its_site() {
    let mut v = vault("save-new");
    let (reply, effect) = save(
        &mut v,
        "https://news.example.com/signup?x=1",
        "alex",
        "fresh-pw",
        false,
    );
    assert_eq!(reply["type"], "saved");
    assert_eq!(reply["outcome"], "created");
    assert!(
        matches!(effect, Effect::Changed),
        "the app must refresh and save"
    );
    let uuid = reply["uuid"].as_str().unwrap();
    assert_eq!(
        *v.session.field_value(uuid, "Password").unwrap(),
        "fresh-pw"
    );
    assert_eq!(
        *v.session.field_value(uuid, "URL").unwrap(),
        "https://news.example.com"
    );
    assert!(v.session.is_dirty());

    let (logins, _) = ask(
        Some(&mut v.session),
        json!({ "type": "logins", "url": "https://news.example.com/login" }),
    );
    assert_eq!(titles(&logins), ["news.example.com"]);
}

#[test]
fn saving_the_login_already_saved_changes_nothing() {
    let mut v = vault("save-same");
    let (reply, effect) = save(&mut v, "https://github.com/session", "alex", SECRET, false);
    assert_eq!(reply["outcome"], "unchanged");
    assert_eq!(reply["uuid"], v.github);
    assert!(matches!(effect, Effect::None));
}

#[test]
fn a_different_saved_password_is_replaced_only_when_the_person_confirms() {
    let mut v = vault("save-replace");
    let (reply, effect) = save(
        &mut v,
        "https://github.com/session",
        "alex",
        "rotated",
        false,
    );
    assert_eq!(reply["outcome"], "differs");
    assert!(matches!(effect, Effect::None));
    assert_eq!(
        *v.session.field_value(&v.github, "Password").unwrap(),
        SECRET
    );

    let (reply, effect) = save(
        &mut v,
        "https://github.com/session",
        "alex",
        "rotated",
        true,
    );
    assert_eq!(reply["outcome"], "updated");
    assert_eq!(reply["uuid"], v.github);
    assert!(matches!(effect, Effect::Changed));
    assert_eq!(
        *v.session.field_value(&v.github, "Password").unwrap(),
        "rotated"
    );
    let versions = v.session.history(&v.github).unwrap().len();
    let kept = (0..versions).any(|i| {
        v.session
            .history_password(&v.github, i)
            .is_ok_and(|p| *p == SECRET)
    });
    assert!(kept, "the replaced password stays in the entry's history");
}

#[test]
fn a_login_saved_for_a_parent_site_is_never_overwritten() {
    // google.com covers accounts.google.com for filling, but the account there
    // may be another one. A new entry is safe; an overwrite is not.
    let mut v = vault("save-parent");
    let (reply, _) = save(
        &mut v,
        "https://accounts.google.com/",
        "alex@gmail.com",
        "other",
        true,
    );
    assert_eq!(reply["outcome"], "created");
    assert_ne!(reply["uuid"], v.google);
    assert_eq!(
        *v.session.field_value(&v.google, "Password").unwrap(),
        "google-password"
    );
}

#[test]
fn a_login_in_the_recycle_bin_is_not_updated() {
    let mut v = vault("save-deleted");
    let (reply, _) = save(&mut v, "https://github.com/", "alex-old", "new-one", true);
    assert_eq!(reply["outcome"], "created");
    assert_ne!(reply["uuid"], v.deleted);
}

#[test]
fn saving_is_refused_when_locked_empty_or_not_a_web_page() {
    let (reply, _) = ask(
        None,
        json!({ "type": "save", "url": "https://github.com", "username": "a", "password": "b" }),
    );
    assert_eq!(reply["code"], "locked");

    let mut v = vault("save-bad");
    let before = v.session.info().entry_count;
    for (url, password) in [
        ("https://github.com", ""),
        ("chrome://settings", "pw"),
        ("file:///x", "pw"),
    ] {
        let (reply, effect) = save(&mut v, url, "alex", password, true);
        assert_eq!(reply["code"], "bad-request", "{url} {password:?}");
        assert!(matches!(effect, Effect::None));
    }
    assert_eq!(v.session.info().entry_count, before, "nothing was added");
}

// --- generating ------------------------------------------------------------------

#[test]
fn a_generated_password_is_kept_before_it_is_handed_out() {
    let mut v = vault("generate-new");
    let (reply, effect) = ask(
        Some(&mut v.session),
        json!({ "type": "generate", "url": "https://shop.example.net/join", "username": "alex" }),
    );
    assert_eq!(reply["type"], "generated");
    let password = reply["password"].as_str().unwrap().to_string();
    assert_eq!(password.chars().count(), 20);
    assert!(
        matches!(effect, Effect::Changed),
        "the app must refresh and save"
    );
    let uuid = reply["uuid"].as_str().unwrap();
    assert_eq!(*v.session.field_value(uuid, "Password").unwrap(), password);
    assert_eq!(
        *v.session.field_value(uuid, "URL").unwrap(),
        "https://shop.example.net"
    );
}

#[test]
fn generating_for_a_saved_login_keeps_the_old_password_in_history() {
    let mut v = vault("generate-change");
    let (reply, _) = ask(
        Some(&mut v.session),
        json!({ "type": "generate", "url": "https://github.com/settings/password", "username": "alex" }),
    );
    assert_eq!(
        reply["uuid"], v.github,
        "a password change updates the existing login"
    );
    let fresh = reply["password"].as_str().unwrap();
    assert_eq!(
        *v.session.field_value(&v.github, "Password").unwrap(),
        fresh
    );
    let versions = v.session.history(&v.github).unwrap().len();
    let kept = (0..versions).any(|i| {
        v.session
            .history_password(&v.github, i)
            .is_ok_and(|p| *p == SECRET)
    });
    assert!(
        kept,
        "the old password survives in case the form is never sent"
    );
}

#[test]
fn generating_needs_an_unlocked_vault_and_a_web_page() {
    let (reply, _) = ask(
        None,
        json!({ "type": "generate", "url": "https://github.com", "username": "" }),
    );
    assert_eq!(reply["code"], "locked");
    let mut v = vault("generate-bad");
    let (reply, effect) = ask(
        Some(&mut v.session),
        json!({ "type": "generate", "url": "chrome://settings", "username": "" }),
    );
    assert_eq!(reply["code"], "bad-request");
    assert!(matches!(effect, Effect::None));
}
