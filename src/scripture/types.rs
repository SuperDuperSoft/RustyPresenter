//! FEAT-5 shared types. Standalone here so Part B can re-export them from `model.rs`
//! without moving them; serde shapes match spec §4 exactly.
use crate::model::VerseText;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// `Data.notes_seen` key for the one-time "slides are now fitted" toast.
pub const UPGRADE_NOTE_KEY: &str = "feat5-measured-breaks";

/// Canonical verse identity in the English (KJV-style) versification: 0-based index
/// into `bible::BOOKS`, chapter, verse. Stored on `VerseText::id` when the lookup knows it, else derived from labels.
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

/// One more Bible shown with an item's main one (Nov 5 spec §2). `verses` is stored only for Bibles kept on this
/// computer; online text lives in the capped cache (Nov 5 spec §5).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct AlsoShow {
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

/// One Also Bible's text for one parallel slide (plain; no styles), headed by its code.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ParallelText { pub code: String, pub text: String }

/// The Also Bibles of one scripture slide, drawn beside or under the main text (Nov 5 spec §3).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct Parallel {
    /// The main Bible's code, heading its column.
    pub main_code: String,
    /// One column per Also Bible, in order.
    pub also: Vec<ParallelText>,
    #[serde(default)]
    pub layout: ParallelLayout,
}

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also: Vec<AlsoShow>,
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
        assert_eq!((f.also.is_empty(), f.layout, f.breaks.is_empty()), (true, ParallelLayout::SideBySide, true));
    }
    #[test]
    fn verse_breaks_is_empty_skips_serializing() {
        let f: ScriptureFields = serde_json::from_str(OLD).unwrap();
        let v = serde_json::to_value(&f).unwrap();
        assert!(v.get("breaks").is_none() && v.get("also").is_none());
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
    fn also_show_offline_verses_round_trip_and_online_omits_empty() {
        let s = AlsoShow { translation: "WEB".into(), verses: vec![VerseText::new("John 3:16", "For God so loved")] };
        let back: AlsoShow = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let online = AlsoShow { translation: "NIV".into(), verses: vec![] };
        assert_eq!(serde_json::to_string(&online).unwrap(), r#"{"translation":"NIV"}"#);
    }
    #[test]
    fn unknown_parallel_layout_loads_as_side_by_side() {
        assert_eq!(serde_json::from_str::<ParallelLayout>("\"three_column\"").unwrap(), ParallelLayout::SideBySide);
        let f: ScriptureFields = serde_json::from_str(&OLD.replace("\"per_slide\":0", "\"per_slide\":0,\"layout\":\"three_column\"")).unwrap();
        assert_eq!(f.layout, ParallelLayout::SideBySide);
    }
}
