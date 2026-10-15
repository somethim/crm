//! Every display and parse formatter. UI, reports and io never format ad hoc.
//!
//! - money `1.234,56 Lekë`, amount `1.234,56`, receipt amount `1.234` / `1.234,50`;
//! - quantity `1.234`;
//! - date `dd.MM.yyyy`, timestamp `dd.MM.yyyy HH:mm` (Europe/Tirane);
//! - report range `dd.MM.yyyy - dd.MM.yyyy`.

use crate::messages;
use crate::money::{Money, grouped};
use crate::qty::Qty;
use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

pub use crate::money::{ParseMode, parse_amount};

/// A value typed or imported by the user could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// Not a valid amount.
    #[error("{}", messages::invalid_amount(.0))]
    Amount(String),
    /// A valid amount too large for qindarka in `i64`.
    #[error("{}", messages::amount_out_of_range(.0))]
    AmountOutOfRange(String),
    /// Not a valid whole quantity.
    #[error("{}", messages::invalid_quantity(.0))]
    Quantity(String),
    /// Not a valid date.
    #[error("{}", messages::invalid_date(.0))]
    Date(String),
}

impl ParseError {
    /// The Albanian message shown to the user.
    pub fn user_message(&self) -> String {
        self.to_string()
    }
}

/// `1.234,56 Lekë`; negatives as `-0,05 Lekë`.
pub fn money(m: Money) -> String {
    format!("{} {}", amount(m), messages::CURRENCY)
}

/// `1.234,56 LEK`: the order list's amounts, with the currency code as Bilanc
/// shows them (the document, receipt and reports keep [`money`]).
pub fn money_lek(m: Money) -> String {
    format!("{} {}", amount(m), messages::CURRENCY_CODE)
}

/// `1.234,56` (no currency suffix).
pub fn amount(m: Money) -> String {
    let abs = m.0.unsigned_abs();
    format!(
        "{}{},{:02}",
        sign(m.0),
        group_thousands(abs / 100),
        abs % 100
    )
}

/// Receipt amount (001 A8): `1.234` when the qindarka are zero, else `1.234,50`.
pub fn amount_compact(m: Money) -> String {
    if m.0 % 100 == 0 {
        format!("{}{}", sign(m.0), group_thousands(m.0.unsigned_abs() / 100))
    } else {
        amount(m)
    }
}

/// `1.234`.
pub fn qty(q: Qty) -> String {
    format!("{}{}", sign(q.0), group_thousands(q.0.unsigned_abs()))
}

/// `dd.MM.yyyy`.
pub fn date(d: Date) -> String {
    format!("{:02}.{:02}.{:04}", d.day(), d.month(), d.year())
}

/// The month and year in Albanian: `Tetor 2026`.
pub fn month_year(d: Date) -> String {
    let index = usize::try_from(d.month() - 1).unwrap_or(0);
    format!("{} {}", messages::MONTHS[index], d.year())
}

/// `dd.MM.yyyy HH:mm` in `tz` (normally [`crate::time::tirane`]).
pub fn timestamp(ts: Timestamp, tz: &TimeZone) -> String {
    let dt = ts.to_zoned(tz.clone()).datetime();
    format!("{} {:02}:{:02}", date(dt.date()), dt.hour(), dt.minute())
}

/// `dd.MM.yyyy - dd.MM.yyyy` (report header; Q7).
pub fn report_range(from: Date, to: Date) -> String {
    format!("{} - {}", date(from), date(to))
}

/// Parses `d.M.yyyy`, `dd.MM.yyyy` or `yyyy-MM-dd`.
pub fn parse_date(input: &str) -> Result<Date, ParseError> {
    let s = input.trim();
    let invalid = || ParseError::Date(s.to_owned());
    let (year, month, day) = if s.contains('-') {
        let mut it = s.split('-');
        let (y, m, d) = (it.next(), it.next(), it.next());
        if it.next().is_some() {
            return Err(invalid());
        }
        (
            number(y, 4, 4).ok_or_else(invalid)?,
            number(m, 2, 2).ok_or_else(invalid)?,
            number(d, 2, 2).ok_or_else(invalid)?,
        )
    } else {
        let mut it = s.split('.');
        let (d, m, y) = (it.next(), it.next(), it.next());
        if it.next().is_some() {
            return Err(invalid());
        }
        (
            number(y, 4, 4).ok_or_else(invalid)?,
            number(m, 1, 2).ok_or_else(invalid)?,
            number(d, 1, 2).ok_or_else(invalid)?,
        )
    };
    let year = i16::try_from(year).map_err(|_| invalid())?;
    let month = i8::try_from(month).map_err(|_| invalid())?;
    let day = i8::try_from(day).map_err(|_| invalid())?;
    Date::new(year, month, day).map_err(|_| invalid())
}

/// Parses a whole quantity: digits with optional `.` thousands separators (`1.234`).
pub fn parse_qty(input: &str) -> Result<Qty, ParseError> {
    let s = input.trim();
    let invalid = || ParseError::Quantity(s.to_owned());
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Err(invalid());
    }
    let digits = if s.contains('.') {
        grouped(s, '.').ok_or_else(invalid)?
    } else {
        s.to_owned()
    };
    digits.parse::<i64>().map(Qty).map_err(|_| invalid())
}

