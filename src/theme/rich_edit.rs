//! The pure model behind the rich text editor (spec §3.5, Part B's `RichEdit` widget). The text box
//! shows plain text; this model keeps the spans in step with each edit, the toolbar's pressed state,
//! a pending style for an empty selection, and the Formatting chips.
use std::ops::Range;

use crate::theme::richtext::{self, RunStyle, StyleSpan};

/// The on/off buttons of the toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    Bold,
    Italic,
    Underline,
}

impl Toggle {
    fn flag(self, style: &mut RunStyle) -> &mut bool {
        match self {
            Toggle::Bold => &mut style.bold,
            Toggle::Italic => &mut style.italic,
            Toggle::Underline => &mut style.underline,
        }
    }
}

/// One Formatting chip: its label and the byte range it selects.
#[derive(Clone, Debug, PartialEq)]
pub struct RangeChip {
    pub label: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Default)]
pub struct RichEditModel {
    pub text: String,
    pub spans: Vec<StyleSpan>,
    /// Style picked with an empty selection, used by the next text typed at that byte position.
    pending: Option<(usize, RunStyle)>,
}

impl RichEditModel {
    pub fn new(text: &str, spans: Vec<StyleSpan>) -> Self {
        Self { text: text.to_string(), spans: richtext::normalize(text, spans), pending: None }
    }

    /// The box's text changed to `new_text`. Typed or pasted text takes the pending style if it was
    /// picked where the edit starts, else the style of the character before it.
    pub fn edited(&mut self, new_text: &str) {
        let (start, removed, inserted) = richtext::diff_edit(&self.text, new_text);
        let inherit = match self.pending.take() {
            Some((pos, style)) if pos == start => style,
            _ => richtext::style_at(&self.spans, start),
        };
        let spans = richtext::adjust(&self.spans, start, removed, inserted, Some(&inherit));
        self.text = new_text.to_string();
        self.spans = richtext::normalize(&self.text, spans);
    }

    pub fn cursor_moved(&mut self, cursor: usize) {
        if self.pending.as_ref().is_some_and(|(pos, _)| *pos != cursor) {
            self.pending = None;
        }
    }

    /// Turns `t` off when the whole selection has it, else on. An empty selection sets the pending style.
    pub fn toggle(&mut self, sel: Range<usize>, t: Toggle) {
        let on = !*t.flag(&mut self.pressed(sel.clone()));
        self.change(sel, move |s| *t.flag(s) = on);
    }

    /// `None` is "Theme color".
    pub fn set_color(&mut self, sel: Range<usize>, color: Option<String>) {
        self.change(sel, move |s| s.color = color.clone());
    }

    /// Percent of the theme size; 0 is Normal.
    pub fn set_size(&mut self, sel: Range<usize>, size: u16) {
        self.change(sel, move |s| s.size = size);
    }

    pub fn clear(&mut self, sel: Range<usize>) {
        self.change(sel, |s| *s = RunStyle::default());
    }

    /// The toolbar state: the pending style if there is one, else the style the whole selection shares
    /// (for an empty selection, the style typing there would get).
    pub fn pressed(&self, sel: Range<usize>) -> RunStyle {
        if let Some((pos, style)) = &self.pending {
            if sel.is_empty() && *pos == sel.start {
                return style.clone();
            }
        }
        let sel = self.snap(sel);
        if sel.is_empty() {
            return richtext::style_at(&self.spans, sel.start);
        }
        let mut pieces = self.pieces(sel).into_iter();
        let mut common = pieces.next().unwrap_or_default();
        for p in pieces {
            common.bold &= p.bold;
            common.italic &= p.italic;
            common.underline &= p.underline;
            if common.color != p.color {
                common.color = None;
            }
            if common.size != p.size {
                common.size = 0;
            }
        }
        common
    }

    pub fn chips(&self) -> Vec<RangeChip> {
        richtext::ranges(&self.text, &self.spans)
            .into_iter()
            .map(|(r, style, text)| RangeChip { label: crate::theme::strings::range_chip(&style, &text), start: r.start, end: r.end })
            .collect()
    }

