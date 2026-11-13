//! Everything the desktop app does, with no dependency on the UI toolkit.
//!
//! # Why this crate exists
//!
//! All of this used to live inside the Tauri crate, where reaching any of it
//! meant constructing a `tauri::State`. The consequence was that the two most
//! dangerous paths in the product — saving and syncing — had no automated tests
//! at all, and were checked by driving a real window with `xdotool` and reading
//! screenshots. That is not a way to verify a password manager.
//!
//! So the shell keeps only `#[tauri::command]` wrappers that deserialise, call
//! [`Session`], and serialise back. Everything with a decision in it is here,
//! and is an ordinary Rust function with ordinary Rust tests.
//!
//! # The rule this crate inherits
//!
//! Secrets do not leave here except through [`Session::field_value`], which is
//! deliberately awkward to call and returns a `Zeroizing` buffer. The view
//! models in [`model`] have no shape that can carry a protected value.

//! # Map
//!
//! - [`session`]: an unlocked vault, its edits, and saving it.
//! - [`sync`]: preview and apply a merge with the stored copy.
//! - [`drive`]: Google Drive sign-in, listing and handover.
//! - [`browser`]: answers the browser extension over a local socket.
//! - [`source`]: where a vault lives. [`model`]: wire types. [`CoreError`]: errors.
//! - Small services: [`clipboard`], [`favicon`], [`generate`], [`health`],
//!   [`recents`], [`totp`].

#![forbid(unsafe_code)]

pub mod browser;
pub mod clipboard;
pub mod drive;
mod error;
pub mod favicon;
pub mod generate;
pub mod health;
pub mod model;
pub mod recents;
mod secrets;
pub mod session;
pub mod source;
pub mod sync;
pub mod totp;
pub mod webdav;

pub use error::CoreError;
pub use session::Session;

#[cfg(test)]
mod bindings;

/// Seconds before a copied secret is wiped from the clipboard.
///
/// Policy lives here rather than in the shell so the shell cannot quietly
/// disagree with it. Matches KeePassXC's default.
pub const CLIPBOARD_CLEAR_SECONDS: u32 = 15;
