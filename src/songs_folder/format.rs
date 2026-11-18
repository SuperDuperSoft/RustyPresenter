//! One song as a ChordPro text file (`.chordpro`, UTF-8), the way the songs folder keeps it. Notepad can open it,
//! the chords stay in the words (`[G]Amazing`), and other worship programs read it. The song's details are ChordPro
//! directives: `{title}`, `{artist}`, one `{composer}` per writer, `{year}`, `{copyright}`, `{ccli}`, `{key}` and
//! `{transpose}`. What only RustyPresenter uses is in `{x_rustypresenter_…}` directives, which ChordPro lets other
//! programs skip: the publisher, the order, the lines per slide, the text size and the song's id.
//!
//! Each section name in the words ("Verse 1", "[Chorus]") becomes `{start_of_verse: Verse 1}` … `{end_of_verse}`
//! (`chorus` for a chorus or refrain, `bridge` for a bridge, `verse` for every other part, so other programs read
//! it); everything else in the words is written as it is. A song written and read back is the same song: a line
//! another program would take for a ChordPro directive or comment is kept in `{x_rustypresenter_line: "…"}`, and a
//! detail that cannot sit on one line as it is (a line break in the copyright, a `}`) is written tidied for other
//! programs, with the exact text beside it in `{x_rustypresenter_exact_<detail>: "…"}`.
use crate::model::Song;
use uuid::Uuid;

/// The file ending the songs folder writes.
pub const EXT: &str = "chordpro";
/// What starts every directive only RustyPresenter reads.
const X: &str = "x_rustypresenter_";

/// The first line of every file, for whoever opens it.
const HEADER: &str = "# A RustyPresenter song (ChordPro). The {x_rustypresenter_…} lines are for RustyPresenter; other programs skip them.";

/// `{name: value}`, `{name value}` or `{name}` → (lowercase name, value). None when `line` is not one.
fn directive(line: &str) -> Option<(String, String)> {
    let inner = line.strip_prefix('{')?.strip_suffix('}')?;
    let split = inner.find(|c: char| c == ':' || c.is_whitespace()).unwrap_or(inner.len());
    let name = inner[..split].to_ascii_lowercase();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')) { return None }
    let value = inner[split..].trim_start();
    Some((name, value.strip_prefix(':').unwrap_or(value).trim().to_string()))
}

/// The directives this file format gives a meaning. Any other `{…}` line in the words (a slide label such as
/// `{Instrumental}`) is words.
fn is_directive(name: &str) -> bool {
    matches!(name, "title" | "t" | "subtitle" | "st" | "artist" | "composer" | "lyricist" | "copyright" | "ccli" | "year" | "key" | "k"
        | "transpose" | "chorus" | "sov" | "eov" | "soc" | "eoc" | "sob" | "eob" | "new_song" | "ns" | "comment" | "c" | "meta")
        || name.starts_with("start_of_") || name.starts_with("end_of_") || name.starts_with(X)
}

/// A detail as one tidy line: line breaks become spaces, braces become brackets, no space around it.
fn tidy(value: &str) -> String {
    value.split(['\r', '\n']).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ").replace('{', "(").replace('}', ")").trim().to_string()
}

fn json(s: &str) -> String { serde_json::to_string(s).unwrap_or_default() }

/// `{name: value}` for a detail, with its exact text beside it when the tidy line is not exactly it.
fn detail(out: &mut Vec<String>, name: &str, exact_name: &str, value: &str) {
    let line = tidy(value);
    out.push(format!("{{{name}: {line}}}"));
    if line != value { out.push(format!("{{{X}exact_{exact_name}: {}}}", json(value))) }
}

/// The ChordPro kind a section name is written under.
fn kind_of(label: &str) -> &'static str {
    let head = label.split_whitespace().next().unwrap_or("").trim_matches(['[', ']', ':']).to_lowercase();
    match head.as_str() { "chorus" | "refrain" => "chorus", "bridge" => "bridge", _ => "verse" }
}

/// A line of the words, kept from being read as a directive or a comment.
fn words_line(line: &str) -> String {
    let t = line.trim();
    let taken = t.starts_with('#') || directive(t).is_some_and(|(n, _)| is_directive(&n));
    if taken { format!("{{{X}line: {}}}", json(line)) } else { line.to_string() }
}

