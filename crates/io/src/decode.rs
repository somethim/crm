//! Text decoding of CSV bytes: BOM sniffing, UTF-8, then a Windows-1252 fallback.

use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252};

/// The encoding a file was decoded with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectedEncoding {
    /// UTF-8 with a byte-order mark.
    Utf8Bom,
    /// UTF-8 without a byte-order mark.
    Utf8,
    /// UTF-16 little-endian (with a BOM).
    Utf16Le,
    /// UTF-16 big-endian (with a BOM).
    Utf16Be,
    /// Windows-1252 (the fallback for bytes that are not valid UTF-8).
    Windows1252,
}

/// Decodes `bytes` to text, dropping any BOM.
///
/// A BOM decides the encoding (UTF-8, UTF-16 LE/BE). Without one the bytes are
/// UTF-8 when valid, else Windows-1252 (older Excel "CSV" exports).
pub fn to_utf8(bytes: &[u8]) -> (String, DetectedEncoding) {
    if let Some((encoding, bom_len)) = Encoding::for_bom(bytes) {
        let detected = if encoding == UTF_8 {
            DetectedEncoding::Utf8Bom
        } else if encoding == UTF_16LE {
            DetectedEncoding::Utf16Le
        } else if encoding == UTF_16BE {
            DetectedEncoding::Utf16Be
        } else {
            DetectedEncoding::Utf8Bom
        };
        let (text, _) = encoding.decode_without_bom_handling(&bytes[bom_len..]);
        return (text.into_owned(), detected);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_owned(), DetectedEncoding::Utf8),
        Err(_) => {
            let (text, _) = WINDOWS_1252.decode_without_bom_handling(bytes);
            (text.into_owned(), DetectedEncoding::Windows1252)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "Kodi,Përshkrimi,Njësi\nA1,ÇAJ,COPE\n";

    fn utf16(bom: [u8; 2], le: bool) -> Vec<u8> {
        let mut out = bom.to_vec();
        for unit in TEXT.encode_utf16() {
            out.extend(if le {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        out
    }

    #[test]
    fn utf8_with_and_without_bom() {
        assert_eq!(
            to_utf8(TEXT.as_bytes()),
            (TEXT.to_owned(), DetectedEncoding::Utf8)
        );
        let mut bom = b"\xEF\xBB\xBF".to_vec();
        bom.extend_from_slice(TEXT.as_bytes());
        assert_eq!(to_utf8(&bom), (TEXT.to_owned(), DetectedEncoding::Utf8Bom));
    }

    #[test]
    fn utf16_with_bom() {
        assert_eq!(
            to_utf8(&utf16([0xFF, 0xFE], true)),
            (TEXT.to_owned(), DetectedEncoding::Utf16Le)
        );
        assert_eq!(
            to_utf8(&utf16([0xFE, 0xFF], false)),
            (TEXT.to_owned(), DetectedEncoding::Utf16Be)
        );
    }

    #[test]
    fn invalid_utf8_falls_back_to_1252() {
        let (bytes, _, _) = WINDOWS_1252.encode(TEXT);
        assert!(std::str::from_utf8(&bytes).is_err());
        assert_eq!(
            to_utf8(&bytes),
            (TEXT.to_owned(), DetectedEncoding::Windows1252)
        );
    }
}
