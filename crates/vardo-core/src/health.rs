//! What is wrong with the passwords in a vault.
//!
//! The one thing a password manager can do that a text file cannot: look at the
//! whole set at once and tell you which of them are going to hurt. Storing
//! secrets is the easy half.
//!
//! # What leaves this module
//!
//! Entry UUIDs and a category. Never a password, never a length, never a hash,
//! never a comparison anyone could run backwards. Reuse is found by hashing in
//! memory and reporting only *that* two entries match - which entries, not what
//! they share. A "password health" screen that leaked the lengths of everyone's
//! passwords would be a strange way to help.
//!
//! # On strength
//!
//! Deliberately crude, and labelled as such. Real strength estimation means
//! zxcvbn: a dictionary, an l33t-speak table, keyboard adjacency graphs, and
//! about a megabyte of data. That is a reasonable dependency for a product that
//! is mostly a strength meter, and a poor one here.
//!
//! What this catches instead is the class of password nobody defends: short
//! ones, single-character-class ones, and the handful that appear in every
//! breach corpus ever published. It will not tell you `Tr0ub4dor&3` is weaker
//! than it looks. It will tell you `qwerty123` is in your vault, which is the
//! finding that actually matters.

use std::collections::HashMap;

use serde::Serialize;
use vardo_vault::Vault;

use crate::entries::uuid_text;

/// Roughly two years. Long enough not to nag about a password from last
/// spring, short enough to catch one that predates a breach you have forgotten
/// about.
const STALE_SECONDS: i64 = 63_072_000;

/// Below this, length alone decides it. Twelve is where a random-ish password
/// stops being brute-forceable by anyone who wants it.
const SHORT: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Issue {
    /// The same password is on more than one entry.
    ///
    /// The most useful finding here by a distance: one breach then opens
    /// several accounts, and nothing about either account tells you so.
    Reused,
    /// Short, or one character class, or a well-known password.
    Weak,
    /// Not changed in a long time.
    Stale,
    /// The entry's own expiry date has passed.
    Expired,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub uuid: String,
    pub title: String,
    pub issue: Issue,
    /// How many entries share this password. Only set for [`Issue::Reused`],
    /// and it is a count rather than a list so one finding cannot be used to
    /// enumerate the others.
    pub shared_with: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub findings: Vec<Finding>,
    /// Entries that were examined, so the UI can say "3 of 412" rather than a
    /// bare count that could mean anything.
    pub checked: usize,
}

pub fn review(vault: &Vault, now_unix: i64) -> Report {
    let entries = vault.entries();
    let mut findings = Vec::new();
    let mut checked = 0usize;

    // Grouped by password so reuse is one pass rather than a comparison of
    // every entry against every other, which on a large vault is the
    // difference between instant and noticeable.
    let mut by_password: HashMap<String, Vec<usize>> = HashMap::new();

    for (index, entry) in entries.iter().enumerate() {
        let Some(field) = entry.password() else {
            continue;
        };
        // `expose` rather than a plain getter, so reading a secret is visible
        // at the call site. It stays inside this function: nothing derived from
        // it leaves the module except a category.
        let password = field.expose();
        if password.is_empty() {
            continue;
        }
        checked += 1;
        by_password
            .entry(password.to_string())
            .or_default()
            .push(index);
    }

    for (password, indices) in &by_password {
        let shared = indices.len();

        for &index in indices {
            let Some(entry) = entries.get(index) else {
                continue;
            };
            let uuid = entry.uuid().map(uuid_text).unwrap_or_default();
            let title = entry.title();

            if shared > 1 {
                findings.push(Finding {
                    uuid: uuid.clone(),
                    title: title.clone(),
                    issue: Issue::Reused,
                    shared_with: Some(shared),
                });
            }

            if is_weak(password) {
                findings.push(Finding {
                    uuid: uuid.clone(),
                    title: title.clone(),
                    issue: Issue::Weak,
                    shared_with: None,
                });
            }

            let times = entry.times();

            // Both halves matter. KDBX stores an expiry date and a flag for
            // whether it counts, and a vault written by another client can
            // easily carry a stale date with the flag off - honouring the date
            // alone would invent expiries the user never set.
            if times.expires {
                if let Some(at) = times.expires_at {
                    if at.unix_seconds() < now_unix {
                        findings.push(Finding {
                            uuid: uuid.clone(),
                            title: title.clone(),
                            issue: Issue::Expired,
                            shared_with: None,
                        });
                    }
                }
            }

            if let Some(modified) = times.last_modified {
                if now_unix - modified.unix_seconds() > STALE_SECONDS {
                    findings.push(Finding {
                        uuid,
                        title,
                        issue: Issue::Stale,
                        shared_with: None,
                    });
                }
            }
        }
    }

    // Reused first, then weak, then expired, then stale: worst consequence
    // first rather than alphabetically, because a list ordered by category name
    // buries the finding that matters under the one that does not.
    findings.sort_by_key(|f| {
        (
            match f.issue {
                Issue::Reused => 0,
                Issue::Weak => 1,
                Issue::Expired => 2,
                Issue::Stale => 3,
            },
            f.title.to_lowercase(),
        )
    });

    Report { findings, checked }
}

