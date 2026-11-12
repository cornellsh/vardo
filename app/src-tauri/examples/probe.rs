//! Smoke-check fixtures through the real open path, without a GUI.
//!
//! `cargo run -p vardo-app --example probe -- fixtures/*.kdbx`
//!
//! Useful when the UI misbehaves and the question is whether the core or the
//! shell is at fault.

use vardo_core::source::{SourceContext, VaultSource};
use vardo_core::Session;

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: probe <file.kdbx>...");
        std::process::exit(2);
    }

    let ctx = SourceContext::local_only(std::env::temp_dir());
    for path in paths {
        // Fixture passwords only. Never put a real password here.
        let opened = ["demopass", "password"].iter().find_map(|pw| {
            let session = Session::open(&VaultSource::file(&path), &ctx, pw, |_| {}).ok()?;
            Some((pw, session))
        });
        match opened {
            Some((pw, session)) => {
                let info = session.info();
                println!(
                    "{path} | pw={pw} | {} | {} entries | writable={} | name={}",
                    info.format, info.entry_count, info.writable, info.name,
                );
            }
            None => println!("{path} | did not open with any known fixture password"),
        }
    }
}
