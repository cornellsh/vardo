//! Outer/inner header parsing.
//!
//! Every field is retained, including ones we do not interpret. Losslessness is
//! a property of the design, not a feature we maintain: an element we never
//! understood is an element we never destroyed.

use crate::cursor::Cursor;
use crate::error::{Error, Result};

pub const SIG1: u32 = 0x9AA2_D903;
pub const SIG2: u32 = 0xB54B_FB67;

pub const CIPHER_AES256_CBC: [u8; 16] = [
    0x31, 0xc1, 0xf2, 0xe6, 0xbf, 0x71, 0x43, 0x50, 0xbe, 0x58, 0x05, 0x21, 0x6a, 0xfc, 0x5a, 0xff,
];
pub const CIPHER_CHACHA20: [u8; 16] = [
    0xd6, 0x03, 0x8a, 0x2b, 0x8b, 0x6f, 0x4c, 0xb5, 0xa5, 0x24, 0x33, 0x9a, 0x31, 0xdb, 0xb5, 0x9a,
];
pub const CIPHER_TWOFISH: [u8; 16] = [
    0xad, 0x68, 0xf2, 0x9f, 0x57, 0x6f, 0x4b, 0xb9, 0xa3, 0x6a, 0xd4, 0x7a, 0xf9, 0x65, 0x34, 0x6c,
];

pub const KDF_AES: [u8; 16] = [
    0xc9, 0xd9, 0xf3, 0x9a, 0x62, 0x8a, 0x44, 0x60, 0xbf, 0x74, 0x0d, 0x08, 0xc1, 0x8a, 0x4f, 0xea,
];
pub const KDF_ARGON2D: [u8; 16] = [
    0xef, 0x63, 0x6d, 0xdf, 0x8c, 0x29, 0x44, 0x4b, 0x91, 0xf7, 0xa9, 0xa4, 0x03, 0xe3, 0x0a, 0x0c,
];
pub const KDF_ARGON2ID: [u8; 16] = [
    0x9e, 0x29, 0x8b, 0x19, 0x56, 0xdb, 0x47, 0x73, 0xb2, 0x3d, 0xfc, 0x3e, 0xc6, 0xf0, 0xa1, 0xe6,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
}

impl Version {
    pub fn is_kdbx4(self) -> bool {
        self.major == 4
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

// ---------------------------------------------------------------------------
// VariantDictionary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum VdValue {
    U32(u32),
    U64(u64),
    Bool(bool),
    I32(i32),
    I64(i64),
    Str(String),
    Bytes(Vec<u8>),
}

/// Ordered key/value map. Order is preserved because we have to write it back
/// byte-identically when the KDF parameters are untouched.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VariantDictionary {
    pub version: u16,
    pub items: Vec<(String, VdValue)>,
}

