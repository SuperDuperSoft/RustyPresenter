//! A slide's chords as a chord sheet (Jake, November 18, 2026, comparing with SongSelect's chord sheet PDF: "We want
//! the chords to appear more like the PDF. That is much more readable."). Each line of words with chords becomes two
//! rows: the chords on top, each one starting right above the letter it falls on, and the words below without
//! brackets. Pure: no UI. The stage window and the phone's stage page draw what is worked out here.
//!
//! A line is split into segments, a chord and the words up to the next chord (`split_line`). Each segment is drawn
//! as a little column, the chord on top and its words below, as wide as the wider of the two (with a small gap
//! after a chord that is wider than its words), so a chord always starts over its own letter whatever the font, and
//! chords close together never run into each other: the words get a little room instead, as on a chord sheet.
//! Long lines wrap between words; a chord stays with the word (or the part of a word) under it (`items`).
use crate::slides::is_chord;

/// A chord (empty for none) and the words after it, up to the next chord.
pub type Segment = (String, String);
/// One line of the song as segments.
pub type ChordLine = Vec<Segment>;

/// A slide's chords as written (`[D]  Sometimes the [A/D]night`) → its lines as segments.
pub fn sheet(text: &str) -> Vec<ChordLine> { text.lines().map(split_line).collect() }

/// One line → segments. Words before the first chord are a segment with no chord; a line with no chords is one
/// such segment; an empty line has none. `[*Softly]` (a ChordPro note for the band) sits in the chord row like a
/// chord. Brackets that are not chords (`[x2]`) stay in the words, as does a `[` that is never closed. Spaces are
/// kept as written (they place the chords of a line with no words), except at the end of the line.
pub fn split_line(line: &str) -> ChordLine {
    let mut segs: ChordLine = vec![(String::new(), String::new())];
    let mut rest = line;
    while let Some(i) = rest.find('[') {
        segs.last_mut().unwrap().1.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        let close = tail.find(']').filter(|&e| !tail[..e].contains('['));
        match close {
            Some(e) if is_chord(tail[..e].trim()) => { segs.push((tail[..e].trim().to_string(), String::new())); rest = &tail[e + 1..]; }
            Some(e) if tail[..e].starts_with('*') => { segs.push((tail[1..e].trim().to_string(), String::new())); rest = &tail[e + 1..]; }
            Some(e) => { segs.last_mut().unwrap().1.push_str(&rest[i..i + e + 2]); rest = &tail[e + 1..]; }
            None => { segs.last_mut().unwrap().1.push('['); rest = tail; }
        }
    }
    segs.last_mut().unwrap().1.push_str(rest);
    if let Some(last) = segs.last_mut() { last.1.truncate(last.1.trim_end().len()) }
    if segs[0].1.is_empty() && (segs.len() > 1 || segs[0].0.is_empty()) { segs.remove(0); }
    segs
}

/// Whether a line has any chord (it gets a chord row).
pub fn has_chords(line: &[Segment]) -> bool { line.iter().any(|(c, _)| !c.is_empty()) }

/// Whether a line has any words (it gets a words row). A line of chords alone has only its chord row.
pub fn has_words(line: &[Segment]) -> bool { line.iter().any(|(_, w)| !w.trim().is_empty()) }

/// The pieces of a line that stay together on one row when it wraps: each word with the chord over it. A segment's
/// words are cut after each space (spaces before the first word stay with it); a chord that falls inside a word keeps
/// that word's parts together.
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
        // Glued to the piece before when that one stops inside a word.
        let glue = out.last().and_then(|i| i.last()).is_some_and(|(_, w)| !w.is_empty() && !w.ends_with(char::is_whitespace));
        match out.last_mut() { Some(item) if glue => item.push(p), _ => out.push(vec![p]) }
    }
    out
}

/// `words` cut after each run of spaces that follows a word: "  Sometimes the " → ["  Sometimes ", "the "]. Words
/// of only spaces stay one piece.
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

/// Sizes the layout needs, in pixels, for one font size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// The height of the chord row and of the words row.
    pub chord_h: f32,
    pub words_h: f32,
    /// Room after a chord that is wider than its words.
    pub gap: f32,
}

/// One piece of text placed in the sheet, in pixels from its top left.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed { pub x: f32, pub y: f32, pub text: String, pub chord: bool }

/// A chord sheet laid out: its pieces and its size.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Laid { pub pieces: Vec<Placed>, pub w: f32, pub h: f32 }

