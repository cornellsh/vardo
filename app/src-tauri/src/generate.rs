//! Password generation.
//!
//! In Rust rather than JavaScript for two reasons. The generated value can be
//! written straight into the vault without becoming a JS string, and the
//! randomness is the OS CSPRNG — `Math.random()` is explicitly not
//! cryptographically secure, and browser-side generators have shipped
//! predictable passwords because of it.

use crate::model::GeneratorOptions;

const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &[u8] = b"0123456789";
const SYMBOLS: &[u8] = b"!@#$%^&*()-_=+[]{};:,.?";

/// Characters that are easy to misread when transcribing a password by hand.
const AMBIGUOUS: &[u8] = b"0O1lI|`'\"";

pub fn password(length: usize, options: &GeneratorOptions) -> Result<String, String> {
    if length == 0 || length > 512 {
        return Err("length must be between 1 and 512".into());
    }

    let mut alphabet: Vec<u8> = Vec::new();
    if options.lowercase {
        alphabet.extend_from_slice(LOWER);
    }
    if options.uppercase {
        alphabet.extend_from_slice(UPPER);
    }
    if options.digits {
        alphabet.extend_from_slice(DIGITS);
    }
    if options.symbols {
        alphabet.extend_from_slice(SYMBOLS);
    }
    if options.exclude_ambiguous {
        alphabet.retain(|c| !AMBIGUOUS.contains(c));
    }

    if alphabet.is_empty() {
        return Err("no character classes selected".into());
    }

    let mut out = String::with_capacity(length);
    for _ in 0..length {
        out.push(alphabet[uniform_index(alphabet.len())?] as char);
    }
    Ok(out)
}

/// A uniformly distributed index into `len`.
///
/// Rejection sampling, not `% len`. The modulo shortcut biases towards the low
/// end of the alphabet whenever `len` does not divide the range evenly, which
/// quietly reduces entropy — the kind of flaw that never shows up in testing
/// because the output still looks random.
fn uniform_index(len: usize) -> Result<usize, String> {
    debug_assert!(len > 0);
    let len = len as u32;
    let limit = u32::MAX - (u32::MAX % len) - 1;

    loop {
        let mut buf = [0u8; 4];
        getrandom::fill(&mut buf).map_err(|e| format!("system RNG unavailable: {e}"))?;
        let value = u32::from_le_bytes(buf);
        if value <= limit {
            return Ok((value % len) as usize);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> GeneratorOptions {
        GeneratorOptions {
            lowercase: true,
            uppercase: true,
            digits: true,
            symbols: true,
            exclude_ambiguous: false,
        }
    }

    #[test]
    fn produces_the_requested_length() {
        assert_eq!(password(32, &opts()).unwrap().chars().count(), 32);
    }

    #[test]
    fn rejects_an_empty_alphabet() {
        let none = GeneratorOptions {
            lowercase: false,
            uppercase: false,
            digits: false,
            symbols: false,
            exclude_ambiguous: false,
        };
        assert!(password(16, &none).is_err());
    }

    #[test]
    fn excluding_ambiguous_characters_removes_all_of_them() {
        let mut o = opts();
        o.exclude_ambiguous = true;
        for _ in 0..50 {
            let pw = password(64, &o).unwrap();
            assert!(
                !pw.bytes().any(|b| AMBIGUOUS.contains(&b)),
                "found an ambiguous character in {pw}"
            );
        }
    }

    #[test]
    fn respects_the_selected_classes() {
        let digits_only = GeneratorOptions {
            lowercase: false,
            uppercase: false,
            digits: true,
            symbols: false,
            exclude_ambiguous: false,
        };
        let pw = password(64, &digits_only).unwrap();
        assert!(pw.chars().all(|c| c.is_ascii_digit()), "{pw}");
    }

    #[test]
    fn does_not_repeat_itself() {
        // Not a randomness test — just a guard against a generator that is
        // seeded once or returns a constant.
        let a = password(32, &opts()).unwrap();
        let b = password(32, &opts()).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn covers_the_whole_alphabet() {
        // Rejection sampling exists to keep this uniform; a modulo-biased
        // generator still passes, but a badly broken one does not.
        let lower = GeneratorOptions {
            lowercase: true,
            uppercase: false,
            digits: false,
            symbols: false,
            exclude_ambiguous: false,
        };
        // Several maximum-length passwords rather than one huge one: 512 is a
        // deliberate cap, and the test must not be the reason it moves.
        let distinct: std::collections::HashSet<char> = (0..10)
            .flat_map(|_| password(512, &lower).unwrap().chars().collect::<Vec<_>>())
            .collect();
        assert_eq!(distinct.len(), 26, "some letters never appeared");
    }
}
