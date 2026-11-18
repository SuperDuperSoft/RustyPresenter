//! Chords moved up or down for the stage screen (Jake, November 17, 2026: "Make the chords transposable as well.").
//! A song keeps its chords as written (`Song::lyrics`) and its key (`Song::key`); `Song::transpose` says how many
//! semitones the band plays it higher or lower. Only what the stage screen shows changes: each chord's root and bass
//! note move, the rest of the chord stays (`[F#m7/C#]` +2 → `[G#m7/D#]`). Sharps or flats are chosen from the key
//! the song ends up in, as musicians write it: F, B♭, E♭, A♭ and D♭ (and the minor keys that use flats) are written
//! with flats, every other key with sharps, so the key of B♭ never shows an A♯. Anything in brackets that is not a
//! chord (`[x2]`) is left alone. The audience screen never shows chords.
use crate::{model::Song, slides::is_chord};

/// The furthest a song can be moved either way, in semitones.
pub const MAX: i8 = 11;

const SHARPS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const FLATS: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];

/// A song's key: its home note (C = 0 … B = 11) and whether it is minor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key { pub root: u8, pub minor: bool }

/// A note name at the start of `s` ("C", "F#", "Bb", "E♭"): its place (C = 0 … B = 11) and the text after it.
fn note(s: &str) -> Option<(u8, &str)> {
    let base = match s.chars().next()? { 'C' => 0, 'D' => 2, 'E' => 4, 'F' => 5, 'G' => 7, 'A' => 9, 'B' => 11, _ => return None };
    let rest = &s[1..];
    for (mark, step) in [("#", 1), ("♯", 1), ("b", 11), ("♭", 11)] {
        if let Some(r) = rest.strip_prefix(mark) { return Some(((base + step) % 12, r)) }
    }
    Some((base, rest))
}

impl Key {
    /// A key as people write it: "G", "Bb", "F#m", "E minor", "Ebmaj", "A min". None when it does not start with a
    /// note name.
    pub fn parse(s: &str) -> Option<Key> {
        let (root, rest) = note(s.trim())?;
        let rest = rest.trim().to_lowercase();
        let minor = rest == "-" || (rest.starts_with('m') && !rest.starts_with("maj"));
        let known = rest.is_empty() || minor || rest.starts_with("maj");
        known.then_some(Key { root, minor })
    }

    /// The key a chord stands for, read from its root ("Em7" → E minor, "Csus4" → C major).
    fn of_chord(chord: &str) -> Option<Key> {
        let main = chord.split('/').next()?;
        let (root, rest) = note(main)?;
        Some(Key { root, minor: rest.starts_with('m') && !rest.starts_with("maj") })
    }

    /// The same key `steps` semitones higher (lower when negative).
    pub fn moved(self, steps: i8) -> Key { Key { root: (self.root + shift(steps)) % 12, ..self } }

    /// Whether music in this key is written with flats: F, B♭, E♭, A♭, D♭ major; D, G, C, F, B♭, E♭ minor.
    pub fn uses_flats(self) -> bool {
        if self.minor { matches!(self.root, 2 | 7 | 0 | 5 | 10 | 3) } else { matches!(self.root, 5 | 10 | 3 | 8 | 1) }
    }

    /// The key's name as the app writes it: "Bb", "F#m".
    pub fn name(self) -> String {
        let names = if self.uses_flats() { &FLATS } else { &SHARPS };
        format!("{}{}", names[self.root as usize], if self.minor { "m" } else { "" })
    }
}

/// `steps` as a move upwards within one octave (0 … 11).
pub fn shift(steps: i8) -> u8 { (i16::from(steps)).rem_euclid(12) as u8 }

/// A transpose amount kept to the range the app offers.
pub fn clamp(steps: i8) -> i8 { steps.clamp(-MAX, MAX) }

/// One chord moved `steps` semitones, written with flats or sharps. None when `chord` is not a chord.
pub fn chord(chord: &str, steps: i8, flats: bool) -> Option<String> {
    if !is_chord(chord) { return None }
    let names = if flats { &FLATS } else { &SHARPS };
    let up = |n: u8| names[((n + shift(steps)) % 12) as usize];
    let (main, bass) = match chord.split_once('/') { Some((m, b)) => (m, Some(b)), None => (chord, None) };
    let (root, quality) = note(main)?;
    let mut out = format!("{}{quality}", up(root));
    if let Some(b) = bass {
        let (n, rest) = note(b)?;
        out.push('/');
        out.push_str(up(n));
        out.push_str(rest);
    }
    Some(out)
}

