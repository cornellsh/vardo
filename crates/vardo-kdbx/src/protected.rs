//! Protected values: the inner stream cipher over `Protected="True"` fields.
//!
//! KDBX encrypts passwords a second time, *inside* the already-encrypted
//! payload, with a single keystream shared across the whole document. Each
//! protected value consumes keystream in **document order**, so the values are
//! positionally coupled: decrypt one out of order and every subsequent value is
//! garbage. This is the single most fragile invariant in the format.
//!
//! # Why decryption does not mutate the DOM
//!
//! Rewriting plaintext back into the tree would destroy byte-stability, and
//! re-encrypting would need a fresh keystream for the whole document. Instead
//! the DOM stays pristine and plaintexts are handed back in an ordinal-indexed
//! table. Callers hold an ordinal; the bytes on disk never move.
//!
//! Note for the merge engine: protected ciphertext **is expected to differ**
//! between two saves of identical data, because writers generate a new inner
//! key each time. Merge must compare decrypted plaintext for these fields, and
//! only these fields, or every sync will look like a conflict.

use sha2::{Digest, Sha256, Sha512};
use zeroize::Zeroizing;

use crate::error::{Error, Result};
use crate::header::InnerStreamCipher;
use crate::xml;

/// Salsa20's nonce is a hard-coded constant in the KeePass specification.
const SALSA20_NONCE: [u8; 8] = [0xE8, 0x30, 0x09, 0x4B, 0x97, 0x20, 0x5D, 0x2A];

/// The attribute marking a value as protected, and the value it must have.
const PROTECTED_ATTR: &str = "Protected";

enum Stream {
    ChaCha20(Box<chacha20::ChaCha20>),
    Salsa20(Box<salsa20::Salsa20>),
    /// `InnerRandomStreamID = 0`. Values carry no second layer of encryption.
    Plain,
}

pub struct InnerStream {
    stream: Stream,
}

impl InnerStream {
    pub fn new(cipher: InnerStreamCipher, key: &[u8]) -> Result<Self> {
        use chacha20::cipher::KeyIvInit;

        let stream = match cipher {
            InnerStreamCipher::ChaCha20 => {
                // key = SHA512(innerKey)[0..32], nonce = SHA512(innerKey)[32..44]
                let h = Sha512::digest(key);
                let k: [u8; 32] = h[0..32].try_into().unwrap();
                let n: [u8; 12] = h[32..44].try_into().unwrap();
                Stream::ChaCha20(Box::new(chacha20::ChaCha20::new(&k.into(), &n.into())))
            }
            InnerStreamCipher::Salsa20 => {
                let k: [u8; 32] = Sha256::digest(key).into();
                Stream::Salsa20(Box::new(salsa20::Salsa20::new(
                    &k.into(),
                    &SALSA20_NONCE.into(),
                )))
            }
            InnerStreamCipher::None => Stream::Plain,
            InnerStreamCipher::ArcFourVariant => {
                // Removed from KeePass in 2014 and cryptographically broken.
                // Refusing is safer than implementing RC4 for a file we would
                // then have to re-encrypt with it.
                return Err(Error::Malformed {
                    what: "InnerRandomStreamID",
                    detail: "ARC4 inner stream is obsolete and unsupported".into(),
                });
            }
            InnerStreamCipher::Unknown(id) => {
                return Err(Error::Malformed {
                    what: "InnerRandomStreamID",
                    detail: format!("unknown inner stream cipher {id}"),
                })
            }
        };
        Ok(Self { stream })
    }

    /// XORs `data` with the next bytes of keystream, advancing the position.
    pub fn apply(&mut self, data: &mut [u8]) {
        use chacha20::cipher::StreamCipher;
        match &mut self.stream {
            Stream::ChaCha20(c) => c.apply_keystream(data),
            Stream::Salsa20(c) => c.apply_keystream(data),
            Stream::Plain => {}
        }
    }
}

/// A decrypted protected value, zeroed on drop.
pub type Secret = Zeroizing<Vec<u8>>;

/// Every protected value in the document, in the order the cipher consumed them.
pub struct ProtectedValues {
    values: Vec<Secret>,
}

/// Redacting. These are the vault's passwords in plaintext.
impl std::fmt::Debug for ProtectedValues {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProtectedValues({} values redacted)", self.values.len())
    }
}

impl ProtectedValues {
    pub fn get(&self, ordinal: usize) -> Option<&Secret> {
        self.values.get(ordinal)
    }

