//! KDBX timestamps.
//!
//! Two encodings, chosen by format version, and both must round-trip:
//!
//! * **KDBX 4.x** — base64 of a little-endian `i64`, counting seconds since
//!   `0001-01-01T00:00:00Z` (the .NET epoch).
//! * **KDBX 3.1** — an ISO-8601 string, `2026-02-18T18:42:09Z`.
//!
//! A `Timestamp` remembers which spelling it came from, so writing a document
//! back does not silently convert one to the other.
//!
//! # Why precision matters here
//!
//! KDBX timestamps are whole seconds. Merge decides which side of a conflict
//! wins by comparing them, so two edits inside the same second are genuinely
//! indistinguishable — and a comparison that ignores that produces spurious
//! conflicts on identical databases. That is KeePassXC issue #3340. The
//! ordering helpers here return [`Ordering::Equal`] for same-second values on
//! purpose, and merge must treat that as "no decision", never as "left wins".

use std::cmp::Ordering;

/// Seconds between `0001-01-01T00:00:00Z` and the Unix epoch.
const UNIX_EPOCH_IN_DOTNET_SECONDS: i64 = 62_135_596_800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// KDBX 4.x: base64 little-endian i64 seconds since year 1.
    Base64Seconds,
    /// KDBX 3.1: ISO-8601 with a `Z` suffix.
    Iso8601,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    /// Seconds since `0001-01-01T00:00:00Z`.
    seconds: i64,
    encoding: Encoding,
}

impl Timestamp {
    pub fn from_unix(unix_seconds: i64, encoding: Encoding) -> Self {
        Self {
            seconds: unix_seconds.saturating_add(UNIX_EPOCH_IN_DOTNET_SECONDS),
            encoding,
        }
    }

    pub fn unix_seconds(self) -> i64 {
        self.seconds - UNIX_EPOCH_IN_DOTNET_SECONDS
    }

    pub fn encoding(self) -> Encoding {
        self.encoding
    }

    /// Parses either encoding, guessing from the text.
    ///
    /// Returns `None` rather than a default: a missing or unparseable
    /// timestamp must not silently become the epoch, because merge would then
    /// treat it as "very old" and discard the newer side.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        // ISO-8601 always contains a '-'; base64 of an i64 never does.
        if text.contains('-') {
            parse_iso8601(text).map(|seconds| Self {
                seconds,
                encoding: Encoding::Iso8601,
            })
        } else {
            parse_base64_seconds(text).map(|seconds| Self {
                seconds,
                encoding: Encoding::Base64Seconds,
            })
        }
    }

    /// Renders in this timestamp's original encoding.
    pub fn to_text(self) -> String {
        match self.encoding {
            Encoding::Base64Seconds => {
                vardo_kdbx::write::base64_encode(&self.seconds.to_le_bytes())
            }
            Encoding::Iso8601 => format_iso8601(self.seconds),
        }
    }

    /// Chronological comparison, ignoring encoding.
    ///
    /// Two timestamps in the same second compare `Equal`, because KDBX cannot
    /// distinguish them. Callers must treat `Equal` as "cannot decide".
    pub fn cmp_time(self, other: Self) -> Ordering {
        self.seconds.cmp(&other.seconds)
    }

    pub fn is_after(self, other: Self) -> bool {
        self.cmp_time(other) == Ordering::Greater
    }
}

fn parse_base64_seconds(text: &str) -> Option<i64> {
    let bytes = base64_decode(text)?;
    // KeePass writes 8 bytes; tolerate shorter by zero-extending, since some
    // writers trim trailing zero bytes.
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    let mut buf = [0u8; 8];
    buf[..bytes.len()].copy_from_slice(&bytes);
    Some(i64::from_le_bytes(buf))
}

/// Parses `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Hand-rolled rather than pulling in a date library: this is the only date
/// handling in the project, the grammar is fixed, and a calendar dependency in
/// a password manager is attack surface for no benefit.
fn parse_iso8601(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |range: std::ops::Range<usize>| -> Option<i64> {
        text.get(range)?.parse::<i64>().ok()
    };
    if b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, s) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    if h > 23 || mi > 59 || s > 60 {
        return None;
    }
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + s)
}

