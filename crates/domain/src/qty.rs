//! Whole-number quantities (stock and line quantities).

use serde::{Deserialize, Serialize};
use std::iter::Sum;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// A whole number of units. Stock is never negative; deltas may be.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Qty(pub i64);

impl Qty {
    /// Zero units.
    pub const ZERO: Qty = Qty(0);

    /// The raw number.
    pub const fn get(self) -> i64 {
        self.0
    }

    /// True when below zero.
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }
}

impl Add for Qty {
    type Output = Qty;
    fn add(self, rhs: Qty) -> Qty {
        Qty(self.0 + rhs.0)
    }
}

impl AddAssign for Qty {
    fn add_assign(&mut self, rhs: Qty) {
        self.0 += rhs.0;
    }
}

impl Sub for Qty {
    type Output = Qty;
    fn sub(self, rhs: Qty) -> Qty {
        Qty(self.0 - rhs.0)
    }
}

impl SubAssign for Qty {
    fn sub_assign(&mut self, rhs: Qty) {
        self.0 -= rhs.0;
    }
}

impl Neg for Qty {
    type Output = Qty;
    fn neg(self) -> Qty {
        Qty(-self.0)
    }
}

impl Sum for Qty {
    fn sum<I: Iterator<Item = Qty>>(iter: I) -> Qty {
        iter.fold(Qty::ZERO, Add::add)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        assert_eq!(Qty(5) + Qty(3), Qty(8));
        assert_eq!(Qty(5) - Qty(8), Qty(-3));
        assert!((Qty(5) - Qty(8)).is_negative());
        assert_eq!(-Qty(2), Qty(-2));
        assert_eq!([Qty(1), Qty(2)].into_iter().sum::<Qty>(), Qty(3));
    }
}
