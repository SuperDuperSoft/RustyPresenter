//! Rich text: plain text plus a list of style spans (spec §4.4). Offsets are UTF-8 byte offsets, `end`
//! exclusive. A tidy span list is sorted, non-overlapping, non-empty and never holds the default style,
//! so unstyled text has no span at all.
use std::ops::Range;

use serde::{Deserialize, Serialize};

/// How a run of text differs from the theme. The default is "as the theme says".
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct RunStyle {
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Percent of the theme size; 0 means 100.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub size: u16,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct StyleSpan {
    pub start: u32,
    pub end: u32,
    #[serde(flatten)]
    pub style: RunStyle,
}

/// The size choices in the toolbar, as (percent, label). 0 is the theme size.
pub const SIZES: [(u16, &str); 4] = [(75, "Small"), (0, "Normal"), (125, "Large"), (150, "Huge")];

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &u16) -> bool {
    *n == 0
}

fn floor_boundary(text: &str, mut i: usize) -> usize {
    i = i.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_boundary(text: &str, mut i: usize) -> usize {
    i = i.min(text.len());
    while !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Sorts already non-overlapping spans, drops empty and default-style ones and merges equal neighbours
/// that touch.
fn tidy(mut spans: Vec<StyleSpan>) -> Vec<StyleSpan> {
    spans.retain(|s| s.start < s.end && s.style != RunStyle::default());
    spans.sort_by_key(|s| s.start);
    let mut out: Vec<StyleSpan> = Vec::with_capacity(spans.len());
    for s in spans {
        match out.last_mut() {
            Some(last) if last.end == s.start && last.style == s.style => last.end = s.end,
            _ => out.push(s),
        }
    }
    out
}

/// The parts of tidy `spans` that lie outside `start..end`.
fn outside(spans: &[StyleSpan], start: u32, end: u32) -> Vec<StyleSpan> {
    let mut out = Vec::new();
    for s in spans {
        if s.start < start {
            out.push(StyleSpan { start: s.start, end: s.end.min(start), style: s.style.clone() });
        }
        if s.end > end {
            out.push(StyleSpan { start: s.start.max(end), end: s.end, style: s.style.clone() });
        }
    }
    out
}

fn bounds(range: &Range<usize>) -> (u32, u32) {
    (range.start as u32, range.end as u32)
}

/// Makes any span list tidy for `text`: starts snap down and ends snap up to char boundaries, offsets
/// are clamped to the text, empty and default-style spans go, a later span wins where spans overlap,
/// and equal neighbours that touch are merged.
pub fn normalize(text: &str, spans: Vec<StyleSpan>) -> Vec<StyleSpan> {
    let snapped: Vec<StyleSpan> = spans
        .into_iter()
        .map(|s| StyleSpan {
            start: floor_boundary(text, s.start as usize) as u32,
            end: ceil_boundary(text, s.end as usize) as u32,
            style: s.style,
        })
        .filter(|s| s.start < s.end)
        .collect();
    let mut cuts: Vec<u32> = snapped.iter().flat_map(|s| [s.start, s.end]).collect();
    cuts.sort_unstable();
    cuts.dedup();
    let mut painted = Vec::new();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        // The last span in input order that covers this piece wins.
        if let Some(top) = snapped.iter().rev().find(|s| s.start <= a && b <= s.end) {
            painted.push(StyleSpan { start: a, end: b, style: top.style.clone() });
        }
    }
    tidy(painted)
}

/// Changes the style of `range` with `f`. Unstyled text inside the range starts from the default style.
pub fn apply(spans: &[StyleSpan], range: Range<usize>, f: impl Fn(&mut RunStyle)) -> Vec<StyleSpan> {
    let (start, end) = bounds(&range);
    if start >= end {
        return spans.to_vec();
    }
    let mut out = outside(spans, start, end);
    let mut push = |a: u32, b: u32, mut style: RunStyle| {
        f(&mut style);
        out.push(StyleSpan { start: a, end: b, style });
    };
    let mut inside: Vec<&StyleSpan> = spans.iter().filter(|s| s.start < end && s.end > start).collect();
    inside.sort_by_key(|s| s.start);
    let mut cur = start;
    for s in inside {
        let (a, b) = (s.start.max(start), s.end.min(end));
        if cur < a {
            push(cur, a, RunStyle::default());
        }
        push(a, b, s.style.clone());
        cur = b;
    }
    if cur < end {
        push(cur, end, RunStyle::default());
    }
    tidy(out)
}

/// Removes all formatting inside `range`.
pub fn clear(spans: &[StyleSpan], range: Range<usize>) -> Vec<StyleSpan> {
    let (start, end) = bounds(&range);
    if start >= end {
        return spans.to_vec();
    }
    tidy(outside(spans, start, end))
}

/// The spans inside `range`, cut to it and rebased so `range.start` becomes 0.
pub fn slice(spans: &[StyleSpan], range: Range<usize>) -> Vec<StyleSpan> {
    let (start, end) = bounds(&range);
    let cut = spans
        .iter()
        .filter(|s| s.start < end && s.end > start)
        .map(|s| StyleSpan { start: s.start.max(start) - start, end: s.end.min(end) - start, style: s.style.clone() })
        .collect();
    tidy(cut)
}

/// Cuts an announcement body into slide blocks the way `slides::text_slides` does ("\r\n" → "\n", split
/// on a blank line, trim, drop empty blocks), carrying each block's spans along, rebased to the block.
pub fn blocks(body: &str, spans: &[StyleSpan]) -> Vec<(String, Vec<StyleSpan>)> {
    // Drop the '\r' of each "\r\n" and remember where, so span offsets can follow.
    let mut text = String::with_capacity(body.len());
    let mut dropped: Vec<usize> = Vec::new(); // byte offsets in `body` of the dropped '\r's
    let mut rest = body;
    let mut at = 0;
    while let Some(i) = rest.find("\r\n") {
        text.push_str(&rest[..i]);
        dropped.push(at + i);
        rest = &rest[i + 1..];
        at += i + 1;
    }
    text.push_str(rest);
    let map = |p: u32| p - dropped.iter().filter(|&&r| r < p as usize).count() as u32;
    let spans = normalize(&text, spans.iter().map(|s| StyleSpan { start: map(s.start), end: map(s.end), style: s.style.clone() }).collect());

    let mut out = Vec::new();
    let mut start = 0;
    loop {
        let end = text[start..].find("\n\n").map_or(text.len(), |i| start + i);
        let piece = &text[start..end];
        let lead = piece.len() - piece.trim_start().len();
        let trimmed = piece.trim();
        if !trimmed.is_empty() {
            let a = start + lead;
            out.push((trimmed.to_string(), slice(&spans, a..a + trimmed.len())));
        }
        if end == text.len() {
            break;
        }
        start = end + 2;
    }
    out
}

/// ALL CAPS that keeps formatting. Each piece between span boundaries is uppercased on its own, so a
/// boundary moves only when an earlier piece's UTF-8 length changed ("ﬁ" → "FI" is 3 → 2 bytes;
/// "ß" → "SS" is 2 → 2 bytes and moves nothing).
pub fn uppercase(text: &str, spans: &[StyleSpan]) -> (String, Vec<StyleSpan>) {
    let spans = normalize(text, spans.to_vec());
    let mut cuts: Vec<usize> = spans.iter().flat_map(|s| [s.start as usize, s.end as usize]).chain([0, text.len()]).collect();
    cuts.sort_unstable();
    cuts.dedup();
    let mut out = String::with_capacity(text.len());
    let mut moved: Vec<(usize, usize)> = vec![(0, 0)]; // (old offset, new offset) at every cut
    for w in cuts.windows(2) {
        out.push_str(&text[w[0]..w[1]].to_uppercase());
        moved.push((w[1], out.len()));
    }
    let new_at = |p: u32| moved.iter().find(|(old, _)| *old == p as usize).map_or(p, |(_, new)| *new as u32);
    let spans = spans.into_iter().map(|s| StyleSpan { start: new_at(s.start), end: new_at(s.end), style: s.style }).collect();
    (out, tidy(spans))
}

/// The style of the byte just before `pos`, which is what typing at `pos` continues. Position 0 has
/// the default style.
pub fn style_at(spans: &[StyleSpan], pos: usize) -> RunStyle {
    if pos == 0 {
        return RunStyle::default();
    }
    let p = (pos - 1) as u32;
    spans.iter().find(|s| s.start <= p && p < s.end).map(|s| s.style.clone()).unwrap_or_default()
}

/// What one edit changed between `old` and `new`, as (start, removed, inserted) in bytes: the common
/// prefix, then the common suffix that does not overlap it, both snapped back to char boundaries.
pub fn diff_edit(old: &str, new: &str) -> (usize, usize, usize) {
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let mut prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    while !old.is_char_boundary(prefix) || !new.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let max = a.len().min(b.len()) - prefix;
    let mut suffix = a.iter().rev().zip(b.iter().rev()).take(max).take_while(|(x, y)| x == y).count();
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }
    (prefix, old.len() - suffix - prefix, new.len() - suffix - prefix)
}

