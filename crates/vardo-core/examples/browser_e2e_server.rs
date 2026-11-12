//! Serves the browser socket for an unlocked vault, with no window.
//!
//! Used to check the extension end to end in a real browser on a machine where
//! the desktop app cannot open a window, such as CI or a headless session. It
//! runs exactly the server the app runs; the only difference is that copying
//! and showing the window are printed instead of performed, so a test never
//! touches the real clipboard.
//!
//! ```text
//! cargo run -p vardo-core --example browser_e2e_server -- /tmp/e2e.kdbx demopass
//! ```
//!
//! The socket goes wherever `XDG_RUNTIME_DIR` points, so set that to keep it
//! away from a Vardo you are using.

#[cfg(unix)]
fn main() {
    use std::sync::{Arc, Mutex};
    use vardo_core::browser::Effect;
    use vardo_core::source::{SourceContext, VaultSource};

    let mut args = std::env::args().skip(1);
    let vault = std::path::PathBuf::from(args.next().expect("vault path"));
    let password = args.next().expect("password");
    let dir = vault.parent().expect("parent").to_path_buf();

    let session = vardo_core::Session::open(
        &VaultSource::file(&vault),
        &SourceContext::local_only(&dir),
        &password,
        |_| {},
    )
    .expect("unlock");

    let path = vardo_browser::socket::path();
    println!("serving {}", path.display());
    let perform = Arc::new(|effect: Effect| {
        match effect {
            // Only the length: this output may end up in a CI log.
            Effect::Copy(value) => println!("effect: copy ({} characters)", value.len()),
            Effect::Show => println!("effect: show"),
            Effect::None => {}
        }
        Ok(())
    });
    vardo_core::browser::server::serve(&path, Arc::new(Mutex::new(Some(session))), perform)
        .expect("serve");
}

#[cfg(not(unix))]
fn main() {
    eprintln!("The browser socket is Unix only for now.");
}
