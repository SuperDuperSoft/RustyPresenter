//! Dates in `usage.json`: days as `"YYYY-MM-DD"`, instants as RFC 3339 (chrono is built without its serde feature).
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serializer, de::Error};

const DAY: &str = "%Y-%m-%d";

pub mod date {
    use super::*;
    pub fn serialize<S: Serializer>(d: &NaiveDate, s: S) -> Result<S::Ok, S::Error> { s.serialize_str(&d.format(DAY).to_string()) }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<NaiveDate, D::Error> {
        let text = String::deserialize(d)?;
        NaiveDate::parse_from_str(&text, DAY).map_err(D::Error::custom)
    }
}

pub mod opt_date {
    use super::*;
    pub fn serialize<S: Serializer>(d: &Option<NaiveDate>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_str(&d.format(DAY).to_string()),
            None => s.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<NaiveDate>, D::Error> {
        let text = Option::<String>::deserialize(d)?;
        text.map(|t| NaiveDate::parse_from_str(&t, DAY).map_err(D::Error::custom)).transpose()
    }
}

pub mod instant {
    use super::*;
    pub fn serialize<S: Serializer>(t: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&t.to_rfc3339_opts(SecondsFormat::Secs, true))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        let text = String::deserialize(d)?;
        DateTime::parse_from_rfc3339(&text).map(|t| t.with_timezone(&Utc)).map_err(D::Error::custom)
    }
}
