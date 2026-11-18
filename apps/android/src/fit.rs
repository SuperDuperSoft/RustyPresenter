//! Sizing the stage screen's words on the phone, as `web/stage-view.js` and the stage window do: each box's words
//! as big as its layout allows, made smaller step by step until they fit (down to a floor below the box's own
//! smallest size, so nothing is cut off). The phone has no way to measure text before drawing it, so widths are
//! worked out from the letters (narrow, wide, capitals, Chinese and Japanese characters), a little generously.
//!
//! A chord sheet (`lines` of [chord, words], the desktop's `stage::chords`) is cut into rows here: each word with
//! the chord over it stays together, and the screen draws each row as columns, the chord on top of its words, so a
//! chord always starts over its own letter whatever the font.

/// The smallest words, % of the screen's height (`ui::stage::FLOOR` on the computer).
pub const FLOOR: f32 = 1.2;
/// A line of words is this many times the size high.
pub const LINE: f32 = 1.22;

/// How wide a letter is, in parts of the size.
pub fn char_em(c: char) -> f32 {
    match c {
        ' ' | '\u{00a0}' => 0.28,
        'i' | 'j' | 'l' | '.' | ',' | '\'' | '!' | '|' | ':' | ';' | 'ı' => 0.3,
        'f' | 't' | 'r' | 'I' | '(' | ')' | '[' | ']' | '-' => 0.38,
        'm' | 'w' => 0.88,
        'M' | 'W' => 0.95,
        'A'..='Z' => 0.72,
        '0'..='9' => 0.62,
        c if is_wide(c) => 1.0,
        _ => 0.58,
    }
}

/// Chinese, Japanese and Korean characters (and other full-width ones) take a whole size each.
fn is_wide(c: char) -> bool {
    matches!(c as u32, 0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6)
}

/// How wide `s` is drawn at `size` pixels.
pub fn width(s: &str, size: f32, bold: bool) -> f32 {
    s.chars().map(char_em).sum::<f32>() * size * if bold { 1.1 } else { 1.0 }
}

/// How many lines `text` takes in `w` pixels at `size`, wrapped between words (a word longer than the line is
/// broken, as the screen does).
pub fn lines(text: &str, w: f32, size: f32, bold: bool) -> usize { wrap(text, w, size, bold).0 }

/// The lines, and whether a word had to be broken because it is longer than a line.
fn wrap(text: &str, w: f32, size: f32, bold: bool) -> (usize, bool) {
    let mut broken = false;
    let n = text.split('\n').map(|para| {
        let mut n = 1;
        let mut x = 0.0f32;
        for word in para.split_inclusive(' ') {
            let ww = width(word, size, bold);
            let bare = width(word.trim_end(), size, bold);
            if x > 0.0 && x + bare > w { n += 1; x = 0.0; }
            if bare > w && x == 0.0 { broken = true; n += (bare / w).ceil().max(1.0) as usize - 1; x = bare % w; continue }
            x += ww;
        }
        n
    }).sum();
    (n, broken)
}

/// The sizes tried, % of the screen's height: from `max` down to `min` in 0.4 steps, then on to the floor in
/// 0.2 steps (`fitAll` in `stage-view.js`).
pub fn steps(max: f32, min: f32) -> Vec<f32> {
    let (max, min) = (max.max(0.5), min.max(0.5).min(max.max(0.5)));
    let mut out = vec![];
    let mut s = max;
    out.push(s);
    while s > min + 1e-4 { s = (s - 0.4).max(min); out.push(s) }
    let floor = min.min(FLOOR);
    while s > floor + 1e-4 { s = (s - 0.2).max(floor); out.push(s) }
    out
}

/// The size in pixels for a box `w`×`h` pixels on a screen `screen_h` pixels high.
pub fn fit_text(text: &str, w: f32, h: f32, max: f32, min: f32, screen_h: f32, bold: bool) -> f32 {
    let all = steps(max, min);
    for pct in &all {
        let size = pct / 100.0 * screen_h;
        // A word is never broken while a smaller size keeps it whole ("10:42", not "10:4" over "2").
        let (n, broken) = wrap(text, w, size, bold);
        if n as f32 * size * LINE <= h + 0.5 && !broken { return size }
    }
    all.last().copied().unwrap_or(FLOOR) / 100.0 * screen_h
}

