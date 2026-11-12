//! Google Drive: signing in, finding vaults, and taking one over.
//!
//! Contract: the app asks only for the `drive.file` scope, so Drive shows it
//! only files it created or the user handed over (`dec:drive-first-backend`).
//! Tokens stay on this machine in files written 0600. Network calls go through (`dec:app-owns-network`)
//! `vardo-drive`; this module decides when to make them.
//!
//! - [`auth`]: OAuth with PKCE and a loopback redirect, and the stored token.
//! - [`vaults`]: listing vaults the app can see and uploading a new one.
//! - [`picker`]: the Google Picker handover for a vault the app cannot see yet.

pub mod auth;
pub mod picker;
pub mod vaults;
