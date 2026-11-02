//! FEAT-5 shared types. Standalone here so Part B can re-export them from `model.rs`
//! without moving them; serde shapes match spec §4 exactly.
use crate::model::VerseText;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// `Data.notes_seen` key for the one-time "slides are now fitted" toast.
pub const UPGRADE_NOTE_KEY: &str = "feat5-measured-breaks";

/// Canonical verse identity in the English (KJV-style) versification: 0-based index
/// into `bible::BOOKS`, chapter, verse. Derived from labels, never stored on `VerseText`.
/// Serialised as `[book, chapter, verse]`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VerseId(pub u8, pub u16, pub u16);

impl VerseId {
    pub fn book(self) -> u8 { self.0 }
    pub fn chapter(self) -> u16 { self.1 }
    pub fn verse(self) -> u16 { self.2 }
}

/// How two translations share a slide. A layout from a newer build loads as `SideBySide`
/// (serde needs the `other` variant last, so `SideBySide` is listed second).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum ParallelLayout {
    Stacked,
    #[default]
    #[serde(other)]
    SideBySide,
}

/// An item's second translation. `verses` is stored only for `offline` translations;
/// online text lives in the scripture text cache (spec §5.1a).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct SecondText {
    pub translation: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verses: Vec<VerseText>,
}

/// Manual slide breaks pinned to verse boundaries; a mark on verse v is the gap before v.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[serde(default)]
pub struct VerseBreaks {
    /// Always start a new slide before these verses.
    pub before: BTreeSet<VerseId>,
    /// Do not start an automatic new slide before these verses.
    pub join: BTreeSet<VerseId>,
}

impl VerseBreaks {
    pub fn is_empty(&self) -> bool { self.before.is_empty() && self.join.is_empty() }
}

/// Where a generated scripture slide's text comes from. `part` 0 is whole verse(s)
/// or the first part of a verse split across slides.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlideSource { pub first: VerseId, pub last: VerseId, pub part: u8 }

/// The second translation's text for one parallel slide (plain; no styles).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ParallelText { pub main_code: String, pub second_code: String, pub text: String, pub layout: ParallelLayout }

/// Church default for two translations (`Settings.bible`). `None` = one translation.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct BibleSettings { pub second_translation: Option<String>, pub layout: ParallelLayout }

/// The full FEAT-5 shape of `ItemKind::Scripture`, with the same field attributes
/// the enum variant gets in Part B.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ScriptureFields {
    pub reference: String,
    pub translation: String,
    /// Stored only when `translation` is `offline`; empty on disk otherwise (spec §5.1a).
    pub verses: Vec<VerseText>,
    #[serde(default)]
    pub per_slide: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second: Option<SecondText>,
    #[serde(default)]
    pub layout: ParallelLayout,
    #[serde(default, skip_serializing_if = "VerseBreaks::is_empty")]
    pub breaks: VerseBreaks,
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = r#"{"reference":"John 3:16","translation":"KJV","verses":[{"reference":"John 3:16","text":"For God so loved the world"}],"per_slide":0}"#;

    #[test]
    fn old_scripture_item_loads_with_defaults() {
        let f: ScriptureFields = serde_json::from_str(OLD).unwrap();
        assert_eq!((f.second, f.layout, f.breaks.is_empty()), (None, ParallelLayout::SideBySide, true));
    }
    #[test]
    fn verse_breaks_is_empty_skips_serializing() {
        let f: ScriptureFields = serde_json::from_str(OLD).unwrap();
        let v = serde_json::to_value(&f).unwrap();
        assert!(v.get("breaks").is_none() && v.get("second").is_none());
    }
    #[test]
    fn parallel_layout_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&ParallelLayout::SideBySide).unwrap(), "\"side_by_side\"");
        assert_eq!(serde_json::to_string(&ParallelLayout::Stacked).unwrap(), "\"stacked\"");
    }
    #[test]
    fn verse_id_serializes_as_triple_and_breaks_round_trip() {
        assert_eq!(serde_json::to_string(&VerseId(42, 3, 16)).unwrap(), "[42,3,16]");
        let b: VerseBreaks = serde_json::from_str(r#"{"before":[[42,3,17]]}"#).unwrap();
        assert_eq!(b.before.iter().copied().collect::<Vec<_>>(), vec![VerseId(42, 3, 17)]);
        assert!(b.join.is_empty());
    }
    #[test]
    fn bible_settings_from_empty_object_is_default() {
        assert_eq!(serde_json::from_str::<BibleSettings>("{}").unwrap(), BibleSettings::default());
    }
    #[test]
    fn second_text_offline_verses_round_trip_and_online_omits_empty() {
        let s = SecondText { translation: "WEB".into(), verses: vec![VerseText { reference: "John 3:16".into(), text: "For God so loved".into() }] };
        let back: SecondText = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let online = SecondText { translation: "NIV".into(), verses: vec![] };
        assert_eq!(serde_json::to_string(&online).unwrap(), r#"{"translation":"NIV"}"#);
    }
    #[test]
    fn unknown_parallel_layout_loads_as_side_by_side() {
        assert_eq!(serde_json::from_str::<ParallelLayout>("\"three_column\"").unwrap(), ParallelLayout::SideBySide);
        let s: BibleSettings = serde_json::from_str(r#"{"second_translation":"NIV","layout":"three_column"}"#).unwrap();
        assert_eq!((s.second_translation.as_deref(), s.layout), (Some("NIV"), ParallelLayout::SideBySide));
        let f: ScriptureFields = serde_json::from_str(&OLD.replace("\"per_slide\":0", "\"per_slide\":0,\"layout\":\"three_column\"")).unwrap();
        assert_eq!(f.layout, ParallelLayout::SideBySide);
    }
}
