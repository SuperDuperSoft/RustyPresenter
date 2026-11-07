//! OpenSong song files → Songs. OpenSong keeps each song as an XML file, usually with no extension, in its
//! `Songs` folder (subfolders are categories).
//! Written from the OpenSong file layout (facts, not code): `<song>` with `<title>`, `<author>`, `<copyright>`,
//! `<ccli>`, `<presentation>` (section tags in performance order, "V1 C V2 C"), and `<lyrics>` as plain text:
//! `[V1]`/`[C]`/`[B]`/`[P]`/`[T]` (or a name of its own, `[Coda]`) start a section; lines starting with `.` are
//! chords and with `;` comments; a line starting with a digit belongs to that numbered verse of the section
//! (`[V]` then `1…`, `2…` makes V1 and V2); `||` starts a new slide and `|` a new line; `_` only stretches a
//! word under a long chord. Text before any section is a verse. Files are only read, never written; folders are
//! read with `openlyrics::read_songs`, which tells the two formats apart by content.
use super::chordpro::decode;
use super::common::ImportReport;
use super::easyworship::split_writers;
use super::openlp::{lyrics_from_verses, Verse};
use super::openlyrics::{self, ccli_number, child, format_of, parse_xml, split_tag, text_of, title_or, Format};
use crate::model::Song;
use crate::tr;
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

/// Sections read at most; the lyrics builder refuses more than 500 anyway.
const MAX_PARTS: usize = 1000;

/// One lyric line's text: `||` is a slide break, `|` a line break, `_` goes.
fn tidy(line: &str) -> String {
    line.replace("||", "\n\n").replace('|', "\n").replace('_', "")
}

/// OpenSong lyrics → verses in written order. Err when there are too many sections to be a song.
pub fn lyrics_to_verses(lyrics: &str) -> Result<Vec<Verse>, String> {
    let text = lyrics.replace("\r\n", "\n").replace('\r', "\n");
    let mut verses: Vec<Verse> = Vec::new();
    // The current section's tag, its own verse (lines without a number), and its numbered verses.
    let mut tag = split_tag("");
    let mut own: Option<usize> = None;
    let mut numbered: HashMap<char, usize> = HashMap::new();
    let new_verse = |verses: &mut Vec<Verse>, kind: &str, label: String, name: &str| -> Result<usize, String> {
        if verses.len() >= MAX_PARTS { return Err(tr!("the lyrics have too many sections to be a song")); }
        verses.push(Verse { kind: kind.into(), label, text: String::new(), name: name.into() });
        Ok(verses.len() - 1)
    };
    for raw in text.split('\n') {
        let trimmed = raw.trim();
        if let Some(rest) = raw.trim_start().strip_prefix('[') {
            if let Some((head, after)) = rest.split_once(']') {
                tag = split_tag(head);
                own = None;
                numbered.clear();
                let after = after.trim();
                if after.is_empty() { continue; }
                let i = new_verse(&mut verses, &tag.0, tag.1.clone(), &tag.2)?;
                own = Some(i);
                verses[i].text.push_str(&tidy(after));
                verses[i].text.push('\n');
                continue;
            }
        }
        if trimmed.is_empty() {
            // A blank line breaks the slide in every verse of the section.
            for &i in own.iter().chain(numbered.values()) { verses[i].text.push('\n'); }
            continue;
        }
        let first = raw.chars().next().unwrap_or(' ');
        if first == '.' || first == ';' { continue; }
        let (i, line) = if first.is_ascii_digit() {
            let i = match numbered.get(&first) {
                Some(&i) => i,
                None => {
                    // `[V]` + "1" is V1; `[V2]` + "1" (unusual) is V21, still its own verse.
                    let (kind, label, name) = (&tag.0, format!("{}{first}", tag.1), &tag.2);
                    let name = if name.is_empty() { String::new() } else { format!("{name} {first}") };
                    let i = new_verse(&mut verses, kind, label, &name)?;
                    numbered.insert(first, i);
                    i
                }
            };
            (i, &raw[1..])
        } else {
            let i = match own { Some(i) => i, None => { let i = new_verse(&mut verses, &tag.0, tag.1.clone(), &tag.2)?; own = Some(i); i } };
            (i, raw)
        };
        verses[i].text.push_str(&tidy(line));
        verses[i].text.push('\n');
    }
    Ok(verses)
}

