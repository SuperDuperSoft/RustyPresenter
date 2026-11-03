//! Test-only helpers shared by the FEAT-5 test modules. Later tasks add their own helpers here.
#![allow(dead_code)]
use super::planning::{CharMeasure, PlanTheme, PlannedScripture, ScriptureInput, SecondInput};
use super::{ParallelLayout, VerseBreaks, VerseId};
use super::cache::TextCache;
use super::fetch::{BibleReaders, Fetcher, Lookup};
use super::input::{label_of, verse_id_of};
use super::sources::{SourceInfo, Sources};
use crate::model::VerseText;
use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

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

/// Readers that count calls per path and serve `"t{b}-{c}-{v}"` texts for any parseable reference.
#[derive(Default)]
pub(crate) struct FakeReaders {
    local: AtomicUsize,
    online: AtomicUsize,
    len: Mutex<Option<usize>>,
    local_errors: Mutex<HashMap<String, String>>,
    online_errors: Mutex<HashMap<String, String>>,
}

impl FakeReaders {
    pub(crate) fn local_calls(&self) -> usize { self.local.load(Ordering::SeqCst) }
    pub(crate) fn online_calls(&self) -> usize { self.online.load(Ordering::SeqCst) }
    /// Makes every `local` lookup in `tr` fail with `msg`.
    pub(crate) fn fail_local(&self, tr: &str, msg: &str) { self.local_errors.lock().unwrap().insert(tr.into(), msg.into()); }
    /// Makes every `online` lookup in `tr` fail with `msg`.
    pub(crate) fn fail_online(&self, tr: &str, msg: &str) { self.online_errors.lock().unwrap().insert(tr.into(), msg.into()); }
    /// Makes every lookup return `n` synthetic verses from Psalm 119:1, wrapping into later chapters.
    pub(crate) fn set_len(&self, n: usize) { *self.len.lock().unwrap() = Some(n); }

    fn serve(&self, errors: &Mutex<HashMap<String, String>>, tr: &str, input: &str) -> Lookup {
        if let Some(msg) = errors.lock().unwrap().get(tr) {
            return Err(msg.clone());
        }
        if let Some(n) = *self.len.lock().unwrap() {
            let vs = (0..n).map(|i| fake_verse(VerseId(18, 119 + (i / 176) as u16, (i % 176) as u16 + 1))).collect();
            return Ok((input.trim().to_string(), vs));
        }
        let (label, vs) = crate::bible::kjv().lookup(input)?;
        Ok((label, vs.into_iter().map(|v| verse_id_of(&v.reference).map_or(v, fake_verse)).collect()))
    }
}

fn fake_verse(id: VerseId) -> VerseText {
    VerseText { reference: label_of(id), text: format!("t{}-{}-{}", id.book(), id.chapter(), id.verse()) }
}

impl BibleReaders for FakeReaders {
    fn local(&self, tr: &str, input: &str) -> Lookup {
        self.local.fetch_add(1, Ordering::SeqCst);
        self.serve(&self.local_errors, tr, input)
    }

    fn online<'a>(&'a self, _root: &'a Path, tr: &'a str, input: &'a str) -> Pin<Box<dyn Future<Output = Lookup> + Send + 'a>> {
        self.online.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { self.serve(&self.online_errors, tr, input) })
    }
}

/// A fetcher on a fresh `tmp_root()` backed by a shared [`FakeReaders`].
pub(crate) fn fetcher_with_fake() -> (Fetcher, Arc<FakeReaders>) {
    let fake = Arc::new(FakeReaders::default());
    let root = tmp_root();
    let f = Fetcher { readers: fake.clone(), cache: Arc::new(TextCache::new(root.clone())), root };
    (f, fake)
}

/// The only theme facts planning needs, for tests.
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default)]
pub(crate) struct TestTheme {
    pub(crate) size: u32,
    pub(crate) lower: bool,
}

impl PlanTheme for TestTheme {
    fn scripture_size(&self) -> u32 { self.size }
    fn is_lower_third(&self) -> bool { self.lower }
}

pub(crate) fn v(reference: &str, text: &str) -> VerseText { VerseText { reference: reference.into(), text: text.into() } }

/// A KJV item with no notices, automatic grouping, side by side, no marks, default theme, 1080p.
pub(crate) fn input(main: &[VerseText], second: Option<(&str, &[VerseText])>) -> ScriptureInput<TestTheme> {
    ScriptureInput {
        main: main.to_vec(),
        main_code: "KJV".into(),
        main_notice: None,
        second: second.map(|(code, vs)| SecondInput { code: code.into(), verses: vs.to_vec(), notice: None }),
        per_slide: 0,
        layout: ParallelLayout::SideBySide,
        breaks: VerseBreaks::default(),
        theme: TestTheme::default(),
        canvas: (1920, 1080),
    }
}

pub(crate) fn labels(p: &PlannedScripture) -> Vec<String> { p.slides.iter().map(|s| s.label.clone()).collect() }

pub(crate) fn cm() -> CharMeasure { CharMeasure::with_limits(60, 120) }

/// A 10-character verse text.
pub(crate) fn short(n: u16) -> String { format!("verse{n:05}") }
