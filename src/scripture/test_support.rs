//! Test-only helpers shared by the FEAT-5 test modules. Later tasks add their own helpers here.
#![allow(dead_code)]
use super::VerseId;
use super::input::{label_of, verse_id_of};
use super::sources::{SourceInfo, Sources};
use crate::model::VerseText;
use std::collections::BTreeSet;
use std::ops::RangeInclusive;
use std::path::PathBuf;

/// A fresh, empty directory under the system temp dir.
pub(crate) fn tmp_root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rp-scripture-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Verses `range` of one chapter, labelled like the readers label them, text `"t{b}-{c}-{v}"`.
pub(crate) fn verses(book: u8, chapter: u16, range: RangeInclusive<u16>) -> Vec<VerseText> {
    range.map(|v| VerseText { reference: label_of(VerseId(book, chapter, v)), text: format!("t{book}-{chapter}-{v}") }).collect()
}

/// `per_chapter` verses (from verse 1) of every chapter in `chapters`.
pub(crate) fn verses_many(book: u8, chapters: RangeInclusive<u16>, per_chapter: u16) -> Vec<VerseText> {
    chapters.flat_map(|c| verses(book, c, 1..=per_chapter)).collect()
}

/// The verse ids of the verses whose labels parse.
pub(crate) fn ids_of(vs: &[VerseText]) -> BTreeSet<VerseId> { vs.iter().filter_map(|v| verse_id_of(&v.reference)).collect() }

/// One `bible_sources` row; `status` is the JSON wording (`"needs_key"`, `"beta"`, ...).
pub(crate) fn row(code: &str, status: &str, copyright: Option<&str>) -> SourceInfo {
    serde_json::from_value(serde_json::json!({ "translation": code, "status": status, "provider": "test", "copyright": copyright })).unwrap()
}

/// Rows from `(code, status)` pairs, with KJV stored locally.
pub(crate) fn rows(pairs: &[(&str, &str)]) -> Sources {
    Sources { rows: pairs.iter().map(|(c, s)| row(c, s, None)).collect(), local: vec!["KJV".into()] }
}

pub(crate) fn ids(xs: &[(u8, u16, u16)]) -> Vec<VerseId> { xs.iter().map(|&(b, c, v)| VerseId(b, c, v)).collect() }

pub(crate) fn set(xs: &[(u8, u16, u16)]) -> BTreeSet<VerseId> { xs.iter().map(|&(b, c, v)| VerseId(b, c, v)).collect() }
