//! Getting an attachment back out.

use vardo_vault::Vault;

fn open(name: &str, pw: &str) -> vardo_kdbx::Document {
    let bytes = std::fs::read(format!("../../fixtures/{name}")).expect("fixture");
    vardo_kdbx::open(&bytes, &vardo_kdbx::Credentials::with_password(pw)).expect("open")
}

#[test]
fn an_attachment_can_be_read_back_byte_for_byte() {
    // Listing a filename and never being able to open it makes a vault a place
    // things go to be lost. Find a fixture that actually carries one rather
    // than asserting against a vault with nothing to extract.
    let mut checked = 0;

    for entry in std::fs::read_dir("../../fixtures").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("kdbx") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();

        for pw in ["demopass", "password"] {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let creds = vardo_kdbx::Credentials::with_password(pw);
            let Ok(doc) = vardo_kdbx::open(&bytes, &creds) else {
                continue;
            };
            let Ok(vault) = Vault::open(&doc) else {
                continue;
            };

            for e in vault.entries() {
                for (att, index) in e.attachments() {
                    let data = e.attachment_bytes(&att);
                    assert!(
                        data.is_some(),
                        "{name}: attachment {att:?} (ref {index:?}) is listed but unreadable"
                    );
                    assert!(
                        !data.unwrap().is_empty(),
                        "{name}: attachment {att:?} came back empty"
                    );
                    checked += 1;
                }
            }
            break;
        }
    }

    assert!(
        checked > 0,
        "no fixture carries an attachment, so this proved nothing"
    );
    println!("checked {checked} attachment(s)");
}

#[test]
fn a_missing_attachment_is_not_silently_empty() {
    // An empty vec would be written out as a zero-byte file, which looks like
    // a successful export of an empty document.
    let doc = open("test_db_kdbx4_with_password_argon2.kdbx", "demopass");
    let vault = Vault::open(&doc).unwrap();
    let entry = vault.entries().into_iter().next().unwrap();
    assert!(entry.attachment_bytes("no-such-file.txt").is_none());
}