/// Well-known passwords, lowercased.
///
/// A short list on purpose. The full breach corpus is hundreds of megabytes and
/// the top few dozen cover the overwhelming majority of real occurrences; past
/// that the returns fall off a cliff and the file becomes the largest thing we
/// ship.
const COMMON: &[&str] = &[
    "password",
    "123456",
    "12345678",
    "123456789",
    "qwerty",
    "qwerty123",
    "abc123",
    "letmein",
    "monkey",
    "dragon",
    "111111",
    "iloveyou",
    "admin",
    "welcome",
    "login",
    "passw0rd",
    "master",
    "hello",
    "freedom",
    "whatever",
    "trustno1",
    "sunshine",
    "princess",
    "football",
    "baseball",
    "shadow",
    "michael",
    "superman",
    "batman",
    "starwars",
    "changeme",
    "secret",
    "test",
    "guest",
];

fn is_weak(password: &str) -> bool {
    let lower = password.to_lowercase();
    if COMMON.contains(&lower.as_str()) {
        return true;
    }

    // Counted in characters, not bytes. A twelve-character passphrase in
    // Japanese is not a four-character password, and `len()` says it is.
    let length = password.chars().count();
    if length < 8 {
        return true;
    }

    let classes = [
        password.chars().any(|c| c.is_ascii_lowercase()),
        password.chars().any(|c| c.is_ascii_uppercase()),
        password.chars().any(|c| c.is_ascii_digit()),
        password.chars().any(|c| !c.is_alphanumeric()),
    ]
    .iter()
    .filter(|present| **present)
    .count();

    // Long enough stops being weak whatever it is made of: a passphrase is all
    // lowercase letters and spaces, and calling it weak would be wrong in the
    // most annoying possible way.
    if length >= 20 {
        return false;
    }

    length < SHORT && classes < 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_passwords_are_weak_at_any_length() {
        for p in ["password", "qwerty123", "PASSWORD", "Letmein"] {
            assert!(is_weak(p), "{p:?} should be weak");
        }
    }

    #[test]
    fn a_long_passphrase_is_not_weak_despite_one_character_class() {
        // The failure that would make this feature hated: telling someone their
        // carefully chosen passphrase is weak because it has no digits.
        assert!(!is_weak("correct horse battery staple"));
        assert!(!is_weak("the quiet wagon rolls onward"));
    }

    #[test]
    fn short_is_weak_however_varied() {
        assert!(is_weak("aB3$"), "four characters is weak whatever is in it");
        assert!(is_weak("aB3$xY"), "six characters likewise");
    }

    #[test]
    fn length_counts_characters_not_bytes() {
        // Nine characters, twenty-seven bytes. Measuring bytes would call this
        // long and measuring bytes wrongly is how non-Latin passwords get
        // silently mis-scored.
        let japanese = "パスワード見本語";
        assert!(japanese.len() > 20, "precondition: many bytes");
        assert!(
            japanese.chars().count() < 12,
            "precondition: few characters"
        );
        assert!(is_weak(japanese));
    }

    #[test]
    fn a_strong_password_is_not_flagged() {
        assert!(!is_weak("Tr0ub4dor&3xK9"));
        assert!(!is_weak("8Fq!zR2v#Lm4"));
    }
}
