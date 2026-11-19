//! Where the live slide is after an edit (spec §5.4): `same_on_screen` and `map_position`.
use super::{Occurrence, occurrences};
use crate::model::Slide;

/// True when two slides look the same in the room: the content only (text, footer, reference, asset).
pub fn same_on_screen(a: &Slide, b: &Slide) -> bool { a.content == b.content }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mapped { Exact(usize), Moved(usize), Lost }

impl Mapped {
    pub fn index(self) -> Option<usize> { match self { Mapped::Exact(i) | Mapped::Moved(i) => Some(i), Mapped::Lost => None } }
}

/// Where slide `old_index` of `old` is in `new`. Song slides follow their section: the same physical occurrence
/// when an alignment of the two played orders agrees on it, else the same label and repeat, else the same place
/// in the order with the same kind and number. Other slides keep their index if they look the same.
pub fn map_position(old: &[Slide], old_index: usize, new: &[Slide]) -> Mapped {
    let Some(r) = old.get(old_index).and_then(|s| s.section.as_ref()) else {
        return match (old.get(old_index), new.get(old_index)) {
            (Some(a), Some(b)) if same_on_screen(a, b) => Mapped::Exact(old_index),
            _ => Mapped::Lost,
        };
    };
    let (ao, bo) = (occurrences(old), occurrences(new));
    let page = usize::from(r.page);
    let at = |q: &Occurrence, exact_ok: bool| {
        if page < q.slides && exact_ok { Mapped::Exact(q.first_slide + page) } else { Mapped::Moved(q.first_slide + page.min(q.slides - 1)) }
    };
    let Some(p) = ao.iter().position(|o| o.occurrence == r.occurrence) else { return Mapped::Lost };
    let a: Vec<String> = ao.iter().map(|o| o.label.trim().to_lowercase()).collect();
    let b: Vec<String> = bo.iter().map(|o| o.label.trim().to_lowercase()).collect();
    let (fwd, back) = (align_forward(&a, &b), align_backward(&a, &b));
    if let (Some(q), Some(q2)) = (fwd[p], back[p]) && q == q2 {
        return at(&bo[q], true);
    }
    // Same label, same repeat.
    let ordinal = r.ordinal as usize;
    if let Some(q) = bo.iter().enumerate().filter(|(i, _)| b[*i] == a[p]).nth(ordinal).map(|(_, q)| q) {
        return at(q, false);
    }
    // Renamed in place: same position in the order, same kind and number.
    if let Some(q) = bo.get(p).filter(|q| q.kind == ao[p].kind && q.number == ao[p].number) {
        return at(q, false);
    }
    Mapped::Lost
}

/// `map_position` when it is known where each played part went (the order changed from the Slides panel):
/// `moved[old occurrence]` is its new occurrence. The same page of that part, else its last page; a part taken out
/// is lost.
pub fn map_moved(old: &[Slide], old_index: usize, new: &[Slide], moved: &[Option<usize>]) -> Mapped {
    let Some(r) = old.get(old_index).and_then(|s| s.section.as_ref()) else { return map_position(old, old_index, new) };
    let Some(Some(to)) = moved.get(usize::from(r.occurrence)) else { return Mapped::Lost };
    let Some(q) = occurrences(new).into_iter().find(|o| usize::from(o.occurrence) == *to) else { return Mapped::Lost };
    let page = usize::from(r.page);
    if page < q.slides { Mapped::Exact(q.first_slide + page) } else { Mapped::Moved(q.first_slide + q.slides - 1) }
}

/// LCS pairing walked from the front, preferring the earliest matches.
fn align_forward(a: &[String], b: &[String]) -> Vec<Option<usize>> {
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] { dp[i + 1][j + 1] + 1 } else { dp[i + 1][j].max(dp[i][j + 1]) };
        }
    }
    let mut out = vec![None; n];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] && dp[i][j] == dp[i + 1][j + 1] + 1 { out[i] = Some(j); i += 1; j += 1 }
        else if dp[i + 1][j] >= dp[i][j + 1] { i += 1 } else { j += 1 }
    }
    out
}

