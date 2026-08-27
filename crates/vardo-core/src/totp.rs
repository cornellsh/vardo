//! One-time codes (RFC 6238 TOTP, RFC 4226 HOTP).
//!
//! # Why this is in Rust and not the WebView
//!
//! The TOTP *seed* is a long-lived shared secret — strictly more valuable than
//! the six-digit code it produces, because it generates every future code. It
//! is stored as a protected field precisely for that reason, and it must not
//! cross into JavaScript any more than a password does. So the seed stays here
//! and only the derived code, which expires in seconds, is handed out.
//!
//! # Where the seed lives
//!
//! KeePass has no standard for this, so clients invented three conventions and
//! all of them are in the wild:
//!
//! - **KeePassXC / KeePassium**: an `otp` field holding an `otpauth://` URI.
//! - **KeeOtp**: an `otp` field holding `key=SECRET&step=30&size=6`.
//! - **KeeWeb / Tray TOTP**: separate `TOTP Seed` and `TOTP Settings` fields,
//!   the latter being `30;6` or `30;S` for Steam.
//!
//! Reading only one of them means silently showing nothing for vaults written
//! by the others, so all three are parsed.

use zeroize::Zeroizing;

/// A parsed one-time-code configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Totp {
    secret: Zeroizing<Vec<u8>>,
    pub digits: u32,
    pub period: u64,
    pub algorithm: Algorithm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    Sha1,
    Sha256,
    Sha512,
}

impl Algorithm {
    fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_uppercase().as_str() {
            "SHA1" => Some(Algorithm::Sha1),
            "SHA256" => Some(Algorithm::Sha256),
            "SHA512" => Some(Algorithm::Sha512),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    pub code: String,
    /// Seconds until this code expires.
    pub seconds_remaining: u64,
    pub period: u64,
}

impl Totp {
    /// Reads whichever convention the entry happens to use.
    ///
    /// `otp` is the combined field; `seed` and `settings` are KeeWeb's split
    /// form. Any of them may be absent.
    pub fn from_fields(
        otp: Option<&str>,
        seed: Option<&str>,
        settings: Option<&str>,
    ) -> Option<Self> {
        if let Some(raw) = otp.map(str::trim).filter(|s| !s.is_empty()) {
            if raw.starts_with("otpauth://") {
                return Self::from_uri(raw);
            }
            return Self::from_keeotp(raw);
        }
        let seed = seed.map(str::trim).filter(|s| !s.is_empty())?;
        Self::from_keeweb(seed, settings)
    }

    /// `otpauth://totp/Label?secret=...&digits=6&period=30&algorithm=SHA1`
    fn from_uri(uri: &str) -> Option<Self> {
        let query = uri.split_once('?')?.1;
        let mut secret = None;
        let mut digits = 6;
        let mut period = 30;
        let mut algorithm = Algorithm::Sha1;

        for pair in query.split('&') {
            let (k, v) = pair.split_once('=')?;
            match k.to_ascii_lowercase().as_str() {
                "secret" => secret = base32_decode(&percent_decode(v)),
                "digits" => digits = percent_decode(v).parse().unwrap_or(6),
                "period" => period = percent_decode(v).parse().unwrap_or(30),
                "algorithm" => {
                    algorithm = Algorithm::parse(&percent_decode(v)).unwrap_or(Algorithm::Sha1)
                }
                _ => {}
            }
        }
        Self::build(secret?, digits, period, algorithm)
    }

    /// `key=SECRET&step=30&size=6`
    fn from_keeotp(raw: &str) -> Option<Self> {
        let mut secret = None;
        let mut digits = 6;
        let mut period = 30;

        for pair in raw.split('&') {
            let (k, v) = pair.split_once('=')?;
            match k.trim().to_ascii_lowercase().as_str() {
                "key" => secret = base32_decode(&percent_decode(v)),
                "size" => digits = v.trim().parse().unwrap_or(6),
                "step" => period = v.trim().parse().unwrap_or(30),
                _ => {}
            }
        }
        Self::build(secret?, digits, period, Algorithm::Sha1)
    }

    /// A bare base32 seed plus `period;digits`, e.g. `30;6`.
    fn from_keeweb(seed: &str, settings: Option<&str>) -> Option<Self> {
        let secret = base32_decode(seed)?;
        let (period, digits) = match settings.map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => {
                let mut parts = s.split(';');
                let period = parts
                    .next()
                    .and_then(|p| p.trim().parse().ok())
                    .unwrap_or(30);
                // "S" means Steam's five-character alphabet, which we do not
                // implement — treat it as the default rather than producing a
                // number that looks right and is not.
                let digits = parts
                    .next()
                    .and_then(|d| d.trim().parse().ok())
                    .unwrap_or(6);
                (period, digits)
            }
            None => (30, 6),
        };
        Self::build(secret, digits, period, Algorithm::Sha1)
    }