impl VariantDictionary {
    pub fn get(&self, key: &str) -> Option<&VdValue> {
        self.items.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_bytes(&self, key: &str) -> Option<&[u8]> {
        match self.get(key)? {
            VdValue::Bytes(b) => Some(b),
            _ => None,
        }
    }

    /// KeePass writes some integers as U32 and some as U64 depending on the
    /// writer; accept either rather than being pedantic about it.
    pub fn get_u64(&self, key: &str) -> Option<u64> {
        match self.get(key)? {
            VdValue::U64(v) => Some(*v),
            VdValue::U32(v) => Some(*v as u64),
            VdValue::I64(v) if *v >= 0 => Some(*v as u64),
            VdValue::I32(v) if *v >= 0 => Some(*v as u64),
            _ => None,
        }
    }

    pub fn parse(buf: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(buf);
        let version = c.u16()?;
        if version >> 8 > 1 {
            return Err(Error::Malformed {
                what: "VariantDictionary",
                detail: format!("unsupported major version {}", version >> 8),
            });
        }
        let mut items = Vec::new();
        loop {
            let ty = c.u8()?;
            if ty == 0 {
                break;
            }
            let klen = c.u32()? as usize;
            let key = String::from_utf8(c.take(klen)?.to_vec()).map_err(|e| Error::Malformed {
                what: "VariantDictionary",
                detail: format!("key is not UTF-8: {e}"),
            })?;
            let vlen = c.u32()? as usize;
            let raw = c.take(vlen)?;
            let val = decode_vd_value(ty, raw, &key)?;
            items.push((key, val));
        }
        Ok(Self { version, items })
    }
}

fn decode_vd_value(ty: u8, raw: &[u8], key: &str) -> Result<VdValue> {
    let bad = |detail: String| Error::Malformed {
        what: "VariantDictionary",
        detail,
    };
    let fixed = |want: usize| -> Result<()> {
        if raw.len() != want {
            Err(bad(format!(
                "key {key:?}: expected {want} bytes for type {ty:#04x}, got {}",
                raw.len()
            )))
        } else {
            Ok(())
        }
    };
    Ok(match ty {
        0x04 => {
            fixed(4)?;
            VdValue::U32(u32::from_le_bytes(raw.try_into().unwrap()))
        }
        0x05 => {
            fixed(8)?;
            VdValue::U64(u64::from_le_bytes(raw.try_into().unwrap()))
        }
        0x08 => {
            fixed(1)?;
            VdValue::Bool(raw[0] != 0)
        }
        0x0C => {
            fixed(4)?;
            VdValue::I32(i32::from_le_bytes(raw.try_into().unwrap()))
        }
        0x0D => {
            fixed(8)?;
            VdValue::I64(i64::from_le_bytes(raw.try_into().unwrap()))
        }
        0x18 => VdValue::Str(
            String::from_utf8(raw.to_vec())
                .map_err(|e| bad(format!("key {key:?}: value is not UTF-8: {e}")))?,
        ),
        0x42 => VdValue::Bytes(raw.to_vec()),
        other => return Err(bad(format!("key {key:?}: unknown value type {other:#04x}"))),
    })
}

// ---------------------------------------------------------------------------
// Outer header
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct OuterField {
    pub id: u8,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct OuterHeader {
    pub version: Version,
    /// Every field in file order, including unrecognised ids.
    pub fields: Vec<OuterField>,
    /// Exact bytes from offset 0 through the end-of-header marker. This is the
    /// HMAC input, so it must be the literal bytes, never a re-serialisation.
    pub raw: Vec<u8>,
}

impl OuterHeader {
    pub fn field(&self, id: u8) -> Option<&[u8]> {
        self.fields
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.data.as_slice())
    }

    pub fn cipher(&self) -> Result<[u8; 16]> {
        let raw = self.field(2).ok_or(Error::MissingHeaderField("CipherID"))?;
        raw.try_into().map_err(|_| Error::Malformed {
            what: "CipherID",
            detail: format!("expected 16 bytes, got {}", raw.len()),
        })
    }

    pub fn compressed(&self) -> Result<bool> {
        let raw = self
            .field(3)
            .ok_or(Error::MissingHeaderField("CompressionFlags"))?;
        if raw.len() != 4 {
            return Err(Error::Malformed {
                what: "CompressionFlags",
                detail: format!("expected 4 bytes, got {}", raw.len()),
            });
        }
        Ok(u32::from_le_bytes(raw.try_into().unwrap()) == 1)
    }

    pub fn master_seed(&self) -> Result<&[u8]> {
        self.field(4).ok_or(Error::MissingHeaderField("MasterSeed"))
    }

    pub fn encryption_iv(&self) -> Result<&[u8]> {
        self.field(7)
            .ok_or(Error::MissingHeaderField("EncryptionIV"))
    }

    pub fn kdf_parameters(&self) -> Result<VariantDictionary> {
        let raw = self
            .field(11)
            .ok_or(Error::MissingHeaderField("KdfParameters"))?;
        VariantDictionary::parse(raw)
    }

    pub fn parse(buf: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(buf);
        let s1 = c.u32()?;
        let s2 = c.u32()?;
        if s1 != SIG1 || s2 != SIG2 {
            return Err(Error::BadSignature(s1, s2));
        }
        let minor = c.u16()?;
        let major = c.u16()?;
        let version = Version { major, minor };
        if major != 3 && major != 4 {
            return Err(Error::UnsupportedVersion { major, minor });
        }

        let mut fields: Vec<OuterField> = Vec::new();
        loop {
            let id = c.u8()?;
            // KDBX3 uses a u16 length; KDBX4 widened it to u32.
            let len = if major >= 4 {
                c.u32()? as usize
            } else {
                c.u16()? as usize
            };
            let data = c.take(len)?.to_vec();
            let end = id == 0;

            // Repeated fields are a parser-differential hazard: one reader
            // takes the first, another the last, and a file can then mean two
            // different things. No legitimate writer emits them.
            if !end && fields.iter().any(|f| f.id == id) {
                return Err(Error::Malformed {
                    what: "outer header",
                    detail: format!("duplicate header field id {id}"),
                });
            }

            fields.push(OuterField { id, data });
            if end {
                break;
            }
        }

        Ok(Self {
            version,
            fields,
            raw: c.consumed().to_vec(),
        })
    }
}

// ---------------------------------------------------------------------------
// Inner header (KDBX4 only)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InnerStreamCipher {
    None,
    ArcFourVariant,
    Salsa20,
    ChaCha20,
    Unknown(i32),
}