/// A chord (empty for none) and the words under it.
pub type Segment = (String, String);

/// One row of a chord sheet as drawn: its columns, and whether it has a chord row and a words row.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Row { pub cols: Vec<Segment>, pub chords: bool, pub words: bool }

pub fn has_chords(line: &[Segment]) -> bool { line.iter().any(|(c, _)| !c.is_empty()) }
pub fn has_words(line: &[Segment]) -> bool { line.iter().any(|(_, w)| !w.trim().is_empty()) }

/// `words` cut after each run of spaces that follows a word: "  Sometimes the " → ["  Sometimes ", "the "].
fn tokens(words: &str) -> Vec<&str> {
    let mut out = vec![];
    let (mut start, mut seen_word, mut in_space) = (0, false, false);
    for (i, ch) in words.char_indices() {
        if ch.is_whitespace() { in_space = true; continue }
        if seen_word && in_space { out.push(&words[start..i]); start = i; }
        seen_word = true;
        in_space = false;
    }
    if start < words.len() { out.push(&words[start..]) }
    out
}

/// The pieces of a line that stay together when it wraps: each word with the chord over it (`chords::items` on
/// the computer).
pub fn items(line: &[Segment]) -> Vec<Vec<Segment>> {
    let mut pieces: Vec<Segment> = vec![];
    for (chord, words) in line {
        let mut first = true;
        for token in tokens(words) {
            pieces.push((if first { chord.clone() } else { String::new() }, token.to_string()));
            first = false;
        }
        if first { pieces.push((chord.clone(), String::new())) }
    }
    let mut out: Vec<Vec<Segment>> = vec![];
    for p in pieces {
        let glue = out.last().and_then(|i| i.last()).is_some_and(|(_, w)| !w.is_empty() && !w.ends_with(char::is_whitespace));
        match out.last_mut() { Some(item) if glue => item.push(p), _ => out.push(vec![p]) }
    }
    out
}

/// A column's width: its words, or its chord with a little room after it when the chord is wider.
fn col_width((chord, words): &Segment, size: f32) -> f32 {
    let w = width(words, size, false);
    if chord.is_empty() { w } else { w.max(width(chord, size, true) + 0.4 * size) }
}

/// The sheet cut into rows no wider than `w` at `size` (a word wider than the row stays whole on its own row).
pub fn chord_rows(lines: &[Vec<Segment>], w: f32, size: f32) -> Vec<Row> {
    let mut out = vec![];
    for line in lines {
        let (chords, words) = (has_chords(line), has_words(line));
        let mut row = Row { cols: vec![], chords, words: words || !chords };
        let mut x = 0.0;
        for item in items(line) {
            let iw: f32 = item.iter().map(|s| col_width(s, size)).sum();
            if !row.cols.is_empty() && x + iw > w + 0.5 {
                out.push(std::mem::replace(&mut row, Row { cols: vec![], chords, words: words || !chords }));
                x = 0.0;
            }
            x += iw;
            row.cols.extend(item);
        }
        out.push(row);
    }
    // A row wrapped from a line with chords has a chord row only when chords fall on it.
    for row in &mut out {
        if row.chords && !row.cols.iter().any(|(c, _)| !c.is_empty()) { row.chords = false; row.words = true; }
    }
    // A chord wider than its words keeps a little room after it (the screen sizes each column by its own text).
    for row in &mut out {
        let n = row.cols.len();
        for (i, (c, words)) in row.cols.iter_mut().enumerate() {
            if !c.is_empty() && i + 1 < n && width(c, 1.0, true) >= width(words, 1.0, false) { c.push('\u{2002}') }
        }
    }
    out
}

/// How high the rows are at `size`.
pub fn rows_height(rows: &[Row], size: f32) -> f32 {
    rows.iter().map(|r| (u8::from(r.chords) + u8::from(r.words)) as f32 * size * LINE).sum()
}