/// The song as the file's text.
pub fn write(song: &Song) -> String {
    let mut out = vec![HEADER.to_string()];
    detail(&mut out, "title", "title", &song.title);
    if !song.artist.is_empty() { detail(&mut out, "artist", "artist", &song.artist) }
    if !song.writers.is_empty() {
        let parts: Vec<&str> = song.writers.split(" | ").collect();
        if parts.iter().all(|p| !p.is_empty() && tidy(p) == *p) {
            for p in parts { out.push(format!("{{composer: {p}}}")) }
        } else {
            detail(&mut out, "composer", "writers", &song.writers);
        }
    }
    if !song.year.is_empty() { detail(&mut out, "year", "year", &song.year) }
    if !song.copyright.is_empty() { detail(&mut out, "copyright", "copyright", &song.copyright) }
    if !song.ccli.is_empty() { detail(&mut out, "ccli", "ccli", &song.ccli) }
    if !song.key.is_empty() { detail(&mut out, "key", "key", &song.key) }
    if song.transpose != 0 { out.push(format!("{{transpose: {}}}", song.transpose)) }
    if !song.publisher.is_empty() { detail(&mut out, &format!("{X}publisher"), "publisher", &song.publisher) }
    if !song.arrangement.is_empty() {
        let plain = song.arrangement.iter().all(|a| !a.is_empty() && !a.contains(',') && tidy(a) == *a) && !song.arrangement[0].starts_with('[');
        let order = if plain { song.arrangement.join(", ") } else { serde_json::to_string(&song.arrangement).unwrap_or_default() };
        out.push(format!("{{{X}order: {order}}}"));
    }
    if song.lines_per_slide != 0 { out.push(format!("{{{X}lines_per_slide: {}}}", song.lines_per_slide)) }
    if song.text_size != 0 { out.push(format!("{{{X}text_size: {}}}", song.text_size)) }
    out.push(format!("{{{X}id: {}}}", song.id));
    out.push(String::new());

    // The words: each section name opens a section, closed before the blank lines that end it.
    let lyrics = song.lyrics.replace("\r\n", "\n");
    let mut open: Option<&'static str> = None;
    let close = |out: &mut Vec<String>, open: &mut Option<&'static str>| {
        if let Some(kind) = open.take() {
            let blanks = out.iter().rev().take_while(|l| l.is_empty()).count();
            out.insert(out.len() - blanks, format!("{{end_of_{kind}}}"));
        }
    };
    for line in lyrics.split('\n') {
        let label = crate::slides::section_label(line);
        let plain = line == line.trim() && !line.contains(['{', '}']) && !line.starts_with('#');
        match label {
            Some(l) if plain => {
                close(&mut out, &mut open);
                let kind = kind_of(&l);
                out.push(format!("{{start_of_{kind}: {line}}}"));
                open = Some(kind);
            }
            _ => out.push(words_line(line)),
        }
    }
    close(&mut out, &mut open);
    let mut text = out.join("\n");
    text.push('\n');
    text
}

/// Whether `text` is a file the songs folder wrote (it carries a song id).
pub fn is_ours(text: &str) -> bool {
    text.lines().any(|l| directive(l.trim()).is_some_and(|(n, v)| n == format!("{X}id") && Uuid::parse_str(&v).is_ok()))
}

/// A detail read back: the exact text when it still matches the line (an edit in Notepad wins over it).
fn exact_or(value: String, exact: Option<String>) -> String {
    match exact { Some(e) if tidy(&e) == tidy(&value) => e, _ => value }
}