    fn build(secret: Vec<u8>, digits: u32, period: u64, algorithm: Algorithm) -> Option<Self> {
        // A zero period would divide by zero; absurd digit counts would panic
        // on the modulus. Refuse rather than produce a wrong code.
        if secret.is_empty() || period == 0 || !(4..=10).contains(&digits) {
            return None;
        }
        Some(Totp {
            secret: Zeroizing::new(secret),
            digits,
            period,
            algorithm,
        })
    }

    /// The code for a given moment.
    pub fn at(&self, unix_seconds: u64) -> Code {
        let counter = unix_seconds / self.period;
        let code = self.hotp(counter);
        Code {
            code,
            seconds_remaining: self.period - (unix_seconds % self.period),
            period: self.period,
        }
    }

    pub fn now(&self) -> Code {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.at(now)
    }

    /// RFC 4226 dynamic truncation.
    fn hotp(&self, counter: u64) -> String {
        let mac = self.hmac(&counter.to_be_bytes());

        // Low four bits of the last byte select where to read from.
        let offset = (mac[mac.len() - 1] & 0x0f) as usize;
        let binary = ((u32::from(mac[offset]) & 0x7f) << 24)
            | (u32::from(mac[offset + 1]) << 16)
            | (u32::from(mac[offset + 2]) << 8)
            | u32::from(mac[offset + 3]);

        let modulus = 10u32.pow(self.digits);
        format!("{:0width$}", binary % modulus, width = self.digits as usize)
    }

    fn hmac(&self, message: &[u8]) -> Vec<u8> {
        use hmac::{Hmac, Mac};
        use sha1::Sha1;
        use sha2::{Sha256, Sha512};

        match self.algorithm {
            Algorithm::Sha1 => {
                let mut m =
                    <Hmac<Sha1> as Mac>::new_from_slice(&self.secret).expect("any key size");
                m.update(message);
                m.finalize().into_bytes().to_vec()
            }
            Algorithm::Sha256 => {
                let mut m =
                    <Hmac<Sha256> as Mac>::new_from_slice(&self.secret).expect("any key size");
                m.update(message);
                m.finalize().into_bytes().to_vec()
            }
            Algorithm::Sha512 => {
                let mut m =
                    <Hmac<Sha512> as Mac>::new_from_slice(&self.secret).expect("any key size");
                m.update(message);
                m.finalize().into_bytes().to_vec()
            }
        }
    }
}