/// The size and the rows for a chord sheet in a box `w`×`h` pixels on a screen `screen_h` pixels high.
pub fn fit_chords(lines: &[Vec<Segment>], w: f32, h: f32, max: f32, min: f32, screen_h: f32) -> (f32, Vec<Row>) {
    let all = steps(max, min);
    for pct in &all {
        let size = pct / 100.0 * screen_h;
        let rows = chord_rows(lines, w, size);
        let widest = rows.iter().map(|r| r.cols.iter().map(|s| col_width(s, size)).sum::<f32>()).fold(0.0, f32::max);
        if rows_height(&rows, size) <= h + 0.5 && widest <= w + 0.5 { return (size, rows) }
    }
    let size = all.last().copied().unwrap_or(FLOOR) / 100.0 * screen_h;
    (size, chord_rows(lines, w, size))
}

/// A box of the stage layout placed on a screen: where it goes and how big its words are, in pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub cell: crate::protocol::Cell,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub size: f32,
    /// The caption's size (0 without one).
    pub caption: f32,
    /// A chord sheet's rows (none for other boxes).
    pub rows: Vec<Row>,
}

/// The stage layout's boxes on a screen `w`×`h` pixels: each placed by its percentages, its words sized to fit
/// below its caption (with a little room at the sides, as the stage page's padding).
pub fn place(cells: &[crate::protocol::Cell], w: f32, h: f32) -> Vec<Placed> {
    cells.iter().map(|cell| {
        let (x, y, cw, ch) = (cell.x / 100.0 * w, cell.y / 100.0 * h, cell.w / 100.0 * w, cell.h / 100.0 * h);
        let caption = if cell.caption.as_deref().is_some_and(|c| !c.is_empty()) { (h * 0.022).clamp(10.0, 28.0) } else { 0.0 };
        let room_h = (ch - caption * LINE).max(1.0);
        let room_w = (cw * 0.97).max(1.0);
        let (size, rows) = match (cell.kind.as_str(), &cell.lines) {
            ("chords", Some(lines)) => fit_chords(lines, room_w, room_h, cell.max, cell.min, h),
            _ => (fit_text(&cell.text, room_w, room_h, cell.max, cell.min, h, cell.bold), vec![]),
        };
        Placed { cell: cell.clone(), x, y, w: cw, h: ch, size, caption, rows }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(c: &str, w: &str) -> Segment { (c.into(), w.into()) }

    #[test]
    fn the_sizes_go_down_as_the_stage_page_does() {
        let s = steps(6.0, 4.0);
        assert_eq!(s.first(), Some(&6.0));
        assert!((s[1] - 5.6).abs() < 1e-4);
        assert!(s.contains(&4.0));
        assert!((s.last().unwrap() - FLOOR).abs() < 1e-4, "on down to the floor");
        assert_eq!(steps(1.0, 1.0), vec![1.0], "already below the floor");
    }

    #[test]
    fn short_words_are_big_and_long_words_smaller() {
        let (w, h, screen) = (800.0, 300.0, 1000.0);
        let short = fit_text("Amen", w, h, 20.0, 5.0, screen, false);
        assert!((short - 200.0).abs() < 1e-3, "the biggest size: {short}");
        let long = fit_text(&"Amazing grace how sweet the sound that saved a wretch like me ".repeat(4), w, h, 20.0, 5.0, screen, false);
        assert!(long < 60.0 && long >= FLOOR / 100.0 * screen, "{long}");
        assert!(lines("Amazing grace how sweet", 10_000.0, 50.0, false) == 1);
        // A clock in a narrow box is made smaller rather than broken in two.
        let clock = fit_text("10:42", 130.0, 125.0, 10.0, 3.0, 900.0, true);
        assert!(width("10:42", clock, true) <= 130.0, "{clock}");
        assert_eq!(lines("one\ntwo", 10_000.0, 50.0, false), 2);
        assert!(lines("Amazing grace how sweet the sound", 300.0, 50.0, false) >= 3);
    }

    #[test]
    fn wide_letters_are_wider() {
        assert!(width("WWW", 10.0, false) > width("iii", 10.0, false) * 2.0);
        assert!((width("主耶穌", 10.0, false) - 30.0).abs() < 1e-3);
        assert!(width("Grace", 10.0, true) > width("Grace", 10.0, false));
    }

    #[test]
    fn words_keep_their_chords_as_on_the_computer() {
        // "[D]  Sometimes the [A/D]night" — the computer's own test.
        let line = vec![seg("D", "  Sometimes the "), seg("A/D", "night")];
        assert_eq!(items(&line), vec![vec![seg("D", "  Sometimes ")], vec![seg("", "the ")], vec![seg("A/D", "night")]]);
        // A chord inside a word keeps the word's parts together.
        let split = vec![seg("", "Ama"), seg("G", "zing "), seg("", "grace")];
        assert_eq!(items(&split), vec![vec![seg("", "Ama"), seg("G", "zing ")], vec![seg("", "grace")]]);
        assert_eq!(items(&[seg("G", "")]), vec![vec![seg("G", "")]]);
    }

    #[test]
    fn a_long_line_wraps_between_words() {
        let line = vec![seg("G", "Amazing "), seg("C", "grace how "), seg("G", "sweet the sound")];
        let one = chord_rows(std::slice::from_ref(&line), 10_000.0, 20.0);
        assert_eq!(one.len(), 1);
        assert!(one[0].chords && one[0].words);
        let narrow = chord_rows(std::slice::from_ref(&line), 200.0, 20.0);
        assert!(narrow.len() >= 2, "{narrow:?}");
        let all: String = narrow.iter().flat_map(|r| r.cols.iter().map(|(_, w)| w.clone())).collect();
        assert_eq!(all, "Amazing grace how sweet the sound", "no words lost");
        assert_eq!(narrow[0].cols[0].0.trim_end(), "G");
        let last = narrow.last().unwrap();
        assert_eq!(last.chords, last.cols.iter().any(|(c, _)| !c.is_empty()), "no empty chord row over words without chords");
        // A line of chords alone has only its chord row; an empty line is one blank words row.
        let alone = chord_rows(&[vec![seg("G", " "), seg("D", "")], vec![]], 1000.0, 20.0);
        assert!(alone[0].chords && !alone[0].words);
        assert!(!alone[1].chords && alone[1].words);
    }

    #[test]
    fn a_chord_wider_than_its_word_gets_room_after_it() {
        let rows = chord_rows(&[vec![seg("Gsus4", "a "), seg("G", "word")]], 1000.0, 20.0);
        assert_eq!(rows[0].cols[0].0, "Gsus4\u{2002}");
        assert_eq!(rows[0].cols[1].0, "G", "the last column needs none");
    }

    #[test]
    fn boxes_are_placed_on_the_screen() {
        let cells = vec![
            crate::protocol::Cell { x: 10.0, y: 20.0, w: 50.0, h: 50.0, kind: "text".into(), text: "Amazing grace".into(), max: 10.0, min: 3.0, caption: Some("Now".into()), ..Default::default() },
            crate::protocol::Cell { x: 0.0, y: 0.0, w: 100.0, h: 50.0, kind: "chords".into(), max: 8.0, min: 3.0, lines: Some(vec![vec![seg("G", "Amazing "), seg("C", "grace")]]), ..Default::default() },
        ];
        let p = place(&cells, 1000.0, 500.0);
        assert_eq!((p[0].x, p[0].y, p[0].w, p[0].h), (100.0, 100.0, 500.0, 250.0));
        assert!(p[0].caption > 0.0 && p[0].size <= 50.0);
        assert_eq!(p[1].caption, 0.0);
        assert_eq!(p[1].rows.len(), 1);
        assert!(p[1].rows[0].chords);
    }

    #[test]
    fn a_chord_sheet_is_made_to_fit() {
        let verse: Vec<Vec<Segment>> = (0..8).map(|_| vec![seg("G", "Amazing "), seg("C", "grace how "), seg("G", "sweet the sound")]).collect();
        let (size, rows) = fit_chords(&verse, 600.0, 400.0, 10.0, 3.0, 1000.0);
        assert!(size < 100.0, "{size}");
        assert!(rows_height(&rows, size) <= 400.5 || size <= FLOOR / 100.0 * 1000.0 + 1e-3);
        let (big, _) = fit_chords(&[vec![seg("G", "Amen")]], 600.0, 400.0, 10.0, 3.0, 1000.0);
        assert!((big - 100.0).abs() < 1e-3);
    }
}
