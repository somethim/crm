//! `data/crm.key` v1: the data key wrapped under the password and under the
//! recovery key, plus the wipe-code verifier. Fixed 260 bytes, little-endian:
//!
//! | Offset | Size | Field |
//! |---:|---:|---|
//! | 0 | 6 | magic `CRMKEY` |
//! | 6 | 1 | format version 1 |
//! | 7 | 1 | KDF id 1 (Argon2id v0x13) |
//! | 8 | 12 | `m_kib`, `t`, `p` (u32 each) |
//! | 20 | 16 | `db_id` |
//! | 36 | 88 | password slot: salt 16 · nonce 24 · ciphertext 32 · tag 16 |
//! | 124 | 88 | recovery slot (same layout) |
//! | 212 | 48 | wipe verifier: salt 16 · Argon2id(wipe code) 32 |
//!
//! Slot AEAD: XChaCha20-Poly1305 under Argon2id(secret, slot salt), with AAD =
//! header (bytes 0..36) ‖ `P` or `R`.

use super::codes::{RecoveryKey, WipeCode};
use super::kdf::KdfParams;
use crate::error::VaultError;
use chacha20poly1305::aead::array::Array;
use chacha20poly1305::aead::inout::InOutBuf;
use chacha20poly1305::{AeadInOut, KeyInit, XChaCha20Poly1305, XNonce};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

/// The 32-byte data key on the services side.
pub type DataKey = Zeroizing<[u8; 32]>;

/// Size of a v1 key file.
pub const KEY_FILE_LEN: usize = 260;
const MAGIC: &[u8; 6] = b"CRMKEY";
const VERSION: u8 = 1;
const KDF_ARGON2ID: u8 = 1;
const HEADER: usize = 36;
const PASSWORD_SLOT: usize = 36;
const RECOVERY_SLOT: usize = 124;
const WIPE_VERIFIER: usize = 212;
const SLOT_LEN: usize = 88;

/// A parsed (or freshly built) key file. Holds no plaintext secret.
#[derive(Clone, PartialEq, Eq)]
pub struct KeyFile {
    bytes: [u8; KEY_FILE_LEN],
    params: KdfParams,
}

impl std::fmt::Debug for KeyFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyFile")
            .field("params", &self.params)
            .finish_non_exhaustive()
    }
}

fn random<const N: usize>() -> Result<[u8; N], VaultError> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|_| VaultError::Crypto)?;
    Ok(out)
}

/// Constant-time equality over 32 bytes (no early exit).
fn same_32(a: &[u8; 32], b: &[u8]) -> bool {
    let mut diff = u8::from(b.len() != 32);
    for (x, y) in a.iter().zip(b.iter().chain(std::iter::repeat(&0xFF))) {
        diff |= x ^ y;
    }
    std::hint::black_box(diff) == 0
}

impl KeyFile {
    /// Builds a key file for `data_key` with a fresh `db_id`, salts and nonces
    /// (three Argon2 runs).
    pub fn create(
        data_key: &DataKey,
        password: &str,
        recovery: &RecoveryKey,
        wipe: &WipeCode,
        params: KdfParams,
    ) -> Result<KeyFile, VaultError> {
        if !params.in_bounds() {
            return Err(VaultError::KeyFileInvalid);
        }
        let mut bytes = [0u8; KEY_FILE_LEN];
        bytes[..6].copy_from_slice(MAGIC);
        bytes[6] = VERSION;
        bytes[7] = KDF_ARGON2ID;
        bytes[8..12].copy_from_slice(&params.m_kib.to_le_bytes());
        bytes[12..16].copy_from_slice(&params.t.to_le_bytes());
        bytes[16..20].copy_from_slice(&params.p.to_le_bytes());
        bytes[20..36].copy_from_slice(&random::<16>()?);
        let mut file = KeyFile { bytes, params };
        file.seal(PASSWORD_SLOT, b'P', data_key, password.as_bytes())?;
        file.seal(RECOVERY_SLOT, b'R', data_key, recovery.secret())?;
        let salt = random::<16>()?;
        let verifier = params.derive(wipe.secret(), &salt)?;
        file.bytes[WIPE_VERIFIER..WIPE_VERIFIER + 16].copy_from_slice(&salt);
        file.bytes[WIPE_VERIFIER + 16..KEY_FILE_LEN].copy_from_slice(verifier.as_ref());
        Ok(file)
    }