fn number(part: Option<&str>, min: usize, max: usize) -> Option<i64> {
    let part = part?;
    if !(min..=max).contains(&part.len()) || !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

fn sign(v: i64) -> &'static str {
    if v < 0 { "-" } else { "" }
}

fn group_thousands(mut n: u64) -> String {
    let mut groups = Vec::new();
    loop {
        groups.push(n % 1000);
        n /= 1000;
        if n == 0 {
            break;
        }
    }
    let mut out = groups.pop().map(|g| g.to_string()).unwrap_or_default();
    while let Some(g) = groups.pop() {
        out.push_str(&format!(".{g:03}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date as d;

    #[test]
    fn money_formatting() {
        let cases = [
            (0, "0,00 Lekë"),
            (5, "0,05 Lekë"),
            (-5, "-0,05 Lekë"),
            (100, "1,00 Lekë"),
            (123_456, "1.234,56 Lekë"),
            (-123_456, "-1.234,56 Lekë"),
            (100_000_000, "1.000.000,00 Lekë"),
            (99_999, "999,99 Lekë"),
            (i64::MIN, "-92.233.720.368.547.758,08 Lekë"),
        ];
        for (minor, want) in cases {
            assert_eq!(money(Money(minor)), want);
        }
        assert_eq!(amount(Money(123_456)), "1.234,56");
    }

    #[test]
    fn money_with_the_currency_code() {
        assert_eq!(money_lek(Money(123_456)), "1.234,56 LEK");
        assert_eq!(money_lek(Money(-5)), "-0,05 LEK");
        assert_eq!(money_lek(Money(0)), "0,00 LEK");
    }

    #[test]
    fn compact_amounts() {
        assert_eq!(amount_compact(Money(123_400)), "1.234");
        assert_eq!(amount_compact(Money(123_450)), "1.234,50");
        assert_eq!(amount_compact(Money(123_405)), "1.234,05");
        assert_eq!(amount_compact(Money(0)), "0");
        assert_eq!(amount_compact(Money(-850)), "-8,50");
        assert_eq!(amount_compact(Money(-800)), "-8");
    }

    #[test]
    fn quantities() {
        assert_eq!(qty(Qty(0)), "0");
        assert_eq!(qty(Qty(999)), "999");
        assert_eq!(qty(Qty(1_234)), "1.234");
        assert_eq!(qty(Qty(-1_234_567)), "-1.234.567");
        for (input, want) in [
            ("1.234", 1_234),
            ("12", 12),
            (" 1.000.000 ", 1_000_000),
            ("0", 0),
        ] {
            assert_eq!(parse_qty(input), Ok(Qty(want)), "{input}");
        }
        for input in ["", "-1", "1,5", "1.23", "abc", "1.2345", ".123"] {
            assert!(parse_qty(input).is_err(), "{input}");
        }
        assert_eq!(
            parse_qty("x").unwrap_err().user_message(),
            "«x» nuk është një sasi e vlefshme."
        );
    }

    #[test]
    fn month_years_are_albanian() {
        assert_eq!(month_year(d(2026, 10, 14)), "Tetor 2026");
        assert_eq!(month_year(d(2026, 1, 1)), "Janar 2026");
        assert_eq!(month_year(d(2026, 11, 30)), "Nëntor 2026");
        assert_eq!(month_year(d(2028, 12, 31)), "Dhjetor 2028");
    }

    #[test]
    fn dates_round_trip() {
        assert_eq!(date(d(2026, 3, 5)), "05.03.2026");
        for day in [d(2026, 1, 1), d(2024, 2, 29), d(2026, 12, 31)] {
            assert_eq!(parse_date(&date(day)), Ok(day));
            assert_eq!(parse_date(&day.to_string()), Ok(day));
        }
        assert_eq!(parse_date("5.3.2026"), Ok(d(2026, 3, 5)));
        assert_eq!(parse_date(" 2026-03-05 "), Ok(d(2026, 3, 5)));
        for bad in [
            "",
            "31.02.2026",
            "2026-3-5",
            "05/03/2026",
            "5.3.26",
            "1.1.1.2026",
            "abc",
        ] {
            assert!(parse_date(bad).is_err(), "{bad}");
        }
        assert_eq!(
            parse_date("x").unwrap_err().user_message(),
            "«x» nuk është një datë e vlefshme (dd.MM.yyyy)."
        );
        assert_eq!(
            report_range(d(2026, 1, 1), d(2026, 1, 31)),
            "01.01.2026 - 31.01.2026"
        );
    }

    #[test]
    fn timestamps_in_tirane() {
        let tz = crate::time::tirane();
        // 2026-07-01 10:30 UTC is 12:30 in Tirana (CEST, UTC+2).
        let summer: Timestamp = "2026-07-01T10:30:00Z".parse().unwrap();
        assert_eq!(timestamp(summer, &tz), "01.07.2026 12:30");
        // 2026-01-31 23:15 UTC is 00:15 on 1 February in Tirana (CET, UTC+1).
        let winter: Timestamp = "2026-01-31T23:15:00Z".parse().unwrap();
        assert_eq!(timestamp(winter, &tz), "01.02.2026 00:15");
    }
}
