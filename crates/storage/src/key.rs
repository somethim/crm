//! The 32-byte database key. Its bytes are only ever formatted in
//! [`DbKey::pragma_text`], into a zeroized buffer.

use std::sync::Arc;
use zeroize::Zeroizing;

/// The SQLite3 Multiple Ciphers (ChaCha20-Poly1305) data key: 32 raw bytes,
/// wiped from memory when the last clone drops. `Debug` is redacted; there is
/// no `Display` and no serde on purpose.
#[derive(Clone)]
pub struct DbKey(Arc<Zeroizing<[u8; 32]>>);

impl DbKey {
    /// Wraps 32 key bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Arc::new(Zeroizing::new(bytes)))
    }

    /// A deterministic key for tests (`seed` picks one of 256 keys).
    #[doc(hidden)]
    pub fn for_tests(seed: u8) -> Self {
        let mut bytes = [0u8; 32];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = seed
                .wrapping_mul(37)
                .wrapping_add((i as u8).wrapping_mul(11))
                ^ 0x5a;
        }
        Self::from_bytes(bytes)
    }

    /// The 64 lowercase hex characters (log-secrecy tests search for them).
    #[doc(hidden)]
    pub fn to_hex_for_tests(&self) -> String {
        self.pragma_text()[4..].to_owned()
    }

    /// `raw:<64 lowercase hex>`, the value of `PRAGMA key` and of the `key=`
    /// URI parameter.
    pub(crate) fn pragma_text(&self) -> Zeroizing<String> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut text = Zeroizing::new(String::with_capacity(4 + 64));
        text.push_str("raw:");
        for byte in self.0.iter() {
            text.push(HEX[usize::from(byte >> 4)] as char);
            text.push(HEX[usize::from(byte & 0x0f)] as char);
        }
        text
    }
}

impl std::fmt::Debug for DbKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DbKey(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pragma_text_is_raw_lowercase_hex() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0xab;
        bytes[1] = 0x01;
        bytes[31] = 0xff;
        let text = DbKey::from_bytes(bytes).pragma_text();
        assert_eq!(text.len(), 68);
        assert!(text.starts_with("raw:ab01"));
        assert!(text.ends_with("ff"));
        assert!(
            text[4..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
    }

    #[test]
    fn debug_hides_the_bytes() {
        let key = DbKey::for_tests(7);
        let debug = format!("{key:?}");
        assert_eq!(debug, "DbKey(<redacted>)");
        assert!(!debug.contains(&key.pragma_text()[4..12]));
    }

    #[test]
    fn test_keys_differ_by_seed_and_repeat() {
        assert_eq!(
            *DbKey::for_tests(7).pragma_text(),
            *DbKey::for_tests(7).pragma_text()
        );
        assert_ne!(
            *DbKey::for_tests(7).pragma_text(),
            *DbKey::for_tests(8).pragma_text()
        );
    }
}
