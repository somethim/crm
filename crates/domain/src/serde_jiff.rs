//! Serde for jiff values without jiff's `serde` feature (not enabled in the
//! workspace pins). Formats match jiff's own: `YYYY-MM-DD` and RFC 3339.

/// `jiff::civil::Date` as `"YYYY-MM-DD"`.
pub mod date {
    use jiff::civil::Date;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(value: &Date, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Date, D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        text.parse().map_err(D::Error::custom)
    }
}

/// `jiff::Timestamp` as an RFC 3339 string.
pub mod timestamp {
    use jiff::Timestamp;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(value: &Timestamp, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Timestamp, D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        text.parse().map_err(D::Error::custom)
    }
}