/// Every `[chord]` in `text` moved `steps` semitones; other brackets and all the words stay as they are.
pub fn text(text: &str, steps: i8, flats: bool) -> String {
    if shift(steps) == 0 { return text.to_string() }
    let mut out = String::with_capacity(text.len() + 8);
    let mut rest = text;
    while let Some(i) = rest.find('[') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        match tail.find([']', '[', '\n']).filter(|&e| tail[e..].starts_with(']')) {
            Some(e) => {
                let inner = &tail[..e];
                out.push('[');
                out.push_str(&chord(inner, steps, flats).unwrap_or_else(|| inner.to_string()));
                out.push(']');
                rest = &tail[e + 1..];
            }
            None => { out.push('['); rest = tail; }
        }
    }
    out.push_str(rest);
    out
}

/// The first chord in `lyrics`, if any.
fn first_chord(lyrics: &str) -> Option<&str> {
    let mut rest = lyrics;
    while let Some(i) = rest.find('[') {
        let tail = &rest[i + 1..];
        let e = tail.find(']')?;
        if is_chord(&tail[..e]) { return Some(&tail[..e]) }
        rest = &tail[e + 1..];
    }
    None
}

/// The key the song is written in: its own, else the one its first chord suggests.
pub fn written_key(song: &Song) -> Option<Key> {
    Key::parse(&song.key).or_else(|| first_chord(&song.lyrics).and_then(Key::of_chord))
}

/// The key the band plays in now (the written key moved by `transpose`).
pub fn playing_key(song: &Song) -> Option<Key> { written_key(song).map(|k| k.moved(song.transpose)) }

/// `chords` (a slide's words with their chords) as the stage screen shows them for `song`.
pub fn for_stage(song: &Song, chords: &str) -> String {
    if shift(song.transpose) == 0 { return chords.to_string() }
    let flats = playing_key(song).is_some_and(Key::uses_flats);
    text(chords, song.transpose, flats)
}

/// "+2", "−3" or "0", as the key controls show an amount.
pub fn amount(steps: i8) -> String {
    match steps { 0 => "0".into(), s if s > 0 => format!("+{s}"), s => format!("−{}", -i16::from(s)) }
}

/// What the key controls say: "G → A, +2" when the key is known and moved, "G" when it is not moved, "+2" when
/// the key is not known, empty for a song not moved with no key.
pub fn describe(song: &Song) -> String {
    match (written_key(song), song.transpose) {
        (Some(k), 0) => k.name(),
        (Some(k), t) => crate::tr!("{from} → {to}, {amount}", from = k.name(), to = k.moved(t).name(), amount = amount(t)),
        (None, 0) => String::new(),
        (None, t) => amount(t),
    }
}

