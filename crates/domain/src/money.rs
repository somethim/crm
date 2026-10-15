//! Money as integer qindarka (1 Lek = 100 qindarka), and amount parsing.
//!
//! `rust_decimal` is used only inside [`parse_amount`]; everything else is `i64`.

use crate::format::ParseError;
use crate::qty::Qty;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use std::iter::Sum;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};
use std::str::FromStr;

/// An amount in qindarka (minor units of the Lek).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Money(pub i64);

impl Money {
    /// Zero Lekë.
    pub const ZERO: Money = Money(0);

    /// The raw qindarka.
    pub const fn minor(self) -> i64 {
        self.0
    }

    /// True when below zero.
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// `self × qty` (a line value), or `None` on overflow.
    pub fn checked_mul_qty(self, qty: Qty) -> Option<Money> {
        self.0.checked_mul(qty.0).map(Money)
    }

    /// `self + other`, or `None` on overflow.
    pub fn checked_add(self, other: Money) -> Option<Money> {
        self.0.checked_add(other.0).map(Money)
    }
}

impl Add for Money {
    type Output = Money;
    fn add(self, rhs: Money) -> Money {
        Money(self.0 + rhs.0)
    }
}

impl AddAssign for Money {
    fn add_assign(&mut self, rhs: Money) {
        self.0 += rhs.0;
    }
}

impl Sub for Money {
    type Output = Money;
    fn sub(self, rhs: Money) -> Money {
        Money(self.0 - rhs.0)
    }
}

impl SubAssign for Money {
    fn sub_assign(&mut self, rhs: Money) {
        self.0 -= rhs.0;
    }
}

impl Neg for Money {
    type Output = Money;
    fn neg(self) -> Money {
        Money(-self.0)
    }
}

impl Sum for Money {
    fn sum<I: Iterator<Item = Money>>(iter: I) -> Money {
        iter.fold(Money::ZERO, Add::add)
    }
}

impl<'a> Sum<&'a Money> for Money {
    fn sum<I: Iterator<Item = &'a Money>>(iter: I) -> Money {
        iter.copied().sum()
    }
}

/// Where an amount string comes from; decides how a lone separator is read (001 A4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseMode {
    /// CSV/XLSX import: a lone `.` or `,` is always the decimal point (`8.5`, `1.234` = 1,234 Lek).
    Import,
    /// Typed in the app: a lone separator followed by exactly three digits after a
    /// 1–3 digit integer part is a thousands separator (`1.234` = 1234 Lekë).
    Input,
}

/// Parses an amount into qindarka (001 A4).
///
/// Accepts `8`, `8.5`, `8,5`, `1.234,56`, `1,234.56`, `1 234,56`, an optional sign
/// and a trailing `Lekë`/`Leke`/`L` suffix. When both `.` and `,` appear, the last
/// one is the decimal separator. Thousands groups must have three digits. More than
/// two decimals are rounded half-up to qindarka.
pub fn parse_amount(input: &str, mode: ParseMode) -> Result<Money, ParseError> {
    let invalid = || ParseError::Amount(input.trim().to_owned());
    let mut s = input.trim();
    for suffix in ["Lekë", "lekë", "LEKË", "Leke", "leke", "LEKE", "L", "l"] {
        if let Some(rest) = s.strip_suffix(suffix) {
            s = rest;
            break;
        }
    }
    let compact: String = s
        .chars()
        .filter(|c| !matches!(c, ' ' | '\u{a0}' | '\u{202f}' | '\''))
        .collect();
    let (negative, body) = match compact.as_bytes().first() {
        Some(b'-') => (true, &compact[1..]),
        Some(b'+') => (false, &compact[1..]),
        _ => (false, compact.as_str()),
    };
    if body.is_empty()
        || !body.bytes().any(|b| b.is_ascii_digit())
        || !body
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.' || b == b',')
    {
        return Err(invalid());
    }

    let (int_part, frac_part) = split_decimal(body, mode).ok_or_else(invalid)?;
    let text = format!(
        "{}{}.{}",
        if negative { "-" } else { "" },
        if int_part.is_empty() { "0" } else { &int_part },
        if frac_part.is_empty() { "0" } else { frac_part },
    );
    let decimal = Decimal::from_str(&text)
        .map_err(|_| ParseError::AmountOutOfRange(input.trim().to_owned()))?;
    let rounded = decimal.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero);
    rounded
        .checked_mul(Decimal::ONE_HUNDRED)
        .and_then(|d| d.to_i64())
        .map(Money)
        .ok_or_else(|| ParseError::AmountOutOfRange(input.trim().to_owned()))
}

