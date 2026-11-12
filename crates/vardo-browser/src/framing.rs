//! Chrome's native messaging framing: a 32-bit length in the machine's own byte
//! order, then that many bytes of UTF-8 JSON.
//!
//! <https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging#native-messaging-host-protocol>

use std::io::{self, Read, Write};

/// Chrome refuses anything larger from a host, so the host refuses to send it.
pub const MAX_TO_BROWSER: usize = 1024 * 1024;

/// Chrome allows the extension to send up to 64 MiB. Nothing Vardo's extension
/// sends is more than a URL and a few identifiers, so anything near this is a
/// bug or an attack, and is refused before a byte of it is buffered.
pub const MAX_FROM_BROWSER: usize = 64 * 1024;

/// Reads one message. `Ok(None)` is a clean end of input: Chrome closes the
/// host's input when it is done with it.
pub fn read(input: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match input.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_ne_bytes(len) as usize;
    if len > MAX_FROM_BROWSER {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("message of {len} bytes is larger than {MAX_FROM_BROWSER}"),
        ));
    }
    let mut body = vec![0u8; len];
    input.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Writes one message, refusing one Chrome would reject.
pub fn write(output: &mut impl Write, body: &[u8]) -> io::Result<()> {
    if body.len() > MAX_TO_BROWSER {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "reply of {} bytes is larger than Chrome accepts",
                body.len()
            ),
        ));
    }
    output.write_all(&(body.len() as u32).to_ne_bytes())?;
    output.write_all(body)?;
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_survives_the_round_trip() {
        let mut wire = Vec::new();
        write(&mut wire, br#"{"type":"status"}"#).unwrap();
        write(&mut wire, b"{}").unwrap();

        let mut input = wire.as_slice();
        assert_eq!(read(&mut input).unwrap().unwrap(), br#"{"type":"status"}"#);
        assert_eq!(read(&mut input).unwrap().unwrap(), b"{}");
        assert_eq!(read(&mut input).unwrap(), None);
    }

    #[test]
    fn the_length_is_in_native_byte_order() {
        let mut wire = Vec::new();
        write(&mut wire, b"abc").unwrap();
        assert_eq!(&wire[..4], &3u32.to_ne_bytes());
    }

    #[test]
    fn an_oversized_message_is_refused_before_it_is_buffered() {
        // Four bytes claiming four gigabytes. Allocating first and checking
        // afterwards would let any extension bug take the host down.
        let mut input: &[u8] = &u32::MAX.to_ne_bytes();
        assert_eq!(
            read(&mut input).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn a_reply_chrome_would_reject_is_never_sent() {
        let mut wire = Vec::new();
        let too_big = vec![b'x'; MAX_TO_BROWSER + 1];
        assert!(write(&mut wire, &too_big).is_err());
        assert!(wire.is_empty(), "nothing partial may reach the browser");
    }

    #[test]
    fn a_message_cut_short_is_an_error_not_an_end() {
        let mut wire = Vec::new();
        write(&mut wire, b"hello").unwrap();
        wire.truncate(6);
        let mut input = wire.as_slice();
        assert!(read(&mut input).is_err());
    }
}
