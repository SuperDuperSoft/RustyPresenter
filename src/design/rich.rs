//! 1.2: words that are not all alike (paragraphs of styled runs) and tables, as imported decks have them.
//!
//! A text box keeps its plain `text` (what the stage screen, the phone and the editor's text field use) and, when
//! its words differ in look, `paras`: one `Para` per line of `text`, each with runs. A run's unset values come
//! from the box's `TextStyle`, so changing the box's style still changes every run that has no look of its own.
use super::{HAlign, Outline, TextStyle, VAlign};
use serde::{Deserialize, Serialize};

fn is_zero_f(v: &f32) -> bool { *v == 0.0 }
fn is_zero_u8(v: &u8) -> bool { *v == 0 }
fn is_false(v: &bool) -> bool { !*v }

/// One paragraph (one line of the box's text; it wraps as it needs).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Para {
    pub runs: Vec<Run>,
    /// None: the box's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<HAlign>,
    /// Line spacing, times single spacing; None: single.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<f32>,
    /// Space above and below, in 1080p pixels.
    #[serde(skip_serializing_if = "is_zero_f")]
    pub before: f32,
    #[serde(skip_serializing_if = "is_zero_f")]
    pub after: f32,
    /// Indent level, 0–8.
    #[serde(skip_serializing_if = "is_zero_u8")]
    pub level: u8,
    /// The mark before the words ("•", "1.", "a)"); empty: none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub bullet: String,
    /// Laid out but not drawn (a paragraph that comes in on a later step).
    #[serde(skip_serializing_if = "is_false")]
    pub hidden: bool,
}

/// Words with one look. Unset values come from the box's style.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Run {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strike: Option<bool>,
}

impl Run {
    pub fn plain(text: impl Into<String>) -> Run { Run { text: text.into(), ..Run::default() } }

    /// The run's look: the box's style with this run's own values.
    pub fn style(&self, base: &TextStyle) -> TextStyle {
        let mut s = base.clone();
        if let Some(v) = &self.font { s.font = v.clone() }
        if let Some(v) = self.size { s.size = v }
        if let Some(v) = &self.color { s.color = v.clone() }
        if let Some(v) = self.bold { s.bold = v }
        if let Some(v) = self.italic { s.italic = v }
        if let Some(v) = self.underline { s.underline = v }
        if let Some(v) = self.strike { s.strike = v }
        s
    }

    /// The same look with other words.
    fn with_text(&self, text: String) -> Run { Run { text, ..self.clone() } }

    /// Values the box's style already has are dropped (they need not be the run's own).
    pub fn simplify(&mut self, base: &TextStyle) {
        if self.font.as_ref() == Some(&base.font) { self.font = None }
        if self.size == Some(base.size) { self.size = None }
        if self.color.as_ref().is_some_and(|c| c.eq_ignore_ascii_case(&base.color)) { self.color = None }
        if self.bold == Some(base.bold) { self.bold = None }
        if self.italic == Some(base.italic) { self.italic = None }
        if self.underline == Some(base.underline) { self.underline = None }
        if self.strike == Some(base.strike) { self.strike = None }
    }
}

impl Para {
    /// The paragraph's words, without its mark.
    pub fn words(&self) -> String { self.runs.iter().map(|r| r.text.as_str()).collect() }

    /// Its line in the box's plain text: the mark, a space, the words.
    pub fn line_text(&self) -> String {
        if self.bullet.is_empty() { self.words() } else { format!("{} {}", self.bullet, self.words()) }
    }

    /// Every run takes the box's look (the paragraph's own layout stays).
    fn plain_look(&self) -> bool { self.runs.iter().all(|r| r.with_text(String::new()) == Run::default()) }