/// LCS pairing walked from the back, preferring the latest matches.
fn align_backward(a: &[String], b: &[String]) -> Vec<Option<usize>> {
    let (n, m) = (a.len(), b.len());
    let mut pre = vec![vec![0usize; m + 1]; n + 1];
    for i in 1..=n {
        for j in 1..=m {
            pre[i][j] = if a[i - 1] == b[j - 1] { pre[i - 1][j - 1] + 1 } else { pre[i - 1][j].max(pre[i][j - 1]) };
        }
    }
    let mut out = vec![None; n];
    let (mut i, mut j) = (n, m);
    while i > 0 && j > 0 {
        if a[i - 1] == b[j - 1] && pre[i][j] == pre[i - 1][j - 1] + 1 { out[i - 1] = Some(j - 1); i -= 1; j -= 1 }
        else if pre[i - 1][j] >= pre[i][j - 1] { i -= 1 } else { j -= 1 }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::SlideContent, sections::refs::kit};

    const L: &str = "Verse 1\nv1\nChorus\nc\nVerse 2\nv2\nBridge\nb";

    #[test]
    fn same_on_screen_ignores_label_notes_and_section() {
        let a = kit::song(L, &[], 1);
        let mut b = a[1].clone();
        b.label = "Refrain".into();
        b.notes = "watch the leader".into();
        b.section.as_mut().unwrap().occurrence = 7;
        assert!(same_on_screen(&a[1], &b));
    }

    #[test]
    fn same_on_screen_sees_footer_change() {
        let a = kit::song(L, &[], 1);
        let mut b = a[0].clone();
        if let SlideContent::Text { footer, .. } = &mut b.content { *footer = Some("x".into()) }
        assert!(!same_on_screen(&a[0], &b));
    }

    #[test]
    fn map_exact_same_label_ordinal_page() {
        let s = kit::song(L, &["Verse 1", "Chorus", "Verse 2", "Chorus"], 1);
        for i in 0..s.len() { assert_eq!(map_position(&s, i, &s), Mapped::Exact(i)) }
    }

    #[test]
    fn map_moved_when_pages_shrink() {
        let old = kit::song("Chorus\na\nb\nc", &[], 1);
        let new = kit::song("Chorus\na\nb", &[], 1);
        assert_eq!(map_position(&old, 2, &new), Mapped::Moved(1));
    }

    #[test]
    fn map_keeps_physical_occurrence_when_earlier_repeat_removed() {
        let old = kit::song(L, &["Chorus", "Verse 1", "Chorus", "Verse 2", "Chorus"], 1);
        let new = kit::song(L, &["Verse 1", "Chorus", "Verse 2", "Chorus"], 1);
        assert_eq!(map_position(&old, 2, &new), Mapped::Exact(1));
    }

    #[test]
    fn map_keeps_physical_occurrence_when_chorus_inserted_before() {
        let old = kit::song(L, &["Verse 1", "Chorus", "Verse 2"], 1);
        let new = kit::song(L, &["Chorus", "Verse 1", "Chorus", "Verse 2"], 1);
        assert_eq!(map_position(&old, 2, &new), Mapped::Exact(3));
    }

    #[test]
    fn map_ambiguous_alignment_falls_back_to_ordinal() {
        let old = kit::song(L, &["Chorus", "Chorus"], 1);
        let new = kit::song(L, &["Chorus"], 1);
        assert_eq!(map_position(&old, 0, &new), Mapped::Moved(0));
    }

    #[test]
    fn map_moved_when_label_renamed() {
        let old = kit::song(L, &[], 1);
        let new = kit::song(&L.replace("Chorus", "Refrain"), &[], 1);
        assert_eq!(map_position(&old, 1, &new), Mapped::Moved(1));
    }

    #[test]
    fn map_lost_when_section_deleted() {
        let old = kit::song(L, &[], 1);
        let new = kit::song("Verse 1\nv1\nChorus\nc\nVerse 2\nv2", &[], 1);
        assert_eq!(map_position(&old, 3, &new), Mapped::Lost);
    }

    #[test]
    fn map_non_song_by_index() {
        assert_eq!(map_position(&kit::plain(&["a", "b"]), 1, &kit::plain(&["a", "b", "c"])), Mapped::Exact(1));
        assert_eq!(map_position(&kit::plain(&["a", "b"]), 1, &kit::plain(&["a", "B"])), Mapped::Lost);
    }

    #[test]
    fn map_exact_across_blank_line_groups() {
        let old = kit::song("Verse 1\na\nb\nc\n\nd", &[], 2);
        let new = kit::song("Verse 1\na\nb\nC\n\nd", &[], 2);
        assert_eq!(map_position(&old, 2, &new), Mapped::Exact(2));
    }
}
