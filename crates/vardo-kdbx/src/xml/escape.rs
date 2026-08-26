//! Entity encoding and decoding.
//!
//! Decoding is deliberately lenient and encoding deliberately conservative,
//! because these two functions are *not* inverses in this design and must not
//! be treated as such. Text is stored raw and only decoded on access, so an
//! untouched value never passes through `encode_text` at all. That is what
//! makes byte-stable round-tripping possible without having to replicate every
//! writer's escaping quirks.

/// Expands XML entities and numeric character references.
///
/// Unrecognised entities are passed through verbatim rather than raising an
/// error. A `.kdbx` payload has already been authenticated by an HMAC before we
/// see it, so malformed markup here means a buggy writer, not an attacker, and
/// discarding user data over it would be the worse failure.
pub fn decode_text(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'&' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'&' {
                i += 1;
            }
            out.push_str(&raw[start..i]);
            continue;
        }

        // Entities are short; a missing terminator means this is a literal '&'.
        let end = match raw[i..].find(';') {
            Some(rel) if rel <= 12 => i + rel,
            _ => {
                out.push('&');
                i += 1;
                continue;
            }
        };

        let body = &raw[i + 1..end];
        let replacement = match body {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => decode_numeric(body),
        };

        match replacement {
            Some(c) => {
                out.push(c);
                i = end + 1;
            }
            None => {
                out.push('&');
                i += 1;
            }
        }
    }
    out
}

fn decode_numeric(body: &str) -> Option<char> {
    let digits = body.strip_prefix('#')?;
    let cp = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => digits.parse::<u32>().ok()?,
    };
    char::from_u32(cp)
}

/// Escapes a value for embedding in text or an attribute.
///
/// Only used when the application *changes* a value. `>` is escaped even though
/// it is legal bare, matching what KeePass and KeePassXC emit, so that values we
/// write look like values they write.
pub fn encode_text(value: &str, in_attribute: bool) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if in_attribute => out.push_str("&quot;"),
            '\'' if in_attribute => out.push_str("&apos;"),
            // Carriage returns must be escaped or an XML reader will normalise
            // them away, silently corrupting multi-line notes.
            '\r' => out.push_str("&#xD;"),
            '\n' | '\t' => out.push(c),
            // Other C0 controls are not representable in XML 1.0 at all.
            c if (c as u32) < 0x20 => out.push_str(&format!("&#x{:X};", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_named_and_numeric_entities() {
        assert_eq!(decode_text("a&amp;b"), "a&b");
        assert_eq!(decode_text("&lt;tag&gt;"), "<tag>");
        assert_eq!(decode_text("&quot;q&apos;"), "\"q'");
        assert_eq!(decode_text("&#65;&#x42;"), "AB");
        assert_eq!(decode_text("line&#xD;&#xA;break"), "line\r\nbreak");
    }

    #[test]
    fn passes_through_text_without_entities() {
        assert_eq!(decode_text("plain text"), "plain text");
        assert_eq!(decode_text(""), "");
    }

    #[test]
    fn tolerates_malformed_entities() {
        // A bare ampersand is common in passwords typed by humans.
        assert_eq!(decode_text("100% & rising"), "100% & rising");
        assert_eq!(decode_text("&notanentity;"), "&notanentity;");
        assert_eq!(decode_text("&"), "&");
        assert_eq!(decode_text("&#;"), "&#;");
        assert_eq!(decode_text("&#xZZ;"), "&#xZZ;");
        // Unterminated, and longer than any real entity.
        assert_eq!(decode_text("&averyverylongthing"), "&averyverylongthing");
    }

    #[test]
    fn rejects_out_of_range_codepoints() {
        assert_eq!(decode_text("&#x110000;"), "&#x110000;");
        assert_eq!(decode_text("&#xD800;"), "&#xD800;"); // lone surrogate
    }

    #[test]
    fn encodes_markup_and_controls() {
        assert_eq!(encode_text("a&b", false), "a&amp;b");
        assert_eq!(encode_text("<x>", false), "&lt;x&gt;");
        assert_eq!(encode_text("\"", false), "\"");
        assert_eq!(encode_text("\"", true), "&quot;");
        assert_eq!(encode_text("a\rb", false), "a&#xD;b");
        assert_eq!(encode_text("a\u{1}b", false), "a&#x1;b");
        // Tab and newline stay literal; KeePass does not escape them.
        assert_eq!(encode_text("a\n\tb", false), "a\n\tb");
    }

    #[test]
    fn encode_then_decode_is_identity() {
        for s in [
            "plain",
            "a&b<c>d\"e'f",
            "multi\nline\ttab",
            "carriage\r\nreturn",
            "unicode: \u{4e2d}\u{6587} \u{1F512}",
            "\u{1}\u{2}\u{1f}",
            "",
        ] {
            assert_eq!(decode_text(&encode_text(s, false)), s, "text: {s:?}");
            assert_eq!(decode_text(&encode_text(s, true)), s, "attr: {s:?}");
        }
    }
}
