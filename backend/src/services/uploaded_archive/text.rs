//! Decoding an entry's bytes as text: UTF-8 (a BOM is dropped), UTF-16 when
//! a BOM says so, anything else decoded lossily and flagged so the caller
//! can say some characters may be wrong.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub text: String,
    pub encoding: TextEncoding,
    /// Some bytes weren't valid in the encoding and were replaced with
    /// U+FFFD.
    pub lossy: bool,
}

pub fn decode_text(bytes: &[u8]) -> Text {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return utf8(rest);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, TextEncoding::Utf16Le);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, TextEncoding::Utf16Be);
    }
    utf8(bytes)
}

fn utf8(bytes: &[u8]) -> Text {
    match std::str::from_utf8(bytes) {
        Ok(s) => Text {
            text: s.to_string(),
            encoding: TextEncoding::Utf8,
            lossy: false,
        },
        Err(_) => Text {
            text: String::from_utf8_lossy(bytes).into_owned(),
            encoding: TextEncoding::Utf8,
            lossy: true,
        },
    }
}

fn utf16(bytes: &[u8], encoding: TextEncoding) -> Text {
    let units = bytes.chunks_exact(2).map(|pair| {
        let pair = [pair[0], pair[1]];
        match encoding {
            TextEncoding::Utf16Be => u16::from_be_bytes(pair),
            _ => u16::from_le_bytes(pair),
        }
    });
    let odd = !bytes.len().is_multiple_of(2);
    let mut lossy = odd;
    let mut text: String = char::decode_utf16(units)
        .map(|c| {
            c.unwrap_or_else(|_| {
                lossy = true;
                char::REPLACEMENT_CHARACTER
            })
        })
        .collect();
    if odd {
        text.push(char::REPLACEMENT_CHARACTER);
    }
    Text {
        text,
        encoding,
        lossy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16_bytes(s: &str, big_endian: bool) -> Vec<u8> {
        let mut out = if big_endian {
            vec![0xFE, 0xFF]
        } else {
            vec![0xFF, 0xFE]
        };
        for unit in s.encode_utf16() {
            out.extend(if big_endian {
                unit.to_be_bytes()
            } else {
                unit.to_le_bytes()
            });
        }
        out
    }

    #[test]
    fn utf8_with_and_without_a_bom() {
        let plain = decode_text("# Café".as_bytes());
        assert_eq!(plain.text, "# Café");
        assert!(!plain.lossy);
        let bom = decode_text(&[&[0xEF, 0xBB, 0xBF][..], "# Café".as_bytes()].concat());
        assert_eq!(bom.text, "# Café");
        assert_eq!(bom.encoding, TextEncoding::Utf8);
    }

    #[test]
    fn utf16_with_a_bom_decodes_in_either_byte_order() {
        let le = decode_text(&utf16_bytes("# Notes 📝", false));
        assert_eq!(le.text, "# Notes 📝");
        assert_eq!(le.encoding, TextEncoding::Utf16Le);
        assert!(!le.lossy);
        let be = decode_text(&utf16_bytes("# Notes 📝", true));
        assert_eq!(be.text, "# Notes 📝");
        assert_eq!(be.encoding, TextEncoding::Utf16Be);
    }

    #[test]
    fn broken_utf16_is_flagged() {
        // A lone high surrogate, then an odd trailing byte.
        let odd = decode_text(&[0xFF, 0xFE, 0x3D, 0xD8, b'a', 0x00, b'b']);
        assert!(odd.lossy);
        assert_eq!(odd.text, "\u{FFFD}a\u{FFFD}");
    }

    #[test]
    fn latin1_text_decodes_lossily_with_a_flag() {
        // "Café" in Latin-1: 0xE9 alone isn't UTF-8.
        let t = decode_text(b"Caf\xE9 au lait");
        assert!(t.lossy);
        assert_eq!(t.text, "Caf\u{FFFD} au lait");
        assert_eq!(t.encoding, TextEncoding::Utf8);
    }
}