/// RFC 4648 base32, upper-case, padding optional.
///
/// Authenticator secrets are routinely written with spaces and lower case, and
/// refusing those would reject seeds every other client accepts.
fn base32_decode(text: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

    let mut bits = 0u32;
    let mut count = 0u32;
    let mut out = Vec::new();

    for c in text.bytes() {
        if c == b'=' || c == b' ' || c == b'-' || c == b'\t' {
            continue;
        }
        let upper = c.to_ascii_uppercase();
        let value = ALPHABET.iter().position(|a| *a == upper)? as u32;
        bits = (bits << 5) | value;
        count += 5;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    (!out.is_empty()).then_some(out)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(
                    std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                    16,
                ) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The RFC 6238 test seed: ASCII "12345678901234567890".
    const RFC_SECRET_B32: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    fn rfc(algorithm: Algorithm, digits: u32) -> Totp {
        Totp::build(
            base32_decode(RFC_SECRET_B32).unwrap(),
            digits,
            30,
            algorithm,
        )
        .unwrap()
    }

    #[test]
    fn matches_the_rfc6238_sha1_vectors() {
        // Appendix B. If these drift, every code the app shows is wrong, and
        // wrong in a way that looks like the server's fault.
        let t = rfc(Algorithm::Sha1, 8);
        for (time, expected) in [
            (59u64, "94287082"),
            (1_111_111_109, "07081804"),
            (1_111_111_111, "14050471"),
            (1_234_567_890, "89005924"),
            (2_000_000_000, "69279037"),
            (20_000_000_000, "65353130"),
        ] {
            assert_eq!(t.at(time).code, expected, "at t={time}");
        }
    }

    #[test]
    fn matches_the_rfc6238_sha256_vectors() {
        // The RFC uses a longer seed for SHA-256; this is that seed, which is
        // the 20-byte one extended to 32 bytes by repetition.
        let secret = b"12345678901234567890123456789012".to_vec();
        let t = Totp::build(secret, 8, 30, Algorithm::Sha256).unwrap();
        assert_eq!(t.at(59).code, "46119246");
        assert_eq!(t.at(1_111_111_109).code, "68084774");
    }

    #[test]
    fn matches_the_rfc6238_sha512_vectors() {
        let secret = b"1234567890123456789012345678901234567890123456789012345678901234".to_vec();
        let t = Totp::build(secret, 8, 30, Algorithm::Sha512).unwrap();
        assert_eq!(t.at(59).code, "90693936");
        assert_eq!(t.at(1_111_111_109).code, "25091201");
    }

    #[test]
    fn six_digit_codes_are_zero_padded() {
        // A code that drops a leading zero is rejected by the server and looks
        // like a sync problem.
        let t = rfc(Algorithm::Sha1, 6);
        for step in 0..2000u64 {
            let c = t.at(step * 30).code;
            assert_eq!(c.len(), 6, "wrong length at step {step}: {c}");
        }
    }

    #[test]
    fn seconds_remaining_counts_down_within_the_period() {
        let t = rfc(Algorithm::Sha1, 6);
        assert_eq!(t.at(0).seconds_remaining, 30);
        assert_eq!(t.at(1).seconds_remaining, 29);
        assert_eq!(t.at(29).seconds_remaining, 1);
        assert_eq!(t.at(30).seconds_remaining, 30);
    }

    #[test]
    fn the_code_changes_exactly_on_the_period_boundary() {
        let t = rfc(Algorithm::Sha1, 6);
        assert_eq!(t.at(0).code, t.at(29).code);
        assert_ne!(t.at(29).code, t.at(30).code);
    }

    // --- the three conventions in the wild ---

    #[test]
    fn reads_an_otpauth_uri() {
        let uri = format!("otpauth://totp/Example:a@b.com?secret={RFC_SECRET_B32}&issuer=Example");
        let t = Totp::from_fields(Some(&uri), None, None).expect("parse");
        assert_eq!(t.digits, 6);
        assert_eq!(t.period, 30);
        assert_eq!(t.algorithm, Algorithm::Sha1);
        assert_eq!(t.at(59).code, rfc(Algorithm::Sha1, 6).at(59).code);
    }

    #[test]
    fn an_otpauth_uri_honours_digits_period_and_algorithm() {
        let uri =
            format!("otpauth://totp/X?secret={RFC_SECRET_B32}&digits=8&period=60&algorithm=SHA256");
        let t = Totp::from_fields(Some(&uri), None, None).expect("parse");
        assert_eq!(t.digits, 8);
        assert_eq!(t.period, 60);
        assert_eq!(t.algorithm, Algorithm::Sha256);
    }

    #[test]
    fn reads_the_keeotp_form() {
        let raw = format!("key={RFC_SECRET_B32}&size=8&step=30");
        let t = Totp::from_fields(Some(&raw), None, None).expect("parse");
        assert_eq!(t.digits, 8);
        assert_eq!(t.at(59).code, "94287082");
    }

    #[test]
    fn reads_the_keeweb_split_form() {
        let t = Totp::from_fields(None, Some(RFC_SECRET_B32), Some("30;8")).expect("parse");
        assert_eq!(t.digits, 8);
        assert_eq!(t.period, 30);
        assert_eq!(t.at(59).code, "94287082");
    }

    #[test]
    fn a_bare_seed_defaults_to_thirty_seconds_and_six_digits() {
        let t = Totp::from_fields(None, Some(RFC_SECRET_B32), None).expect("parse");
        assert_eq!((t.period, t.digits), (30, 6));
    }

    // --- input that should not produce a plausible-looking wrong code ---

    #[test]
    fn base32_tolerates_the_way_people_actually_write_seeds() {
        // Sites print secrets in lower case, in spaced groups, with padding.
        let spaced = "gezd gnbv gy3t qojq gezd gnbv gy3t qojq";
        assert_eq!(
            base32_decode(spaced),
            base32_decode(RFC_SECRET_B32),
            "a seed copied from a website must still work"
        );
        assert_eq!(base32_decode("MZXW6==="), base32_decode("MZXW6"));
    }

    #[test]
    fn rubbish_is_rejected_rather_than_silently_decoded() {
        assert!(base32_decode("!!!!").is_none());
        assert!(base32_decode("").is_none());
        // 0, 1 and 8 are not in the base32 alphabet, which is exactly why
        // authenticator secrets never contain them.
        assert!(base32_decode("ABCD0189").is_none());
    }

    #[test]
    fn absurd_parameters_are_refused_rather_than_panicking() {
        let secret = base32_decode(RFC_SECRET_B32).unwrap();
        assert!(Totp::build(secret.clone(), 6, 0, Algorithm::Sha1).is_none());
        assert!(Totp::build(secret.clone(), 20, 30, Algorithm::Sha1).is_none());
        assert!(Totp::build(secret, 0, 30, Algorithm::Sha1).is_none());
        assert!(Totp::build(Vec::new(), 6, 30, Algorithm::Sha1).is_none());
    }

    #[test]
    fn an_entry_with_no_otp_fields_has_no_code() {
        assert!(Totp::from_fields(None, None, None).is_none());
        assert!(Totp::from_fields(Some("  "), None, None).is_none());
    }
}
