//! Scripture slide planning (FEAT-5 spec §5.4, Nov 5 spec §3): the union of every Bible's verses (the main one and
//! up to `MAX_ALSO` more), grouping by manual marks, `per_slide` or measured fit, and each slide's text, labels and
//! footer.
pub mod measure;
pub mod refs;
pub mod split;

pub use measure::{CharMeasure, DEFAULT_SCRIPTURE_SIZE, Fit, MIN_PX, Measure, PlanTheme, readable_px};
pub use refs::join_refs;

use super::input::verse_id;
use super::{ParallelLayout, ParallelText, SlideSource, VerseBreaks, VerseId, strings};
use crate::model::VerseText;
use std::collections::{BTreeMap, BTreeSet};

/// An Also Bible's resolved verses and copyright notice.
#[derive(Clone, Debug, PartialEq)]
pub struct AlsoInput {
    pub code: String,
    pub verses: Vec<VerseText>,
    pub notice: Option<String>,
}

/// Everything planning needs for one scripture item.
#[derive(Clone, Debug)]
pub struct ScriptureInput<T: PlanTheme> {
    pub main: Vec<VerseText>,
    pub main_code: String,
    pub main_notice: Option<String>,
    /// The Also Bibles, in column order.
    pub also: Vec<AlsoInput>,
    pub per_slide: u8,
    pub layout: ParallelLayout,
    pub breaks: VerseBreaks,
    pub theme: T,
    pub canvas: (u32, u32),
}

/// One planned slide; Part B converts it to a `Slide` with `Slide::new`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedSlide {
    pub label: String,
    pub text: String,
    pub reference: String,
    pub footer: Option<String>,
    /// One column per Also Bible, in order; empty for a single Bible.
    pub also: Vec<ParallelText>,
    pub source: Option<SlideSource>,
    pub split: bool,
}

/// The planned slides of one item, its operator notes and the automatic gaps.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedScripture {
    pub slides: Vec<PlannedSlide>,
    pub notes: Vec<String>,
    /// Verses before which the baseline plan (`join` ignored) starts a new slide.
    pub auto_gaps: BTreeSet<VerseId>,
    /// `auto_at[i]`: union verse `i` starts a slide in the automatic plan.
    pub auto_at: Vec<bool>,
}

/// One entry of the verse sequence: the main Bible's text and each Also Bible's, any of which may be missing.
#[derive(Debug, Clone, PartialEq)]
pub struct UnionVerse {
    pub id: Option<VerseId>,
    pub main: Option<VerseText>,
    /// One slot per Also Bible, in column order.
    pub also: Vec<Option<VerseText>>,
}

impl UnionVerse {
    /// Column `c`: 0 is the main Bible, 1.. the Also Bibles.
    fn column(&self, c: usize) -> Option<&VerseText> { if c == 0 { self.main.as_ref() } else { self.also[c - 1].as_ref() } }
    fn column_mut(&mut self, c: usize) -> &mut Option<VerseText> { if c == 0 { &mut self.main } else { &mut self.also[c - 1] } }
    /// The first column that has this verse.
    fn shown(&self) -> Option<&VerseText> { self.main.as_ref().or_else(|| self.also.iter().flatten().next()) }
}

/// Every Bible's verses in id order. Verses without an id keep their stored order right after the previous id'd
/// verse of the same Bible (at the start if none).
pub fn union<T: PlanTheme>(input: &ScriptureInput<T>) -> Vec<UnionVerse> {
    let n = input.also.len();
    let empty = || UnionVerse { id: None, main: None, also: vec![None; n] };
    let mut by_id: BTreeMap<VerseId, UnionVerse> = BTreeMap::new();
    // Unlabelled verses (and repeats of an id) keyed by the previous id'd verse; `None` = the start.
    let mut loose: BTreeMap<Option<VerseId>, Vec<UnionVerse>> = BTreeMap::new();
    let columns = std::iter::once(input.main.as_slice()).chain(input.also.iter().map(|a| a.verses.as_slice()));
    for (c, verses) in columns.enumerate() {
        let mut prev = None;
        for vt in verses {
            if let Some(id) = verse_id(vt) {
                let u = by_id.entry(id).or_insert_with(|| UnionVerse { id: Some(id), ..empty() });
                let slot = u.column_mut(c);
                if slot.is_none() {
                    *slot = Some(vt.clone());
                    prev = Some(id);
                    continue;
                }
            }
            let mut u = empty();
            *u.column_mut(c) = Some(vt.clone());
            loose.entry(prev).or_default().push(u);
        }
    }
    let mut out = loose.remove(&None).unwrap_or_default();
    for (id, u) in by_id {
        out.push(u);
        out.extend(loose.remove(&Some(id)).unwrap_or_default());
    }
    out
}

