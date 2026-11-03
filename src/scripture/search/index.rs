//! The in-memory keyword index of one local translation and its ranking (spec §3.1, §5.5).
use super::query::{SearchQuery, Term, first_match, snippet, tokenize};
use crate::bible::BOOKS;
use crate::model::VerseText;
use crate::scripture::VerseId;
use crate::scripture::input::verse_id_of;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

/// Most results a search page shows.
pub const RESULT_CAP: usize = 200;
/// Old Testament books are `0..NT_START`, the New Testament `NT_START..`.
pub const NT_START: u8 = 39;
/// A safety stop for a reader that never fails (Psalms, the longest book, has 150).
const MAX_CHAPTERS: u16 = 200;

/// Which part of the Bible a search covers.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(from = "ScopeRepr", into = "ScopeRepr")]
pub enum Scope {
    #[default]
    All,
    Ot,
    Nt,
    Book(u8),
}

/// The JSON shape of `Scope`: `"all"|"ot"|"nt"` or `{"book": n}`.
#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(untagged)]
enum ScopeRepr {
    Unit(ScopeUnit),
    Book { book: u8 },
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum ScopeUnit {
    All,
    Ot,
    Nt,
}

impl From<ScopeRepr> for Scope {
    fn from(r: ScopeRepr) -> Self {
        match r {
            ScopeRepr::Unit(ScopeUnit::All) => Scope::All,
            ScopeRepr::Unit(ScopeUnit::Ot) => Scope::Ot,
            ScopeRepr::Unit(ScopeUnit::Nt) => Scope::Nt,
            ScopeRepr::Book { book } => Scope::Book(book),
        }
    }
}

impl From<Scope> for ScopeRepr {
    fn from(s: Scope) -> Self {
        match s {
            Scope::All => ScopeRepr::Unit(ScopeUnit::All),
            Scope::Ot => ScopeRepr::Unit(ScopeUnit::Ot),
            Scope::Nt => ScopeRepr::Unit(ScopeUnit::Nt),
            Scope::Book(book) => ScopeRepr::Book { book },
        }
    }
}

/// Result order. An order from a newer build loads as `Best`
/// (serde needs the `other` variant last, so `Best` is listed second).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    Bible,
    #[default]
    #[serde(other)]
    Best,
}

/// One search result row.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: VerseId,
    pub reference: String,
    pub snippet: String,
}

/// Every verse of one translation in Bible order (the ordinal), with an inverted word index.
pub struct SearchIndex {
    verses: Vec<VerseId>,
    refs: Vec<Box<str>>,
    texts: Vec<Box<str>>,
    /// Each verse's tokens as word ids.
    norm: Vec<Box<[u32]>>,
    /// Sorted words (the word id is the position) and the ordinals each occurs in.
    words: Vec<(Box<str>, Vec<u32>)>,
}

/// Read every chapter of every book (`"<book> 1"`, `"<book> 2"`, … until a lookup fails) and index it.
/// `None` when `cancel` is set; it is checked before each chapter read.
pub fn build(read_chapter: &mut dyn FnMut(&str) -> Result<(String, Vec<VerseText>), String>, cancel: &AtomicBool) -> Option<SearchIndex> {
    let (mut verses, mut refs, mut texts, mut norm) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    // Word ids in first-seen order while reading; renumbered to sorted order at the end.
    let mut ids: HashMap<String, u32> = HashMap::new();
    let mut postings: Vec<Vec<u32>> = Vec::new();
    for (_, name, _) in BOOKS {
        for n in 1..=MAX_CHAPTERS {
            if cancel.load(Ordering::SeqCst) { return None; }
            let Ok((_, found)) = read_chapter(&format!("{name} {n}")) else { break };
            for v in found {
                let Some(id) = verse_id_of(&v.reference) else { continue };
                let ordinal = verses.len() as u32;
                let tokens: Box<[u32]> = tokenize(&v.text).into_iter().map(|w| {
                    let next = postings.len() as u32;
                    let word = *ids.entry(w).or_insert(next);
                    if word == next { postings.push(Vec::new()); }
                    let list = &mut postings[word as usize];
                    if list.last() != Some(&ordinal) { list.push(ordinal); }
                    word
                }).collect();
                verses.push(id);
                refs.push(v.reference.into_boxed_str());
                texts.push(v.text.into_boxed_str());
                norm.push(tokens);
            }
        }
    }
    let mut sorted: Vec<(String, u32)> = ids.into_iter().collect();
    sorted.sort_unstable();
    let mut renumber = vec![0u32; sorted.len()];
    for (new, (_, old)) in sorted.iter().enumerate() { renumber[*old as usize] = new as u32; }
    for tokens in &mut norm { for w in tokens.iter_mut() { *w = renumber[*w as usize]; } }
    let words = sorted.into_iter().map(|(w, old)| (w.into_boxed_str(), std::mem::take(&mut postings[old as usize]))).collect();
    Some(SearchIndex { verses, refs, texts, norm, words })
}

