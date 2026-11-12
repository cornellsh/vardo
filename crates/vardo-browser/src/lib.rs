//! The browser extension's half of Vardo, minus the browser.
//!
//! Everything here is shared by the desktop app and the native host:
//!
//! - [`protocol`]: the messages, which double as the security policy.
//! - [`matching`]: whether a login belongs to a page.
//! - [`framing`]: Chrome's native messaging wire format.
//! - [`socket`]: where the app listens for the host.
//!
//! The design and its reasons are `dec:own-extension` and
//! `dec:native-messaging-relay`. The rules it keeps are the `inv:` entries
//! under "Browser" in `docs/INVARIANTS.md`.

#![forbid(unsafe_code)]

pub mod framing;
pub mod matching;
pub mod protocol;
pub mod socket;
