//! Standard base64, strict on input.
//!
//! KDBX uses base64 for UUIDs, timestamps, binary attachments and protected
//! values. A lenient decoder is a liability here: silently accepting
//! `Zm==Zm9v` (two concatenated strings) would drop half the data, and
//! accepting stray characters would let two implementations disagree about
//! what a file contains.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const INVALID: u8 = 0xFF;

fn decode_table() -> [u8; 256] {
    let mut t = [INVALID; 256];
    let mut i = 0usize;
    while i < 64 {
        t[ALPHABET[i] as usize] = i as u8;
        i += 1;
    }
    t
}

pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Decodes standard base64. Returns `None` on anything malformed.
///
/// ASCII whitespace is skipped, because some writers wrap long values, but
/// padding must appear only at the very end and nowhere else.
pub fn decode(s: &str) -> Option<Vec<u8>> {
    let table = decode_table();
    let bytes: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    if bytes.is_empty() {
        return Some(Vec::new());
    }

    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let last_chunk = bytes.len() / 4;
    for (ci, chunk) in bytes.chunks(4).enumerate() {
        let pad = chunk.iter().filter(|&&b| b == b'=').count();
        // Interior padding means two concatenated base64 strings. Decoders
        // that accept it silently discard everything after the padding.
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
    fn matches_rfc4648_vectors() {
        for (plain, encoded) in [
            (&b""[..], ""),
            (&b"f"[..], "Zg=="),
            (&b"fo"[..], "Zm8="),
            (&b"foo"[..], "Zm9v"),
            (&b"foob"[..], "Zm9vYg=="),
            (&b"fooba"[..], "Zm9vYmE="),
            (&b"foobar"[..], "Zm9vYmFy"),
        ] {
            assert_eq!(encode(plain), encoded);
            assert_eq!(decode(encoded).unwrap(), plain);
        }
    }

    #[test]
    fn round_trips_arbitrary_lengths() {
        for len in 0..128 {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(decode(&encode(&data)).unwrap(), data, "len {len}");
        }
    }

    #[test]
    fn tolerates_wrapped_lines() {
        assert_eq!(decode("Zm9v\nYmFy").unwrap(), b"foobar");
        assert_eq!(decode("Zm9v YmFy").unwrap(), b"foobar");
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in [
            "Zg=",       // wrong length
            "Zm9!",      // invalid character
            "=Zm9",      // padding not at end
            "Z===",      // too much padding
            "Zm==Zm9v",  // interior padding: two concatenated strings
            "Zm9vYmFyZ", // not a multiple of 4
        ] {
            assert!(decode(bad).is_none(), "should reject {bad:?}");
        }
    }
}
