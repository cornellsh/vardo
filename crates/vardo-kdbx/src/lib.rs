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

pub mod base64;
pub mod crypto;
pub mod cursor;
pub mod error;
pub mod header;
pub mod limits;
pub mod protected;
pub mod write;
pub mod xml;

use std::io::Read;

pub use crypto::Credentials;
pub use error::{Error, Result};
pub use header::{InnerHeader, OuterHeader, Version};
pub use limits::Limits;

use sha2::{Digest, Sha256};

/// A decrypted KDBX container, one step before XML parsing.
pub struct Document {
    pub outer: OuterHeader,
    pub inner: InnerHeader,
    /// The decrypted, decompressed XML payload, exactly as stored.
    pub xml: Vec<u8>,
    /// Carried so that later stages (XML parsing) inherit the same ceilings.
    limits: Limits,
}

/// Redacting. `xml` is the decrypted vault in plaintext; a derived `Debug`
/// would put every password into any log line that formats a `Document`.
impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("version", &self.outer.version)
            .field("xml", &format_args!("<{} bytes redacted>", self.xml.len()))
            .field("inner", &self.inner)
            .finish()
    }
}

/// Inflates gzip data, refusing to expand beyond `max`.
///
/// `read_to_end` on a `GzDecoder` will happily turn a few kilobytes into all
/// available memory. Reading through a `Take` and then checking for one extra
/// byte distinguishes "exactly at the limit" from "truncated at the limit".
fn decompress_bounded(compressed: &[u8], max: u64) -> Result<Vec<u8>> {
    let mut decoder = flate2::read::GzDecoder::new(compressed).take(max.saturating_add(1));
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    if out.len() as u64 > max {
        return Err(Error::ResourceLimit(format!(
            "decompressed payload exceeds {max} bytes (possible decompression bomb)"
        )));
    }
    Ok(out)
}

impl Document {
    pub fn version(&self) -> Version {
        self.outer.version
    }

    /// Parses the payload into a round-trippable DOM.
    pub fn parse_xml(&self) -> Result<xml::Document> {
        xml::parse(&self.xml, self.limits.xml)
    }

    /// Decrypts every `Protected="True"` value, in document order.
    pub fn protected_values(&self, dom: &xml::Document) -> Result<protected::ProtectedValues> {
        protected::decrypt_all(dom, self.inner.stream_cipher, &self.inner.stream_key)
    }

    /// The payload with protected values replaced by plaintext.
    ///
    /// This is the only form in which two KDBX files can meaningfully be
    /// compared; see [`protected::canonical_payload`].
    pub fn canonical_payload(&self, dom: &xml::Document) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        protected::canonical_payload(dom, self.inner.stream_cipher, &self.inner.stream_key)
    }

    /// Re-encodes `dom` into a complete KDBX file, preserving this document's
    /// version, cipher, compression and KDF settings.
    ///
    /// The output is deliberately *not* byte-identical to the input: fresh
    /// seeds and IVs are minted on every save, because reusing them would be a
    /// cryptographic flaw. What is preserved exactly is the decrypted payload.
    /// See [`crate::write`] for the full argument.
    pub fn save(&self, dom: &xml::Document, creds: &Credentials) -> Result<Vec<u8>> {
        self.save_with(dom, creds, &mut write::OsRng)
    }

    /// As [`Document::save`], with injectable randomness for testing.
    pub fn save_with(
        &self,
        dom: &xml::Document,
        creds: &Credentials,
        rng: &mut dyn write::Rng,
    ) -> Result<Vec<u8>> {
        let opts = write::SaveOptions::preserving(self)?;
        write::save(
            dom,
            &self.inner,
            &self.inner.binaries,
            creds,
            &opts,
            &self.limits,
            rng,
        )
    }
}

