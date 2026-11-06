//! Editing a song's order as chips (spec §3.5), and where Edit words puts the caret (spec §3.4).
use super::{SectionKind, resolve_order};
use crate::slides::{parse_sections, section_label};

#[derive(Clone, Debug, PartialEq)]
pub struct ArrChipData { pub label: String, pub short: String, pub kind: SectionKind, pub missing: bool, pub faded: bool }

/// The section labels in written order (repeats kept).
pub fn written_order(lyrics: &str) -> Vec<String> { parse_sections(lyrics).into_iter().map(|s| s.label).collect() }

/// The chips for `arrangement` (the written order, faded, when it is empty), the `+ section` labels (each written
/// label once) and whether the strip shows the written order.
pub fn chips(lyrics: &str, arrangement: &[String]) -> (Vec<ArrChipData>, Vec<String>, bool) {
    let written = written_order(lyrics);
    let chip = |label: &str, faded: bool| {
        let (kind, number) = SectionKind::from_label(label);
        let missing = !written.iter().any(|w| w.eq_ignore_ascii_case(label.trim()));
        ArrChipData { label: label.trim().to_string(), short: kind.short(number), kind, missing, faded }
    };
    let is_written = arrangement.is_empty();
    let chips = if is_written { written.iter().map(|l| chip(l, true)).collect() } else { arrangement.iter().map(|l| chip(l, false)).collect() };
    let mut adds: Vec<String> = Vec::new();
    for l in &written { if !adds.iter().any(|a| a.eq_ignore_ascii_case(l)) { adds.push(l.clone()) } }
    (chips, adds, is_written)
}

/// Moves entry `from` to `to` (the service list's drop rule: remove, then insert at `to` clamped).
pub fn move_entry(v: &mut Vec<String>, from: usize, to: usize) {
    if from >= v.len() || from == to { return }
    let e = v.remove(from);
    let to = to.min(v.len());
    v.insert(to, e);
}

pub fn remove(v: &mut Vec<String>, i: usize) { if i < v.len() { v.remove(i); } }

/// An empty order (the written one) becomes the written order spelled out, so it can be changed.
pub fn ensure_real(v: &mut Vec<String>, lyrics: &str) { if v.is_empty() { *v = written_order(lyrics) } }

pub fn add(v: &mut Vec<String>, lyrics: &str, label: &str) {
    ensure_real(v, lyrics);
    v.push(label.to_string());
}

/// The byte offset of the first lyric line after the `nth` (0-based) heading named `label` (any case).
pub fn heading_offset(lyrics: &str, label: &str, nth: usize) -> Option<usize> {
    let mut offset = 0;
    let mut seen = 0;
    let mut after_heading = false;
    for line in lyrics.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let text = line.trim_end_matches(['\n', '\r']);
        if after_heading {
            if text.trim().is_empty() { continue }
            return Some(start);
        }
        if section_label(text).is_some_and(|l| l.eq_ignore_ascii_case(label.trim())) {
            if seen == nth { after_heading = true } else { seen += 1 }
        }
    }
    None
}

/// The caret for Edit words: the first lyric line of the written section that played occurrence `occurrence`.
pub fn caret_for_occurrence(lyrics: &str, order: &[String], occurrence: u16) -> Option<usize> {
    let labels = written_order(lyrics);
    let idx = *resolve_order(order, &labels).get(occurrence as usize)?;
    let nth = labels[..idx].iter().filter(|l| l.trim().eq_ignore_ascii_case(labels[idx].trim())).count();
    heading_offset(lyrics, &labels[idx], nth)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> { s.iter().map(|s| s.to_string()).collect() }

    #[test]
    fn move_entry_left_and_right() {
        let mut a = v(&["a", "b", "c"]);
        move_entry(&mut a, 0, 2);
        assert_eq!(a, v(&["b", "c", "a"]));
        let mut a = v(&["a", "b", "c"]);
        move_entry(&mut a, 2, 0);
        assert_eq!(a, v(&["c", "a", "b"]));
    }

    #[test]
    fn drop_on_self_does_nothing() {
        let mut a = v(&["a", "b"]);
        move_entry(&mut a, 1, 1);
        move_entry(&mut a, 5, 0);
        assert_eq!(a, v(&["a", "b"]));
    }

    #[test]
    fn drop_on_end_zone_moves_last() {
        let mut a = v(&["a", "b", "c"]);
        move_entry(&mut a, 0, 3);
        assert_eq!(a, v(&["b", "c", "a"]));
    }

    #[test]
    fn remove_entry() {
        let mut a = v(&["a", "b"]);
        remove(&mut a, 0);
        remove(&mut a, 9);
        assert_eq!(a, v(&["b"]));
    }

    #[test]
    fn first_add_materialises_written_order() {
        let mut a = Vec::new();
        add(&mut a, "Verse 1\na\nChorus\nx", "Chorus");
        assert_eq!(a, v(&["Verse 1", "Chorus", "Chorus"]));
    }

    #[test]
    fn reset_to_written_order() {
        let (c, _, written) = chips("Verse 1\na\nChorus\nx", &[]);
        assert!(written);
        assert!(c.iter().all(|c| c.faded));
        assert_eq!(c.iter().map(|c| c.short.as_str()).collect::<Vec<_>>(), ["V1", "C"]);
    }

    #[test]
    fn missing_entries_flagged() {
        let (c, _, written) = chips("Verse 1\na\nChorus\nx", &v(&["Verse 1", "Coda"]));
        assert!(!written);
        assert!(!c[0].missing);
        assert!(c[1].missing);
        assert_eq!((c[1].kind, c[1].short.as_str()), (SectionKind::Ending, "E"));
    }

    #[test]
    fn adds_are_distinct_in_written_order() {
        let (_, adds, _) = chips("Verse\na\nChorus\nx\nVerse\nb", &[]);
        assert_eq!(adds, v(&["Verse", "Chorus"]));
    }

    #[test]
    fn heading_offset_finds_nth_heading() {
        let l = "Verse 1\na\nChorus\nx\nChorus\ny";
        assert_eq!(heading_offset(l, "chorus", 1).map(|i| &l[i..]), Some("y"));
        assert_eq!(heading_offset(l, "Bridge", 0), None);
    }

    #[test]
    fn heading_offset_handles_crlf_and_multibyte() {
        let l = "Verse 1\r\nÉl es 🙏\r\n\r\nChorus\r\nx";
        let i = heading_offset(l, "Chorus", 0).unwrap();
        assert!(l.is_char_boundary(i));
        assert!(l[i..].starts_with('x'));
    }

    #[test]
    fn heading_offset_none_for_unlabelled() { assert_eq!(heading_offset("a\n\nb", "Part 2", 0), None) }

    #[test]
    fn caret_for_occurrence_with_duplicate_plain_verses() {
        let l = "Verse\na\nVerse\nb\nVerse\nc\nVerse\nd";
        let at = |o| caret_for_occurrence(l, &[], o).map(|i| &l[i..i + 1]);
        assert_eq!((at(0), at(2), at(3)), (Some("a"), Some("c"), Some("d")));
    }

    #[test]
    fn caret_for_occurrence_with_repeated_arranged_chorus() {
        let l = "Verse 1\na\nChorus\nx\nVerse 2\nb";
        let order = v(&["Chorus", "Verse 1", "Chorus", "Verse 2"]);
        let at = |o| caret_for_occurrence(l, &order, o).map(|i| &l[i..i + 1]);
        assert_eq!((at(0), at(2), at(3), at(9)), (Some("x"), Some("x"), Some("b"), None));
    }
}
