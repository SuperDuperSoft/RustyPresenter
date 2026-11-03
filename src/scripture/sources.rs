//! Translation sources: the `bible_sources` rows plus the translations stored on this computer.
use super::strings::{STATUS_NEEDS_KEY, STATUS_OFFLINE, STATUS_ONLINE, STATUS_UNAVAILABLE};
use serde::{Deserialize, Serialize};

/// A translation's status in `bible_sources`. A status from a newer build loads as `Unknown`
/// and counts as not usable.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    Offline,
    Online,
    NeedsKey,
    Unavailable,
    #[serde(other)]
    Unknown,
}

/// One row of `state_json["bible_sources"]`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SourceInfo {
    pub translation: String,
    pub status: SourceStatus,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub signup_url: Option<String>,
    #[serde(default)]
    pub copyright: Option<String>,
}

/// The `bible_sources` rows plus `local`: the translation codes stored on this computer
/// (KJV and every bundled/imported translation).
#[derive(Clone, Debug, Default)]
pub struct Sources {
    pub rows: Vec<SourceInfo>,
    pub local: Vec<String>,
}

impl Sources {
    /// Before `bible_sources` is known: only KJV, offline.
    pub fn kjv_only() -> Sources { Sources { rows: vec![], local: vec!["KJV".into()] } }

    fn row(&self, tr: &str) -> Option<&SourceInfo> {
        self.rows.iter().find(|r| r.translation.eq_ignore_ascii_case(tr.trim()))
    }

    fn is_local(&self, tr: &str) -> bool { self.local.iter().any(|l| l.eq_ignore_ascii_case(tr.trim())) }
}

/// The row's status; with no row, `Offline` for a translation stored here, else `Unavailable`.
pub fn status_of(sources: &Sources, tr: &str) -> SourceStatus {
    match sources.row(tr) {
        Some(r) => r.status,
        None if sources.is_local(tr) => SourceStatus::Offline,
        None => SourceStatus::Unavailable,
    }
}

/// Offline and online translations can be fetched.
pub fn is_usable(s: SourceStatus) -> bool { matches!(s, SourceStatus::Offline | SourceStatus::Online) }

/// Has a `bible_sources` row or is stored on this computer.
pub fn is_known(sources: &Sources, tr: &str) -> bool { sources.row(tr).is_some() || sources.is_local(tr) }

/// The translation's copyright notice; `None` when missing or blank.
pub fn copyright_of(sources: &Sources, tr: &str) -> Option<String> {
    sources.row(tr)?.copyright.as_deref().map(str::trim).filter(|c| !c.is_empty()).map(str::to_string)
}

/// The Settings wording for a status.
pub fn status_wording(s: SourceStatus) -> &'static str {
    match s {
        SourceStatus::Offline => STATUS_OFFLINE,
        SourceStatus::Online => STATUS_ONLINE,
        SourceStatus::NeedsKey => STATUS_NEEDS_KEY,
        SourceStatus::Unavailable | SourceStatus::Unknown => STATUS_UNAVAILABLE,
    }
}

/// The row's provider; `""` with no row.
pub fn provider_of<'a>(sources: &'a Sources, tr: &str) -> &'a str { sources.row(tr).map_or("", |r| r.provider.as_str()) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripture::test_support::row;

    #[test]
    fn bible_sources_row_parses() {
        let r: SourceInfo = serde_json::from_str(r#"{"translation":"NIV","status":"needs_key","provider":"api.bible","signup_url":"https://example.org/signup"}"#).unwrap();
        assert_eq!((r.status, r.copyright.clone()), (SourceStatus::NeedsKey, None));
    }
    #[test]
    fn unknown_status_loads_as_unknown_and_is_not_usable() {
        let r: SourceInfo = serde_json::from_str(r#"{"translation":"NIV","status":"beta","provider":"x"}"#).unwrap();
        assert_eq!(r.status, SourceStatus::Unknown);
        assert!(!is_usable(r.status));
    }
    #[test]
    fn kjv_is_offline_before_sources_are_known() {
        assert_eq!(status_of(&Sources::kjv_only(), "KJV"), SourceStatus::Offline);
        assert_eq!(status_of(&Sources::kjv_only(), "NIV"), SourceStatus::Unavailable);
    }
    #[test]
    fn local_translations_are_offline_before_sources_are_known() {
        let s = Sources { rows: vec![], local: vec!["KJV".into(), "WEB".into(), "BSB".into()] };
        assert_eq!((status_of(&s, "WEB"), status_of(&s, "bsb"), status_of(&s, "ESV")),
                   (SourceStatus::Offline, SourceStatus::Offline, SourceStatus::Unavailable));
        let with_row = Sources { rows: vec![row("WEB", "unavailable", None)], ..s };
        assert_eq!(status_of(&with_row, "WEB"), SourceStatus::Unavailable);   // a row always wins
    }
    #[test]
    fn copyright_comes_from_bible_sources() {
        let s = Sources { rows: vec![row("KJV", "offline", Some("")), row("NIV", "online", Some("Scripture quotations … NIV®")), row("ESV", "online", None)], local: vec![] };
        assert_eq!((copyright_of(&s, "KJV"), copyright_of(&s, "ESV"), copyright_of(&s, "CSB")), (None, None, None));
        assert_eq!(copyright_of(&s, "NIV").as_deref(), Some("Scripture quotations … NIV®"));
    }
}