    /// `words` in this paragraph, keeping the looks of what stayed the same at either end.
    fn retext(&self, words: &str) -> Para {
        let old: Vec<char> = self.words().chars().collect();
        let new: Vec<char> = words.chars().collect();
        let pre = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let suf = old[pre..].iter().rev().zip(new[pre..].iter().rev()).take_while(|(a, b)| a == b).count();
        let (cut_end, mid): (usize, String) = (old.len() - suf, new[pre..new.len() - suf].iter().collect());
        let mut runs = Vec::new();
        let mut at = 0;
        let mut put = false;
        for r in &self.runs {
            let n = r.text.chars().count();
            let (s, e) = (at, at + n);
            let mut t = String::new();
            for (k, c) in r.text.chars().enumerate() {
                let i = s + k;
                if i == pre && !put { t.push_str(&mid); put = true }
                if i < pre || i >= cut_end { t.push(c) }
            }
            at = e;
            if !t.is_empty() { runs.push(r.with_text(t)) }
        }
        // Typed at the very end: the last run goes on.
        if !put && !mid.is_empty() {
            match runs.last_mut() {
                Some(r) => r.text.push_str(&mid),
                None => runs.push(self.runs.last().cloned().unwrap_or_default().with_text(mid)),
            }
        }
        if runs.is_empty() && let Some(r) = self.runs.first() { runs.push(r.with_text(String::new())) }
        Para { runs, ..self.clone() }
    }
}

/// Some words have a look of their own (not only paragraph layout or marks).
pub fn mixed(paras: &[Para]) -> bool { paras.iter().any(|p| !p.plain_look()) }

/// The box's plain text for `paras`.
pub fn plain(paras: &[Para]) -> String { paras.iter().map(Para::line_text).collect::<Vec<_>>().join("\n") }

/// The box's paragraphs: its own, else each line of `text` in the box's look.
pub fn paragraphs(text: &str, paras: &[Para]) -> Vec<Para> {
    if !paras.is_empty() { return paras.to_vec() }
    text.split('\n').map(|l| Para { runs: vec![Run::plain(l)], ..Para::default() }).collect()
}

/// `paras` after the box's text was typed over with `new`: paragraphs whose line is the same stay as they were;
/// a changed one keeps its mark and the looks of its unchanged ends; new lines take the look of the one before.
/// Empty when nothing is left that differs from the box's own look.
pub fn retext(paras: &[Para], new: &str) -> Vec<Para> {
    if paras.is_empty() { return vec![] }
    let old: Vec<String> = paras.iter().map(Para::line_text).collect();
    let lines: Vec<&str> = new.split('\n').collect();
    let pre = old.iter().zip(&lines).take_while(|(a, b)| a.as_str() == **b).count();
    let suf = old[pre..].iter().rev().zip(lines[pre..].iter().rev()).take_while(|(a, b)| a.as_str() == **b).count();
    let (old_mid, new_mid) = (&paras[pre..paras.len() - suf], &lines[pre..lines.len() - suf]);
    let mut out: Vec<Para> = paras[..pre].to_vec();
    for (k, line) in new_mid.iter().enumerate() {
        let tpl = old_mid.get(k).or(old_mid.last()).or(out.last()).cloned().unwrap_or_default();
        let tpl = if k < old_mid.len() { tpl } else { Para { runs: tpl.runs.first().cloned().into_iter().collect(), before: 0.0, ..tpl } };
        let mark = format!("{} ", tpl.bullet);
        let p = match line.strip_prefix(mark.as_str()).filter(|_| !tpl.bullet.is_empty()) {
            Some(words) => tpl.retext(words),
            None => Para { bullet: String::new(), ..tpl.retext(line) },
        };
        out.push(p);
    }
    out.extend_from_slice(&paras[paras.len() - suf..]);
    if out.iter().all(|p| p.plain_look() && p.align.is_none() && p.line.is_none() && p.before == 0.0 && p.after == 0.0 && p.level == 0 && p.bullet.is_empty()) { return vec![] }
    out
}

/// After the box's style went from `before` to `after`: runs lose their own value of what changed, so the change
/// shows on every word.
pub fn restyle(paras: &mut [Para], before: &TextStyle, after: &TextStyle) {
    for r in paras.iter_mut().flat_map(|p| p.runs.iter_mut()) {
        if before.font != after.font { r.font = None }
        if before.size != after.size { r.size = None }
        if before.color != after.color { r.color = None }
        if before.bold != after.bold { r.bold = None }
        if before.italic != after.italic { r.italic = None }
        if before.underline != after.underline { r.underline = None }
        if before.strike != after.strike { r.strike = None }
    }
    if before.align != after.align { for p in paras.iter_mut() { p.align = None } }
    if before.line != after.line { for p in paras.iter_mut() { p.line = None } }
}

