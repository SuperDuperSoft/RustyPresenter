//! Words SongSelect splits into syllables (Jake, November 18, 2026: "notice there's a hyphen in 'defense'. There
//! should not be."). SongSelect's ChordPro download breaks a word where a chord falls inside it, and writes the break
//! as space, hyphen, space: `my [Dbsus]de - [Db]fense`, `righteous - [F#m]ness`, and sometimes with no chord at the
//! break at all: `with You a - gain`. The room then showed "de - fense". Here the pieces are put back together, with
//! the chord kept where it falls (`de[Db]fense`), so the room shows "defense" and the stage screen shows the chord over
//! "fense" inside the whole word.
//!
//! A break is joined only when it looks like SongSelect's: a letter (with any chords after it), one or more spaces,
//! a plain hyphen `-`, one or more spaces, any chords, then a small letter. Real dashes stay: a capital or anything
//! that is not a letter after the dash ("Holy - Lord", "love - 1st"), an en or em dash ("–", "—"), a hyphen with no
//! spaces ("well-known"), or a dash at the start or end of a line.
use crate::model::Song;

/// Characters looked at for the `[` of a chord before a break.
const MAX_CHORD: usize = 64;

/// Where a SongSelect break is in `line`: the byte range of its spaces and hyphen, and whether a chord sits at it.
fn breaks(line: &str) -> Vec<(std::ops::Range<usize>, bool)> {
    let b = line.as_bytes();
    let mut out = vec![];
    for (i, _) in line.match_indices('-') {
        // Spaces before the hyphen…
        let mut s = i;
        while s > 0 && b[s - 1] == b' ' { s -= 1 }
        if s == i { continue }
        // …then the chords before them, then a letter.
        let mut j = s;
        let mut chord = false;
        while let Some(open) = chord_before(line, j) { j = open; chord = true }
        if !line[..j].chars().next_back().is_some_and(char::is_alphabetic) { continue }
        // Spaces after the hyphen…
        let mut e = i + 1;
        while e < b.len() && b[e] == b' ' { e += 1 }
        if e == i + 1 { continue }
        // …then the chords after them, then a small letter.
        let mut k = e;
        while let Some(close) = chord_after(line, k) { k = close; chord = true }
        if !line[k..].chars().next().is_some_and(char::is_lowercase) { continue }
        out.push((s..e, chord));
    }
    out
}

/// A chord that ends just before byte `at` (`…[Db]` + at): where its `[` is.
fn chord_before(line: &str, at: usize) -> Option<usize> {
    let head = &line[..at];
    if !head.ends_with(']') { return None }
    let open = head.rfind('[').filter(|&o| at - o <= MAX_CHORD)?;
    crate::slides::is_chord(head[open + 1..head.len() - 1].trim()).then_some(open)
}

/// A chord that starts at byte `at` (`[Db]…`): where it ends.
fn chord_after(line: &str, at: usize) -> Option<usize> {
    let tail = &line[at..];
    if !tail.starts_with('[') { return None }
    let close = tail.find(']').filter(|&c| c <= MAX_CHORD)?;
    crate::slides::is_chord(tail[1..close].trim()).then_some(at + close + 1)
}

/// One line with SongSelect's syllable breaks joined: `my [Dbsus]de - [Db]fense` → `my [Dbsus]de[Db]fense`,
/// `a - gain` → `again`. Everything else in the line stays as it is.
pub fn join_line(line: &str) -> String {
    let found = breaks(line);
    if found.is_empty() { return line.to_string() }
    let mut out = String::with_capacity(line.len());
    let mut at = 0;
    for (r, _) in found {
        out.push_str(&line[at..r.start]);
        at = r.end;
    }
    out.push_str(&line[at..]);
    out
}

/// Every line of `text` with its breaks joined.
pub fn join_text(text: &str) -> String {
    if !text.contains(" -") { return text.to_string() }
    text.split('\n').map(join_line).collect::<Vec<_>>().join("\n")
}

