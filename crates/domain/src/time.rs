//! The clock, in Europe/Tirane.
//!
//! jiff reads the system tz database on Linux and bundles it on Windows, so the
//! lookup works on both. If it still fails, the system zone is used.

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

/// The IANA name of the business time zone.
pub const TIME_ZONE: &str = "Europe/Tirane";

/// The Europe/Tirane zone, or the system zone when the lookup fails.
pub fn tirane() -> TimeZone {
    TimeZone::get(TIME_ZONE).unwrap_or_else(|_| TimeZone::system())
}

/// Today's date in Tirana (the default `issued_on`; 001 A2).
pub fn today() -> Date {
    Timestamp::now().to_zoned(tirane()).date()
}

/// The current instant.
pub fn now() -> Timestamp {
    Timestamp::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tirane_zone_resolves() {
        assert_eq!(tirane().iana_name(), Some(TIME_ZONE));
        let today = today();
        assert!(today.year() >= 2026);
        assert!(now() > Timestamp::UNIX_EPOCH);
    }
}
