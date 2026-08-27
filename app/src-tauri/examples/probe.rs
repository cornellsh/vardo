//! Smoke-check fixtures through the real open path, without a GUI.
//!
//! `cargo run -p vardo-app --example probe -- fixtures/*.kdbx`
//!
//! Useful when the UI misbehaves and the question is whether the core or the
//! shell is at fault.

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: probe <file.kdbx>...");
        std::process::exit(2);
    }

    for path in paths {
        let Ok(bytes) = std::fs::read(&path) else {
            println!("{path} | UNREADABLE");
            continue;
        };
        // Fixture passwords only. Never put a real password here.
        let mut opened = false;
        for pw in ["demopass", "password"] {
            let creds = vardo_kdbx::Credentials::with_password(pw);
            if let Ok(doc) = vardo_kdbx::open(&bytes, &creds) {
                let v = vardo_vault::Vault::open(&doc).unwrap();
                println!(
                    "{} | pw={} | KDBX {}.{} | {} entries | writable={} | name={}",
                    path,
                    pw,
                    doc.version().major,
                    doc.version().minor,
                    v.entries().len(),
                    v.is_writable(),
                    v.database_name(),
                );
                opened = true;
                break;
            }
        }
        if !opened {
            println!("{path} | did not open with any known fixture password");
        }
    }
}