impl From<i32> for InnerStreamCipher {
    fn from(v: i32) -> Self {
        match v {
            0 => Self::None,
            1 => Self::ArcFourVariant,
            2 => Self::Salsa20,
            3 => Self::ChaCha20,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Clone)]
pub struct Binary {
    /// Bit 0 set means "protected"; other bits are undefined and preserved.
    pub flags: u8,
    pub data: Vec<u8>,
}

// Attachments are user data — a bank statement, an SSH key. Never render them.
impl std::fmt::Debug for Binary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Binary")
            .field("flags", &self.flags)
            .field("data", &format_args!("<{} bytes redacted>", self.data.len()))
            .finish()
    }
}

#[derive(Clone)]
pub struct InnerHeader {
    pub stream_cipher: InnerStreamCipher,
    pub stream_key: Vec<u8>,
    pub binaries: Vec<Binary>,
    /// Field ids we did not recognise, kept in order for round-tripping.
    pub unknown: Vec<(u8, Vec<u8>)>,
}

/// Redacting. `stream_key` decrypts every password in the vault, so a derived
/// `Debug` would put the entire vault into any log line that formats a header.
impl std::fmt::Debug for InnerHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InnerHeader")
            .field("stream_cipher", &self.stream_cipher)
            .field(
                "stream_key",
                &format_args!("<{} bytes redacted>", self.stream_key.len()),
            )
            .field("binaries", &self.binaries.len())
            .field("unknown", &self.unknown.len())
            .finish()
    }
}

impl InnerHeader {
    /// Parses the inner header and returns it plus the remaining payload,
    /// which is the XML document.
    pub fn parse(buf: &[u8]) -> Result<(Self, &[u8])> {
        let mut c = Cursor::new(buf);
        let mut stream_cipher = None;
        let mut stream_key = None;
        let mut binaries = Vec::new();
        let mut unknown = Vec::new();

        loop {
            let id = c.u8()?;
            // Note: signed in the spec, unlike the outer header's unsigned length.
            let len = c.i32()?;
            if len < 0 {
                return Err(Error::Malformed {
                    what: "inner header",
                    detail: format!("negative field length {len} for id {id}"),
                });
            }
            let data = c.take(len as usize)?;
            match id {
                0 => break,
                1 => {
                    if data.len() != 4 {
                        return Err(Error::Malformed {
                            what: "InnerRandomStreamID",
                            detail: format!("expected 4 bytes, got {}", data.len()),
                        });
                    }
                    stream_cipher = Some(InnerStreamCipher::from(i32::from_le_bytes(
                        data.try_into().unwrap(),
                    )));
                }
                2 => stream_key = Some(data.to_vec()),
                3 => {
                    let (flags, rest) = data.split_first().ok_or(Error::Malformed {
                        what: "Binary",
                        detail: "empty binary record (missing flags byte)".into(),
                    })?;
                    binaries.push(Binary {
                        flags: *flags,
                        data: rest.to_vec(),
                    });
                }
                other => unknown.push((other, data.to_vec())),
            }
        }

        let header = Self {
            stream_cipher: stream_cipher.ok_or(Error::MissingHeaderField("InnerRandomStreamID"))?,
            stream_key: stream_key.ok_or(Error::MissingHeaderField("InnerRandomStreamKey"))?,
            binaries,
            unknown,
        };
        Ok((header, c.rest()))
    }
}
