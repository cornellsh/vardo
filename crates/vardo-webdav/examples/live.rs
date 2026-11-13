//! The WebDAV backend against a real server, for `docs/RUNBOOKS.md`.
//!
//! `cargo run -p vardo-webdav --features network --example live -- URL USER PASSWORD`
//! where URL names a file that may be overwritten. Checks a conditional write,
//! a write landing in between, and that nothing is written blindly.

use vardo_sync::backend::WriteOutcome;
use vardo_sync::StorageBackend;
use vardo_webdav::ureq_transport::UreqTransport;
use vardo_webdav::WebDavBackend;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [url, user, password] = args.as_slice() else {
        eprintln!("usage: live URL USER PASSWORD");
        std::process::exit(2);
    };
    let mut http = UreqTransport::new();
    let mut dav = WebDavBackend::new(&mut http, url, user, password);
    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!(
            "{} {name} {}",
            if ok { "ok  " } else { "FAIL" },
            if ok { String::new() } else { detail }
        );
        failures += usize::from(!ok);
    };

    dav.write(b"base").expect("plain write to create the file");
    let (bytes, state) = dav.read().expect("read");
    check(
        "the file reads back",
        bytes == b"base",
        format!("{bytes:?}"),
    );
    check(
        "the server gives an ETag",
        !state.version.is_empty(),
        state.version.clone(),
    );

    let outcome = dav.write_if_unchanged(b"merged", b"base");
    check(
        "an unchanged file takes a conditional write",
        matches!(outcome, Ok(WriteOutcome::Written(_))),
        format!("{outcome:?}"),
    );

    // Another device writes; our next write was based on "merged".
    dav.write(b"other device")
        .expect("the other device's write");
    let outcome = dav.write_if_unchanged(b"ours", b"merged");
    check(
        "a file changed elsewhere is handed back, not overwritten",
        matches!(&outcome, Ok(WriteOutcome::Superseded { buried }) if buried == b"other device"),
        format!("{outcome:?}"),
    );
    let (now, _) = dav.read().expect("read");
    check(
        "the other device's write survives",
        now == b"other device",
        format!("{now:?}"),
    );

    std::process::exit(i32::from(failures > 0));
}
