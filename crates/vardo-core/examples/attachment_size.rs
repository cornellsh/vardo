//! How attachment size bounds every save, for `fact:kdbx-attachment-cost`.
//!
//! `cargo run --release -p vardo-core --example attachment_size`
//!
//! Adds one attachment of random bytes, the incompressible worst case, to a
//! copy of the Argon2id fixture, then times a save and a fresh unlock. KDBX has
//! no delta format, so the save time and file size are what every sync pays.
//! Sizes stop at the 8 MB cap in `session/edit.rs`. The fixture's KDF is
//! deliberately cheap, so the times are what size adds to a real unlock.

use std::time::Instant;

use vardo_core::source::{SourceContext, VaultSource};
use vardo_core::Session;

const FIXTURE: &str = "fixtures/test_db_kdbx4_with_password_argon2id.kdbx";

fn main() {
    println!("attachment MB | file MB | save s | unlock s");
    for mb in [0usize, 1, 2, 4, 8] {
        let dir = std::env::temp_dir().join(format!("vardo-attach-{mb}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let vault = dir.join("v.kdbx");
        std::fs::copy(FIXTURE, &vault).unwrap();
        let source = VaultSource::file(&vault);
        let ctx = SourceContext::local_only(&dir);

        let mut session = Session::open(&source, &ctx, "demopass", |_| {}).unwrap();
        if mb > 0 {
            let mut bytes = vec![0u8; mb * 1024 * 1024];
            getrandom::fill(&mut bytes).unwrap();
            let file = dir.join("blob.bin");
            std::fs::write(&file, &bytes).unwrap();
            let uuid = session.list_entries(None, None)[0].uuid.clone();
            session.add_attachment(&uuid, &file).unwrap();
        }
        let started = Instant::now();
        session.save(&ctx).unwrap();
        let save = started.elapsed().as_secs_f64();
        drop(session);

        let started = Instant::now();
        Session::open(&source, &ctx, "demopass", |_| {}).unwrap();
        let unlock = started.elapsed().as_secs_f64();
        let size = std::fs::metadata(&vault).unwrap().len() as f64 / 1_048_576.0;
        println!("{mb:>13} | {size:>7.1} | {save:>6.2} | {unlock:>8.2}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