/// Lays `lines` out no wider than `max_w` when it can be (a word with its chord wider than that sticks out: the
/// caller makes the size smaller). `width(text, chord)` is how wide a text is drawn at this size (chords bold).
/// Rows are left-aligned with each other, as on a chord sheet; the caller places the whole sheet in its box.
pub fn lay_out(lines: &[ChordLine], max_w: f32, m: Metrics, width: impl Fn(&str, bool) -> f32) -> Laid {
    let mut out = Laid::default();
    let mut y = 0.0f32;
    for line in lines {
        let (chords, words) = (has_chords(line), has_words(line));
        let row_h = if chords { m.chord_h } else { 0.0 } + if words || !chords { m.words_h } else { 0.0 };
        let items = items(line);
        let n_items = items.len();
        let mut x = 0.0f32;
        let mut started = false;
        for (k, item) in items.into_iter().enumerate() {
            let last_item = k + 1 == n_items;
            let n = item.len();
            let ws: Vec<f32> = item.iter().enumerate().map(|(j, (c, w))| {
                let ww = width(w, false);
                if c.is_empty() { return ww }
                let cw = width(c, true) + if last_item && j + 1 == n { 0.0 } else { m.gap };
                ww.max(cw)
            }).collect();
            let item_w: f32 = ws.iter().sum();
            if started && x + item_w > max_w + 0.5 { y += row_h; x = 0.0; }
            started = true;
            for ((c, w), pw) in item.into_iter().zip(ws) {
                if !c.is_empty() { out.pieces.push(Placed { x, y, text: c, chord: true }) }
                if words && !w.trim().is_empty() { out.pieces.push(Placed { x, y: y + if chords { m.chord_h } else { 0.0 }, text: w, chord: false }) }
                x += pw;
            }
            out.w = out.w.max(x);
        }
        y += row_h;
    }
    out.h = y;
    out
}

/// The colour the chords are drawn in: a clear colour that is not the words' and reads well on the background
/// (gold on a dark screen, or blue when the words are gold; darker ones on a light screen).
pub fn chord_color(words: &str, background: &str) -> String {
    let dark = luminance(background) < 0.4;
    let choices: &[&str] = if dark { &["#f5c66b", "#6fd0ff", "#7ee2a8"] } else { &["#9a5b00", "#0b5cad", "#1f7a3f"] };
    let w = crate::render::parse_color(words);
    let far = |c: &&&str| {
        let k = crate::render::parse_color(c);
        (0..3).map(|i| (i32::from(k[i]) - i32::from(w[i])).abs()).sum::<i32>() > 120
    };
    choices.iter().find(far).unwrap_or(&choices[0]).to_string()
}

/// How light a colour is, 0 (black) to 1 (white), as the eye sees it.
fn luminance(c: &str) -> f32 {
    let [r, g, b, _] = crate::render::parse_color(c);
    let lin = |v: u8| { let v = f32::from(v) / 255.0; if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) } };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// The lines as plain text with bracketed chords again (for screens too old to draw a chord sheet).