/// The paragraphs of words in look `from` (`paras`, else `text` line by line) with their runs' looks spelled out
/// against `to` instead; empty when nothing then differs from `to`.
pub fn rebase(paras: &[Para], text: &str, from: &TextStyle, to: &TextStyle) -> Vec<Para> {
    let out: Vec<Para> = paragraphs(text, paras).into_iter().map(|mut p| {
        for r in &mut p.runs {
            let s = r.style(from);
            *r = Run { text: std::mem::take(&mut r.text), font: Some(s.font), size: Some(s.size), color: Some(s.color), bold: Some(s.bold), italic: Some(s.italic), underline: Some(s.underline), strike: Some(s.strike) };
            r.simplify(to);
        }
        if p.align.is_none() && from.align != to.align { p.align = Some(from.align) }
        p
    }).collect();
    retext(&out, text)
}

/// The largest character boundary of `s` at or before byte `i`.
fn floor(s: &str, i: usize) -> usize { (0..=i.min(s.len())).rev().find(|&k| s.is_char_boundary(k)).unwrap_or(0) }

/// The words from byte `range.start` to byte `range.end` of the box's `text` take the look `f` gives them (`f`
/// is handed each word's present look and changes what it wants); the rest stay as they are. A mark before a
/// paragraph is not a word and keeps the box's look. Empty when nothing then differs from the box's own look.
pub fn style_range(text: &str, paras: &[Para], base: &TextStyle, range: std::ops::Range<usize>, f: &dyn Fn(&mut TextStyle)) -> Vec<Para> {
    let mut out = paragraphs(text, paras);
    let mut at = 0;
    for p in &mut out {
        let len = p.line_text().len();
        let mark = if p.bullet.is_empty() { 0 } else { p.bullet.len() + 1 };
        let (from, to) = (range.start.max(at + mark), range.end.min(at + len));
        if from < to {
            let (from, to) = (from - at - mark, to - at - mark);
            let mut runs: Vec<Run> = Vec::new();
            let mut push = |r: Run| match runs.last_mut() {
                // Neighbours with one look are one run.
                Some(last) if last.with_text(String::new()) == r.with_text(String::new()) => last.text.push_str(&r.text),
                _ => runs.push(r),
            };
            let mut pos = 0;
            for r in std::mem::take(&mut p.runs) {
                let (s, e) = (pos, pos + r.text.len());
                pos = e;
                let (a, b) = (floor(&r.text, from.clamp(s, e) - s), floor(&r.text, to.clamp(s, e) - s));
                if a > 0 { push(r.with_text(r.text[..a].to_string())) }
                if b > a {
                    let mut look = r.style(base);
                    f(&mut look);
                    let mut own = Run { text: r.text[a..b].to_string(), font: Some(look.font), size: Some(look.size), color: Some(look.color),
                        bold: Some(look.bold), italic: Some(look.italic), underline: Some(look.underline), strike: Some(look.strike) };
                    own.simplify(base);
                    push(own);
                }
                if b < r.text.len() { push(r.with_text(r.text[b..].to_string())) }
            }
            if runs.is_empty() { runs.push(Run::default()) }
            p.runs = runs;
        }
        at += len + 1;
    }
    retext(&out, text)
}

/// The look of the word at byte `at` of the box's `text` (the box's own where there is none).
pub fn look_at(text: &str, paras: &[Para], base: &TextStyle, at: usize) -> TextStyle {
    let mut start = 0;
    for p in paragraphs(text, paras) {
        let len = p.line_text().len();
        if at <= start + len {
            let mut pos = start + if p.bullet.is_empty() { 0 } else { p.bullet.len() + 1 };
            for r in &p.runs {
                pos += r.text.len();
                if at < pos { return r.style(base) }
            }
            return p.runs.last().map_or_else(|| base.clone(), |r| r.style(base));
        }
        start += len + 1;
    }
    base.clone()
}