/// Spec §5.4 steps 1–5: group the union verses, split each `TooLong` group into even parts,
/// and record where the automatic plan (`join` ignored) would start slides.
pub fn plan_scripture<T: PlanTheme>(input: &ScriptureInput<T>, m: &mut dyn Measure) -> PlannedScripture {
    let verses = union(input);
    let mut slides = vec![];
    for g in group_verses(input, &verses, m, true) {
        let (main, also) = compose(input, &verses, &g);
        if m.fits(&main, &also, input.layout) == Fit::TooLong {
            let parts = split::split_slide(&main, &also, input.layout, m);
            for (part, (text, also)) in parts.into_iter().enumerate() {
                slides.push(slide(input, &verses, &g, text, also, part, true));
            }
        } else {
            slides.push(slide(input, &verses, &g, main, also, 0, false));
        }
    }
    let mut auto_at = vec![false; verses.len()];
    for g in group_verses(input, &verses, m, false).iter().skip(1) {
        auto_at[g[0]] = true;
    }
    let auto_gaps = verses.iter().zip(&auto_at).filter(|(_, at)| **at).filter_map(|(u, _)| u.id).collect();
    PlannedScripture { slides, notes: missing_notes(input, &verses), auto_gaps, auto_at }
}

/// Spec §5.4 steps 1–3: `before` starts a new slide, `join` (when `honor_join`) appends, else
/// `per_slide` counts or the measured `Comfortable` test decides. Returns union indices.
fn group_verses<T: PlanTheme>(input: &ScriptureInput<T>, verses: &[UnionVerse], m: &mut dyn Measure, honor_join: bool) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = vec![];
    let mut group: Vec<usize> = vec![];
    for (i, u) in verses.iter().enumerate() {
        if !group.is_empty() {
            let marked = |set: &BTreeSet<VerseId>| u.id.is_some_and(|id| set.contains(&id));
            let new_slide = if marked(&input.breaks.before) {
                true
            } else if honor_join && marked(&input.breaks.join) {
                false
            } else if input.per_slide > 0 {
                group.len() >= input.per_slide as usize
            } else {
                let probe: Vec<usize> = group.iter().copied().chain([i]).collect();
                let (main, also) = compose(input, verses, &probe);
                m.fits(&main, &also, input.layout) != Fit::Comfortable
            };
            if new_slide {
                groups.push(std::mem::take(&mut group));
            }
        }
        group.push(i);
    }
    if !group.is_empty() {
        groups.push(group);
    }
    groups
}

/// Part `part` of the planned slide for the union verses at `group`, with its composed `text`
/// and Also columns. Parts after the first get " (cont.)" after the refs.
fn slide<T: PlanTheme>(
    input: &ScriptureInput<T>,
    verses: &[UnionVerse],
    group: &[usize],
    text: String,
    also: Vec<ParallelText>,
    part: usize,
    split: bool,
) -> PlannedSlide {
    let shown: Vec<&VerseText> = group.iter().filter_map(|&i| verses[i].shown()).collect();
    let mut label = join_refs(&shown);
    if part > 0 {
        label.push_str(" (cont.)");
    }
    let codes: Vec<&str> = std::iter::once(input.main_code.as_str()).chain(input.also.iter().map(|a| a.code.as_str())).collect();
    let reference = format!("{label} ({})", codes.join(" · "));
    let notices = std::iter::once(input.main_notice.as_deref()).chain(input.also.iter().map(|a| a.notice.as_deref()));
    let footer = notices.into_iter().flatten().filter(|n| !n.trim().is_empty()).collect::<Vec<_>>().join(" · ");
    let ids: Vec<VerseId> = group.iter().filter_map(|&i| verses[i].id).collect();
    let part = u8::try_from(part).unwrap_or(u8::MAX);
    let source = ids.first().zip(ids.last()).map(|(&first, &last)| SlideSource { first, last, part });
    PlannedSlide { label, text, reference, footer: (!footer.is_empty()).then_some(footer), also, source, split }
}

/// The main slide text and each Also column's, for the union verses at `group`.
fn compose<T: PlanTheme>(input: &ScriptureInput<T>, verses: &[UnionVerse], group: &[usize]) -> (String, Vec<ParallelText>) {
    let column = |c: usize| {
        group.iter().filter_map(|&i| verses[i].column(c).map(|vt| verse_text(verses[i].id, vt))).collect::<Vec<_>>().join(" ")
    };
    let also = input.also.iter().enumerate().map(|(k, a)| ParallelText { code: a.code.clone(), text: column(k + 1) }).collect();
    (column(0), also)
}

/// `"¹⁶\u{2009}text"`; a verse without an id has no number.
fn verse_text(id: Option<VerseId>, vt: &VerseText) -> String {
    match id {
        Some(id) => format!("{}\u{2009}{}", superscript(id.verse()), vt.text),
        None => vt.text.clone(),
    }
}

fn superscript(n: u16) -> String {
    const DIGITS: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    n.to_string().chars().map(|c| c.to_digit(10).map_or(c, |d| DIGITS[d as usize])).collect()
}

/// "NIV doesn't include Acts 8:37." for each id'd verse a Bible lacks while another has it, verse by verse, Bibles in
/// column order.
fn missing_notes<T: PlanTheme>(input: &ScriptureInput<T>, verses: &[UnionVerse]) -> Vec<String> {
    if input.also.is_empty() { return vec![] }
    let codes: Vec<&str> = std::iter::once(input.main_code.as_str()).chain(input.also.iter().map(|a| a.code.as_str())).collect();
    let mut out = vec![];
    for u in verses.iter().filter(|u| u.id.is_some()) {
        let Some(shown) = u.shown() else { continue };
        for (c, code) in codes.iter().enumerate() {
            if u.column(c).is_none() { out.push(strings::missing_verse(code, &shown.reference)) }
        }
    }
    out
}

#[cfg(test)]
mod tests;