    /// The chip's ×: clears the formatting of chip `i`'s range.
    pub fn remove_chip(&mut self, i: usize) {
        if let Some(c) = self.chips().get(i) {
            self.spans = richtext::clear(&self.spans, c.start..c.end);
        }
    }

    /// **B** is disabled when the theme is already bold.
    pub fn bold_disabled(theme_weight: u16) -> bool {
        theme_weight >= 700
    }

    /// Applies `f` to the selection, or, for an empty selection, to the pending style there.
    fn change(&mut self, sel: Range<usize>, f: impl Fn(&mut RunStyle)) {
        let sel = self.snap(sel);
        if sel.is_empty() {
            let mut style = self.pressed(sel.clone());
            f(&mut style);
            self.pending = Some((sel.start, style));
        } else {
            self.spans = richtext::apply(&self.spans, sel, f);
        }
    }

    /// The selection clamped to the text and widened to char boundaries.
    fn snap(&self, sel: Range<usize>) -> Range<usize> {
        let t = &self.text;
        let mut a = sel.start.min(t.len());
        let mut b = sel.end.clamp(a, t.len());
        while !t.is_char_boundary(a) {
            a -= 1;
        }
        while !t.is_char_boundary(b) {
            b += 1;
        }
        a..b
    }

    /// The styles covering a non-empty `sel`, unstyled gaps included as the default style.
    fn pieces(&self, sel: Range<usize>) -> Vec<RunStyle> {
        let (start, end) = (sel.start as u32, sel.end as u32);
        let mut out = Vec::new();
        let mut cur = start;
        for s in self.spans.iter().filter(|s| s.start < end && s.end > start) {
            if cur < s.start {
                out.push(RunStyle::default());
            }
            out.push(s.style.clone());
            cur = s.end;
        }
        if cur < end {
            out.push(RunStyle::default());
        }
        out
    }
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
    fn gold(s: &mut RunStyle) { s.color = Some("#f5b94a".into()); }

    #[test]
    fn new_normalizes() {
        let m = RichEditModel::new("abcdef", vec![sp(4, 2, bold), sp(0, 3, bold), sp(3, 20, bold)]);
        assert_eq!(m.spans, vec![sp(0, 6, bold)]);
    }