/// Numbers in range: sizes, spacing, levels.
pub fn sanitise(paras: &mut Vec<Para>) {
    paras.truncate(500);
    for p in paras.iter_mut() {
        p.runs.truncate(200);
        for r in &mut p.runs {
            if r.text.contains('\n') { r.text = r.text.replace('\n', " ") }
            if let Some(s) = r.size { r.size = Some(if s.is_finite() { s.clamp(super::SIZE_MIN, super::SIZE_MAX) } else { 72.0 }) }
        }
        if let Some(l) = p.line { p.line = Some(if l.is_finite() { l.clamp(0.5, 5.0) } else { 1.0 }) }
        p.before = if p.before.is_finite() { p.before.clamp(0.0, 400.0) } else { 0.0 };
        p.after = if p.after.is_finite() { p.after.clamp(0.0, 400.0) } else { 0.0 };
        p.level = p.level.min(8);
    }
}

/// A table: columns and rows sized as shares of the object's box, and its cells row by row.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Table {
    /// Column widths and row heights, in any unit (each is its share of the sum).
    pub cols: Vec<f32>,
    pub rows: Vec<f32>,
    /// `rows.len() × cols.len()` cells, row by row.
    pub cells: Vec<Cell>,
    /// The cells' words' look (a cell's runs change it).
    pub style: TextStyle,
}

/// One cell of a table.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Cell {
    pub text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub paras: Vec<Para>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    /// Left, top, right, bottom.
    #[serde(skip_serializing_if = "no_borders")]
    pub borders: [Option<Outline>; 4],
    /// How many columns and rows it covers (0 counts as 1).
    #[serde(skip_serializing_if = "is_zero_u16")]
    pub span_cols: u16,
    #[serde(skip_serializing_if = "is_zero_u16")]
    pub span_rows: u16,
    /// Covered by a cell before it that spans it: not drawn.
    #[serde(skip_serializing_if = "is_false")]
    pub merged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valign: Option<VAlign>,
}

fn is_zero_u16(v: &u16) -> bool { *v == 0 }
fn no_borders(b: &[Option<Outline>; 4]) -> bool { b.iter().all(Option::is_none) }

/// Most rows and columns a table keeps.
pub const TABLE_MAX: usize = 60;

impl Table {
    /// The cell at row `r`, column `c`.
    pub fn cell(&self, r: usize, c: usize) -> Option<&Cell> { self.cells.get(r * self.cols.len() + c) }

    /// The words, a row per line, cells split by tabs (the editor's text field).
    pub fn text(&self) -> String {
        let n = self.cols.len().max(1);
        self.cells.chunks(n).map(|row| row.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join("\t")).collect::<Vec<_>>().join("\n")
    }

    /// Typed-over words (as `text` gives them) back into the cells; cells keep their look, extra words are dropped.
    pub fn set_text(&mut self, t: &str) {
        let n = self.cols.len().max(1);
        let lines: Vec<&str> = t.split('\n').collect();
        for (i, cell) in self.cells.iter_mut().enumerate() {
            let words = lines.get(i / n).and_then(|l| l.split('\t').nth(i % n)).unwrap_or("").to_string();
            if words != cell.text {
                cell.paras = retext(&cell.paras, &words);
                cell.text = words;
            }
        }
    }