    /// Parses a v1 file: length, magic, version, KDF id and the parameter
    /// bounds (refused before any Argon2 run).
    pub fn parse(bytes: &[u8]) -> Result<KeyFile, VaultError> {
        let bytes: [u8; KEY_FILE_LEN] = bytes.try_into().map_err(|_| VaultError::KeyFileInvalid)?;
        if &bytes[..6] != MAGIC || bytes[6] != VERSION || bytes[7] != KDF_ARGON2ID {
            return Err(VaultError::KeyFileInvalid);
        }
        let word = |at: usize| {
            u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        let params = KdfParams {
            m_kib: word(8),
            t: word(12),
            p: word(16),
        };
        if !params.in_bounds() {
            return Err(VaultError::KeyFileInvalid);
        }
        Ok(KeyFile { bytes, params })
    }

    /// Reads and parses `path`.
    pub fn read(path: &Path) -> Result<KeyFile, VaultError> {
        let bytes = std::fs::read(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => VaultError::KeyMissing,
            _ => VaultError::Io(e.to_string()),
        })?;
        KeyFile::parse(&bytes)
    }

    /// The 260 bytes.
    pub fn to_bytes(&self) -> [u8; KEY_FILE_LEN] {
        self.bytes
    }

    /// The install's random id, the same in every copy of this key file.
    pub fn db_id(&self) -> [u8; 16] {
        let mut id = [0u8; 16];
        id.copy_from_slice(&self.bytes[20..36]);
        id
    }

    /// The KDF parameters stored in the file.
    pub fn params(&self) -> KdfParams {
        self.params
    }

    fn aad(&self, tag: u8) -> [u8; HEADER + 1] {
        let mut aad = [0u8; HEADER + 1];
        aad[..HEADER].copy_from_slice(&self.bytes[..HEADER]);
        aad[HEADER] = tag;
        aad
    }

    fn seal(
        &mut self,
        at: usize,
        tag: u8,
        data_key: &DataKey,
        secret: &[u8],
    ) -> Result<(), VaultError> {
        let salt = random::<16>()?;
        let nonce = random::<24>()?;
        let kek = self.params.derive(secret, &salt)?;
        let cipher = XChaCha20Poly1305::new(&Array::from(*kek));
        let mut buf = Zeroizing::new(**data_key);
        let aad = self.aad(tag);
        let mac = cipher
            .encrypt_inout_detached(&XNonce::from(nonce), &aad, InOutBuf::from(&mut buf[..]))
            .map_err(|_| VaultError::Crypto)?;
        let slot = &mut self.bytes[at..at + SLOT_LEN];
        slot[..16].copy_from_slice(&salt);
        slot[16..40].copy_from_slice(&nonce);
        slot[40..72].copy_from_slice(&buf[..]);
        slot[72..88].copy_from_slice(&mac);
        Ok(())
    }

    fn open(&self, at: usize, tag: u8, secret: &[u8]) -> Option<DataKey> {
        let slot = &self.bytes[at..at + SLOT_LEN];
        let salt: [u8; 16] = slot[..16].try_into().ok()?;
        let nonce: [u8; 24] = slot[16..40].try_into().ok()?;
        let mac: [u8; 16] = slot[72..88].try_into().ok()?;
        let kek = self.params.derive(secret, &salt).ok()?;
        let cipher = XChaCha20Poly1305::new(&Array::from(*kek));
        let mut buf: DataKey = Zeroizing::new([0u8; 32]);
        buf.copy_from_slice(&slot[40..72]);
        cipher
            .decrypt_inout_detached(
                &XNonce::from(nonce),
                &self.aad(tag),
                InOutBuf::from(&mut buf[..]),
                &Array::from(mac),
            )
            .ok()?;
        Some(buf)
    }

    /// The data key, if `password` opens the password slot.
    pub fn open_password(&self, password: &str) -> Option<DataKey> {
        self.open(PASSWORD_SLOT, b'P', password.as_bytes())
    }

    /// The data key, if `recovery` opens the recovery slot.
    pub fn open_recovery(&self, recovery: &RecoveryKey) -> Option<DataKey> {
        self.open(RECOVERY_SLOT, b'R', recovery.secret())
    }

    /// Whether `code` is this file's wipe code (constant-time compare).
    pub fn verify_wipe(&self, code: &WipeCode) -> bool {
        let salt: [u8; 16] = match self.bytes[WIPE_VERIFIER..WIPE_VERIFIER + 16].try_into() {
            Ok(salt) => salt,
            Err(_) => return false,
        };
        match self.params.derive(code.secret(), &salt) {
            Ok(derived) => same_32(&derived, &self.bytes[WIPE_VERIFIER + 16..]),
            Err(_) => false,
        }
    }