    #[test]
    fn rich_edit_bold_selection_round_trips() {
        let mut m = RichEditModel::new("Sunday 10am", vec![]);
        m.toggle(0..6, Toggle::Bold);
        let json = serde_json::to_string(&m.spans).unwrap();
        assert_eq!(json, r#"[{"start":0,"end":6,"bold":true}]"#);
        let back: Vec<StyleSpan> = serde_json::from_str(&json).unwrap();
        assert_eq!(RichEditModel::new("Sunday 10am", back).spans, m.spans);
    }

    #[test]
    fn toggle_twice_removes() {
        let mut m = RichEditModel::new("Sunday 10am", vec![]);
        m.toggle(0..6, Toggle::Italic);
        m.toggle(0..6, Toggle::Italic);
        assert_eq!(m.spans, vec![]);
        // a partly bold selection becomes all bold first
        let mut m = RichEditModel::new("Sunday 10am", vec![sp(0, 3, bold)]);
        m.toggle(0..6, Toggle::Bold);
        assert_eq!(m.spans, vec![sp(0, 6, bold)]);
        m.toggle(2..4, Toggle::Underline);
        m.toggle(2..4, Toggle::Underline);
        assert_eq!(m.spans, vec![sp(0, 6, bold)]);
    }

    #[test]
    fn rich_edit_pending_style_applies_to_typed_text() {
        let mut m = RichEditModel::new("Hi ", vec![]);
        m.toggle(3..3, Toggle::Bold);
        m.edited("Hi there");
        assert_eq!(m.spans, vec![sp(3, 8, |s| s.bold = true)]);
    }

    #[test]
    fn pending_turns_style_off_at_span_end() {
        let mut m = RichEditModel::new("Hi", vec![sp(0, 2, bold)]);
        assert!(m.pressed(2..2).bold);
        m.toggle(2..2, Toggle::Bold);
        assert!(!m.pressed(2..2).bold);
        m.edited("Hi!");
        assert_eq!(m.spans, vec![sp(0, 2, bold)]);
    }

    #[test]
    fn moving_cursor_drops_pending() {
        let mut m = RichEditModel::new("Hi there", vec![]);
        m.toggle(3..3, Toggle::Bold);
        m.cursor_moved(3);
        assert!(m.pressed(3..3).bold);
        m.cursor_moved(1);
        assert!(!m.pressed(3..3).bold);
        m.edited("Hi xthere");
        assert_eq!(m.spans, vec![]);
    }

    #[test]
    fn pressed_shows_style_common_to_selection() {
        let m = RichEditModel::new("Sunday 10am", vec![sp(0, 6, |s| { s.bold = true; s.size = 125; }), sp(6, 11, bold)]);
        let p = m.pressed(0..11);
        assert!(p.bold);
        assert_eq!(p.size, 0);
        assert_eq!(m.pressed(1..3).size, 125);
        assert!(!m.pressed(5..11).italic);
        assert!(!RichEditModel::new("abc", vec![sp(0, 2, bold)]).pressed(0..3).bold);
    }

    #[test]
    fn color_size_and_clear_change_the_selection() {
        let mut m = RichEditModel::new("Pancake breakfast", vec![]);
        m.set_color(0..7, Some("#f5b94a".into()));
        m.set_size(0..7, 125);
        assert_eq!(m.spans, vec![sp(0, 7, |s| { gold(s); s.size = 125; })]);
        m.set_color(0..7, None);
        assert_eq!(m.spans, vec![sp(0, 7, |s| s.size = 125)]);
        m.toggle(0..17, Toggle::Bold);
        m.clear(3..17);
        assert_eq!(m.spans, vec![sp(0, 3, |s| { s.bold = true; s.size = 125; })]);
    }

    #[test]
    fn chips_list_formatted_ranges() {
        let m = RichEditModel::new("Sunday 10am\n\nPancake breakfast", vec![sp(0, 11, bold), sp(13, 30, |s| { gold(s); s.size = 125; })]);
        assert_eq!(m.chips(), vec![
            RangeChip { label: "Bold: ‘Sunday 10am’".into(), start: 0, end: 11 },
            RangeChip { label: "Gold, Large: ‘Pancake breakfast’".into(), start: 13, end: 30 },
        ]);
    }

    #[test]
    fn rich_edit_chip_x_clears_range() {
        let mut m = RichEditModel::new("Sunday 10am", vec![sp(0, 6, bold), sp(7, 11, gold)]);
        m.remove_chip(0);
        assert_eq!(m.spans, vec![sp(7, 11, gold)]);
        m.remove_chip(5); // out of range: nothing happens
        assert_eq!(m.spans, vec![sp(7, 11, gold)]);
    }

    #[test]
    fn pasted_text_takes_style_before_cursor() {
        let mut m = RichEditModel::new("abcd", vec![sp(0, 2, italic)]);
        m.cursor_moved(2);
        m.edited("ababccd");
        assert_eq!(m.spans, vec![sp(0, 5, italic)]);
    }

    #[test]
    fn edit_with_emoji_keeps_boundaries() {
        let mut m = RichEditModel::new("a🙏b", vec![sp(1, 5, bold)]);
        m.edited("axb");
        for s in &m.spans {
            assert!(m.text.is_char_boundary(s.start as usize) && m.text.is_char_boundary(s.end as usize) && s.end as usize <= m.text.len());
        }
        assert_eq!(m.spans, crate::theme::richtext::normalize(&m.text, m.spans.clone()));
        assert_eq!(m.spans, vec![]);
        // one emoji for another that shares its first three bytes
        let mut m = RichEditModel::new("a🙏b", vec![sp(1, 5, bold)]);
        m.edited("a🙂b");
        assert_eq!(m.text, "a🙂b");
        assert_eq!(m.spans, vec![]);
        m.toggle(1..5, Toggle::Bold);
        m.edited("a🙂🙏b");
        assert_eq!(m.spans, vec![sp(1, 9, bold)]);
    }

    #[test]
    fn rich_edit_b_disabled_in_bold_theme() {
        assert!(RichEditModel::bold_disabled(700));
        assert!(!RichEditModel::bold_disabled(600));
    }
}