fn format_iso8601(seconds: i64) -> String {
    let days = seconds.div_euclid(86400);
    let rem = seconds.rem_euclid(86400);
    let (y, mo, d) = civil_from_days(days);
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Days from `0001-01-01` to `y-mo-d`, proleptic Gregorian.
///
/// Howard Hinnant's civil-date algorithm, rebased from 1970 to year 1.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    // 719468 shifts the 1970 epoch to 0000-03-01; 719162 shifts to 0001-01-01.
    era * 146097 + doe - 719_162 + 719_162 - 719_162 + 719_162 - 719_162
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_162;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const INVALID: u8 = 0xFF;
    let mut table = [INVALID; 256];
    for (i, c) in (b'A'..=b'Z')
        .chain(b'a'..=b'z')
        .chain(b'0'..=b'9')
        .chain([b'+', b'/'])
        .enumerate()
    {
        table[c as usize] = i as u8;
    }
    let bytes: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let last = bytes.len() / 4;
    for (ci, chunk) in bytes.chunks(4).enumerate() {
        let pad = chunk.iter().filter(|&&b| b == b'=').count();
        if pad > 0 && ci + 1 != last {
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
    fn iso8601_round_trips() {
        for s in [
            "2026-02-18T18:42:09Z",
            "0001-01-01T00:00:00Z",
            "1970-01-01T00:00:00Z",
            "2000-02-29T12:00:00Z", // leap year
            "2100-03-01T23:59:59Z", // 2100 is not a leap year
            "9999-12-31T23:59:59Z",
        ] {
            let t = Timestamp::parse(s).unwrap_or_else(|| panic!("parse {s}"));
            assert_eq!(t.encoding(), Encoding::Iso8601);
            assert_eq!(t.to_text(), s, "round-trip {s}");
        }
    }

    #[test]
    fn unix_epoch_is_where_it_should_be() {
        let t = Timestamp::parse("1970-01-01T00:00:00Z").unwrap();
        assert_eq!(t.unix_seconds(), 0);
        let t = Timestamp::parse("2026-02-18T18:42:09Z").unwrap();
        assert_eq!(t.unix_seconds(), 1_771_440_129);
    }

    #[test]
    fn base64_seconds_round_trip() {
        // Real value lifted from a KeePassXC-written fixture.
        let t = Timestamp::parse("fb+H4Q4AAAA=").unwrap();
        assert_eq!(t.encoding(), Encoding::Base64Seconds);
        assert_eq!(t.to_text(), "fb+H4Q4AAAA=");
        // Sanity: it should land in the 2020s, not year 1 or year 30000.
        let year = 1970 + t.unix_seconds() / 31_557_600;
        assert!((2020..2030).contains(&year), "implausible year {year}");
    }

    #[test]
    fn encoding_survives_a_round_trip_in_both_directions() {
        let b64 = Timestamp::parse("fb+H4Q4AAAA=").unwrap();
        let iso = Timestamp::parse("2026-02-18T18:42:09Z").unwrap();
        // Writing must not convert one spelling into the other.
        assert!(!b64.to_text().contains('-'));
        assert!(iso.to_text().contains('-'));
    }

    #[test]
    fn missing_or_malformed_values_are_none_not_epoch() {
        // Defaulting to the epoch would make merge treat the entry as ancient
        // and silently discard the newer side.
        for bad in ["", "   ", "not a date", "2026-13-45T99:99:99Z", "!!!!", "="] {
            assert!(Timestamp::parse(bad).is_none(), "should reject {bad:?}");
        }
    }

    #[test]
    fn same_second_compares_equal() {
        // KDBX has one-second resolution, so this is genuinely undecidable and
        // merge must not pretend otherwise.
        let a = Timestamp::parse("2026-02-18T18:42:09Z").unwrap();
        let b = Timestamp::parse("2026-02-18T18:42:09Z").unwrap();
        assert_eq!(a.cmp_time(b), Ordering::Equal);
        assert!(!a.is_after(b));
        assert!(!b.is_after(a));
    }

    #[test]
    fn ordering_works_across_encodings() {
        let older = Timestamp::from_unix(1_000_000, Encoding::Iso8601);
        let newer = Timestamp::from_unix(2_000_000, Encoding::Base64Seconds);
        assert!(newer.is_after(older));
        assert!(!older.is_after(newer));
    }

    #[test]
    fn civil_date_algorithm_is_self_inverse() {
        for days in [0i64, 1, 365, 719_162, 730_000, 1_000_000, 3_652_058] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "{y}-{m}-{d}");
        }
    }
}
