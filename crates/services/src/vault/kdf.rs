//! Argon2id key derivation with per-file parameters.

use crate::error::VaultError;
use argon2::{Algorithm, Argon2, Params, Version};
use zeroize::Zeroizing;

/// Largest accepted memory cost (1 GiB), checked before Argon2 runs.
pub(crate) const MAX_M_KIB: u32 = 1_048_576;
/// Largest accepted time cost.
pub(crate) const MAX_T: u32 = 16;
/// Largest accepted parallelism.
pub(crate) const MAX_P: u32 = 8;

/// Argon2id v0x13 parameters, stored in every key file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    /// Memory in KiB.
    pub m_kib: u32,
    /// Passes.
    pub t: u32,
    /// Lanes.
    pub p: u32,
}

impl KdfParams {
    /// 64 MiB, 3 passes, 1 lane.
    pub const PRODUCTION: KdfParams = KdfParams {
        m_kib: 65_536,
        t: 3,
        p: 1,
    };

    /// Cheap parameters for tests (8 MiB, 1 pass).
    #[doc(hidden)]
    pub const TEST: KdfParams = KdfParams {
        m_kib: 8_192,
        t: 1,
        p: 1,
    };

    /// Within the parse bounds (no lower bound, so tests can be cheap).
    pub(crate) fn in_bounds(&self) -> bool {
        self.m_kib <= MAX_M_KIB && self.t <= MAX_T && self.p <= MAX_P
    }

    /// 32 bytes from `secret` and `salt`.
    pub(crate) fn derive(
        &self,
        secret: &[u8],
        salt: &[u8; 16],
    ) -> Result<Zeroizing<[u8; 32]>, VaultError> {
        if !self.in_bounds() {
            return Err(VaultError::KeyFileInvalid);
        }
        let params =
            Params::new(self.m_kib, self.t, self.p, Some(32)).map_err(|_| VaultError::Crypto)?;
        let mut out = Zeroizing::new([0u8; 32]);
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(secret, salt, out.as_mut())
            .map_err(|_| VaultError::Crypto)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_is_deterministic_and_salted() {
        let a = KdfParams::TEST.derive(b"secret", &[1; 16]).unwrap();
        let b = KdfParams::TEST.derive(b"secret", &[1; 16]).unwrap();
        let c = KdfParams::TEST.derive(b"secret", &[2; 16]).unwrap();
        assert_eq!(*a, *b);
        assert_ne!(*a, *c);
    }

    #[test]
    fn out_of_bounds_is_refused_before_argon2() {
        let huge = KdfParams {
            m_kib: u32::MAX,
            t: 1,
            p: 1,
        };
        assert_eq!(
            huge.derive(b"x", &[0; 16]).unwrap_err(),
            VaultError::KeyFileInvalid
        );
    }

    #[test]
    #[ignore = "timing only"]
    fn production_params_timing() {
        let start = std::time::Instant::now();
        KdfParams::PRODUCTION.derive(b"password", &[7; 16]).unwrap();
        println!("Argon2id PRODUCTION: {:?}", start.elapsed());
    }
}
