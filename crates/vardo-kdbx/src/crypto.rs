//! Key derivation, header/block authentication, and payload decryption.
//!
//! All primitives come from the RustCrypto family. We implement the KDBX
//! *composition* of them, not the primitives themselves.

use aes::cipher::{BlockDecryptMut, BlockEncrypt, KeyInit, KeyIvInit};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha512};
use zeroize::{Zeroize, Zeroizing};

use crate::cursor::Cursor;
use crate::error::{Error, Result};
use crate::header::{self, VariantDictionary};
use crate::limits::Limits;

type HmacSha256 = Hmac<Sha256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

/// The index KDBX uses for the header's own HMAC block.
const HEADER_BLOCK_INDEX: u64 = u64::MAX;

/// Credentials that compose into the KDBX composite key.
///
/// Password-only for now by design; key files and challenge-response slot in
/// here later without changing anything downstream.
#[derive(Default)]
pub struct Credentials {
    password: Option<Zeroizing<Vec<u8>>>,
}

impl Credentials {
    pub fn with_password(password: &str) -> Self {
        Self {
            password: Some(Zeroizing::new(password.as_bytes().to_vec())),
        }
    }

    /// composite = SHA256( SHA256(password) || <keyfile> || ... )
    ///
    /// Each credential contributes a fixed 32-byte digest, concatenated in a
    /// fixed order, then hashed once more.
    pub fn composite_key(&self) -> Zeroizing<[u8; 32]> {
        let mut outer = Sha256::new();
        if let Some(pw) = &self.password {
            let mut inner = Sha256::digest(pw.as_slice());
            outer.update(inner);
            inner.zeroize();
        }
        Zeroizing::new(outer.finalize().into())
    }
}

/// Everything derived from the composite key plus the master seed.
pub struct MasterKeys {
    pub cipher_key: Zeroizing<[u8; 32]>,
    pub hmac_key: Zeroizing<[u8; 64]>,
}

impl MasterKeys {
    pub fn derive(
        creds: &Credentials,
        master_seed: &[u8],
        kdf_params: &VariantDictionary,
        limits: &Limits,
    ) -> Result<Self> {
        let composite = creds.composite_key();
        let transformed = transform_key(&composite, kdf_params, limits)?;

        // cipher key = SHA256(masterSeed || transformedKey)
        let mut h = Sha256::new();
        h.update(master_seed);
        h.update(transformed.as_slice());
        let cipher_key: [u8; 32] = h.finalize().into();

        // hmac key = SHA512(masterSeed || transformedKey || 0x01)
        let mut h = Sha512::new();
        h.update(master_seed);
        h.update(transformed.as_slice());
        h.update([0x01u8]);
        let hmac_key: [u8; 64] = h.finalize().into();

        Ok(Self {
            cipher_key: Zeroizing::new(cipher_key),
            hmac_key: Zeroizing::new(hmac_key),
        })
    }

    /// Per-block HMAC key = SHA512(index_le || hmacKey).
    fn block_key(&self, index: u64) -> Zeroizing<[u8; 64]> {
        let mut h = Sha512::new();
        h.update(index.to_le_bytes());
        h.update(self.hmac_key.as_slice());
        Zeroizing::new(h.finalize().into())
    }

    /// Computes the header HMAC that authenticates `header_raw`.
    pub fn header_hmac(&self, header_raw: &[u8]) -> [u8; 32] {
        let key = self.block_key(HEADER_BLOCK_INDEX);
        let mut mac =
            <HmacSha256 as Mac>::new_from_slice(key.as_slice()).expect("HMAC accepts any key len");
        mac.update(header_raw);
        mac.finalize().into_bytes().into()
    }

    pub fn verify_header_hmac(&self, header_raw: &[u8], expected: &[u8]) -> Result<()> {
        let key = self.block_key(HEADER_BLOCK_INDEX);
        let mut mac =
            <HmacSha256 as Mac>::new_from_slice(key.as_slice()).expect("HMAC accepts any key len");
        mac.update(header_raw);
        // Constant-time comparison; `verify_slice` is the whole reason we use
        // the mac API rather than comparing digests by hand.
        mac.verify_slice(expected)
            .map_err(|_| Error::HeaderAuthFailed)
    }
}

