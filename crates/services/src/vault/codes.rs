//! The recovery key and the wipe code: Crockford Base32 with a tagged CRC-32
//! check, and the classification of whatever the user typed.

use crate::error::VaultError;
use zeroize::Zeroizing;

/// Crockford Base32.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// Random characters of a recovery key (150 bits).
pub(crate) const RECOVERY_PAYLOAD: usize = 30;
/// Random characters of a wipe code (50 bits).
pub(crate) const WIPE_PAYLOAD: usize = 10;
const CHECK: usize = 2;
const RECOVERY_TAG: u8 = b'R';
const WIPE_TAG: u8 = b'W';
/// The longest password accepted.
pub const PASSWORD_MAX_CHARS: usize = 1024;
/// The shortest password accepted.
pub const PASSWORD_MIN_CHARS: usize = 6;

/// CRC-32 (IEEE, reflected 0xEDB88320), bitwise.
fn crc32(parts: &[&[u8]]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for part in parts {
        for &byte in *part {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
    }
    !crc
}

/// The two check characters: the low 10 bits of CRC-32 over `tag ‖ payload`.
fn check_chars(tag: u8, payload: &[u8]) -> [u8; CHECK] {
    let value = crc32(&[&[tag], payload]) & 0x3FF;
    [
        ALPHABET[(value >> 5) as usize],
        ALPHABET[(value & 31) as usize],
    ]
}

/// `len` random alphabet characters plus the check characters for `tag`.
fn generate(len: usize, tag: u8) -> Result<Zeroizing<String>, VaultError> {
    let mut random = Zeroizing::new(vec![0u8; len]);
    getrandom::fill(&mut random).map_err(|_| VaultError::Crypto)?;
    let mut code = Zeroizing::new(String::with_capacity(len + CHECK));
    for byte in random.iter() {
        // 256 is a multiple of 32: the low 5 bits are unbiased.
        code.push(ALPHABET[usize::from(byte & 31)] as char);
    }
    let check = check_chars(tag, code.as_bytes());
    code.push(check[0] as char);
    code.push(check[1] as char);
    Ok(code)
}

/// Uppercase; drop spaces and `-`; `O`→`0`, `I`/`L`→`1`.
fn normalise(raw: &str) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(raw.len()));
    for c in raw.trim().chars() {
        let c = match c.to_ascii_uppercase() {
            ' ' | '-' => continue,
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        out.push(c);
    }
    out
}

fn is_valid(code: &str, payload: usize, tag: u8) -> bool {
    let bytes = code.as_bytes();
    bytes.len() == payload + CHECK
        && bytes.iter().all(|b| ALPHABET.contains(b))
        && check_chars(tag, &bytes[..payload]) == [bytes[payload], bytes[payload + 1]]
}

/// Groups of 4 joined by `-`.
fn groups(code: &str) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(code.len() + code.len() / 4));
    for (i, c) in code.chars().enumerate() {
        if i > 0 && i % 4 == 0 {
            out.push('-');
        }
        out.push(c);
    }
    out
}

/// The recovery key (30 + 2 characters), normalised.
#[derive(Clone, PartialEq, Eq)]
pub struct RecoveryKey(Zeroizing<String>);

/// The emergency wipe code (10 + 2 characters), normalised.
#[derive(Clone, PartialEq, Eq)]
pub struct WipeCode(Zeroizing<String>);

impl RecoveryKey {
    pub(crate) fn generate() -> Result<Self, VaultError> {
        generate(RECOVERY_PAYLOAD, RECOVERY_TAG).map(Self)
    }

    /// `XXXX-XXXX-…` (8 groups of 4).
    pub fn display_groups(&self) -> Zeroizing<String> {
        groups(&self.0)
    }

    pub(crate) fn secret(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl WipeCode {
    pub(crate) fn generate() -> Result<Self, VaultError> {
        generate(WIPE_PAYLOAD, WIPE_TAG).map(Self)
    }

    /// `XXXX-XXXX-XXXX` (3 groups of 4).
    pub fn display_groups(&self) -> Zeroizing<String> {
        groups(&self.0)
    }

    pub(crate) fn secret(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl std::fmt::Debug for RecoveryKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryKey(<redacted>)")
    }
}

impl std::fmt::Debug for WipeCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WipeCode(<redacted>)")
    }
}

/// What a typed entry is. Passwords are never normalised.
pub enum Entry {
    /// A valid recovery key.
    Recovery(RecoveryKey),
    /// A valid wipe code.
    Wipe(WipeCode),
    /// Anything else, exactly as typed.
    Password(Zeroizing<String>),
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Entry::Recovery(_) => "Entry::Recovery(<redacted>)",
            Entry::Wipe(_) => "Entry::Wipe(<redacted>)",
            Entry::Password(_) => "Entry::Password(<redacted>)",
        })
    }
}

