//! Password health, and what it must never say.

use vardo_core::health::{self, Issue};
use vardo_vault::Vault;

const PW: &str = "demopass";

fn vault_from(name: &str) -> (vardo_kdbx::Document, vardo_kdbx::Credentials) {
    let bytes = std::fs::read(format!("../../fixtures/{name}")).expect("fixture");
    let creds = vardo_kdbx::Credentials::with_password(PW);
    let doc = vardo_kdbx::open(&bytes, &creds).expect("open");
    (doc, creds)
}

#[test]
fn the_report_never_contains_a_password() {
    // The property that matters. A health screen that leaked the passwords it
    // was reporting on, or their lengths, or anything reversible, would be a
    // remarkable way to help someone with their security.
    let (doc, _) = vault_from("test_db_kdbx41_with_password_aes.kdbx");
    let vault = Vault::open(&doc).expect("vault");

    // Collect the real passwords so the assertion is against actual secrets
    // rather than a guess at what they might be.
    let secrets: Vec<String> = vault
        .entries()
        .iter()
        .filter_map(|e| e.password())
        .map(|f| f.expose().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    assert!(!secrets.is_empty(), "fixture must have passwords to leak");

    let report = health::review(&vault, 1_760_000_000);
    let json = serde_json::to_string(&report).expect("serialise");

    for secret in &secrets {
        assert!(
            !json.contains(secret.as_str()),
            "the report leaked a password"
        );
    }
}

#[test]
fn reuse_reports_a_count_and_not_the_other_entries() {
    // Naming the entries that share a password turns one finding into a map of
    // which accounts fall together, which is the thing an attacker who gets a
    // glance at the screen most wants.
    let (doc, _) = vault_from("test_db_kdbx4_with_password_argon2.kdbx");
    let vault = Vault::open(&doc).expect("vault");
    let report = health::review(&vault, 1_760_000_000);

    for finding in &report.findings {
        if finding.issue == Issue::Reused {
            assert!(finding.shared_with.unwrap_or(0) >= 2);
        } else {
            assert!(
                finding.shared_with.is_none(),
                "only reuse carries a count: {finding:?}"
            );
        }
    }
}

#[test]
fn an_entry_with_no_password_is_not_examined() {
    // Otherwise every note and every bookmark in a vault gets reported as
    // having a weak password, which is both wrong and the fastest way to make
    // the whole feature ignored.
    let (doc, _) = vault_from("test_db_kdbx4_with_password_argon2.kdbx");
    let vault = Vault::open(&doc).expect("vault");
    let report = health::review(&vault, 1_760_000_000);

    let with_passwords = vault
        .entries()
        .iter()
        .filter(|e| {
            e.password()
                .map(|p| !p.expose().is_empty())
                .unwrap_or(false)
        })
        .count();
    assert_eq!(report.checked, with_passwords);
}

#[test]
fn findings_lead_with_the_worst_thing() {
    // A list sorted by category name puts "expired" above "reused", which
    // buries the finding that actually opens several accounts at once.
    let (doc, _) = vault_from("test_db_kdbx41_with_password_aes.kdbx");
    let vault = Vault::open(&doc).expect("vault");
    let report = health::review(&vault, 1_760_000_000);

    let rank = |i: Issue| match i {
        Issue::Reused => 0,
        Issue::Weak => 1,
        Issue::Expired => 2,
        Issue::Stale => 3,
    };
    let ranks: Vec<i32> = report.findings.iter().map(|f| rank(f.issue)).collect();
    assert!(
        ranks.windows(2).all(|w| w[0] <= w[1]),
        "findings must be ordered worst first: {ranks:?}"
    );
}
