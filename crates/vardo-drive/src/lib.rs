//! Google Drive: installed-app OAuth and a [`vardo_sync::StorageBackend`].
//!
//! Everything here is written against a small [`http::HttpTransport`] seam so
//! it can be tested without a network or a Google account. That is not
//! fastidiousness — nobody can run an integration suite against real Drive
//! without credentials, so code reachable only that way is code nobody tests.
//!
//! The real client lives behind the `network` feature. The contract tests that
//! must eventually run against live Drive are listed in
//! `tests/contract_notes.md`; their only job is to prove `vardo_sync::fake`
//! does not lie.

#![forbid(unsafe_code)]

pub mod drive;
pub mod http;
pub mod loopback;
pub mod oauth;

#[cfg(feature = "network")]
pub mod ureq_transport;

pub use drive::DriveBackend;
pub use oauth::{ClientConfig, Pkce, Tokens};