/// Classifies a typed entry: a string that normalises to a valid recovery key
/// or wipe code is one (the tag makes them exclusive); anything else is a password.
pub fn classify(raw: &str) -> Entry {
    let code = normalise(raw);
    if is_valid(&code, RECOVERY_PAYLOAD, RECOVERY_TAG) {
        Entry::Recovery(RecoveryKey(code))
    } else if is_valid(&code, WIPE_PAYLOAD, WIPE_TAG) {
        Entry::Wipe(WipeCode(code))
    } else {
        Entry::Password(Zeroizing::new(raw.to_owned()))
    }
}

/// A new password: 6 to 1024 characters, and never shaped like a recovery key
/// or a wipe code.
pub fn check_new_password(password: &str) -> Result<(), VaultError> {
    let chars = password.chars().count();
    if chars < PASSWORD_MIN_CHARS {
        return Err(VaultError::TooShort);
    }
    if chars > PASSWORD_MAX_CHARS {
        return Err(VaultError::PasswordNotAllowed);
    }
    match classify(password) {
        Entry::Password(_) => Ok(()),
        _ => Err(VaultError::PasswordNotAllowed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_wipe(entry: &Entry, code: &WipeCode) -> bool {
        matches!(entry, Entry::Wipe(w) if w == code)
    }

    #[test]
    fn crc32_matches_the_ieee_check_value() {
        assert_eq!(crc32(&[b"123456789"]), 0xCBF4_3926);
    }

    #[test]
    fn round_trip_with_sloppy_typing() {
        for _ in 0..50 {
            let rk = RecoveryKey::generate().unwrap();
            let shown = rk.display_groups();
            assert_eq!(shown.len(), 39);
            assert_eq!(shown.matches('-').count(), 7);
            let sloppy = format!(
                "  {} ",
                shown
                    .to_lowercase()
                    .replace('-', " - ")
                    .replace('0', "o")
                    .replace('1', "l")
            );
            assert!(matches!(classify(&sloppy), Entry::Recovery(k) if k == rk));
            let wipe = WipeCode::generate().unwrap();
            let shown = wipe.display_groups();
            assert_eq!(shown.len(), 14);
            let sloppy = shown.replace('-', "").replace('1', "I").to_lowercase();
            assert!(is_wipe(&classify(&sloppy), &wipe));
        }
    }

    #[test]
    fn typos_never_give_the_same_wipe_code() {
        let mut still_wipe_shaped = 0;
        let mut variants = 0;
        for _ in 0..20 {
            let code = WipeCode::generate().unwrap();
            let base: Vec<u8> = code.0.as_bytes().to_vec();
            for i in 0..base.len() {
                for &c in ALPHABET {
                    if c == base[i] {
                        continue;
                    }
                    let mut v = base.clone();
                    v[i] = c;
                    let entry = classify(std::str::from_utf8(&v).unwrap());
                    assert!(!is_wipe(&entry, &code));
                    variants += 1;
                    still_wipe_shaped += usize::from(matches!(entry, Entry::Wipe(_)));
                }
                if i + 1 < base.len() && base[i] != base[i + 1] {
                    let mut v = base.clone();
                    v.swap(i, i + 1);
                    let entry = classify(std::str::from_utf8(&v).unwrap());
                    assert!(!is_wipe(&entry, &code));
                    variants += 1;
                    still_wipe_shaped += usize::from(matches!(entry, Entry::Wipe(_)));
                }
            }
        }
        // A 10-bit check lets about 1 in 1024 typos through as *another* code.
        assert!(
            still_wipe_shaped * 200 < variants,
            "{still_wipe_shaped} of {variants}"
        );
    }

    #[test]
    fn the_kinds_are_exclusive() {
        for _ in 0..100 {
            let rk = RecoveryKey::generate().unwrap();
            assert!(matches!(classify(&rk.display_groups()), Entry::Recovery(_)));
            let wipe = WipeCode::generate().unwrap();
            assert!(matches!(classify(&wipe.display_groups()), Entry::Wipe(_)));
        }
    }

    #[test]
    fn passwords_are_kept_verbatim() {
        match classify(" Fjalë Kalim-1 ") {
            Entry::Password(p) => assert_eq!(p.as_str(), " Fjalë Kalim-1 "),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn new_password_rules() {
        assert_eq!(check_new_password("12345"), Err(VaultError::TooShort));
        assert_eq!(check_new_password("ëëëëë"), Err(VaultError::TooShort));
        let wipe = WipeCode::generate().unwrap();
        assert_eq!(
            check_new_password(&wipe.display_groups()),
            Err(VaultError::PasswordNotAllowed)
        );
        let rk = RecoveryKey::generate().unwrap();
        assert_eq!(
            check_new_password(&rk.display_groups()),
            Err(VaultError::PasswordNotAllowed)
        );
        assert_eq!(check_new_password("fjalëkalim"), Ok(()));
        assert_eq!(
            check_new_password(&"x".repeat(1025)),
            Err(VaultError::PasswordNotAllowed)
        );
    }

    #[test]
    fn debug_is_redacted() {
        let wipe = WipeCode::generate().unwrap();
        assert_eq!(format!("{wipe:?}"), "WipeCode(<redacted>)");
        let entry = classify("my password");
        assert!(!format!("{entry:?}").contains("password"));
    }
}