/// Sorted, de-duplicated union of lists.
fn union(lists: impl Iterator<Item = impl IntoIterator<Item = u32>>) -> Vec<u32> {
    let mut all: Vec<u32> = lists.flatten().collect();
    all.sort_unstable();
    all.dedup();
    all
}

/// Items of sorted `a` that are also in sorted `b`.
fn intersect(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => { out.push(a[i]); i += 1; j += 1; }
        }
    }
    out
}

impl SearchIndex {
    /// The word ids a query word matches: a prefix range for 3+ letters, else the exact word.
    /// Words are sorted, so either is one contiguous range.
    fn id_range(&self, term: &str) -> Range<u32> {
        let start = self.words.partition_point(|(w, _)| &**w < term);
        let end = if term.chars().count() >= 3 {
            start + self.words[start..].partition_point(|(w, _)| w.starts_with(term))
        } else {
            start + usize::from(self.words.get(start).is_some_and(|(w, _)| &**w == term))
        };
        start as u32..end as u32
    }

    /// Ordinals containing any word in `ids`.
    fn postings(&self, ids: Range<u32>) -> Vec<u32> {
        union(ids.map(|i| self.words[i as usize].1.iter().copied()))
    }

    /// The ordinal range of a scope (verses are in Bible order).
    fn scope_range(&self, scope: Scope) -> Range<u32> {
        let (lo, hi) = match scope {
            Scope::All => (0, u16::from(u8::MAX) + 1),
            Scope::Ot => (0, u16::from(NT_START)),
            Scope::Nt => (u16::from(NT_START), u16::from(u8::MAX) + 1),
            Scope::Book(b) => (u16::from(b), u16::from(b) + 1),
        };
        let at = |book: u16| self.verses.partition_point(|v| u16::from(v.book()) < book) as u32;
        at(lo)..at(hi)
    }

    /// Whether verse `o` has one match per range, consecutively, starting at token `pos`.
    fn run_at(&self, o: u32, pos: usize, ranges: &[Range<u32>]) -> bool {
        let tokens = &self.norm[o as usize];
        pos + ranges.len() <= tokens.len() && ranges.iter().enumerate().all(|(k, r)| r.contains(&tokens[pos + k]))
    }

    fn has_run(&self, o: u32, ranges: &[Range<u32>]) -> bool {
        (0..self.norm[o as usize].len()).any(|p| self.run_at(o, p, ranges))
    }

    /// Ordinals of the verses in `scope` that match every term, in Bible order.
    pub fn find(&self, q: &SearchQuery, scope: Scope) -> Vec<u32> {
        let range = self.scope_range(scope);
        let mut hits: Vec<u32> = range.collect();
        for term in &q.terms {
            if hits.is_empty() { break; }
            match term {
                Term::Word(w) => hits = intersect(&hits, &self.postings(self.id_range(w))),
                Term::Phrase(ws) => {
                    let ranges: Vec<Range<u32>> = ws.iter().map(|w| self.id_range(w)).collect();
                    for r in &ranges { hits = intersect(&hits, &self.postings(r.clone())); }
                    hits.retain(|&o| self.has_run(o, &ranges));
                }
            }
        }
        hits
    }

    /// 1: every query word consecutive and in order; 2: all within `2 × words + 2` tokens; 3: the rest.
    fn tier(&self, o: u32, ranges: &[Range<u32>]) -> u8 {
        if self.has_run(o, ranges) { return 1; }
        let window = 2 * ranges.len() + 2;
        // (position, query word) for every token matching a query word, in position order.
        let marks: Vec<(usize, usize)> = self.norm[o as usize].iter().enumerate()
            .flat_map(|(p, id)| ranges.iter().enumerate().filter(|(_, r)| r.contains(id)).map(move |(k, _)| (p, k)))
            .collect();
        // The shortest span holding every query word: a sliding window over the marks.
        let (mut counts, mut covered, mut lo) = (vec![0usize; ranges.len()], 0, 0);
        for hi in 0..marks.len() {
            if counts[marks[hi].1] == 0 { covered += 1; }
            counts[marks[hi].1] += 1;
            while covered == ranges.len() {
                if marks[hi].0 - marks[lo].0 < window { return 2; }
                counts[marks[lo].1] -= 1;
                if counts[marks[lo].1] == 0 { covered -= 1; }
                lo += 1;
            }
        }
        3
    }

