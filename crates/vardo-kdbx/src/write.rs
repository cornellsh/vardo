//! Container encoding.
//!
//! # What "lossless" means at this layer
//!
//! It does **not** mean byte-identical files. It cannot, and it must not:
//!
//! * gzip output is implementation-defined, so recompressing an untouched
//!   payload with a different deflate library legitimately yields different
//!   bytes;
//! * every save regenerates the master seed and the encryption IV, because
//!   reusing an IV across two different plaintexts under AES-CBC or ChaCha20
//!   leaks their XOR. Deterministic output would be a vulnerability.
//!
//! The invariant that actually matters, and that the tests enforce, is:
//!
//! > **`payload(save(open(f))) == payload(f)`, byte for byte.**
//!
//! Merge operates on decrypted XML, never on ciphertext, so payload stability
//! is what prevents phantom conflicts. Container bytes are expected to differ
//! on every save and nobody should ever diff them.
//!
//! # Inner keystream reuse
//!
//! Protected values share one keystream, positionally. If a save reused the
//! previous inner key and any value had changed length or content, the
//! overlapping keystream would expose the XOR of the old and new secrets — a
//! textbook two-time pad against the user's passwords.
//!
//! So every save mints a fresh inner key and re-encrypts **all** protected
//! values. This is not an optimisation to skip when nothing changed.

use flate2::{write::GzEncoder, Compression};
use sha2::{Digest, Sha256};
use std::io::Write as _;
use zeroize::Zeroizing;

use crate::crypto::{self, Credentials};
use crate::error::{Error, Result};
use crate::header::{
    self, Binary, InnerHeader, InnerStreamCipher, VariantDictionary, VdValue, Version,
};
use crate::limits::Limits;
use crate::protected;
use crate::xml;

/// Fresh cryptographic randomness for a save.
///
/// Injectable so tests can pin it; production always uses the OS CSPRNG.
pub trait Rng {
    fn fill(&mut self, out: &mut [u8]) -> Result<()>;
}

pub struct OsRng;

impl Rng for OsRng {
    fn fill(&mut self, out: &mut [u8]) -> Result<()> {
        getrandom::fill(out).map_err(|e| Error::Kdf(format!("OS randomness unavailable: {e}")))
    }
}

/// How to encode. Defaults mirror the file that was opened, so a save preserves
/// the user's chosen format rather than silently upgrading it.
#[derive(Debug, Clone)]
pub struct SaveOptions {
    pub version: Version,
    pub cipher: [u8; 16],
    pub compress: bool,
    /// KDF parameters. The salt is regenerated on every save regardless.
    pub kdf: VariantDictionary,
    pub inner_cipher: InnerStreamCipher,
}

impl SaveOptions {
    /// Derives options from an opened document, preserving its format exactly.
    pub fn preserving(doc: &crate::Document) -> Result<Self> {
        // Check the version first: a KDBX 3.1 document has no KdfParameters
        // field, and reporting that as the error would be misleading.
        if doc.outer.version.major != 4 {
            return Err(Error::UnsupportedVersion {
                major: doc.outer.version.major,
                minor: doc.outer.version.minor,
            });
        }
        Ok(Self {
            version: doc.outer.version,
            cipher: doc.outer.cipher()?,
            compress: doc.outer.compressed()?,
            kdf: doc.outer.kdf_parameters()?,
            inner_cipher: doc.inner.stream_cipher,
        })
    }
}

