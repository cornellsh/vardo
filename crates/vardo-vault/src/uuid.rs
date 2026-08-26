//! KDBX UUIDs.
//!
//! Stored as base64 of 16 raw bytes, not as the hyphenated text form. They are
//! the identity that merge matches entries on across devices, so parsing must
//! be exact — a UUID that silently becomes zero would make two unrelated
//! entries look like the same one and merge them together.

use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uuid([u8; 16]);

impl Uuid {
    pub const NIL: Uuid = Uuid([0; 16]);

    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// True for the all-zero UUID, which KDBX uses to mean "unset" for
    /// optional references such as `RecycleBinUUID`.
    pub fn is_nil(&self) -> bool {
        self.0 == [0; 16]
    }

    /// Parses the base64 form KDBX stores.
    ///
    /// Strict about length: 16 bytes exactly. Anything else is a different
    /// kind of value that we should not silently reinterpret.
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = vardo_kdbx::base64::decode(text.trim())?;
        Some(Self(<[u8; 16]>::try_from(bytes.as_slice()).ok()?))
    }

    pub fn to_base64(self) -> String {
        vardo_kdbx::base64::encode(&self.0)
    }
}

/// Hyphenated form, for logs and the UI. Never written to a KDBX file.
impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let h = |r: std::ops::Range<usize>| -> String {
            self.0[r].iter().map(|b| format!("{b:02x}")).collect()
        };
        write!(
            f,
            "{}-{}-{}-{}-{}",
            h(0..4),
            h(4..6),
            h(6..8),
            h(8..10),
            h(10..16)
        )
    }
}

impl fmt::Debug for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Uuid({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_base64() {
        let u = Uuid::from_bytes([
            0x17, 0x81, 0x93, 0x0d, 0x6f, 0xf7, 0xf9, 0x4b, 0xbe, 0xb1, 0x1c, 0x23, 0x5f, 0xaa,
            0x1d, 0xf0,
        ]);
        let text = u.to_base64();
        assert_eq!(Uuid::parse(&text).unwrap(), u);
    }

    #[test]
    fn parses_a_real_fixture_value() {
        // Lifted from test_db_kdbx41_with_password_aes.kdbx.
        let u = Uuid::parse("F4GTDW/3+Uu+sRwjX6od8A==").unwrap();
        assert_eq!(u.to_base64(), "F4GTDW/3+Uu+sRwjX6od8A==");
        assert!(!u.is_nil());
    }

    #[test]
    fn nil_uuid_is_recognised() {
        let u = Uuid::parse("AAAAAAAAAAAAAAAAAAAAAA==").unwrap();
        assert!(u.is_nil());
        assert_eq!(u, Uuid::NIL);
    }

    #[test]
    fn rejects_wrong_lengths_rather_than_padding_them() {
        // Silently zero-extending would make distinct entries collide during
        // merge, which is worse than refusing to read the field.
        assert!(Uuid::parse("Zm9v").is_none()); // 3 bytes
        assert!(Uuid::parse("").is_none()); // empty
        assert!(Uuid::parse("not base64!").is_none());
        // 17 bytes
        assert!(Uuid::parse(&vardo_kdbx::base64::encode(&[0u8; 17])).is_none());
    }

    #[test]
    fn display_is_the_conventional_hyphenated_form() {
        let u = Uuid::from_bytes([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
            0xcd, 0xef,
        ]);
        assert_eq!(u.to_string(), "01234567-89ab-cdef-0123-456789abcdef");
    }
}
