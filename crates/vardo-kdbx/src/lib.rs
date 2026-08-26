//! KDBX container reading, built for round-trip fidelity.
//!
//! The design rule for this crate: **the parsed document is the source of
//! truth, and anything we do not understand is preserved verbatim.** A typed
//! model is a *view* over the document, never a replacement for it. Closed
//! typed models are why other KDBX libraries lose data on round-trip — an
//! element the model does not name is an element that silently disappears.
//!
//! This module currently implements reading down to the raw XML payload, which
//! is both the first half of the container work and the ground-truth oracle the
//! conformance harness diffs against.

#![forbid(unsafe_code)]

pub mod crypto;
pub mod cursor;
pub mod error;
pub mod header;

use std::io::Read;

pub use crypto::Credentials;
pub use error::{Error, Result};
pub use header::{InnerHeader, OuterHeader, Version};

use sha2::{Digest, Sha256};

/// A decrypted KDBX container, one step before XML parsing.
pub struct Document {
    pub outer: OuterHeader,
    pub inner: InnerHeader,
    /// The decrypted, decompressed XML payload, exactly as stored.
    pub xml: Vec<u8>,
}

impl Document {
    pub fn version(&self) -> Version {
        self.outer.version
    }
}

/// Decrypts a KDBX file down to its raw XML payload.
///
/// Authentication is checked before anything is decrypted: the header digest,
/// the header HMAC, and then every block's HMAC. A wrong password and a
/// corrupted file both surface as [`Error::HeaderAuthFailed`], which is a
/// property of the format rather than a limitation here — KDBX deliberately
/// gives an attacker no way to distinguish them.
pub fn open(bytes: &[u8], creds: &Credentials) -> Result<Document> {
    let outer = OuterHeader::parse(bytes)?;
    if !outer.version.is_kdbx4() {
        // KDBX 3.1 uses a different integrity scheme (StreamStartBytes and a
        // hashed, unauthenticated block stream). Deliberately unimplemented
        // rather than silently mishandled.
        return Err(Error::UnsupportedVersion {
            major: outer.version.major,
            minor: outer.version.minor,
        });
    }

    let header_len = outer.raw.len();
    let mut c = cursor::Cursor::new(&bytes[header_len..]);
    let header_sha = c.take(32)?;
    let header_hmac = c.take(32)?;

    // Integrity of the header itself, before we trust any field in it.
    let actual_sha = Sha256::digest(&outer.raw);
    if actual_sha.as_slice() != header_sha {
        return Err(Error::Malformed {
            what: "header",
            detail: "SHA-256 mismatch (file is corrupt)".into(),
        });
    }

    let kdf_params = outer.kdf_parameters()?;
    let master_seed = outer.master_seed()?;
    let keys = crypto::MasterKeys::derive(creds, master_seed, &kdf_params)?;
    keys.verify_header_hmac(&outer.raw, header_hmac)?;

    // Past this point the credentials are known-good, so any further failure is
    // genuine corruption rather than a bad password.
    let ciphertext = crypto::read_hmac_block_stream(&keys, c.rest())?;
    let plaintext = crypto::decrypt(
        outer.cipher()?,
        &keys.cipher_key,
        outer.encryption_iv()?,
        &ciphertext,
    )?;

    let payload = if outer.compressed()? {
        let mut out = Vec::with_capacity(plaintext.len() * 3);
        flate2::read::GzDecoder::new(plaintext.as_slice()).read_to_end(&mut out)?;
        out
    } else {
        plaintext
    };

    let (inner, xml) = InnerHeader::parse(&payload)?;
    Ok(Document {
        outer,
        inner,
        xml: xml.to_vec(),
    })
}
