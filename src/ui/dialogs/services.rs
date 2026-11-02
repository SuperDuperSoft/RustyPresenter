//! The Services dialog (Plan 3 Task 19). So far only the default name its prompts offer.
use super::strings as tr;
use chrono::{Datelike, NaiveDate};

/// "Service 11/2/2026", as `new Date().toLocaleDateString()` shows on an en-US church PC (`operator.js:803-804`).
pub fn name_for(date: NaiveDate) -> String { tr::default_service_name(&format!("{}/{}/{}", date.month(), date.day(), date.year())) }

/// What the New and Copy prompts start with.
pub fn default_name() -> String { name_for(chrono::Local::now().date_naive()) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_name_has_no_leading_zeros() {
        assert_eq!(name_for(NaiveDate::from_ymd_opt(2026, 3, 8).unwrap()), "Service 3/8/2026");
        assert_eq!(name_for(NaiveDate::from_ymd_opt(2026, 11, 22).unwrap()), "Service 11/22/2026");
    }

    #[test]
    fn default_name_is_today() {
        assert!(default_name().starts_with("Service "));
    }
}