/// Moves tidy `spans` through one edit: `start..start + removed` is cut out, spans after it shift by
/// `inserted − removed`, and the inserted bytes get `inherit` (`None` = plain text).
pub fn adjust(spans: &[StyleSpan], start: usize, removed: usize, inserted: usize, inherit: Option<&RunStyle>) -> Vec<StyleSpan> {
    let (start, cut_end) = (start as u32, (start + removed) as u32);
    let (removed, inserted) = (removed as u32, inserted as u32);
    let mut out = Vec::with_capacity(spans.len() + 2);
    for s in spans {
        // The part before the edit stays where it is.
        if s.start < start {
            out.push(StyleSpan { start: s.start, end: s.end.min(start), style: s.style.clone() });
        }
        // The part after the removed bytes moves past the inserted ones.
        if s.end > cut_end {
            let from = s.start.max(cut_end);
            out.push(StyleSpan { start: from - removed + inserted, end: s.end - removed + inserted, style: s.style.clone() });
        }
    }
    if let Some(style) = inherit {
        out.push(StyleSpan { start, end: start + inserted, style: style.clone() });
    }
    tidy(out)
}

/// Longest label text in a Formatting chip, in characters, before "…".
const RANGE_LABEL_CHARS: usize = 24;

/// One entry per formatted range, for the Formatting chips: the range, its style and its text, cut at
/// 24 characters with "…".
pub fn ranges(text: &str, spans: &[StyleSpan]) -> Vec<(Range<usize>, RunStyle, String)> {
    normalize(text, spans.to_vec())
        .into_iter()
        .map(|s| {
            let r = s.start as usize..s.end as usize;
            let words = &text[r.clone()];
            let label = match words.char_indices().nth(RANGE_LABEL_CHARS) {
                Some((cut, _)) => format!("{}…", &words[..cut]),
                None => words.to_string(),
            };
            (r, s.style, label)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sp(s: u32, e: u32, f: impl Fn(&mut RunStyle)) -> StyleSpan {
        let mut st = RunStyle::default();
        f(&mut st);
        StyleSpan { start: s, end: e, style: st }
    }
    fn bold(s: &mut RunStyle) { s.bold = true; }
    fn italic(s: &mut RunStyle) { s.italic = true; }

    #[test]
    fn normalize_snaps_to_char_boundaries() {
        let t = "Café ß 🙏"; // é = 2 bytes at 3..5, 🙏 = 4 bytes at 9..13
        assert_eq!(normalize(t, vec![sp(4, 10, |s| s.bold = true)]), vec![sp(3, 13, |s| s.bold = true)]);
        assert_eq!(normalize(t, vec![sp(20, 30, |s| s.bold = true)]), vec![]);
    }

    #[test]
    fn normalize_merges_and_splits_overlaps() {
        let t = "abcdefghijkl";
        assert_eq!(normalize(t, vec![sp(4, 8, bold), sp(0, 4, bold)]), vec![sp(0, 8, bold)]);
        assert_eq!(normalize(t, vec![sp(0, 6, bold), sp(3, 9, italic)]), vec![sp(0, 3, bold), sp(3, 9, italic)]);
        // empty, reversed and default-style spans go
        assert_eq!(normalize(t, vec![sp(2, 2, bold), sp(5, 3, bold), sp(0, 4, |_| {})]), vec![]);
        // a later span inside an earlier one splits it
        assert_eq!(normalize(t, vec![sp(0, 9, bold), sp(3, 6, italic)]), vec![sp(0, 3, bold), sp(3, 6, italic), sp(6, 9, bold)]);
    }

    #[test]
    fn apply_bold_to_middle_splits_span() {
        let out = apply(&[sp(0, 10, italic)], 3..6, bold);
        assert_eq!(out, vec![sp(0, 3, italic), sp(3, 6, |s| { s.italic = true; s.bold = true; }), sp(6, 10, italic)]);
        // a gap in the range gets the default style first
        assert_eq!(apply(&[sp(0, 2, italic)], 1..5, bold), vec![sp(0, 1, italic), sp(1, 2, |s| { s.italic = true; s.bold = true; }), sp(2, 5, bold)]);
        // applying a style the neighbour already has merges them
        assert_eq!(apply(&[sp(0, 3, bold)], 3..6, bold), vec![sp(0, 6, bold)]);
    }

    #[test]
    fn clear_formatting_removes_only_range() {
        assert_eq!(clear(&[sp(0, 10, bold), sp(12, 14, italic)], 3..6), vec![sp(0, 3, bold), sp(6, 10, bold), sp(12, 14, italic)]);
        assert_eq!(clear(&[sp(0, 10, bold)], 0..10), vec![]);
    }

    #[test]
    fn slice_cuts_and_rebases() {
        assert_eq!(slice(&[sp(0, 4, bold), sp(6, 12, italic)], 2..8), vec![sp(0, 2, bold), sp(4, 6, italic)]);
        assert_eq!(slice(&[sp(0, 4, bold)], 4..8), vec![]);
    }

    #[test]
    fn slice_by_blank_line_blocks_rebases_offsets() {
        let body = "Sunday 10am\n\n  Pancake breakfast  ";
        let b = blocks(body, &[sp(0, 6, |s| s.bold = true), sp(15, 22, |s| s.color = Some("#f5b94a".into()))]);
        assert_eq!(b, vec![("Sunday 10am".into(), vec![sp(0, 6, |s| s.bold = true)]), ("Pancake breakfast".into(), vec![sp(0, 7, |s| s.color = Some("#f5b94a".into()))])]);
    }

    #[test]
    fn blocks_without_spans_match_text_slides() {
        assert_eq!(blocks("a\r\n\r\nb", &[]), vec![("a".to_string(), vec![]), ("b".to_string(), vec![])]);
        assert_eq!(blocks("  \n\n \n\n", &[]), vec![]);
        assert_eq!(blocks("a\n\n\nb", &[]), vec![("a".to_string(), vec![]), ("b".to_string(), vec![])]);
    }

    #[test]
    fn blocks_remap_spans_over_crlf() {
        // "one\r\ntwo\r\n\r\nthree": "two" is bytes 5..8, "three" is 12..17; after "\r\n" → "\n" they are 4..7 and 9..14
        let b = blocks("one\r\ntwo\r\n\r\nthree", &[sp(5, 8, bold), sp(12, 17, italic)]);
        assert_eq!(b, vec![("one\ntwo".into(), vec![sp(4, 7, bold)]), ("three".into(), vec![sp(0, 5, italic)])]);
    }

    // Plan decision 14: "ß → SS" changes characters (1 → 2), not bytes (2 → 2), so spans stay put.
    #[test]
    fn uppercase_keeps_spans_aligned_for_eszett() {
        let (t, s) = uppercase("groß ja", &[sp(5, 7, |s| s.bold = true)]); // " j" is bytes 5..7
        assert_eq!(t, "GROSS JA");
        assert_eq!(s, vec![sp(5, 7, |s| s.bold = true)]);
    }

    // "ﬁ" (U+FB01) is 3 bytes and uppercases to "FI" (2 bytes): spans after it move left by 1.
    #[test]
    fn uppercase_shifts_spans_when_byte_length_changes() {
        let (t, s) = uppercase("ﬁne ja", &[sp(6, 8, |s| s.bold = true)]); // "ja" is bytes 6..8
        assert_eq!(t, "FINE JA");
        assert_eq!(s, vec![sp(5, 7, |s| s.bold = true)]);
        let (_, s2) = uppercase("ﬁne ja", &[sp(0, 3, |s| s.italic = true)]); // span over the ligature itself
        assert_eq!(s2, vec![sp(0, 2, |s| s.italic = true)]);
    }

    #[test]
    fn style_at_reads_the_char_before() {
        let spans = [sp(2, 4, bold)];
        assert_eq!(style_at(&spans, 0), RunStyle::default());
        assert_eq!(style_at(&spans, 2), RunStyle::default());
        assert!(style_at(&spans, 3).bold);
        assert!(style_at(&spans, 4).bold);
        assert_eq!(style_at(&spans, 5), RunStyle::default());
    }

    #[test]
    fn spans_serialize_flat_and_skip_defaults() {
        assert_eq!(serde_json::to_string(&sp(0, 3, |s| s.bold = true)).unwrap(), r#"{"start":0,"end":3,"bold":true}"#);
        let back: StyleSpan = serde_json::from_str(r##"{"start":1,"end":2,"color":"#ff4f55","size":125}"##).unwrap();
        assert_eq!(back, sp(1, 2, |s| { s.color = Some("#ff4f55".into()); s.size = 125; }));
        assert_eq!(SIZES[1], (0, "Normal"));
    }

    #[test]
    fn diff_edit_finds_middle_insert() {
        assert_eq!(diff_edit("Sunday 10am", "Sunday at 10am"), (7, 0, 3));
        assert_eq!(diff_edit("abc", "abc"), (3, 0, 0));
        assert_eq!(diff_edit("aaa", "aaaa"), (3, 0, 1)); // suffix never overlaps the prefix
        assert_eq!(diff_edit("Sunday", "Sun"), (3, 3, 0));
        // "é" (c3 a9) → "è" (c3 a8) shares a first byte, but the edit starts at the char
        assert_eq!(diff_edit("café", "cafè"), (3, 2, 2));
    }

    #[test]
    fn typing_inside_span_extends_it() {
        let spans = [sp(0, 6, bold)];
        assert_eq!(adjust(&spans, 3, 0, 2, Some(&style_at(&spans, 3))), vec![sp(0, 8, bold)]);
    }

    #[test]
    fn typing_at_span_end_inherits_style() {
        let spans = [sp(0, 6, bold)];
        assert_eq!(adjust(&spans, 6, 0, 1, Some(&style_at(&spans, 6))), vec![sp(0, 7, bold)]);
        // plain typing at the end leaves the span alone
        assert_eq!(adjust(&spans, 6, 0, 1, None), vec![sp(0, 6, bold)]);
        // plain typing inside a span splits it
        assert_eq!(adjust(&spans, 3, 0, 1, None), vec![sp(0, 3, bold), sp(4, 7, bold)]);
    }

    #[test]
    fn delete_across_spans_shrinks_both() {
        assert_eq!(adjust(&[sp(0, 4, bold), sp(4, 8, italic)], 2, 4, 0, None), vec![sp(0, 2, bold), sp(2, 4, italic)]);
        // deleting a whole span drops it; spans after the edit move left
        assert_eq!(adjust(&[sp(2, 4, bold), sp(6, 8, italic)], 1, 4, 0, None), vec![sp(2, 4, italic)]);
    }

    #[test]
    fn ranges_label_cuts_long_text() {
        let t = "Pancake breakfast this Saturday morning";
        let r = ranges(t, &[sp(0, t.len() as u32, |s| s.bold = true)]);
        assert_eq!(r[0].2, "Pancake breakfast this S…");
        let r = ranges("Sunday 10am", &[sp(0, 6, bold), sp(7, 11, italic)]);
        assert_eq!(r, vec![(0..6, sp(0, 0, bold).style, "Sunday".to_string()), (7..11, sp(0, 0, italic).style, "10am".to_string())]);
    }
}
