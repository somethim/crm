//! The failed-attempt lockout (007): five failures in a row → 30 s, shared by
//! the start-up gate and the in-app lock through one [`Vault`](super::Vault).

use std::time::{Duration, Instant};

/// Failed attempts that trigger the lockout.
pub const MAX_ATTEMPTS: u32 = 5;
/// How long the lockout lasts.
pub const LOCKOUT: Duration = Duration::from_secs(30);

/// The in-memory failure counter (it resets on restart; accepted in 007).
#[derive(Debug, Default)]
pub(crate) struct AttemptGuard {
    failures: u32,
    locked_until: Option<Instant>,
}

impl AttemptGuard {
    /// `Err(seconds left, rounded up)` while locked out.
    pub(crate) fn check(&mut self, now: Instant) -> Result<(), u64> {
        match self.locked_until {
            Some(until) if until > now => {
                let left = until - now;
                Err(left.as_secs() + u64::from(left.subsec_nanos() > 0))
            }
            Some(_) => {
                self.locked_until = None;
                self.failures = 0;
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// Records a failure; the fifth starts the lockout and returns its seconds.
    pub(crate) fn fail(&mut self, now: Instant) -> Option<u64> {
        self.failures += 1;
        if self.failures >= MAX_ATTEMPTS {
            self.failures = 0;
            self.locked_until = Some(now + LOCKOUT);
            Some(LOCKOUT.as_secs())
        } else {
            None
        }
    }

    pub(crate) fn succeed(&mut self) {
        self.failures = 0;
        self.locked_until = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockout_after_five_failures_then_expires() {
        let start = Instant::now();
        let mut guard = AttemptGuard::default();
        for _ in 0..4 {
            assert!(guard.check(start).is_ok());
            assert_eq!(guard.fail(start), None);
        }
        assert_eq!(guard.fail(start), Some(30));
        assert_eq!(guard.check(start), Err(30));
        assert_eq!(guard.check(start + Duration::from_millis(10_500)), Err(20));
        assert!(guard.check(start + LOCKOUT).is_ok());
        // The counter starts over after the lockout.
        assert_eq!(guard.fail(start + LOCKOUT), None);
    }

    #[test]
    fn success_resets_the_counter() {
        let now = Instant::now();
        let mut guard = AttemptGuard::default();
        for _ in 0..4 {
            guard.fail(now);
        }
        guard.succeed();
        for _ in 0..4 {
            assert_eq!(guard.fail(now), None);
        }
    }
}
