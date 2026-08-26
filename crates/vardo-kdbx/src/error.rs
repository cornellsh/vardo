use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("not a KDBX file (bad signature {0:#010x}/{1:#010x})")]
    BadSignature(u32, u32),

    #[error("unsupported KDBX version {major}.{minor}")]
    UnsupportedVersion { major: u16, minor: u16 },

    #[error("truncated input: wanted {wanted} bytes at offset {offset}, had {available}")]
    Truncated {
        offset: usize,
        wanted: usize,
        available: usize,
    },

    #[error("malformed {what}: {detail}")]
    Malformed { what: &'static str, detail: String },

    #[error("unknown cipher UUID {0}")]
    UnknownCipher(String),

    #[error("unknown KDF UUID {0}")]
    UnknownKdf(String),

    #[error("required header field {0} missing")]
    MissingHeaderField(&'static str),

    /// Header HMAC mismatch. Either the password/key is wrong, or the header
    /// was tampered with. KDBX cannot distinguish these two cases, by design.
    #[error("header authentication failed: wrong credentials or corrupt file")]
    HeaderAuthFailed,

    #[error("block {index} failed authentication (corrupt or truncated file)")]
    BlockAuthFailed { index: u64 },

    #[error("key derivation failed: {0}")]
    Kdf(String),

    #[error("decompression failed: {0}")]
    Decompress(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