/// The song in a parsed OpenSong document, and the title it is listed under even when it can't be read.
pub(crate) fn from_document(doc: &roxmltree::Document, fallback: Option<&str>) -> (String, Result<Song, String>) {
    let root = doc.root_element();
    let field = |name: &str| child(root, name).map(text_of).unwrap_or_default();
    let title = title_or(&field("title"), fallback);
    // The lyrics keep their line breaks, so not `text_of`.
    let lyrics: String = child(root, "lyrics").map(|l| l.descendants().filter(|d| d.is_text()).filter_map(|d| d.text()).collect()).unwrap_or_default();
    if lyrics.trim().is_empty() { return (title, Err(tr!("no lyrics"))); }
    let song = lyrics_to_verses(&lyrics)
        .and_then(|v| lyrics_from_verses(v, &field("presentation").replace(',', " ")))
        .map(|(lyrics, arrangement)| Song {
            id: Uuid::new_v4(), title: title.clone(), writers: split_writers(&field("author")), ccli: ccli_number(&field("ccli")),
            copyright: field("copyright"), lyrics, arrangement,
        });
    (title, song)
}

/// The song in an OpenSong file's text. Err says why it can't be read.
pub fn parse_str(text: &str) -> Result<Song, String> {
    let doc = parse_xml(text)?;
    match format_of(&doc) {
        Some(Format::OpenSong) => from_document(&doc, None).1,
        Some(Format::OpenLyrics) => Err(tr!("it is an OpenLyrics song, not OpenSong")),
        None => Err(tr!("it isn't an OpenSong song")),
    }
}

/// The song in an OpenSong file's bytes, in whatever encoding it was saved.
pub fn parse_bytes(bytes: &[u8]) -> Result<Song, String> { parse_str(&decode(bytes)) }