/// Encodes a document whose protected values are **already plaintext**.
///
/// This is the correct entry point for anything that has been edited. See the
/// warning on [`save`] for why.
#[allow(clippy::too_many_arguments)]
pub fn save_plaintext(
    dom: &xml::Document,
    binaries: &[Binary],
    creds: &Credentials,
    opts: &SaveOptions,
    limits: &Limits,
    rng: &mut dyn Rng,
) -> Result<Vec<u8>> {
    if opts.version.major != 4 {
        return Err(Error::UnsupportedVersion {
            major: opts.version.major,
            minor: opts.version.minor,
        });
    }

    let mut inner_key = Zeroizing::new(vec![0u8; 64]);
    rng.fill(&mut inner_key)?;
    let dom = encrypt_protected(dom, opts.inner_cipher, &inner_key)?;

    let mut payload = Vec::new();
    write_inner_header(&mut payload, opts.inner_cipher, &inner_key, binaries);
    payload.extend_from_slice(&dom.to_bytes());

    let plaintext = if opts.compress {
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&payload)?;
        enc.finish()?
    } else {
        payload
    };

    let mut master_seed = vec![0u8; 32];
    rng.fill(&mut master_seed)?;

    let iv_len = if opts.cipher == header::CIPHER_CHACHA20 {
        12
    } else {
        16
    };
    let mut iv = vec![0u8; iv_len];
    rng.fill(&mut iv)?;

    let mut kdf = opts.kdf.clone();
    randomize_kdf_salt(&mut kdf, rng)?;

    let header_raw = build_outer_header(opts, &master_seed, &iv, &kdf)?;
    let keys = crypto::MasterKeys::derive(creds, &master_seed, &kdf, limits)?;
    let ciphertext = crypto::encrypt(opts.cipher, &keys.cipher_key, &iv, &plaintext)?;

    let mut out = header_raw.clone();
    out.extend_from_slice(&Sha256::digest(&header_raw));
    out.extend_from_slice(&keys.header_hmac(&header_raw));
    crypto::write_hmac_block_stream(&keys, &ciphertext, &mut out);
    Ok(out)
}

/// Encodes a document whose protected values are still ciphertext.
///
/// # This is only valid for an **unmodified** document
///
/// Protected values share one keystream and are decrypted by **position**. This
/// function decrypts them from `dom` at save time, which silently produces
/// garbage if any protected element was added, removed or reordered since the
/// document was opened: the ciphertext is keyed to the old positions, and the
/// ordinals are computed from the new ones.
///
/// Deleting one entry therefore corrupts the password of every entry after it.
/// That is not hypothetical — it is what this function did until a merge test
/// caught three untouched entries decrypting to garbage.
///
/// **Anything that edits a document must hold plaintext while unlocked and use
/// [`save_plaintext`].** `vardo_vault::Vault` does exactly that.
pub fn save(
    dom: &xml::Document,
    old_inner: &InnerHeader,
    binaries: &[Binary],
    creds: &Credentials,
    opts: &SaveOptions,
    limits: &Limits,
    rng: &mut dyn Rng,
) -> Result<Vec<u8>> {
    if opts.version.major != 4 {
        return Err(Error::UnsupportedVersion {
            major: opts.version.major,
            minor: opts.version.minor,
        });
    }

    // Decrypt from the (assumed unmodified) document, then hand the plaintext
    // form to the safe path.
    let plain = protected::plaintext_document(dom, old_inner.stream_cipher, &old_inner.stream_key)?;
    save_plaintext(&plain, binaries, creds, opts, limits, rng)
}

/// Encrypts every protected value in document order under a fresh keystream.
fn encrypt_protected(
    dom: &xml::Document,
    cipher: InnerStreamCipher,
    key: &[u8],
) -> Result<xml::Document> {
    let mut out = dom.clone();
    let mut stream = protected::InnerStream::new(cipher, key)?;
    encrypt_element(&mut out.root, &mut stream);
    Ok(out)
}

fn encrypt_element(e: &mut xml::Element, stream: &mut protected::InnerStream) {
    if protected::is_protected(e) {
        let text = e.text();
        // Empty values consume no keystream, matching `decrypt_all`.
        if text.is_empty() {
            e.set_text("");
        } else {
            let mut buf = Zeroizing::new(text.into_bytes());
            stream.apply(&mut buf);
            e.set_text(&base64_encode(&buf));
        }
        return;
    }
    for child in e.children.iter_mut() {
        if let xml::Node::Element(child) = child {
            encrypt_element(child, stream);
        }
    }
}

fn randomize_kdf_salt(kdf: &mut VariantDictionary, rng: &mut dyn Rng) -> Result<()> {
    for (key, value) in kdf.items.iter_mut() {
        if key == "S" {
            if let VdValue::Bytes(salt) = value {
                // Preserve the existing salt length; Argon2 and AES-KDF differ.
                let mut fresh = vec![0u8; salt.len().max(32)];
                rng.fill(&mut fresh)?;
                *salt = fresh;
                return Ok(());
            }
        }
    }
    Err(Error::MissingHeaderField("KdfParameters.S"))
}

