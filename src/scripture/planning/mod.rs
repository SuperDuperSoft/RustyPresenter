//! Scripture slide planning (spec §5.4): the union of both translations' verses, grouping
//! by manual marks, `per_slide` or measured fit, and each slide's text, labels and footer.
pub mod measure;
pub mod refs;

pub use measure::{CharMeasure, DEFAULT_SCRIPTURE_SIZE, Fit, MIN_PX, Measure, PlanTheme, readable_px};
pub use refs::join_refs;

use super::input::verse_id_of;
use super::{ParallelLayout, ParallelText, SlideSource, VerseBreaks, VerseId, strings};
use crate::model::VerseText;
use std::collections::{BTreeMap, BTreeSet};

/// The second translation's resolved verses and copyright notice.
#[derive(Clone, Debug, PartialEq)]
pub struct SecondInput {
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
    pub second: Option<SecondInput>,
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
    pub second: Option<ParallelText>,
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

/// One entry of the verse sequence: main text, second text, or both.
#[derive(Debug, Clone, PartialEq)]
pub struct UnionVerse {
    pub id: Option<VerseId>,
    pub main: Option<VerseText>,
    pub second: Option<VerseText>,
}

/// Both translations' verses in id order. Verses without an id keep their stored order right
/// after the previous id'd verse of the same translation (at the start if none).
pub fn union<T: PlanTheme>(input: &ScriptureInput<T>) -> Vec<UnionVerse> {
    let mut by_id: BTreeMap<VerseId, UnionVerse> = BTreeMap::new();
    // Unlabelled verses (and repeats of an id) keyed by the previous id'd verse; `None` = the start.
    let mut loose: BTreeMap<Option<VerseId>, Vec<UnionVerse>> = BTreeMap::new();
    let second = input.second.as_ref().map_or(&[][..], |s| s.verses.as_slice());
    for (is_second, verses) in [(false, input.main.as_slice()), (true, second)] {
        let mut prev = None;
        for vt in verses {
            if let Some(id) = verse_id_of(&vt.reference) {
                let u = by_id.entry(id).or_insert(UnionVerse { id: Some(id), main: None, second: None });
                let slot = if is_second { &mut u.second } else { &mut u.main };
                if slot.is_none() {
                    *slot = Some(vt.clone());
                    prev = Some(id);
                    continue;
                }
            }
            let (main, second) = if is_second { (None, Some(vt.clone())) } else { (Some(vt.clone()), None) };
            loose.entry(prev).or_default().push(UnionVerse { id: None, main, second });
        }
    }
    let mut out = loose.remove(&None).unwrap_or_default();
    for (id, u) in by_id {
        out.push(u);
        out.extend(loose.remove(&Some(id)).unwrap_or_default());
    }
    out
}

/// Spec §5.4 steps 1–5. A `TooLong` group is emitted whole (Task A10 splits it).
pub fn plan_scripture<T: PlanTheme>(input: &ScriptureInput<T>, m: &mut dyn Measure) -> PlannedScripture {
    let verses = union(input);
    let mut groups: Vec<Vec<usize>> = vec![];
    let mut group: Vec<usize> = vec![];
    for (i, u) in verses.iter().enumerate() {
        if !group.is_empty() {
            let marked = |set: &BTreeSet<VerseId>| u.id.is_some_and(|id| set.contains(&id));
            let new_slide = if marked(&input.breaks.before) {
                true
            } else if marked(&input.breaks.join) {
                false
            } else if input.per_slide > 0 {
                group.len() >= input.per_slide as usize
            } else {
                let probe: Vec<usize> = group.iter().copied().chain([i]).collect();
                let (main, second) = compose(input, &verses, &probe);
                m.fits(&main, second.as_ref()) != Fit::Comfortable
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
    PlannedScripture {
        slides: groups.iter().map(|g| slide(input, &verses, g)).collect(),
        notes: missing_notes(input, &verses),
        auto_gaps: BTreeSet::new(),
        auto_at: vec![false; verses.len()],
    }
}

/// One planned slide for the union verses at `group`.
fn slide<T: PlanTheme>(input: &ScriptureInput<T>, verses: &[UnionVerse], group: &[usize]) -> PlannedSlide {
    let (text, second) = compose(input, verses, group);
    let shown: Vec<&VerseText> = group.iter().filter_map(|&i| verses[i].main.as_ref().or(verses[i].second.as_ref())).collect();
    let label = join_refs(&shown);
    let reference = match &input.second {
        Some(s) => format!("{label} ({} · {})", input.main_code, s.code),
        None => format!("{label} ({})", input.main_code),
    };
    let notices = [input.main_notice.as_deref(), input.second.as_ref().and_then(|s| s.notice.as_deref())];
    let footer = notices.into_iter().flatten().filter(|n| !n.trim().is_empty()).collect::<Vec<_>>().join(" · ");
    let ids: Vec<VerseId> = group.iter().filter_map(|&i| verses[i].id).collect();
    let source = ids.first().zip(ids.last()).map(|(&first, &last)| SlideSource { first, last, part: 0 });
    PlannedSlide { label, text, reference, footer: (!footer.is_empty()).then_some(footer), second, source, split: false }
}

/// The main slide text and, in parallel mode, the second column for the union verses at `group`.
fn compose<T: PlanTheme>(input: &ScriptureInput<T>, verses: &[UnionVerse], group: &[usize]) -> (String, Option<ParallelText>) {
    let column = |pick: fn(&UnionVerse) -> Option<&VerseText>| {
        group.iter().filter_map(|&i| pick(&verses[i]).map(|vt| verse_text(verses[i].id, vt))).collect::<Vec<_>>().join(" ")
    };
    let main = column(|u| u.main.as_ref());
    let second = input.second.as_ref().map(|s| ParallelText {
        main_code: input.main_code.clone(),
        second_code: s.code.clone(),
        text: column(|u| u.second.as_ref()),
        layout: input.layout,
    });
    (main, second)
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

/// "NIV doesn't include Acts 8:37." for each id'd verse that only one translation has.
fn missing_notes<T: PlanTheme>(input: &ScriptureInput<T>, verses: &[UnionVerse]) -> Vec<String> {
    let Some(second) = &input.second else { return vec![] };
    verses
        .iter()
        .filter(|u| u.id.is_some())
        .filter_map(|u| match (&u.main, &u.second) {
            (Some(vt), None) => Some(strings::missing_verse(&second.code, &vt.reference)),
            (None, Some(vt)) => Some(strings::missing_verse(&input.main_code, &vt.reference)),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests;