/// A file the songs folder wrote, read back into the song it was written from. Also reads one edited in Notepad
/// (changed words, details, a section added) and one with the blank line under the details removed.
pub fn read(text: &str) -> Result<Song, String> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut song = Song::default();
    let mut writers: Vec<String> = Vec::new();
    let mut exact: std::collections::HashMap<String, String> = Default::default();
    let mut id = None;
    let mut words: Vec<String> = Vec::new();
    let mut in_details = true;
    let lines: Vec<&str> = text.strip_suffix('\n').unwrap_or(&text).split('\n').collect();
    for line in lines {
        let t = line.trim();
        if in_details && t.is_empty() { in_details = false; continue }
        if t.starts_with('#') { continue }
        let d = directive(t).filter(|(n, _)| is_directive(n));
        let Some((name, value)) = d else {
            in_details = false;
            words.push(line.to_string());
            continue;
        };
        let x = name.strip_prefix(X);
        match (name.as_str(), x) {
            ("title" | "t", _) => song.title = value,
            ("artist", _) => song.artist = value,
            ("composer" | "lyricist", _) => if !value.is_empty() { writers.push(value) },
            ("year", _) => song.year = value,
            ("copyright", _) => song.copyright = value,
            ("ccli", _) => song.ccli = value,
            ("key" | "k", _) => song.key = value,
            ("transpose", _) => song.transpose = value.trim_start_matches('+').parse::<i8>().map(crate::transpose::clamp).unwrap_or(0),
            (_, Some("publisher")) => song.publisher = value,
            (_, Some("order")) => {
                song.arrangement = if value.starts_with('[') { serde_json::from_str(&value).unwrap_or_default() }
                    else { value.split(',').map(str::trim).filter(|v| !v.is_empty()).map(String::from).collect() };
            }
            (_, Some("lines_per_slide")) => song.lines_per_slide = value.parse::<u8>().unwrap_or(0).min(Song::MAX_LINES),
            (_, Some("text_size")) => song.text_size = value.parse().unwrap_or(0),
            (_, Some("id")) => id = Uuid::parse_str(&value).ok(),
            (_, Some("line")) => { in_details = false; words.push(serde_json::from_str(&value).unwrap_or(value)) }
            (_, Some(e)) if e.starts_with("exact_") => { exact.insert(e["exact_".len()..].to_string(), serde_json::from_str(&value).unwrap_or(value)); }
            (n, _) if n.starts_with("start_of_") || matches!(n, "sov" | "soc" | "sob") => { in_details = false; if !value.is_empty() { words.push(value) } }
            // `{end_of_…}`, `{chorus}`, comments and the rest add nothing to the words.
            _ => {}
        }
    }
    let mut take = |name: &str, v: &mut String| { let e = exact.remove(name); *v = exact_or(std::mem::take(v), e) };
    take("title", &mut song.title);
    take("artist", &mut song.artist);
    take("year", &mut song.year);
    take("copyright", &mut song.copyright);
    take("ccli", &mut song.ccli);
    take("key", &mut song.key);
    take("publisher", &mut song.publisher);
    let mut joined = writers.join(" | ");
    take("writers", &mut joined);
    song.writers = joined;
    song.lyrics = words.join("\n");
    song.id = id.ok_or_else(|| crate::tr!("it has no RustyPresenter song id"))?;
    if song.title.trim().is_empty() { return Err(crate::tr!("it has no title")) }
    Ok(song)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(title: &str, lyrics: &str) -> Song { Song { id: Uuid::new_v4(), title: title.into(), lyrics: lyrics.into(), ..Default::default() } }

    fn round_trip(s: &Song) {
        let text = write(s);
        assert!(is_ours(&text), "{text}");
        assert_eq!(&read(&text).unwrap(), s, "written as:\n{text}");
        // Windows line endings (an editor that saves them) read the same.
        assert_eq!(&read(&text.replace('\n', "\r\n")).unwrap(), s);
    }

    #[test]
    fn a_full_song_is_the_same_when_read_back() {
        let s = Song {
            id: Uuid::new_v4(), title: "Amazing Grace".into(), writers: "John Newton | Edwin Excell".into(), ccli: "22025".into(),
            copyright: "Public Domain".into(), artist: "Chris Tomlin".into(), publisher: "Sixsteps".into(), year: "1779".into(),
            lyrics: "Intro\n[G] [D] [Em] [C]\n\nVerse 1\n[G]Amazing [C]grace, how [G]sweet the sound\nThat saved a wretch like me\n\nI once was lost\n\n[Chorus]\nMy chains are gone\n{Instrumental}\n\nBridge\nla la".into(),
            arrangement: vec!["Intro".into(), "Verse 1".into(), "Chorus".into(), "Verse 1".into()], lines_per_slide: 3, text_size: 96,
            key: "G".into(), transpose: -2,
        };
        round_trip(&s);
        let text = write(&s);
        // Other programs see standard ChordPro.
        for want in ["{title: Amazing Grace}", "{composer: John Newton}", "{composer: Edwin Excell}", "{ccli: 22025}", "{key: G}", "{transpose: -2}",
            "{start_of_verse: Verse 1}", "{start_of_chorus: [Chorus]}", "{end_of_chorus}", "{start_of_bridge: Bridge}", "{start_of_verse: Intro}",
            "{x_rustypresenter_order: Intro, Verse 1, Chorus, Verse 1}", "[G]Amazing [C]grace"] {
            assert!(text.contains(want), "{want} in:\n{text}");
        }
        // The blank lines between sections are outside them.
        assert!(text.contains("That saved a wretch like me\n\nI once was lost\n{end_of_verse}\n\n{start_of_chorus"), "{text}");
        // The app's own ChordPro reader (Import) reads it too, with the chords.
        let other = crate::import::chordpro::parse_str(&text).songs.remove(0);
        assert_eq!((other.title.as_str(), other.ccli.as_str(), other.key.as_str()), ("Amazing Grace", "22025", "G"));
        assert!(other.lyrics.contains("[G]Amazing [C]grace"));
    }

    #[test]
    fn odd_songs_are_the_same_when_read_back() {
        let odd = [
            song("Plain", "no sections at all\njust words"),
            song("Empty", ""),
            song("Blank lines", "\n\nVerse 1\na\n\n\n\nChorus\nb\n\n"),
            song("Lines that look like ChordPro", "{title: not the title}\n# not a comment\n  #indented\n{c: Chorus}\n{start_of_verse}\n{Tag}\n{key: X}"),
            song("Spaces", "  Verse 1  \n   leading spaces\ntrailing spaces   \n\tTab"),
            song("Ünïcödé — “quotes” & ‘more’ 🙏", "Verse 1\nÉl es Señor ♪ 主 Святый\n[F#m7/C#]chord [x2] [*Slowly]"),
            song("Chorus: style", "Chorus:\nsing\nVERSE 2\nloud\n[Verse 3]\nsoft\n(Chorus)\n"),
            song("Braces } in { title", "Verse 1\nlike } this {"),
        ];
        for s in &odd { round_trip(s) }
        let mut s = song("Line\nbreaks", "Verse 1\nx");
        s.copyright = "© 2020 One\n(Admin. by Two) {x}".into();
        s.writers = "A|B | ".into();
        s.artist = " spaced ".into();
        s.arrangement = vec!["Verse 1, part 2".into(), " Chorus".into()];
        s.ccli = "7 ".into();
        round_trip(&s);
        let text = write(&s);
        assert!(text.contains("{copyright: © 2020 One (Admin. by Two) (x)}"), "a tidy line for other programs:\n{text}");
        let mut s = song("Hymn", "Verse 1\nx");
        s.arrangement = vec!["[Chorus]".into()];
        round_trip(&s);
    }

    #[test]
    fn edits_made_in_notepad_are_read() {
        let s = Song { writers: "A | B".into(), copyright: "line one\nline two".into(), ..song("Grace", "Verse 1\nold words") };
        let text = write(&s)
            .replace("old words", "new words")
            .replace("{title: Grace}", "{title: Grace Again}")
            .replace("{composer: B}\n", "")
            .replace("{copyright: line one line two}", "{copyright: changed}");
        let r = read(&text).unwrap();
        assert_eq!((r.title.as_str(), r.writers.as_str(), r.lyrics.as_str()), ("Grace Again", "A", "Verse 1\nnew words"));
        assert_eq!(r.copyright, "changed", "the edit wins over the exact text kept beside it");
        assert_eq!(r.id, s.id);
        // The blank line under the details taken out, a section added by hand.
        let text = write(&s).replace(&format!("{}\n\n", s.id), &format!("{}\n", s.id)).replace("{end_of_verse}", "{end_of_verse}\n{soc: Chorus}\nsing\n{eoc}");
        assert_eq!(read(&text).unwrap().lyrics, "Verse 1\nold words\nChorus\nsing");
        // Not ours.
        assert!(!is_ours("{title: X}\n[G]la"));
        assert!(read("{title: X}\nla").is_err());
    }

    #[test]
    fn many_songs_round_trip() {
        // Every combination of a few awkward pieces, so nothing slips through.
        let pieces = ["Verse 1", "[G]Amazing", "", "{Tag}", "Chorus", "#5", "  x", "Bridge 2b", "[C] [G]", "{eoc}", "(Chorus)", "Tag"];
        let mut n = 0;
        for a in pieces { for b in pieces { for c in pieces {
            let mut s = song(&format!("Song {n}"), &format!("{a}\n{b}\n{c}"));
            s.transpose = (n % 23) as i8 - 11;
            round_trip(&s);
            n += 1;
        } } }
    }
}
