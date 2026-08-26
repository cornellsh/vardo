//! Resource ceilings for untrusted input.
//!
//! # The pre-authentication window
//!
//! KDBX authenticates its header with an HMAC — but that HMAC's key is derived
//! by running the KDF, and the KDF's parameters live *in the header*. So the
//! order of operations is unavoidably:
//!
//! ```text
//!   read KDF parameters  (UNAUTHENTICATED, attacker-controlled)
//!         |
//!   run the KDF          (expensive, still unauthenticated)
//!         |
//!   verify header HMAC   (first point at which anything is trusted)
//! ```
//!
//! There is no way to reorder this; it is inherent to the format. A file whose
//! header claims `Argon2 M=2GiB, I=4294967295` will therefore try to allocate
//! two gigabytes and grind for hours **before** anyone discovers the password
//! is wrong — or that the file was never a real vault at all.
//!
//! That is a denial-of-service reachable by anyone who can put a file where the
//! app will open it: a shared vault, a synced folder, a compromised cloud
//! account. Bounding these parameters is the only available defence, which is
//! why this module exists rather than trusting the format's own maxima (the
//! spec permits `M` up to 2 GiB and `I` up to 2^32-1).
//!
//! Defaults are generous enough to open any vault a human actually configured,
//! and tight enough that a hostile file fails in about a second.

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Maximum Argon2 memory, in bytes. The spec allows ~2 GiB.
    pub max_kdf_memory: u64,
    /// Maximum Argon2 iterations. The spec allows 2^32-1.
    pub max_kdf_iterations: u64,
    /// Maximum Argon2 lanes.
    pub max_kdf_parallelism: u64,
    /// Maximum AES-KDF rounds. The spec allows 2^64-1, which never terminates.
    pub max_aes_kdf_rounds: u64,
    /// Maximum size of the decompressed payload, guarding against gzip bombs.
    pub max_decompressed_size: u64,
    /// Maximum accepted input file size.
    pub max_file_size: u64,
    pub xml: crate::xml::ParseLimits,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            // KeePassXC's own UI tops out around 1 GiB; real vaults use 64 MiB.
            max_kdf_memory: 1024 * 1024 * 1024,
            // Vaults in the wild use 2-40. A thousand is already ~60s.
            max_kdf_iterations: 1000,
            max_kdf_parallelism: 64,
            // KeePass defaults are 6e3-6e7. 1e8 is roughly a second with AES-NI.
            max_aes_kdf_rounds: 100_000_000,
            // Largest real vaults are a few hundred MB with attachments.
            max_decompressed_size: 512 * 1024 * 1024,
            max_file_size: 512 * 1024 * 1024,
            xml: crate::xml::ParseLimits::default(),
        }
    }
}

impl Limits {
    /// Removes every ceiling.
    ///
    /// Only appropriate for a file the user has explicitly chosen to open after
    /// being told it exceeds normal bounds — never as a default, and never for
    /// a file arriving over a sync backend.
    pub fn unlimited() -> Self {
        Self {
            max_kdf_memory: u64::MAX,
            max_kdf_iterations: u64::MAX,
            max_kdf_parallelism: u64::MAX,
            max_aes_kdf_rounds: u64::MAX,
            max_decompressed_size: u64::MAX,
            max_file_size: u64::MAX,
            xml: crate::xml::ParseLimits {
                max_depth: usize::MAX,
                max_elements: usize::MAX,
                max_attributes_per_element: usize::MAX,
                max_name_len: usize::MAX,
            },
        }
    }

    pub(crate) fn check(&self, what: &str, value: u64, max: u64) -> Result<()> {
        if value > max {
            return Err(Error::ResourceLimit(format!(
                "{what} is {value}, above the {max} limit; open with Limits::unlimited() if this \
                 file is genuinely yours"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_admit_a_real_world_vault() {
        // Parameters measured from an actual KeePassXC-generated vault.
        let l = Limits::default();
        assert!(l.check("memory", 64 * 1024 * 1024, l.max_kdf_memory).is_ok());
        assert!(l.check("iterations", 14, l.max_kdf_iterations).is_ok());
        assert!(l.check("parallelism", 2, l.max_kdf_parallelism).is_ok());
    }

    #[test]
    fn defaults_reject_spec_maximum_parameters() {
        // Values the KDBX spec explicitly permits, and which would hang or OOM.
        let l = Limits::default();
        assert!(l.check("memory", 0x7FFF_FFFF, l.max_kdf_memory).is_err());
        assert!(l
            .check("iterations", 0xFFFF_FFFF, l.max_kdf_iterations)
            .is_err());
        assert!(l
            .check("rounds", u64::MAX, l.max_aes_kdf_rounds)
            .is_err());
    }

    #[test]
    fn unlimited_admits_everything() {
        let l = Limits::unlimited();
        assert!(l.check("memory", u64::MAX, l.max_kdf_memory).is_ok());
    }

    #[test]
    fn error_message_names_the_escape_hatch() {
        let l = Limits::default();
        let e = l.check("memory", u64::MAX, l.max_kdf_memory).unwrap_err();
        assert!(e.to_string().contains("Limits::unlimited"));
    }
}