    /// `hits` ordered for display: phrase-like matches first, then close-together words, then the rest,
    /// each tier in Bible order. `Order::Bible` is Bible order only.
    pub fn rank(&self, hits: &[u32], q: &SearchQuery, order: Order) -> Vec<u32> {
        let mut out = hits.to_vec();
        match order {
            Order::Bible => out.sort_unstable(),
            Order::Best => {
                let ranges: Vec<Range<u32>> = q.words().into_iter().map(|w| self.id_range(w)).collect();
                out.sort_by_cached_key(|&o| (self.tier(o, &ranges), o));
            }
        }
        out
    }

    /// The total number of results and the first 200 as rows.
    pub fn page(&self, ranked: &[u32], q: &SearchQuery) -> (usize, Vec<Hit>) {
        let rows = ranked.iter().take(RESULT_CAP).map(|&o| {
            let (i, text) = (o as usize, &*self.texts[o as usize]);
            Hit { id: self.verses[i], reference: self.refs[i].to_string(), snippet: snippet(text, first_match(text, q)) }
        }).collect();
        (ranked.len(), rows)
    }

    /// One whole chapter from the index, for the context view.
    pub fn chapter(&self, book: u8, chapter: u16) -> Vec<VerseText> {
        let start = self.verses.partition_point(|v| (v.book(), v.chapter()) < (book, chapter));
        let len = self.verses[start..].iter().take_while(|v| (v.book(), v.chapter()) == (book, chapter)).count();
        (start..start + len).map(|i| VerseText { reference: self.refs[i].to_string(), text: self.texts[i].to_string() }).collect()
    }

