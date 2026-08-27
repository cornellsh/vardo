//! The two shapes of a safe write, and the difference between them.
//!
//! `write_if_unchanged` exists so the protocol can be written against the
//! operation that is actually safe, while backends that cannot perform it
//! emulate it themselves. Before this, every backend inherited Google Drive's
//! inability to refuse a write, whether or not it shared it.

use vardo_sync::backend::{StorageBackend, WriteOutcome};
use vardo_sync::fake::Server;
use vardo_sync::local::LocalFile;

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("vardo-cas-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("vault.kdbx")
}

// --- the strong form --------------------------------------------------------

#[test]
fn a_local_file_refuses_rather_than_burying() {
    let path = scratch("refuse");
    let mut a = LocalFile::new(&path);
    a.write(b"original").unwrap();

    // Another writer gets there first.
    let mut other = LocalFile::new(&path);
    other.write(b"theirs").unwrap();

    // We still believe the remote holds "original".
    let outcome = a.write_if_unchanged(b"ours", b"original").unwrap();

    match outcome {
        WriteOutcome::Superseded { buried } => assert_eq!(buried, b"theirs"),
        WriteOutcome::Written(_) => panic!("overwrote a change it had not seen"),
    }

    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"theirs",
        "a refusal must leave the other write intact; the point of refusing is \
         that nothing is destroyed in the first place"
    );
}

#[test]
fn a_local_file_writes_when_the_expectation_holds() {
    let path = scratch("accept");
    let mut b = LocalFile::new(&path);
    b.write(b"original").unwrap();

    match b.write_if_unchanged(b"ours", b"original").unwrap() {
        WriteOutcome::Written(_) => {}
        WriteOutcome::Superseded { .. } => panic!("refused a write it should have accepted"),
    }
    assert_eq!(std::fs::read(&path).unwrap(), b"ours");
}

#[test]
fn an_empty_expectation_means_nothing_should_be_there_yet() {
    let path = scratch("firstwrite");
    let mut b = LocalFile::new(&path);

    match b.write_if_unchanged(b"first", b"").unwrap() {
        WriteOutcome::Written(_) => {}
        WriteOutcome::Superseded { .. } => panic!("refused a legitimate first write"),
    }

    // Now something IS there, so the same call must refuse.
    let mut c = LocalFile::new(&path);
    match c.write_if_unchanged(b"second", b"").unwrap() {
        WriteOutcome::Superseded { buried } => assert_eq!(buried, b"first"),
        WriteOutcome::Written(_) => panic!("clobbered an existing file on a first write"),
    }
}

// --- the emulated form ------------------------------------------------------

#[test]
fn the_default_emulation_detects_after_the_fact() {
    // FakeDrive models Drive, which has no conditional write, so it inherits
    // the default. It cannot refuse — it writes, then reports what it buried.
    let server = Server::new();
    let mut drive = server.connect();
    drive.write(b"original").unwrap();
    drive.write(b"theirs").unwrap();

    let outcome = drive.write_if_unchanged(b"ours", b"original").unwrap();

    match outcome {
        WriteOutcome::Superseded { buried } => assert_eq!(
            buried, b"theirs",
            "the emulation must hand back what it destroyed, because that is \
             the only thing that makes the loss recoverable"
        ),
        WriteOutcome::Written(_) => panic!("failed to notice it had buried a change"),
    }
}

#[test]
fn the_emulation_accepts_when_the_expectation_holds() {
    let server = Server::new();
    let mut drive = server.connect();
    drive.write(b"original").unwrap();

    match drive.write_if_unchanged(b"ours", b"original").unwrap() {
        WriteOutcome::Written(_) => {}
        WriteOutcome::Superseded { buried } => {
            panic!("refused a legitimate write, buried={buried:?}")
        }
    }
}

// --- the difference that matters -------------------------------------------

#[test]
fn refusing_preserves_the_other_write_and_emulating_does_not() {
    // This is the whole argument for the trait shape, stated as a test.
    //
    // Both backends end up reporting the same thing to the protocol, so sync
    // recovers either way. The difference is what happened on the storage in
    // the meantime: the emulated path destroyed a write and then went and dug
    // it out of the revision history, while the native path never destroyed it.
    let path = scratch("compare");
    let mut local = LocalFile::new(&path);
    local.write(b"original").unwrap();
    LocalFile::new(&path).write(b"theirs").unwrap();
    let _ = local.write_if_unchanged(b"ours", b"original").unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"theirs",
        "native: the other write is still the live content"
    );

    let server = Server::new();
    let mut drive = server.connect();
    drive.write(b"original").unwrap();
    drive.write(b"theirs").unwrap();
    let _ = drive.write_if_unchanged(b"ours", b"original").unwrap();
    let (live, _) = drive.read().unwrap();
    assert_eq!(
        live, b"ours",
        "emulated: our write is live and theirs survives only as a revision"
    );
}