pub fn plain(lines: &[ChordLine]) -> String {
    lines.iter().map(|l| l.iter().map(|(c, w)| if c.is_empty() { w.clone() } else { format!("[{c}]{w}") }).collect::<String>()).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(c: &str, w: &str) -> Segment { (c.into(), w.into()) }

    #[test]
    fn a_line_splits_into_chords_and_the_words_after_them() {
        assert_eq!(split_line("[D]  Sometimes the [A/D]night"), vec![seg("D", "  Sometimes the "), seg("A/D", "night")]);
        assert_eq!(split_line("You could [A7]touch it, but your heart would [F#m]break "),
            vec![seg("", "You could "), seg("A7", "touch it, but your heart would "), seg("F#m", "break")], "words first, then the chords");
        assert_eq!(split_line("[A]Oh,  [D]God, You are my [A/C#]God    [A]"),
            vec![seg("A", "Oh,  "), seg("D", "God, You are my "), seg("A/C#", "God    "), seg("A", "")], "spaces kept, a chord at the end");
    }

    #[test]
    fn a_line_of_chords_alone_keeps_its_spacing() {
        let l = split_line("[D]      [G/D]    [A/D]");
        assert_eq!(l, vec![seg("D", "      "), seg("G/D", "    "), seg("A/D", "")]);
        assert!(has_chords(&l) && !has_words(&l));
    }

    #[test]
    fn words_alone_annotations_and_other_brackets_stay_words() {
        assert_eq!(split_line("Sometimes the morning came too soon"), vec![seg("", "Sometimes the morning came too soon")]);
        assert_eq!(split_line(""), Vec::<Segment>::new());
        assert_eq!(split_line("(1st Ending)    And I will [G/B]follow You"), vec![seg("", "(1st Ending)    And I will "), seg("G/B", "follow You")]);
        assert_eq!(split_line("Sing it [x2] [G]now"), vec![seg("", "Sing it [x2] "), seg("G", "now")], "[x2] is not a chord");
        assert_eq!(split_line("an [unclosed [G]bracket"), vec![seg("", "an [unclosed "), seg("G", "bracket")]);
        assert_eq!(split_line("[*Softly][G]Amazing"), vec![seg("Softly", ""), seg("G", "Amazing")], "a note for the band sits with the chords");
        let words = split_line("And on this (the) road");
        assert!(!has_chords(&words) && has_words(&words));
    }

    #[test]
    fn wrapping_keeps_each_chord_with_its_word() {
        let l = split_line("[D]  Sometimes the [A/D]night is beau[G]tiful");
        let it = items(&l);
        assert_eq!(it, vec![
            vec![seg("D", "  Sometimes ")], vec![seg("", "the ")], vec![seg("A/D", "night ")], vec![seg("", "is ")],
            vec![seg("", "beau"), seg("G", "tiful")],
        ], "a chord inside a word keeps the word whole");
        assert_eq!(items(&split_line("[D]      [G/D]    [A/D]")), vec![vec![seg("D", "      ")], vec![seg("G/D", "    ")], vec![seg("A/D", "")]]);
    }

    /// Every character 10 px wide, chords 12 px: like a fixed font, so the numbers are easy to check.
    fn w(t: &str, chord: bool) -> f32 { t.chars().count() as f32 * if chord { 12.0 } else { 10.0 } }
    const M: Metrics = Metrics { chord_h: 20.0, words_h: 25.0, gap: 5.0 };

    #[test]
    fn chords_sit_over_their_letters_and_never_overlap() {
        let laid = lay_out(&[split_line("[D]  Sometimes the [A/D]night")], 10_000.0, M, w);
        let at = |t: &str| laid.pieces.iter().find(|p| p.text == t).map(|p| (p.x, p.y)).unwrap();
        assert_eq!(at("D"), (0.0, 0.0));
        assert_eq!(at("  Sometimes "), (0.0, 20.0), "the words under the chords");
        assert_eq!(at("A/D").0, at("night").0, "A/D over the n of night");
        assert_eq!(at("A/D").0, 160.0);
        assert_eq!(laid.h, 45.0);
        // Two chords on one short word: the word gets room so the chords don't run together.
        let laid = lay_out(&[split_line("[Asus][A]praise")], 10_000.0, M, w);
        let xs: Vec<f32> = laid.pieces.iter().filter(|p| p.chord).map(|p| p.x).collect();
        assert_eq!(xs, vec![0.0, 4.0 * 12.0 + 5.0], "the second chord starts after the first and a gap");
    }

    #[test]
    fn chords_alone_make_one_row_and_words_alone_no_chord_row() {
        let laid = lay_out(&[split_line("[D]      [G/D]    [A/D]"), split_line("Sometimes the day")], 10_000.0, M, w);
        assert_eq!(laid.h, 20.0 + 25.0, "a chord row, then a words row");
        let xs: Vec<f32> = laid.pieces.iter().filter(|p| p.chord).map(|p| p.x).collect();
        assert_eq!(xs, vec![0.0, 60.0, 101.0], "spaced as written (G/D is a little wider than its spaces)");
        let words: Vec<&Placed> = laid.pieces.iter().filter(|p| !p.chord).collect();
        assert_eq!(words.iter().map(|p| p.text.as_str()).collect::<String>(), "Sometimes the day", "no words drawn for the chords alone");
        assert!(words.iter().all(|p| p.y == 20.0), "the words line has no chord row");
    }

    #[test]
    fn a_long_line_wraps_between_words_with_its_chords() {
        let laid = lay_out(&[split_line("[G]Sometimes the [C]morning came too soon")], 150.0, M, w);
        assert!(laid.w <= 150.0, "{}", laid.w);
        assert!(laid.h > 45.0, "more than one row");
        let c = laid.pieces.iter().find(|p| p.text == "C").unwrap();
        let morning = laid.pieces.iter().find(|p| p.text == "morning ").unwrap();
        assert_eq!((c.x, c.y + 20.0), (morning.x, morning.y), "the chord moved down with its word");
    }

    #[test]
    fn chords_get_a_colour_of_their_own_that_reads_on_the_background() {
        assert_eq!(chord_color("#ffffff", "#07080a"), "#f5c66b");
        assert_eq!(chord_color("#f5c66b", "#07080a"), "#6fd0ff", "gold words: blue chords");
        assert_eq!(chord_color("#111111", "#ffffff"), "#9a5b00", "a light screen gets a darker colour");
    }

    #[test]
    fn the_sheet_can_be_written_back_with_brackets() {
        let t = "[D]  Sometimes the [A/D]night\nYou could [A7]touch it";
        assert_eq!(plain(&sheet(t)), t);
    }
}