/// Splits `body` (digits, `.` and `,` only) into integer digits (grouping removed)
/// and fraction digits, or `None` when the separators are malformed.
fn split_decimal(body: &str, mode: ParseMode) -> Option<(String, &str)> {
    let dots = body.matches('.').count();
    let commas = body.matches(',').count();
    match (dots, commas) {
        (0, 0) => Some((body.to_owned(), "")),
        (_, 0) | (0, _) => {
            let sep = if dots > 0 { '.' } else { ',' };
            let count = dots + commas;
            if count > 1 {
                // Repeated separator: thousands grouping, no decimals.
                return grouped(body, sep).map(|digits| (digits, ""));
            }
            let (int_part, frac_part) = body.split_once(sep)?;
            let thousands =
                mode == ParseMode::Input && frac_part.len() == 3 && is_leading_group(int_part);
            if thousands {
                Some((format!("{int_part}{frac_part}"), ""))
            } else {
                Some((int_part.to_owned(), frac_part))
            }
        }
        _ => {
            // Both present: the last one is the decimal separator, exactly once.
            let last = body.rfind(['.', ','])?;
            let dec = body.as_bytes()[last] as char;
            let thousands = if dec == '.' { ',' } else { '.' };
            let (int_part, frac_part) = (&body[..last], &body[last + 1..]);
            if int_part.contains(dec) || frac_part.contains(thousands) {
                return None;
            }
            Some((grouped(int_part, thousands)?, frac_part))
        }
    }
}

/// A valid first thousands group: 1–3 digits, no leading zero.
fn is_leading_group(group: &str) -> bool {
    (1..=3).contains(&group.len()) && !group.starts_with('0')
}

/// Removes `sep` from `s` when it groups digits as `d{1,3}(sep ddd)*`.
pub(crate) fn grouped(s: &str, sep: char) -> Option<String> {
    let mut parts = s.split(sep);
    let first = parts.next()?;
    if !is_leading_group(first) {
        return None;
    }
    let mut out = first.to_owned();
    for part in parts {
        if part.len() != 3 {
            return None;
        }
        out.push_str(part);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        let a = Money(150);
        let b = Money(50);
        assert_eq!(a + b, Money(200));
        assert_eq!(a - b, Money(100));
        assert_eq!(-a, Money(-150));
        assert_eq!([a, b, Money(1)].iter().sum::<Money>(), Money(201));
        assert_eq!(vec![a, b].into_iter().sum::<Money>(), Money(200));
        assert!((b - a).is_negative());
        assert!(!Money::ZERO.is_negative());
        assert_eq!(Money(850).checked_mul_qty(Qty(3)), Some(Money(2550)));
        assert_eq!(Money(i64::MAX).checked_mul_qty(Qty(2)), None);
    }

    #[test]
    fn parse_amount_table() {
        use ParseMode::{Import, Input};
        let ok = [
            ("8.5", Import, 850),
            ("1.234,56", Input, 123_456),
            ("1,234.56", Input, 123_456),
            ("1.234", Input, 123_400),
            ("1.234", Import, 123),
            ("12,345", Input, 1_234_500),
            ("12,345", Import, 1_235),
            ("0,005", Input, 1),
            ("0,004", Input, 0),
            ("8", Input, 800),
            ("8,5", Input, 850),
            ("8,50", Import, 850),
            ("1 234,56", Input, 123_456),
            ("1.234.567", Input, 123_456_700),
            ("1.234.567", Import, 123_456_700),
            ("1.234.567,89", Import, 123_456_789),
            ("  65 Lekë ", Input, 6_500),
            ("65L", Input, 6_500),
            ("-12,5", Input, -1_250),
            ("-0,005", Input, -1),
            ("12.345", Import, 1_235),
            ("67.899", Import, 6_790),
            ("1234,5", Input, 123_450),
            (",5", Input, 50),
            ("8.", Import, 800),
            ("0", Input, 0),
        ];
        for (input, mode, want) in ok {
            assert_eq!(
                parse_amount(input, mode),
                Ok(Money(want)),
                "{input} {mode:?}"
            );
        }
    }

    #[test]
    fn parse_amount_rejects_garbage() {
        for input in [
            "abc",
            "",
            "  ",
            "Lekë",
            "1,2,3.4.5",
            "1.23.4",
            "1..2",
            "12a",
            "1,234.5,6",
            "0.123.456",
            "--1",
            "1.2,3.4",
        ] {
            assert!(
                matches!(
                    parse_amount(input, ParseMode::Input),
                    Err(ParseError::Amount(_))
                ),
                "{input:?}"
            );
        }
        assert!(matches!(
            parse_amount("99999999999999999999", ParseMode::Input),
            Err(ParseError::AmountOutOfRange(_))
        ));
        assert_eq!(
            parse_amount("abc", ParseMode::Import)
                .unwrap_err()
                .user_message(),
            "«abc» nuk është një shumë e vlefshme."
        );
    }
}