impl Document {
    /// The bytes of one attachment from the binary pool.
    ///
    /// Attachments are user data - a bank statement, an SSH key, the recovery
    /// codes someone will need exactly once and at the worst moment. Until this
    /// existed they could be listed but never taken out, which makes a vault a
    /// place things go to be lost.
    pub fn binary(&self, index: usize) -> Option<&[u8]> {
        self.inner.binaries.get(index).map(|b| b.data.as_slice())
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
    open_with_limits(bytes, creds, Limits::default())
}

/// As [`open`], with explicit resource ceilings.
///
/// Use this when a file legitimately exceeds the defaults and the user has
/// confirmed they want it opened anyway. See [`crate::limits`] for why the
/// ceilings cannot simply be removed.
pub fn open_with_limits(bytes: &[u8], creds: &Credentials, limits: Limits) -> Result<Document> {
    limits.check("file size", bytes.len() as u64, limits.max_file_size)?;

    let outer = OuterHeader::parse(bytes)?;
    if outer.version.major == 3 {
        return open_kdbx3(bytes, creds, limits, outer);
    }
    if !outer.version.is_kdbx4() {
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
    // Bounded inside `derive`: these parameters are attacker-controlled and
    // are consumed before the HMAC below can vouch for them.
    let keys = crypto::MasterKeys::derive(creds, master_seed, &kdf_params, &limits)?;
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
        decompress_bounded(&plaintext, limits.max_decompressed_size)?
    } else {
        plaintext
    };

    let (inner, xml) = InnerHeader::parse(&payload)?;
    Ok(Document {
        outer,
        inner,
        limits,
        xml: xml.to_vec(),
    })
}

/// Reads KDBX 3.1.
///
/// Structurally different from KDBX4 in three ways that matter:
///
/// * the KDF is always AES-KDF, parameterised by two dedicated header fields
///   rather than a VariantDictionary;
/// * there is no HMAC. Integrity rests on unkeyed SHA-256 per block, so the
///   format detects corruption but not tampering;
/// * the credential check is `StreamStartBytes` — the first 32 plaintext bytes
///   must match a header field. A wrong password therefore surfaces *after*
///   decryption rather than before it.
///
/// There is no inner header; the inner stream cipher lives in the outer header
/// and attachments are inlined in the XML rather than pooled.
///
/// Read-only on purpose. Writing 3.1 would mean emitting a format with no
/// authenticated integrity, and every client that can read 3.1 can also read
/// 4.x.
fn open_kdbx3(
    bytes: &[u8],
    creds: &Credentials,
    limits: Limits,
    outer: OuterHeader,
) -> Result<Document> {
    let master_seed = outer.master_seed()?;
    let transform_seed = outer
        .field(5)
        .ok_or(Error::MissingHeaderField("TransformSeed"))?;
    let rounds_raw = outer
        .field(6)
        .ok_or(Error::MissingHeaderField("TransformRounds"))?;
    let rounds = u64::from_le_bytes(rounds_raw.try_into().map_err(|_| Error::Malformed {
        what: "TransformRounds",
        detail: format!("expected 8 bytes, got {}", rounds_raw.len()),
    })?);
    let stream_start = outer
        .field(9)
        .ok_or(Error::MissingHeaderField("StreamStartBytes"))?
        .to_vec();

    let key = crypto::derive_kdbx3_key(creds, master_seed, transform_seed, rounds, &limits)?;
    let ciphertext = &bytes[outer.raw.len()..];
    let plaintext = crypto::decrypt(outer.cipher()?, &key, outer.encryption_iv()?, ciphertext)
        // A wrong password usually yields invalid CBC padding, which is
        // indistinguishable from corruption and must not leak which it was.
        .map_err(|_| Error::HeaderAuthFailed)?;

    if plaintext.len() < stream_start.len() || plaintext[..stream_start.len()] != stream_start[..] {
        return Err(Error::HeaderAuthFailed);
    }

    let blocks = crypto::read_hashed_block_stream(&plaintext[stream_start.len()..])?;
    let payload = if outer.compressed()? {
        decompress_bounded(&blocks, limits.max_decompressed_size)?
    } else {
        blocks
    };

    // Synthesised: 3.1 keeps these in the outer header, but everything
    // downstream expects the KDBX4 shape.
    let stream_id = outer
        .field(10)
        .ok_or(Error::MissingHeaderField("InnerRandomStreamID"))?;
    let stream_id = i32::from_le_bytes(stream_id.try_into().map_err(|_| Error::Malformed {
        what: "InnerRandomStreamID",
        detail: "expected 4 bytes".into(),
    })?);
    let inner = InnerHeader {
        stream_cipher: header::InnerStreamCipher::from(stream_id),
        stream_key: outer
            .field(8)
            .ok_or(Error::MissingHeaderField("InnerRandomStreamKey"))?
            .to_vec(),
        binaries: Vec::new(),
        unknown: Vec::new(),
    };

    Ok(Document {
        outer,
        inner,
        limits,
        xml: payload,
    })
}
