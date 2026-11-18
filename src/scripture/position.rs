//! Keeping the operator's place when slides are rebuilt, and the Find a verse dialog's
//! selection and reply-ordering helpers (spec §5.1, §5.5).

use super::SlideSource;
use super::planning::refs::join_refs;
use super::strings::add_button;
use crate::model::VerseText;

/// Which rebuilt slide stands where the operator was. With a source: the new slide whose verse
/// range holds the old first verse with the same `part`, else that range's part 0, else 0.
/// Without one (a non-scripture item): the same index, clamped. Always 0 for no slides.
pub fn index_for_source(old: Option<&SlideSource>, old_index: usize, new: &[Option<SlideSource>]) -> usize {
    let Some(old) = old else { return old_index.min(new.len().saturating_sub(1)) };
    let holds = |s: &SlideSource| (s.first..=s.last).contains(&old.first);
    let find = |part: u8| new.iter().position(|n| n.as_ref().is_some_and(|s| holds(s) && s.part == part));
    find(old.part).or_else(|| find(0)).unwrap_or(0)
}

/// The verses picked in the Find a verse dialog's chapter list: `anchor` stays where the
/// click was, `head` moves with Shift+click and Shift+arrows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection { pub anchor: usize, pub head: usize }

impl Selection {
    /// A single verse.
    pub fn at(i: usize) -> Self { Self { anchor: i, head: i } }

    /// Shift+click on verse `i` of a `len`-verse chapter.
    pub fn shift_to(&mut self, i: usize, len: usize) { self.head = i.min(len.saturating_sub(1)); }

    /// Shift+Up/Down by `delta`; nothing happens past either end of the chapter.
    pub fn extend(&mut self, delta: i32, len: usize) {
        let next = self.head as i64 + delta as i64;
        if (0..len as i64).contains(&next) {
            self.head = next as usize;
        }
    }

    /// First and last selected index, inclusive and ordered.
    pub fn range(&self) -> (usize, usize) { (self.anchor.min(self.head), self.anchor.max(self.head)) }
}

/// Numbers searches so a slow reply to an older query is dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SeqGuard(u64);

impl SeqGuard {
    /// The number for a new request; every earlier number stops being current.
    pub fn next(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }

    pub fn is_current(&self, n: u64) -> bool { n == self.0 }
}

/// "Add Psalm 46:10–11" for the selected verses of `chapter`.
pub fn add_label(chapter: &[VerseText], sel: &Selection) -> String {
    let (a, b) = sel.range();
    let picked: Vec<&VerseText> = chapter.iter().skip(a).take(b + 1 - a).collect();
    add_button(&join_refs(&picked))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripture::test_support::verses;
    use crate::scripture::VerseId;

    fn s(first: (u8, u16, u16), last: (u8, u16, u16), part: u8) -> SlideSource {
        SlideSource { first: VerseId(first.0, first.1, first.2), last: VerseId(last.0, last.1, last.2), part }
    }

    fn src(first: (u8, u16, u16), last: (u8, u16, u16), part: u8) -> Option<SlideSource> { Some(s(first, last, part)) }

    fn psalm_46() -> Vec<VerseText> { verses(18, 46, 1..=11) }

    #[test]
    fn index_for_source_same_first_verse() {
        let new = [src((42, 3, 16), (42, 3, 17), 0), src((42, 3, 18), (42, 3, 19), 0), src((42, 3, 18), (42, 3, 19), 1)];
        assert_eq!(index_for_source(Some(&s((42, 3, 18), (42, 3, 18), 1)), 0, &new), 2);
        assert_eq!(index_for_source(Some(&s((42, 3, 19), (42, 3, 19), 3)), 0, &new), 1);
    }

    #[test]
    fn index_for_source_falls_back_to_zero() {
        let new = [src((42, 3, 16), (42, 3, 17), 0), src((42, 3, 18), (42, 3, 19), 0)];
        assert_eq!(index_for_source(Some(&s((42, 5, 1), (42, 5, 1), 0)), 1, &new), 0);
        assert_eq!(index_for_source(Some(&s((42, 3, 16), (42, 3, 16), 0)), 1, &[]), 0);
    }

    #[test]
    fn index_without_source_keeps_index_clamped() {
        assert_eq!(index_for_source(None, 7, &[None, None, None]), 2);
        assert_eq!(index_for_source(None, 1, &[None, None]), 1);
        assert_eq!(index_for_source(None, 3, &[]), 0);
    }

    #[test]
    fn shift_down_stops_at_chapter_end() {
        let mut sel = Selection::at(9);
        sel.extend(1, 11);
        assert_eq!(sel.range(), (9, 10));
        sel.extend(1, 11);
        assert_eq!(sel.range(), (9, 10)); // already at the last verse: no-op
        sel.extend(-1, 11);
        assert_eq!(sel.range(), (9, 9));
        sel.extend(-1, 11);
        assert_eq!(sel.range(), (8, 9));
        let mut top = Selection::at(0);
        top.extend(-1, 5);
        assert_eq!(top.range(), (0, 0));
    }

    #[test]
    fn shift_click_extends_range_and_add_label_updates() {
        let ch = psalm_46();
        let mut sel = Selection::at(9);
        assert_eq!(add_label(&ch, &sel), "Add Psalm 46:10");
        sel.shift_to(10, ch.len());
        assert_eq!(add_label(&ch, &sel), "Add Psalm 46:10–11");
        sel.shift_to(7, ch.len());
        assert_eq!((sel.range(), add_label(&ch, &sel)), ((7, 9), "Add Psalm 46:8–10".to_string())); // anchor stays at 9
    }

    #[test]
    fn seq_guard_drops_older_replies() {
        let mut g = SeqGuard::default();
        let a = g.next();
        let b = g.next();
        assert!(!g.is_current(a) && g.is_current(b));
        assert!(b > a);
    }
}
