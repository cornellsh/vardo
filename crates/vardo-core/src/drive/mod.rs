//! Google Drive: signing in, finding vaults, and taking one over.
//!
//! Contract: the app asks only for the `drive.file` scope, so Drive shows it
//! only files it created or the user handed over (`dec:drive-first-backend`).
//! The refresh token goes to the OS credential store. Network calls go through
//! `vardo-drive`, and this module decides when to make them
//! (`dec:app-owns-network`).
//!
//! - [`auth`]: OAuth with PKCE and a loopback redirect, and fresh access tokens.
//! - [`vaults`]: listing vaults the app can see and uploading a new one.
//! - [`picker`]: the Google Picker handover for a vault the app cannot see yet.
//! - [`tokens`]: where the refresh token is kept.

pub mod auth;
pub mod picker;
pub mod tokens;
pub mod vaults;