    /// A copy whose password slot opens with `new_password` (fresh salt and
    /// nonce); the header, the recovery slot and the wipe verifier are kept.
    pub fn rewrap_password(
        &self,
        data_key: &DataKey,
        new_password: &str,
    ) -> Result<KeyFile, VaultError> {
        let mut file = self.clone();
        file.seal(PASSWORD_SLOT, b'P', data_key, new_password.as_bytes())?;
        Ok(file)
    }
}

/// Writes `<path>.tmp`, syncs it, then renames it over `path`.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), VaultError> {
    let io = |e: std::io::Error| VaultError::Io(e.to_string());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp).map_err(io)?;
        file.write_all(bytes).map_err(io)?;
        file.sync_all().map_err(io)?;
    }
    std::fs::rename(&tmp, path).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Even cheaper than `KdfParams::TEST`: the byte-flip matrix derives ~800 times.
    const TINY: KdfParams = KdfParams {
        m_kib: 64,
        t: 1,
        p: 1,
    };

    fn sample() -> (KeyFile, DataKey, RecoveryKey, WipeCode) {
        let key: DataKey = Zeroizing::new([9u8; 32]);
        let rk = RecoveryKey::generate().unwrap();
        let wipe = WipeCode::generate().unwrap();
        let file = KeyFile::create(&key, "fjalëkalim", &rk, &wipe, TINY).unwrap();
        (file, key, rk, wipe)
    }

    #[test]
    fn both_secrets_open_the_same_key() {
        let (file, key, rk, wipe) = sample();
        assert_eq!(file.to_bytes().len(), KEY_FILE_LEN);
        assert_eq!(*file.open_password("fjalëkalim").unwrap(), *key);
        assert_eq!(*file.open_recovery(&rk).unwrap(), *key);
        assert!(file.verify_wipe(&wipe));
        assert!(file.open_password("fjalëkalin").is_none());
        assert!(!file.verify_wipe(&WipeCode::generate().unwrap()));
        assert!(
            file.open_recovery(&RecoveryKey::generate().unwrap())
                .is_none()
        );
        let parsed = KeyFile::parse(&file.to_bytes()).unwrap();
        assert_eq!(parsed, file);
        assert_eq!(parsed.params(), TINY);
    }

    #[test]
    fn any_flipped_byte_breaks_what_it_covers() {
        let (file, _key, rk, wipe) = sample();
        let bytes = file.to_bytes();
        for offset in 0..KEY_FILE_LEN {
            let mut tampered = bytes;
            tampered[offset] ^= 0x01;
            let Ok(t) = KeyFile::parse(&tampered) else {
                // Magic, version, KDF id or out-of-range parameters.
                assert!(offset < 20, "parse refused offset {offset}");
                continue;
            };
            let password = t.open_password("fjalëkalim").is_some();
            let recovery = t.open_recovery(&rk).is_some();
            let wipe_ok = t.verify_wipe(&wipe);
            match offset {
                // The header is in both AADs (and the parameters feed every derive).
                o if o < HEADER => assert!(!password && !recovery, "{o}"),
                o if o < RECOVERY_SLOT => assert!(!password && recovery && wipe_ok, "{o}"),
                o if o < WIPE_VERIFIER => assert!(password && !recovery && wipe_ok, "{o}"),
                o => assert!(password && recovery && !wipe_ok, "{o}"),
            }
        }
    }

    #[test]
    fn huge_params_are_refused_by_parse() {
        let (file, ..) = sample();
        let mut bytes = file.to_bytes();
        bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(KeyFile::parse(&bytes), Err(VaultError::KeyFileInvalid));
        assert_eq!(
            KeyFile::parse(&bytes[..259]),
            Err(VaultError::KeyFileInvalid)
        );
        let mut bad_magic = file.to_bytes();
        bad_magic[0] = b'X';
        assert_eq!(KeyFile::parse(&bad_magic), Err(VaultError::KeyFileInvalid));
    }

    #[test]
    fn rewrap_keeps_the_id_and_the_recovery_slot() {
        let (file, key, rk, wipe) = sample();
        let new = file.rewrap_password(&key, "tjetër-fjalëkalim").unwrap();
        assert_eq!(new.db_id(), file.db_id());
        assert!(new.open_password("fjalëkalim").is_none());
        assert_eq!(*new.open_password("tjetër-fjalëkalim").unwrap(), *key);
        assert_eq!(*new.open_recovery(&rk).unwrap(), *key);
        assert!(new.verify_wipe(&wipe));
    }

    #[test]
    fn write_atomic_replaces_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data").join("crm.key");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert!(!dir.path().join("data").join("crm.key.tmp").exists());
    }
}