    /// Interprets a value as UTF-8. Invalid sequences are replaced rather than
    /// erroring: a corrupt password should still be *visible* to the user so
    /// they can fix it, not hidden behind a parse failure.
    pub fn get_str(&self, ordinal: usize) -> Option<Zeroizing<String>> {
        self.get(ordinal)
            .map(|v| Zeroizing::new(String::from_utf8_lossy(v).into_owned()))
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// Returns true if this element carries `Protected="True"` (case-insensitively,
/// because KeePass and KeePassXC have historically disagreed on casing).
pub fn is_protected(e: &xml::Element) -> bool {
    e.attr(PROTECTED_ATTR)
        .is_some_and(|a| a.value().eq_ignore_ascii_case("true"))
}

/// Assigns each protected element an ordinal, in document order.
///
/// The ordinal is the index into [`ProtectedValues`]. Because the keystream is
/// positional, this traversal order *is* the decryption key — it must match
/// what the writer used, which is a plain depth-first walk.
pub fn protected_ordinals(doc: &xml::Document) -> Vec<&xml::Element> {
    doc.root.descendants().filter(|e| is_protected(e)).collect()
}

/// Decrypts every protected value in the document.
pub fn decrypt_all(
    doc: &xml::Document,
    cipher: InnerStreamCipher,
    key: &[u8],
) -> Result<ProtectedValues> {
    let mut stream = InnerStream::new(cipher, key)?;
    let mut values = Vec::new();

    for (ordinal, element) in protected_ordinals(doc).into_iter().enumerate() {
        let text = element.text();
        // An empty protected value is legal and consumes no keystream.
        if text.trim().is_empty() {
            values.push(Zeroizing::new(Vec::new()));
            continue;
        }
        let mut raw = Zeroizing::new(base64_decode(text.trim()).ok_or_else(|| {
            Error::Malformed {
                what: "protected value",
                detail: format!(
                    "<{}> at ordinal {ordinal} is not valid base64",
                    element.name
                ),
            }
        })?);
        stream.apply(&mut raw);
        values.push(raw);
    }

    Ok(ProtectedValues { values })
}

/// Standard base64 with padding. Returns `None` on any invalid input.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const INVALID: u8 = 0xFF;
    let mut table = [INVALID; 256];
    let mut i = 0u8;
    for c in b'A'..=b'Z' {
        table[c as usize] = i;
        i += 1;
    }
    for c in b'a'..=b'z' {
        table[c as usize] = i;
        i += 1;
    }
    for c in b'0'..=b'9' {
        table[c as usize] = i;
        i += 1;
    }
    table[b'+' as usize] = 62;
    table[b'/' as usize] = 63;

    let bytes: Vec<u8> = s
        .bytes()
        .filter(|b| !matches!(b, b'\n' | b'\r' | b' ' | b'\t'))
        .collect();
    if bytes.len() % 4 != 0 {
        return None;
    }

    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let last_chunk = bytes.len() / 4;
    for (ci, chunk) in bytes.chunks(4).enumerate() {
        let pad = chunk.iter().filter(|&&b| b == b'=').count();
        // Padding is only ever the final one or two characters of the final
        // chunk. Interior padding means two concatenated base64 strings, which
        // some decoders silently accept and thereby lose data.
        if pad > 0 && ci + 1 != last_chunk {
            return None;
        }
        if pad > 2 || (pad > 0 && chunk[4 - pad..].iter().any(|&b| b != b'=')) {
            return None;
        }
        let mut acc = 0u32;
        for (j, &b) in chunk.iter().enumerate() {
            let v = if b == b'=' {
                0
            } else {
                let v = table[b as usize];
                if v == INVALID {
                    return None;
                }
                v
            };
            acc |= (v as u32) << (18 - 6 * j);
        }
        out.push((acc >> 16) as u8);
        if pad < 2 {
            out.push((acc >> 8) as u8);
        }
        if pad < 1 {
            out.push(acc as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_reference_vectors() {
        assert_eq!(base64_decode("").unwrap(), b"");
        assert_eq!(base64_decode("Zg==").unwrap(), b"f");
        assert_eq!(base64_decode("Zm8=").unwrap(), b"fo");
        assert_eq!(base64_decode("Zm9v").unwrap(), b"foo");
        assert_eq!(base64_decode("Zm9vYg==").unwrap(), b"foob");
        assert_eq!(base64_decode("Zm9vYmE=").unwrap(), b"fooba");
        assert_eq!(base64_decode("Zm9vYmFy").unwrap(), b"foobar");
        // Whitespace inside base64 is tolerated; some writers wrap lines.
        assert_eq!(base64_decode("Zm9v\nYmFy").unwrap(), b"foobar");
    }

    #[test]
    fn base64_rejects_malformed_input() {
        assert!(base64_decode("Zg=").is_none()); // wrong length
        assert!(base64_decode("Zm9!").is_none()); // invalid char
        assert!(base64_decode("=Zm9").is_none()); // padding not at end
        assert!(base64_decode("Z===").is_none()); // too much padding
        assert!(base64_decode("Zm==Zm9v").is_none()); // interior padding
    }

    #[test]
    fn arc4_and_unknown_streams_are_refused() {
        assert!(InnerStream::new(InnerStreamCipher::ArcFourVariant, b"k").is_err());
        assert!(InnerStream::new(InnerStreamCipher::Unknown(99), b"k").is_err());
    }

    #[test]
    fn plain_stream_is_identity() {
        let mut s = InnerStream::new(InnerStreamCipher::None, b"k").unwrap();
        let mut buf = *b"unchanged";
        s.apply(&mut buf);
        assert_eq!(&buf, b"unchanged");
    }

    #[test]
    fn keystream_is_positional() {
        // Two values decrypted in sequence must not be independently decodable;
        // this is the invariant that makes document order load-bearing.
        let key = b"some inner key";
        let mut a = InnerStream::new(InnerStreamCipher::ChaCha20, key).unwrap();
        let mut first = [0u8; 8];
        let mut second = [0u8; 8];
        a.apply(&mut first);
        a.apply(&mut second);
        assert_ne!(first, second, "keystream must advance between values");

        let mut b = InnerStream::new(InnerStreamCipher::ChaCha20, key).unwrap();
        let mut restart = [0u8; 8];
        b.apply(&mut restart);
        assert_eq!(first, restart, "a fresh stream must reproduce position 0");
    }
}