    /// The words for the stage screen: cells split by "  ", a row per line.
    pub fn words(&self) -> String {
        let n = self.cols.len().max(1);
        self.cells.chunks(n).map(|row| row.iter().filter(|c| !c.merged).map(|c| c.text.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("  "))
            .filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
    }

    /// A drawable table: sizes positive, as many cells as places, at most `TABLE_MAX` each way.
    pub fn sanitise(&mut self) {
        let fix = |v: &mut Vec<f32>| { v.truncate(TABLE_MAX); for x in v.iter_mut() { if !(x.is_finite() && *x > 0.0) { *x = 1.0 } } };
        fix(&mut self.cols);
        fix(&mut self.rows);
        if self.cols.is_empty() { self.cols.push(1.0) }
        if self.rows.is_empty() { self.rows.push(1.0) }
        self.cells.resize(self.cols.len() * self.rows.len(), Cell::default());
        self.style.size = if self.style.size.is_finite() { self.style.size.clamp(super::SIZE_MIN, super::SIZE_MAX) } else { 36.0 };
        for c in &mut self.cells {
            sanitise(&mut c.paras);
            for b in c.borders.iter_mut().flatten() { b.width = if b.width.is_finite() { b.width.clamp(0.0, 100.0) } else { 1.0 } }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(t: &str, bold: bool) -> Run { Run { text: t.into(), bold: bold.then_some(true), ..Run::default() } }

    fn box_of(paras: Vec<Para>) -> Vec<Para> { paras }

    #[test]
    fn a_run_takes_the_box_look_where_it_has_none_of_its_own() {
        let base = TextStyle { size: 40.0, color: "#ffffff".into(), ..TextStyle::default() };
        let r = Run { text: "x".into(), size: Some(60.0), underline: Some(true), ..Run::default() };
        let s = r.style(&base);
        assert_eq!((s.size, s.color.as_str(), s.underline, s.strike), (60.0, "#ffffff", true, false));
    }

    #[test]
    fn plain_text_has_a_line_per_paragraph_with_its_mark() {
        let ps = vec![Para { runs: vec![run("Love ", true), run("one another", false)], bullet: "•".into(), ..Para::default() }, Para { runs: vec![run("John 13", false)], ..Para::default() }];
        assert_eq!(plain(&ps), "• Love one another\nJohn 13");
        assert_eq!(paragraphs("a\nb", &[]).len(), 2);
    }

    #[test]
    fn typing_keeps_the_looks_of_what_stayed() {
        let ps = box_of(vec![
            Para { runs: vec![run("Love ", true), run("one another", false)], bullet: "•".into(), ..Para::default() },
            Para { runs: vec![run("John 13", false)], align: Some(HAlign::Right), ..Para::default() },
        ]);
        // A word changed in the middle of the plain run.
        let out = retext(&ps, "• Love each other\nJohn 13");
        assert_eq!(out[0].runs, vec![run("Love ", true), run("each other", false)]);
        assert_eq!(out[0].bullet, "•");
        assert_eq!(out[1], ps[1]);
        // A line added: it looks like the one before.
        let out = retext(&ps, "• Love one another\n• As I loved you\nJohn 13");
        assert_eq!(out.len(), 3);
        assert_eq!((out[1].bullet.as_str(), out[1].words().as_str()), ("•", "As I loved you"));
        assert_eq!(out[2], ps[1]);
        // Typing into the bold word keeps it bold.
        let out = retext(&ps, "• Lovely one another\nJohn 13");
        assert_eq!(out[0].runs[0], run("Lovely ", true));
        // The mark taken away.
        assert_eq!(retext(&ps, "Love one another\nJohn 13")[0].bullet, "");
        // Nothing special left: no paragraphs.
        assert!(retext(&[Para { runs: vec![run("a", false)], ..Para::default() }], "b").is_empty());
    }

    #[test]
    fn changing_the_box_style_clears_the_runs_own_value() {
        let mut ps = vec![Para { runs: vec![Run { text: "a".into(), color: Some("#ff0000".into()), bold: Some(true), ..Run::default() }], ..Para::default() }];
        let before = TextStyle::default();
        let after = TextStyle { color: "#00ff00".into(), ..before.clone() };
        restyle(&mut ps, &before, &after);
        assert_eq!((ps[0].runs[0].color.clone(), ps[0].runs[0].bold), (None, Some(true)));
    }

    #[test]
    fn a_table_edits_as_tab_separated_lines_and_old_files_load() {
        let mut t = Table { cols: vec![1.0, 2.0], rows: vec![1.0], cells: vec![Cell { text: "Time".into(), ..Cell::default() }, Cell { text: "Event".into(), fill: Some("#ff0000".into()), ..Cell::default() }], ..Table::default() };
        assert_eq!(t.text(), "Time\tEvent");
        t.set_text("9am\tPrayer");
        assert_eq!((t.cells[0].text.as_str(), t.cells[1].text.as_str(), t.cells[1].fill.as_deref()), ("9am", "Prayer", Some("#ff0000")));
        assert_eq!(t.words(), "9am  Prayer");
        let json = serde_json::to_value(&t).unwrap();
        assert_eq!(serde_json::from_value::<Table>(json).unwrap(), t);
        let mut bad = Table { cols: vec![f32::NAN], rows: vec![], ..Table::default() };
        bad.sanitise();
        assert_eq!((bad.cols.as_slice(), bad.rows.len(), bad.cells.len()), (&[1.0][..], 1, 1));
    }
}