/// Runs the KDF.
///
/// Every parameter here is attacker-controlled and, by the structure of the
/// format, cannot be authenticated before use — see [`crate::limits`]. The
/// bounds checks below are load-bearing security controls, not tidiness.
fn transform_key(
    composite: &[u8; 32],
    params: &VariantDictionary,
    limits: &Limits,
) -> Result<Zeroizing<Vec<u8>>> {
    let uuid = params
        .get_bytes("$UUID")
        .ok_or(Error::MissingHeaderField("KdfParameters.$UUID"))?;

    if uuid == header::KDF_ARGON2D || uuid == header::KDF_ARGON2ID {
        let algorithm = if uuid == header::KDF_ARGON2D {
            argon2::Algorithm::Argon2d
        } else {
            argon2::Algorithm::Argon2id
        };
        let salt = params
            .get_bytes("S")
            .ok_or(Error::MissingHeaderField("KdfParameters.S"))?;
        let iterations = params
            .get_u64("I")
            .ok_or(Error::MissingHeaderField("KdfParameters.I"))?;
        // KDBX stores memory in BYTES; argon2 wants KiB.
        let memory_bytes = params
            .get_u64("M")
            .ok_or(Error::MissingHeaderField("KdfParameters.M"))?;
        let parallelism = params
            .get_u64("P")
            .ok_or(Error::MissingHeaderField("KdfParameters.P"))?;
        let version = match params.get_u64("V").unwrap_or(0x13) {
            0x10 => argon2::Version::V0x10,
            0x13 => argon2::Version::V0x13,
            other => return Err(Error::Kdf(format!("unsupported Argon2 version {other:#x}"))),
        };

        limits.check("Argon2 memory", memory_bytes, limits.max_kdf_memory)?;
        limits.check("Argon2 iterations", iterations, limits.max_kdf_iterations)?;
        limits.check("Argon2 parallelism", parallelism, limits.max_kdf_parallelism)?;

        let params = argon2::Params::new(
            u32::try_from(memory_bytes / 1024)
                .map_err(|_| Error::Kdf("memory too large".into()))?,
            u32::try_from(iterations).map_err(|_| Error::Kdf("iterations too large".into()))?,
            u32::try_from(parallelism).map_err(|_| Error::Kdf("parallelism too large".into()))?,
            Some(32),
        )
        .map_err(|e| Error::Kdf(format!("invalid Argon2 parameters: {e}")))?;

        let mut out = Zeroizing::new(vec![0u8; 32]);
        argon2::Argon2::new(algorithm, version, params)
            .hash_password_into(composite, salt, &mut out)
            .map_err(|e| Error::Kdf(format!("Argon2 failed: {e}")))?;
        return Ok(out);
    }

    if uuid == header::KDF_AES {
        let seed = params
            .get_bytes("S")
            .ok_or(Error::MissingHeaderField("KdfParameters.S"))?;
        let rounds = params
            .get_u64("R")
            .ok_or(Error::MissingHeaderField("KdfParameters.R"))?;
        limits.check("AES-KDF rounds", rounds, limits.max_aes_kdf_rounds)?;
        return aes_kdf(composite, seed, rounds);
    }

    Err(Error::UnknownKdf(hex(uuid)))
}

/// AES-KDF: ECB-encrypt the composite key `rounds` times, then SHA256 it.
fn aes_kdf(composite: &[u8; 32], seed: &[u8], rounds: u64) -> Result<Zeroizing<Vec<u8>>> {
    let key: [u8; 32] = seed.try_into().map_err(|_| Error::Malformed {
        what: "AES-KDF seed",
        detail: format!("expected 32 bytes, got {}", seed.len()),
    })?;
    let cipher = aes::Aes256::new(&key.into());

    let mut buf = Zeroizing::new(*composite);
    for _ in 0..rounds {
        // Two independent 16-byte ECB blocks, as KeePass specifies.
        let (a, b) = buf.split_at_mut(16);
        cipher.encrypt_block(aes::cipher::generic_array::GenericArray::from_mut_slice(a));
        cipher.encrypt_block(aes::cipher::generic_array::GenericArray::from_mut_slice(b));
    }
    Ok(Zeroizing::new(Sha256::digest(buf.as_slice()).to_vec()))
}

