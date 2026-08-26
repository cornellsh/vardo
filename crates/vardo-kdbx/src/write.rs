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
        Ok(Self {
            version: doc.outer.version,
            cipher: doc.outer.cipher()?,
            compress: doc.outer.compressed()?,
            kdf: doc.outer.kdf_parameters()?,
            inner_cipher: doc.inner.stream_cipher,
        })
    }
}

/// Encodes a document.
///
/// `dom` supplies the content; `old_inner` supplies the inner key currently
/// protecting the values in `dom`, so they can be re-encrypted under a new one.
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

    // 1. Re-protect values under a fresh inner key. See the module note on
    //    keystream reuse; this step is mandatory, not opportunistic.
    let mut inner_key = Zeroizing::new(vec![0u8; 64]);
    rng.fill(&mut inner_key)?;
    let dom = reprotect(dom, old_inner, opts.inner_cipher, &inner_key)?;

    // 2. Inner header, then the XML payload.
    let mut payload = Vec::new();
    write_inner_header(&mut payload, opts.inner_cipher, &inner_key, binaries);
    payload.extend_from_slice(&dom.to_bytes());

    // 3. Compress.
    let plaintext = if opts.compress {
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&payload)?;
        enc.finish()?
    } else {
        payload
    };

    // 4. Fresh seed, salt and IV for this save.
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

    // 5. Outer header.
    let header_raw = build_outer_header(opts, &master_seed, &iv, &kdf)?;

    // 6. Derive keys and authenticate.
    let keys = crypto::MasterKeys::derive(creds, &master_seed, &kdf, limits)?;
    let ciphertext = crypto::encrypt(opts.cipher, &keys.cipher_key, &iv, &plaintext)?;

    let mut out = header_raw.clone();
    out.extend_from_slice(&Sha256::digest(&header_raw));
    out.extend_from_slice(&keys.header_hmac(&header_raw));
    crypto::write_hmac_block_stream(&keys, &ciphertext, &mut out);
    Ok(out)
}

/// Decrypts every protected value under `old` and re-encrypts under `new`.
fn reprotect(
    dom: &xml::Document,
    old_inner: &InnerHeader,
    new_cipher: InnerStreamCipher,
    new_key: &[u8],
) -> Result<xml::Document> {
    let plain = protected::decrypt_all(dom, old_inner.stream_cipher, &old_inner.stream_key)?;
    let mut out = dom.clone();
    let mut stream = protected::InnerStream::new(new_cipher, new_key)?;

    // Traversal order must match `protected_ordinals`, which is a depth-first
    // walk. Any divergence silently scrambles every value after the first.
    let mut ordinal = 0usize;
    reprotect_element(&mut out.root, &plain, &mut stream, &mut ordinal)?;

    if ordinal != plain.len() {
        return Err(Error::Malformed {
            what: "protected values",
            detail: format!("re-encrypted {ordinal} values but decrypted {}", plain.len()),
        });
    }
    Ok(out)
}

fn reprotect_element(
    e: &mut xml::Element,
    plain: &protected::ProtectedValues,
    stream: &mut protected::InnerStream,
    ordinal: &mut usize,
) -> Result<()> {
    if protected::is_protected(e) {
        let value = plain.get(*ordinal).ok_or_else(|| Error::Malformed {
            what: "protected values",
            detail: format!("no plaintext for ordinal {ordinal}"),
        })?;
        if value.is_empty() {
            // Empty values consume no keystream, matching `decrypt_all`.
            e.set_text("");
        } else {
            let mut buf = Zeroizing::new(value.to_vec());
            stream.apply(&mut buf);
            e.set_text(&base64_encode(&buf));
        }
        *ordinal += 1;
        return Ok(());
    }
    for child in e.children.iter_mut() {
        if let xml::Node::Element(child) = child {
            reprotect_element(child, plain, stream, ordinal)?;
        }
    }
    Ok(())
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

pub(crate) fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrips_against_the_decoder() {
        for len in 0..64 {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let encoded = base64_encode(&data);
            assert_eq!(encoded.len() % 4, 0, "len {len} must be padded");
            let decoded = crate::protected::base64_decode_for_test(&encoded)
                .unwrap_or_else(|| panic!("failed to decode {encoded:?}"));
            assert_eq!(decoded, data, "len {len}");
        }
    }

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