/// Whether a song still has the breaks only SongSelect's ChordPro writes: chords in its words and at least one break
/// with a chord at it (`de - [Db]fense`). Nobody types that by hand, so a song like this came from such a file.
pub fn has_songselect_breaks(lyrics: &str) -> bool {
    lyrics.lines().any(|l| breaks(l).iter().any(|(_, chord)| *chord))
}

/// The one-time tidy of a song already in the library: its breaks joined when it shows SongSelect's sign
/// (`has_songselect_breaks`). Gives each line changed, as it was and as it is now; nothing when nothing changed.
/// A song the church typed itself, or one without such a break, is never touched.
pub fn repair(song: &mut Song) -> Vec<(String, String)> {
    if !has_songselect_breaks(&song.lyrics) { return vec![] }
    let mut changed = vec![];
    let lines: Vec<String> = song.lyrics.split('\n').map(|l| {
        let joined = join_line(l);
        if joined != l { changed.push((l.trim_end_matches('\r').to_string(), joined.trim_end_matches('\r').to_string())) }
        joined
    }).collect();
    if !changed.is_empty() { song.lyrics = lines.join("\n") }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn songselect_breaks_are_joined_with_the_chord_kept_in_the_word() {
        assert_eq!(join_line("You're my [Db]Savior, my [Dbsus]de - [Db]fense"), "You're my [Db]Savior, my [Dbsus]de[Db]fense");
        assert_eq!(join_line("We will be with You a - [Db]gain"), "We will be with You a[Db]gain");
        assert_eq!(join_line("We will be with You a - gain"), "We will be with You again", "no chord at the break");
        // The other sample, "Sometimes By Step".
        assert_eq!(join_line("And on this (the) [G]road to righteous - [F#m]ness "), "And on this (the) [G]road to righteous[F#m]ness ");
        assert_eq!(join_line("And never be - [Em7]yond Your [G/A]reach "), "And never be[Em7]yond Your [G/A]reach ");
        assert_eq!(join_line("de[G] - fense"), "de[G]fense", "a chord before the break");
        assert_eq!(join_line("glo  -  ry and ho - nor"), "glory and honor", "two in a line, wider spaces");
    }

    #[test]
    fn real_dashes_stay() {
        for line in ["Holy - Lord of all", "Holy - [G]Lord", "love - 1st time", "Jesus – my King", "Jesus — my king", "a well-known song",
            "- a dash first", "a dash last -", "word -no space", "word- no space", "[G] - [C]", "(x2) - again", "Jesus [G]- and"] {
            assert_eq!(join_line(line), line);
        }
        assert_eq!(join_text("one - two\nThree - Four"), "onetwo\nThree - Four", "each line by itself");
    }

    #[test]
    fn the_library_tidy_only_touches_songs_from_songselect() {
        let mut from_file = Song { lyrics: "Verse 1\nYou're my [Db]Savior, my [Dbsus]de - [Db]fense\nWe will be with You a - gain".into(), ..Default::default() };
        let changed = repair(&mut from_file);
        assert_eq!(from_file.lyrics, "Verse 1\nYou're my [Db]Savior, my [Dbsus]de[Db]fense\nWe will be with You again");
        assert_eq!(changed.len(), 2);
        assert_eq!(changed[1], ("We will be with You a - gain".into(), "We will be with You again".into()));
        assert!(repair(&mut from_file).is_empty(), "once");
        // Typed by the church: a dash with no chord at it is theirs.
        let mut typed = Song { lyrics: "Verse 1\n[G]Lord, we come - and we bow".into(), ..Default::default() };
        assert!(repair(&mut typed).is_empty());
        assert_eq!(typed.lyrics, "Verse 1\n[G]Lord, we come - and we bow");
        let mut no_chords = Song { lyrics: "Verse 1\nWe will be with You a - gain".into(), ..Default::default() };
        assert!(repair(&mut no_chords).is_empty(), "no chords: not from a ChordPro file");
    }
}