fn build_outer_header(
    opts: &SaveOptions,
    master_seed: &[u8],
    iv: &[u8],
    kdf: &VariantDictionary,
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(&header::SIG1.to_le_bytes());
    out.extend_from_slice(&header::SIG2.to_le_bytes());
    out.extend_from_slice(&opts.version.minor.to_le_bytes());
    out.extend_from_slice(&opts.version.major.to_le_bytes());

    let field = |id: u8, data: &[u8], out: &mut Vec<u8>| {
        out.push(id);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
    };

    field(2, &opts.cipher, &mut out);
    field(3, &u32::from(opts.compress).to_le_bytes(), &mut out);
    field(4, master_seed, &mut out);
    field(7, iv, &mut out);
    field(11, &serialize_variant_dictionary(kdf), &mut out);
    field(0, &[0x0D, 0x0A, 0x0D, 0x0A], &mut out);
    Ok(out)
}

pub(crate) fn serialize_variant_dictionary(vd: &VariantDictionary) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&vd.version.to_le_bytes());
    for (key, value) in &vd.items {
        let (ty, bytes): (u8, Vec<u8>) = match value {
            VdValue::U32(v) => (0x04, v.to_le_bytes().to_vec()),
            VdValue::U64(v) => (0x05, v.to_le_bytes().to_vec()),
            VdValue::Bool(v) => (0x08, vec![u8::from(*v)]),
            VdValue::I32(v) => (0x0C, v.to_le_bytes().to_vec()),
            VdValue::I64(v) => (0x0D, v.to_le_bytes().to_vec()),
            VdValue::Str(v) => (0x18, v.as_bytes().to_vec()),
            VdValue::Bytes(v) => (0x42, v.clone()),
        };
        out.push(ty);
        out.extend_from_slice(&(key.len() as u32).to_le_bytes());
        out.extend_from_slice(key.as_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&bytes);
    }
    out.push(0);
    out
}

fn write_inner_header(
    out: &mut Vec<u8>,
    cipher: InnerStreamCipher,
    key: &[u8],
    binaries: &[Binary],
) {
    let id: i32 = match cipher {
        InnerStreamCipher::None => 0,
        InnerStreamCipher::ArcFourVariant => 1,
        InnerStreamCipher::Salsa20 => 2,
        InnerStreamCipher::ChaCha20 => 3,
        InnerStreamCipher::Unknown(v) => v,
    };
    // Inner header lengths are signed, unlike the outer header's.
    let field = |id: u8, data: &[u8], out: &mut Vec<u8>| {
        out.push(id);
        out.extend_from_slice(&(data.len() as i32).to_le_bytes());
        out.extend_from_slice(data);
    };
    field(1, &id.to_le_bytes(), out);
    field(2, key, out);
    for b in binaries {
        let mut buf = Vec::with_capacity(b.data.len() + 1);
        buf.push(b.flags);
        buf.extend_from_slice(&b.data);
        field(3, &buf, out);
    }
    field(0, &[], out);
}

/// Re-exported for callers that need to encode KDBX values.
pub fn base64_encode(data: &[u8]) -> String {
    crate::base64::encode(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_dictionary_survives_a_round_trip() {
        let vd = VariantDictionary {
            version: 0x0100,
            items: vec![
                ("$UUID".into(), VdValue::Bytes(vec![7; 16])),
                ("I".into(), VdValue::U64(14)),
                ("M".into(), VdValue::U64(67108864)),
                ("P".into(), VdValue::U32(2)),
                ("S".into(), VdValue::Bytes(vec![9; 32])),
                ("V".into(), VdValue::U32(0x13)),
                ("flag".into(), VdValue::Bool(true)),
                ("neg".into(), VdValue::I32(-1)),
                ("big".into(), VdValue::I64(-2)),
                ("name".into(), VdValue::Str("hello".into())),
            ],
        };
        let bytes = serialize_variant_dictionary(&vd);
        let back = VariantDictionary::parse(&bytes).unwrap();
        assert_eq!(back, vd, "order and values must both survive");
    }
}