/// Reads the HMAC-authenticated block stream that follows the outer header.
///
/// Layout per block: `[32-byte HMAC][u32 length][length bytes]`, terminated by
/// a zero-length block. Each block is authenticated *before* decryption, so a
/// corrupt or truncated file fails loudly rather than producing garbage.
pub fn read_hmac_block_stream(keys: &MasterKeys, buf: &[u8]) -> Result<Vec<u8>> {
    let mut c = Cursor::new(buf);
    let mut out = Vec::new();
    let mut index: u64 = 0;

    loop {
        let mac_expected = c.take(32)?;
        let len = c.u32()?;
        let data = c.take(len as usize)?;

        let key = keys.block_key(index);
        let mut mac =
            <HmacSha256 as Mac>::new_from_slice(key.as_slice()).expect("HMAC accepts any key len");
        mac.update(&index.to_le_bytes());
        mac.update(&len.to_le_bytes());
        mac.update(data);
        mac.verify_slice(mac_expected)
            .map_err(|_| Error::BlockAuthFailed { index })?;

        if len == 0 {
            break;
        }
        out.extend_from_slice(data);
        index += 1;
    }
    Ok(out)
}

/// Splits `data` into HMAC-authenticated blocks and appends them to `out`.
///
/// Block size matches KeePass's 1 MiB so that files we write have the same
/// framing shape as files it writes.
pub fn write_hmac_block_stream(keys: &MasterKeys, data: &[u8], out: &mut Vec<u8>) {
    const BLOCK: usize = 1024 * 1024;

    let mut index: u64 = 0;
    let emit = |chunk: &[u8], index: u64, out: &mut Vec<u8>| {
        let key = keys.block_key(index);
        let mut mac =
            <HmacSha256 as Mac>::new_from_slice(key.as_slice()).expect("HMAC accepts any key len");
        let len = chunk.len() as u32;
        mac.update(&index.to_le_bytes());
        mac.update(&len.to_le_bytes());
        mac.update(chunk);
        out.extend_from_slice(&mac.finalize().into_bytes());
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(chunk);
    };

    for chunk in data.chunks(BLOCK) {
        emit(chunk, index, out);
        index += 1;
    }
    // Zero-length terminator block.
    emit(&[], index, out);
}

pub fn encrypt(cipher_id: [u8; 16], key: &[u8; 32], iv: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    match cipher_id {
        header::CIPHER_AES256_CBC => {
            use aes::cipher::BlockEncryptMut;
            let iv: [u8; 16] = iv.try_into().map_err(|_| Error::Malformed {
                what: "EncryptionIV",
                detail: format!("AES-CBC needs a 16-byte IV, got {}", iv.len()),
            })?;
            // PKCS#7 always adds at least one byte, so the buffer must have a
            // full spare block for the padded output.
            let mut buf = vec![0u8; data.len() + 16];
            buf[..data.len()].copy_from_slice(data);
            let n = cbc::Encryptor::<aes::Aes256>::new(key.into(), &iv.into())
                .encrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(&mut buf, data.len())
                .map_err(|e| Error::Malformed {
                    what: "AES-CBC payload",
                    detail: format!("padding failed: {e}"),
                })?
                .len();
            buf.truncate(n);
            Ok(buf)
        }
        // ChaCha20 is a stream cipher, so encryption and decryption are the
        // same operation.
        header::CIPHER_CHACHA20 => decrypt(cipher_id, key, iv, data),
        header::CIPHER_TWOFISH => Err(Error::UnknownCipher(
            "Twofish (not yet implemented)".to_string(),
        )),
        other => Err(Error::UnknownCipher(hex(&other))),
    }
}

pub fn decrypt(cipher_id: [u8; 16], key: &[u8; 32], iv: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    match cipher_id {
        header::CIPHER_AES256_CBC => {
            let iv: [u8; 16] = iv.try_into().map_err(|_| Error::Malformed {
                what: "EncryptionIV",
                detail: format!("AES-CBC needs a 16-byte IV, got {}", iv.len()),
            })?;
            let mut buf = data.to_vec();
            let plain = Aes256CbcDec::new(key.into(), &iv.into())
                .decrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(&mut buf)
                .map_err(|e| Error::Malformed {
                    what: "AES-CBC payload",
                    detail: format!("invalid padding: {e}"),
                })?;
            Ok(plain.to_vec())
        }
        header::CIPHER_CHACHA20 => {
            use chacha20::cipher::StreamCipher;
            let iv: [u8; 12] = iv.try_into().map_err(|_| Error::Malformed {
                what: "EncryptionIV",
                detail: format!("ChaCha20 needs a 12-byte nonce, got {}", iv.len()),
            })?;
            let mut buf = data.to_vec();
            chacha20::ChaCha20::new(key.into(), &iv.into()).apply_keystream(&mut buf);
            Ok(buf)
        }
        header::CIPHER_TWOFISH => Err(Error::UnknownCipher(
            "Twofish (not yet implemented)".to_string(),
        )),
        other => Err(Error::UnknownCipher(hex(&other))),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