    #[cfg(test)]
    pub(crate) fn refs_of(&self, ordinals: &[u32]) -> Vec<String> {
        ordinals.iter().map(|&o| self.refs[o as usize].to_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripture::input::label_of;

    type Read = Result<(String, Vec<VerseText>), String>;

    fn book_index(name: &str) -> u8 { BOOKS.iter().position(|b| b.1 == name).unwrap() as u8 }

    fn served(book: &str, chapter: u16, texts: &[(u16, &str)]) -> Read {
        let b = book_index(book);
        Ok((format!("{book} {chapter}"), texts.iter().map(|&(v, t)| VerseText { reference: label_of(VerseId(b, chapter, v)), text: t.into() }).collect()))
    }

    /// Serves Genesis 1, Psalms 46, John 3 and 1 John 4; empty lower chapters of those books; `Err` elsewhere.
    fn fixture_read(c: &str) -> Read {
        let (book, n) = c.rsplit_once(' ').unwrap();
        let n: u16 = n.parse().unwrap();
        let last = match book { "Genesis" => 1, "Psalms" => 46, "John" => 3, "1 John" => 4, _ => return Err("no".into()) };
        if n < last { return Ok((c.into(), vec![])); }
        if n > last { return Err("no".into()); }
        match book {
            "Genesis" => served(book, 1, &[
                (1, "In the beginning God created the heaven and the earth."),
                (2, "x love god"),
                (3, "god love"),
                (4, "god a b c d e f g h love"),
            ]),
            "Psalms" => served(book, 46, &[(10, "Be still, and know that I am God")]),
            "John" => served(book, 3, &[(16, "For God so loved the world")]),
            _ => served(book, 4, &[(8, "He that loveth not knoweth not God; for God is love.")]),
        }
    }
    fn fixture() -> fn(&str) -> Read { fixture_read }
    fn ix() -> SearchIndex { build(&mut fixture(), &AtomicBool::new(false)).unwrap() }
    fn kjv_index() -> SearchIndex { build(&mut |c: &str| crate::bible::kjv().lookup(c), &AtomicBool::new(false)).unwrap() }
    fn refs(ix: &SearchIndex, q: &str, scope: Scope) -> Vec<String> {
        ix.refs_of(&ix.find(&SearchQuery::parse(q).unwrap(), scope))
    }
    fn ranked_refs(ix: &SearchIndex, q: &str, order: Order) -> Vec<String> {
        let q = SearchQuery::parse(q).unwrap();
        ix.refs_of(&ix.rank(&ix.find(&q, Scope::All), &q, order))
    }

    #[test] fn prefix_matches_loveth_and_loved() { assert_eq!(refs(&ix(), "love", Scope::All), ["Genesis 1:2", "Genesis 1:3", "Genesis 1:4", "John 3:16", "1 John 4:8"]); }
    #[test] fn short_terms_match_exactly() { assert!(refs(&ix(), "an", Scope::All).is_empty()); }
    #[test] fn all_terms_must_match() { assert_eq!(refs(&ix(), "loved world", Scope::All), ["John 3:16"]); }
    #[test] fn phrase_requires_consecutive_words() {
        assert_eq!(refs(&ix(), r#""god so""#, Scope::All), ["John 3:16"]);
        assert!(refs(&ix(), r#""so god""#, Scope::All).is_empty());
    }
    #[test] fn scope_old_testament_excludes_john() { assert!(!refs(&ix(), "god", Scope::Ot).iter().any(|r| r.contains("John"))); }
    #[test] fn scope_one_book() { assert_eq!(refs(&ix(), "god", Scope::Book(18)), ["Psalm 46:10"]); }
    #[test] fn scope_serde_shapes() {
        assert_eq!(serde_json::from_str::<Scope>(r#""nt""#).unwrap(), Scope::Nt);
        assert_eq!(serde_json::from_str::<Scope>(r#"{"book":18}"#).unwrap(), Scope::Book(18));
    }
    #[test] fn rank_tiers_phrase_then_proximity_then_rest() {
        // query "god love": Genesis 1:3 "god love" → tier 1 (consecutive, in order);
        // Genesis 1:2 "x love god" → tier 2 (wrong order, inside the 6-token window); Genesis 1:4 → tier 3 (10 tokens apart)
        // John 3:16 ("God so loved") and 1 John 4:8 ("loveth … God") are tier 2 as well, so they precede Genesis 1:4.
        assert_eq!(ranked_refs(&ix(), "god love", Order::Best), ["Genesis 1:3", "Genesis 1:2", "John 3:16", "1 John 4:8", "Genesis 1:4"]);
    }
    #[test] fn bible_order_option_sorts_by_ordinal() { assert_eq!(ranked_refs(&ix(), "god love", Order::Bible)[..3], ["Genesis 1:2", "Genesis 1:3", "Genesis 1:4"]); }
    #[test] fn build_stops_when_cancelled() {
        assert!(build(&mut fixture(), &AtomicBool::new(true)).is_none());
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let mut r = |c: &str| { calls += 1; if calls == 2 { cancel.store(true, Ordering::SeqCst); } fixture()(c) };
        assert!(build(&mut r, &cancel).is_none());
        assert_eq!(calls, 2);   // checked between chapters, no further reads
    }
    #[test] fn build_stops_book_at_first_failed_chapter() {
        // John 1–2 Err, John 3 would serve text → nothing from John is indexed
        let mut r = |c: &str| if c == "John 3" { Ok(("John 3".into(), vec![VerseText { reference: "John 3:16".into(), text: "loved".into() }])) } else { Err("no".into()) };
        let ix = build(&mut r, &AtomicBool::new(false)).unwrap();
        assert!(refs(&ix, "loved", Scope::All).is_empty());
    }
    #[test] fn fixture_reaches_every_served_verse() {
        assert_eq!(refs(&ix(), "god", Scope::All), ["Genesis 1:1", "Genesis 1:2", "Genesis 1:3", "Genesis 1:4", "Psalm 46:10", "John 3:16", "1 John 4:8"]);
    }
    #[test] fn be_still_and_know_ranks_psalm_46_10_first() {
        let ix = kjv_index();   // build() over crate::bible::kjv().lookup
        let q = SearchQuery::parse("be still and know").unwrap();
        let hits = ix.find(&q, Scope::All);
        assert_eq!(hits.len(), 3);
        let best = ix.rank(&hits, &q, Order::Best);
        assert_eq!(ix.refs_of(&best), ["Psalm 46:10", "Genesis 41:21", "1 Kings 22:3"]);
    }
    #[test] fn caps_at_200_and_reports_total() {
        let ix = kjv_index(); let q = SearchQuery::parse("lord").unwrap();
        let (total, page) = ix.page(&ix.rank(&ix.find(&q, Scope::All), &q, Order::Best), &q);
        assert!(total > 200); assert_eq!(page.len(), 200);
    }
    #[test] fn chapter_reads_one_chapter_from_the_index() {
        let ch = ix().chapter(0, 1);
        assert_eq!(ch.iter().map(|v| v.reference.as_str()).collect::<Vec<_>>(), ["Genesis 1:1", "Genesis 1:2", "Genesis 1:3", "Genesis 1:4"]);
        assert_eq!(ch[2].text, "god love");
        assert!(ix().chapter(0, 2).is_empty());
    }
    #[test] #[ignore] fn index_build_under_two_seconds_release() {   // run manually on the old laptop
        let t = std::time::Instant::now();
        let ix = kjv_index();
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
        assert!(!refs(&ix, "love", Scope::All).is_empty());
    }
}
