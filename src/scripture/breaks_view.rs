//! What the Slide breaks dialog shows (spec §3.3): the verses, the gap before each one with its state, and the
//! slides the marks make. Pure; the dialog asks for it again after every click.
use super::breaks::{GapState, gap_rows};
use super::planning::{Measure, PlanTheme, ScriptureInput, plan_scripture, union};
use super::{VerseBreaks, VerseId};
use serde::Serialize;

/// Longest start of a verse's text a row shows.
const ROW_CHARS: usize = 90;

/// One verse row. `also_only`: only an Also Bible has this verse (shown in italics).
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct VerseRow {
    pub id: Option<VerseId>,
    pub number: String,
    pub text: String,
    pub also_only: bool,
}

/// The gap before verse row `i + 1`.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct GapView {
    pub state: &'static str,
    pub enabled: bool,
}

/// One resulting slide.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct SlideView {
    pub label: String,
    pub text: String,
    /// Too long even at the smallest size: split inside a verse.
    pub too_long: bool,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct BreaksView {
    pub verses: Vec<VerseRow>,
    pub gaps: Vec<GapView>,
    pub slides: Vec<SlideView>,
    /// The marks as Save sends them: `join` only where the automatic plan breaks.
    pub save: VerseBreaks,
}

pub fn state_name(s: GapState) -> &'static str {
    match s {
        GapState::None => "none",
        GapState::Auto => "auto",
        GapState::Added => "added",
        GapState::Kept => "kept",
    }
}

/// The dialog's view of `input` (its `breaks` and `per_slide` are the dialog's current choices).
pub fn breaks_view<T: PlanTheme>(input: &ScriptureInput<T>, m: &mut dyn Measure) -> BreaksView {
    let plan = plan_scripture(input, m);
    let all = union(input);
    let verses = all.iter().map(|u| {
        let shown = u.main.as_ref().or_else(|| u.also.iter().flatten().next());
        let text = shown.map(|v| v.text.as_str()).unwrap_or_default();
        let mut start: String = text.chars().take(ROW_CHARS).collect();
        if text.chars().count() > ROW_CHARS { start.push('…') }
        let number = u.id.map(|id| id.verse().to_string()).unwrap_or_default();
        VerseRow { id: u.id, number, text: start, also_only: u.main.is_none() }
    }).collect();
    let ids: Vec<Option<VerseId>> = all.iter().map(|u| u.id).collect();
    let gaps = gap_rows(&ids, &input.breaks, &plan.auto_at).into_iter().map(|g| GapView { state: state_name(g.state), enabled: g.enabled }).collect();
    let slides = plan.slides.iter().map(|s| SlideView { label: s.label.clone(), text: s.text.clone(), too_long: s.split }).collect();
    BreaksView { verses, gaps, slides, save: super::breaks::for_save(&input.breaks, &plan.auto_gaps) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripture::test_support::{cm, input, short, v};

    #[test]
    fn rows_gaps_and_slides_follow_the_marks() {
        let vs = [v("John 3:16", &short(1)), v("John 3:17", &short(2)), v("John 3:18", &short(3))];
        let mut i = input(&vs, None);
        let view = breaks_view(&i, &mut cm());
        assert_eq!(view.verses.iter().map(|r| r.number.as_str()).collect::<Vec<_>>(), ["16", "17", "18"]);
        assert_eq!(view.gaps.iter().map(|g| g.state).collect::<Vec<_>>(), ["none", "none"]);
        assert_eq!(view.slides.len(), 1);
        i.breaks.before.insert(VerseId(42, 3, 18));
        let view = breaks_view(&i, &mut cm());
        assert_eq!(view.gaps.iter().map(|g| g.state).collect::<Vec<_>>(), ["none", "added"]);
        assert_eq!(view.slides.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["John 3:16–17", "John 3:18"]);
        assert_eq!(view.save.before.len(), 1);
    }

    #[test]
    fn kept_gap_and_save_drops_join_where_no_automatic_break() {
        let mut i = input(&[v("John 3:16", &"a".repeat(40)), v("John 3:17", &"b".repeat(40)), v("John 3:18", &short(3))], None);
        assert_eq!(breaks_view(&i, &mut cm()).gaps[0].state, "auto");
        i.breaks.join.insert(VerseId(42, 3, 17));
        i.breaks.join.insert(VerseId(42, 3, 18)); // not an automatic break: dropped on save
        let view = breaks_view(&i, &mut cm());
        assert_eq!(view.gaps[0].state, "kept");
        assert_eq!(view.save.join.iter().copied().collect::<Vec<_>>(), [VerseId(42, 3, 17)]);
    }

    #[test]
    fn long_rows_are_cut_and_a_split_slide_is_flagged() {
        let i = input(&[v("John 3:16", &"word ".repeat(300))], None);
        let view = breaks_view(&i, &mut cm());
        assert!(view.verses[0].text.ends_with('…') && view.verses[0].text.chars().count() == ROW_CHARS + 1);
        assert!(view.slides.len() > 1 && view.slides.iter().all(|s| s.too_long));
    }
}