/// An OpenSong song file, or OpenSong's `Songs` folder (or any folder of OpenSong or OpenLyrics songs).
pub fn read_opensong(path: &Path) -> Result<ImportReport, String> { openlyrics::read_songs(path) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::common::TempDir;
    use crate::slides::{parse_sections, song_order};

    // Amazing Grace (John Newton, 1779): public domain. The chorus is invented for the test.
    const GRACE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<song>
  <title>Amazing Grace</title>
  <author>John Newton, Traditional</author>
  <copyright>Public Domain</copyright>
  <ccli>22025</ccli>
  <presentation>V1 C V2 C</presentation>
  <key>G</key>
  <lyrics>[V1]
.G             C       G
 Amazing grace! how sweet the sound
;sing gently
 That saved a wretch like me!

 I once was lost, but now am found,
 Was blind, but now I see.
[C]
 Sing it out, sing it out
 Grace that sets us free
[V2]
 'Twas grace that taught my heart to fear,
 And grace my fears relieved;</lyrics>
</song>"#;

    fn labels(s: &Song) -> Vec<String> { parse_sections(&s.lyrics).into_iter().map(|x| x.label).collect() }

    #[test]
    fn metadata_lyrics_and_order() {
        let s = parse_str(GRACE).unwrap();
        assert_eq!(s.title, "Amazing Grace");
        assert_eq!(s.writers, "John Newton | Traditional");
        assert_eq!(s.copyright, "Public Domain");
        assert_eq!(s.ccli, "22025");
        assert_eq!(s.lyrics, "Verse 1\nAmazing grace! how sweet the sound\nThat saved a wretch like me!\n\nI once was lost, but now am found,\nWas blind, but now I see.\n\n\
Chorus\nSing it out, sing it out\nGrace that sets us free\n\n\
Verse 2\n'Twas grace that taught my heart to fear,\nAnd grace my fears relieved;");
        assert_eq!(s.arrangement, ["Verse 1", "Chorus", "Verse 2", "Chorus"]);
        let secs = parse_sections(&s.lyrics);
        let order: Vec<_> = song_order(&s, &secs).into_iter().map(|i| secs[i].label.clone()).collect();
        assert_eq!(order, s.arrangement);
    }

    #[test]
    fn numbered_lines_make_verses() {
        let xml = "<song><title>N</title><presentation>V2 V1</presentation><lyrics>[V]\n.G      D\n1Amazing grace! how sweet the sound\n2'Twas grace that taught my heart to fear\n\n1That saved a wretch like me\n2And grace my fears relieved\n[C]\n Chorus line</lyrics></song>";
        let s = parse_str(xml).unwrap();
        assert_eq!(s.lyrics, "Verse 1\nAmazing grace! how sweet the sound\n\nThat saved a wretch like me\n\n\
Verse 2\n'Twas grace that taught my heart to fear\n\nAnd grace my fears relieved\n\nChorus\nChorus line");
        assert_eq!(s.arrangement, ["Verse 2", "Verse 1"]);
    }

    #[test]
    fn bars_underscores_and_headers() {
        let xml = "<song><title>B</title><lyrics> Text before any header\n[T]\n Tag line one|tag line two||Next slide\n[Coda]\n Wor__ds at the end\n[Pre-Chorus]\n Rising</lyrics></song>";
        let s = parse_str(xml).unwrap();
        assert_eq!(s.lyrics, "Verse\nText before any header\n\nTag\nTag line one\ntag line two\n\nNext slide\n\nCoda\nWords at the end\n\nPre-Chorus\nRising");
        assert_eq!(labels(&s), ["Verse", "Tag", "Coda", "Pre-Chorus"]);
        assert!(s.arrangement.is_empty());
    }

    #[test]
    fn errors_are_clear() {
        assert!(parse_str("not xml").unwrap_err().contains("valid XML"));
        assert_eq!(parse_str("<song><title>X</title><lyrics>  </lyrics></song>").unwrap_err(), "no lyrics");
        assert_eq!(parse_str("<song><title>X</title></song>").unwrap_err(), "no lyrics");
        assert!(parse_str("<song><title>X</title><lyrics>.G C\n;only notes</lyrics></song>").is_err());
        assert!(parse_str("<set/>").unwrap_err().contains("isn't an OpenSong"));
        let many = format!("<song><title>M</title><lyrics>{}</lyrics></song>", (0..1500).map(|i| format!("[V{i}]\n x\n")).collect::<String>());
        assert!(parse_str(&many).is_err());
    }

    #[test]
    fn untitled_and_odd_fields() {
        let s = parse_str("<song><title/><author>A &amp; B</author><ccli>CCLI 123</ccli><lyrics>[C]\n x</lyrics></song>").unwrap();
        assert_eq!(s.title, "Untitled song");
        assert_eq!(s.writers, "A & B");
        assert_eq!(s.ccli, "");
        let utf16: Vec<u8> = "\u{feff}<song><title>Café</title><lyrics>[V1]\r\n x\r\n</lyrics></song>".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(parse_bytes(&utf16).unwrap().title, "Café");
    }

    #[test]
    fn malformed_input_never_panics() {
        for i in 0..GRACE.len() {
            if GRACE.is_char_boundary(i) { let _ = parse_str(&GRACE[..i]); }
        }
        for lyrics in ["[", "]", "[[]]", "1", "9\n9\n9", "|||", "[V1", "[]x", "\u{e9}", "[V]\n1\n\n2"] {
            let _ = parse_str(&format!("<song><title>x</title><lyrics>{lyrics}</lyrics></song>"));
        }
    }

    #[test]
    fn reads_a_songs_folder() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.0.join("Amazing Grace"), GRACE).unwrap();
        let r = read_opensong(&tmp.0).unwrap();
        assert_eq!(r.songs.len(), 1);
        assert!(r.skipped.is_empty());
        assert_eq!(read_opensong(&tmp.0.join("Amazing Grace")).unwrap().songs[0].title, "Amazing Grace");
    }
}