/// The capo that lets a guitarist play the written shapes in the key the song is played in: "Capo 2 to play G
/// shapes". Only for a song moved up 1 to 7 semitones (in effect); empty otherwise.
pub fn capo(song: &Song) -> String {
    let fret = shift(song.transpose);
    if !(1..=7).contains(&fret) { return String::new() }
    match written_key(song) {
        Some(k) => crate::tr!("Capo {fret} to play {key} shapes", fret = fret, key = k.name()),
        None => crate::tr!("Capo {fret} to play the chords as written", fret = fret),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_moves_and_is_spelled_as_musicians_write_it() {
        let majors = ["C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
        let minors = ["Cm", "C#m", "Dm", "Ebm", "Em", "Fm", "F#m", "Gm", "G#m", "Am", "Bbm", "Bm"];
        for (i, name) in majors.iter().enumerate() {
            let k = Key::parse(name).unwrap();
            assert_eq!((k.root as usize, k.minor), (i, false), "{name}");
            assert_eq!(k.name(), *name, "{name} is written as itself");
            for steps in -11..=11i8 {
                assert_eq!(k.moved(steps).name(), majors[(i as i32 + i32::from(steps)).rem_euclid(12) as usize], "{name} {steps}");
            }
        }
        for (i, name) in minors.iter().enumerate() {
            let k = Key::parse(name).unwrap();
            assert_eq!((k.root as usize, k.minor), (i, true), "{name}");
            assert_eq!(k.name(), *name);
        }
        // Other ways of writing a key.
        assert_eq!(Key::parse("A#"), Key::parse("Bb"));
        assert_eq!(Key::parse("E minor"), Some(Key { root: 4, minor: true }));
        assert_eq!(Key::parse("E♭"), Some(Key { root: 3, minor: false }));
        assert_eq!(Key::parse("Cmaj"), Some(Key { root: 0, minor: false }));
        assert_eq!(Key::parse("H"), None);
        assert_eq!(Key::parse(""), None);
        assert_eq!(Key::parse("Gsus"), None, "not a key");
    }

    #[test]
    fn chords_keep_their_quality_and_follow_the_new_key() {
        assert_eq!(chord("F#m7/C#", 2, false).unwrap(), "G#m7/D#");
        assert_eq!(chord("G", 3, true).unwrap(), "Bb", "into B flat: no A#");
        assert_eq!(chord("G", 3, false).unwrap(), "A#");
        assert_eq!(chord("Bbmaj7", 2, false).unwrap(), "Cmaj7");
        assert_eq!(chord("Dsus4", -2, false).unwrap(), "Csus4");
        assert_eq!(chord("Cadd9", 1, true).unwrap(), "Dbadd9");
        assert_eq!(chord("Bdim", 1, false).unwrap(), "Cdim");
        assert_eq!(chord("E7", 12 - 1, false).unwrap(), "D#7");
        assert_eq!(chord("Am/G", -2, false).unwrap(), "Gm/F");
        assert_eq!(chord("C/E", 5, true).unwrap(), "F/A");
        assert_eq!(chord("Cb", 1, false).unwrap(), "C", "C flat is B");
        for not in ["x2", "Chorus", "*Slowly", "H7", "", "repeat"] { assert_eq!(chord(not, 2, false), None, "{not}") }
    }

    #[test]
    fn text_moves_only_the_chords() {
        assert_eq!(text("[G]Amazing [C]grace [x2]\n[D/F#]how sweet", 2, false), "[A]Amazing [D]grace [x2]\n[E/G#]how sweet");
        assert_eq!(text("[G]Amazing", 0, false), "[G]Amazing");
        assert_eq!(text("[G]Amazing", 12, false), "[G]Amazing", "a whole octave is the same chords");
        assert_eq!(text("open [bracket and [G]chord", 2, false), "open [bracket and [A]chord");
        assert_eq!(text("ends with [", 1, false), "ends with [");
    }

    fn song(key: &str, transpose: i8, lyrics: &str) -> Song { Song { key: key.into(), transpose, lyrics: lyrics.into(), ..Default::default() } }

    #[test]
    fn the_stage_spells_from_the_key_played_in() {
        // G up 3 is B flat: flats.
        assert_eq!(for_stage(&song("G", 3, ""), "[G]a [D]b [Em]c"), "[Bb]a [F]b [Gm]c");
        // G up 2 is A: sharps.
        assert_eq!(for_stage(&song("G", 2, ""), "[G]a [D/F#]b"), "[A]a [E/G#]b");
        // No key: the first chord says it (D minor down 2 is C minor, written with flats).
        assert_eq!(for_stage(&song("", -2, "[Dm]x [A7]y"), "[Dm]x [A7]y"), "[Cm]x [G7]y");
        assert_eq!(for_stage(&song("", 0, ""), "[G]a"), "[G]a");
    }

    #[test]
    fn controls_say_where_the_song_is() {
        assert_eq!(describe(&song("G", 2, "")), "G → A, +2");
        assert_eq!(describe(&song("G", -1, "")), "G → F#, −1");
        assert_eq!(describe(&song("G", 0, "")), "G");
        assert_eq!(describe(&song("", 3, "plain words")), "+3");
        assert_eq!(describe(&song("", 0, "plain words")), "");
        assert_eq!(capo(&song("G", 2, "")), "Capo 2 to play G shapes");
        assert_eq!(capo(&song("G", -10, "")), "Capo 2 to play G shapes", "down 10 sounds as up 2");
        assert_eq!(capo(&song("G", -2, "")), "", "capo 10 is no help");
        assert_eq!(capo(&song("", 1, "")), "Capo 1 to play the chords as written");
        assert_eq!((clamp(40), clamp(-40)), (11, -11));
    }
}
