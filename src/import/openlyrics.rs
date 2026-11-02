//! OpenLyrics song files (`.xml`) → Songs. OpenLyrics is the open song format OpenLP exports, and FreeShow,
//! Quelea, OpenSong converters and many worship apps read and write.
//! Written from the OpenLyrics format description (facts, not code): a file is one song,
//! `<song xmlns="http://openlyrics.info/namespace/2009/song">` with `<properties>` (`<titles><title>`,
//! `<authors><author type="words|music|translation">`, `<copyright>`, `<ccliNo>`, `<verseOrder>v1 c v2 c</verseOrder>`)
//! and `<lyrics>` holding `<verse name="v1">` elements. A verse's `<lines>` are its slides; `<br/>` breaks a line
//! and other whitespace is only spacing; `<chord>`s and `<comment>`s are not lyrics, and `<tag>` formatting keeps
//! its text. A song with translations (`<verse lang="de">`) is imported in the language its first verse is in;
//! transliterations are left out. Files are only read, never written.
//!
//! The folder reader also takes OpenSong song files (see `opensong`), which are XML too and often have no file
//! extension, so a church can point it at any folder of exported songs: each file is recognised by its content.
use super::chordpro::decode;
use super::common::{find_files, ImportReport};
use super::openlp::{lyrics_from_verses, verse_text, Verse};
use super::opensong;
use crate::model::Song;
use std::io::Read;
use std::path::Path;
use uuid::Uuid;

/// A file bigger than this is not a song file.
const MAX_FILE: u64 = 4_000_000;
/// Files looked at, and folder depth walked, at most when a folder is imported.
const MAX_FILES: usize = 50_000;
const MAX_DEPTH: usize = 16;
/// `<verse>` elements read at most (the lyrics builder refuses more than 500 anyway).
const MAX_VERSES: usize = 1000;
/// Bytes read to tell whether a file without a song extension is song XML.
const SNIFF: u64 = 1024;

// ---------- shared with OpenSong ----------

fn is_bom_or_space(c: char) -> bool { c.is_whitespace() || c == '\u{feff}' }

/// Element name without its namespace, lowercase.
pub(crate) fn local(n: roxmltree::Node) -> String { n.tag_name().name().to_ascii_lowercase() }

/// The first child element called `name` (any namespace, any case).
pub(crate) fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.is_element() && c.tag_name().name().eq_ignore_ascii_case(name))
}

/// All the text inside `n`, spacing collapsed to single spaces.
pub(crate) fn text_of(n: roxmltree::Node) -> String {
    let mut s = String::new();
    for t in n.descendants().filter(|d| d.is_text()) { s.push_str(t.text().unwrap_or("")); s.push(' '); }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A CCLI song number from a free-text field ("#0123456" → "123456"); empty when it isn't a number.
pub(crate) fn ccli_number(raw: &str) -> String {
    let t = raw.trim().trim_start_matches('#').trim().trim_start_matches('0');
    if !t.is_empty() && t.len() <= 12 && t.chars().all(|c| c.is_ascii_digit()) { t.to_string() } else { String::new() }
}

/// A verse tag or section header → (kind, label, own name) for `lyrics_from_verses`. "v1", "C", "V2a", "t" are
/// a kind letter and a number; "Verse 2", "Chorus", "Pre-Chorus" are a known kind and a number; anything else
/// ("Coda", "Strophe 3", "Outro") is a section name of its own.
pub(crate) fn split_tag(tag: &str) -> (String, String, String) {
    let t = tag.trim();
    let mut chars = t.chars();
    let Some(first) = chars.next() else { return ("v".into(), String::new(), String::new()) };
    let rest = chars.as_str().trim();
    let is_number = |n: &str| !n.is_empty() && n.len() <= 4 && n.starts_with(|c: char| c.is_ascii_digit()) && n.chars().all(|c| c.is_ascii_alphanumeric());
    if first.is_ascii_alphabetic() && (rest.is_empty() || is_number(rest)) {
        return (first.to_ascii_lowercase().to_string(), rest.to_string(), String::new());
    }
    let (word, num) = match t.rsplit_once(char::is_whitespace) {
        Some((w, n)) if is_number(n) => (w.trim(), n),
        _ => (t, ""),
    };
    let code = match word.to_lowercase().as_str() {
        "verse" => "v",
        "chorus" | "refrain" => "c",
        "bridge" => "b",
        "pre-chorus" | "prechorus" | "pre chorus" => "p",
        "intro" => "i",
        "ending" => "e",
        "tag" => "t",
        _ => return ("o".into(), String::new(), t.chars().take(30).collect()),
    };
    (code.into(), num.into(), String::new())
}

/// Parses XML text, refusing DTDs (no entity tricks). The error says what is wrong in words.
pub(crate) fn parse_xml(text: &str) -> Result<roxmltree::Document<'_>, String> {
    roxmltree::Document::parse(text.trim_start_matches(is_bom_or_space)).map_err(|e| format!("it isn't valid XML ({e})"))
}

/// Which song format a parsed XML file is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Format { OpenLyrics, OpenSong }

pub(crate) fn format_of(doc: &roxmltree::Document) -> Option<Format> {
    let root = doc.root_element();
    if local(root) != "song" { return None; }
    let ns = root.tag_name().namespace().unwrap_or("");
    if ns.contains("openlyrics") || child(root, "properties").is_some() { return Some(Format::OpenLyrics); }
    let lyrics = child(root, "lyrics");
    // OpenLyrics' lyrics are `<verse>` elements; OpenSong's are plain text.
    if lyrics.is_some_and(|l| l.children().any(|c| c.is_element() && local(c) == "verse")) { return Some(Format::OpenLyrics); }
    if lyrics.is_some() || child(root, "title").is_some() { return Some(Format::OpenSong); }
    None
}

/// The title a song is listed under: its own, else the file's name, else "Untitled song".
pub(crate) fn title_or(own: &str, fallback: Option<&str>) -> String {
    let own = own.trim();
    if !own.is_empty() { return own.to_string(); }
    fallback.map(str::trim).filter(|f| !f.is_empty()).map(str::to_string).unwrap_or_else(|| "Untitled song".into())
}

// ---------- one song ----------

/// The song in a parsed OpenLyrics document, and the title it is listed under even when it can't be read.
pub(crate) fn from_document(doc: &roxmltree::Document, fallback: Option<&str>) -> (String, Result<Song, String>) {
    let root = doc.root_element();
    let props = child(root, "properties");
    let prop = |name: &str| props.and_then(|p| child(p, name)).map(text_of).unwrap_or_default();

    let all: Vec<_> = child(root, "lyrics").map(|l| l.children().filter(|c| c.is_element() && local(*c) == "verse").collect()).unwrap_or_default();
    // Translations: the language of the first verse; verses without a language belong to every one.
    let lang = all.iter().find_map(|v| v.attribute("lang")).map(str::to_ascii_lowercase);
    let same_lang = |l: Option<&str>| l.is_none() || l.map(str::to_ascii_lowercase) == lang;
    let verses: Vec<_> = all.into_iter().filter(|v| same_lang(v.attribute("lang")) && v.attribute("translit").is_none()).collect();

    let titles: Vec<_> = props.and_then(|p| child(p, "titles")).map(|t| t.children().filter(|c| c.is_element() && local(*c) == "title").collect()).unwrap_or_default();
    let own_title = titles.iter().filter(|t| same_lang(t.attribute("lang"))).map(|t| text_of(*t)).find(|t| !t.is_empty())
        .or_else(|| titles.iter().map(|t| text_of(*t)).find(|t| !t.is_empty())).unwrap_or_default();
    let title = title_or(&own_title, fallback);

    if verses.is_empty() { return (title, Err("no lyrics".into())); }
    if verses.len() > MAX_VERSES { return (title, Err("too many verses to be a song".into())); }

    let mut writers: Vec<String> = Vec::new();
    if let Some(authors) = props.and_then(|p| child(p, "authors")) {
        for a in authors.children().filter(|c| c.is_element() && local(*c) == "author") {
            let name = text_of(a);
            if !name.is_empty() && !writers.iter().any(|w| w.eq_ignore_ascii_case(&name)) { writers.push(name); }
        }
    }

    let parts: Vec<Verse> = verses.iter().map(|v| {
        let (kind, label, name) = split_tag(v.attribute("name").unwrap_or(""));
        Verse { kind, label, text: verse_text(*v), name }
    }).collect();
    let song = lyrics_from_verses(parts, &prop("verseOrder")).map(|(lyrics, arrangement)| Song {
        id: Uuid::new_v4(), title: title.clone(), writers: writers.join(" | "), ccli: ccli_number(&prop("ccliNo")),
        copyright: prop("copyright"), lyrics, arrangement,
    });
    (title, song)
}

/// The song in an OpenLyrics file's text. Err says why it can't be read.
pub fn parse_str(text: &str) -> Result<Song, String> {
    let doc = parse_xml(text)?;
    match format_of(&doc) {
        Some(Format::OpenLyrics) => from_document(&doc, None).1,
        Some(Format::OpenSong) => Err("it is an OpenSong song, not OpenLyrics".into()),
        None => Err("it isn't an OpenLyrics song".into()),
    }
}

/// The song in an OpenLyrics file's bytes, in whatever encoding it was saved.
pub fn parse_bytes(bytes: &[u8]) -> Result<Song, String> { parse_str(&decode(bytes)) }

// ---------- files ----------

fn file_label(p: &Path) -> String { p.file_name().unwrap_or_default().to_string_lossy().into_owned() }

fn is_xml(p: &Path) -> bool { p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("xml")) }

/// A song (or skipped entry) for one file. `sure` is a file the user picked or an `.xml`: problems with it are
/// reported. Other files (OpenSong songs have no extension, but neither do lots of other files) are looked at
/// only when they start like XML, and left out quietly when they aren't songs.
fn read_file(p: &Path, sure: bool, report: &mut ImportReport) {
    if let Err(why) = read_one(p, sure, report) {
        if sure || why.1 { report.skipped.push((file_label(p), why.0)); }
    }
}

/// Err is (why, whether to report it even for a file that isn't an `.xml`).
fn read_one(p: &Path, sure: bool, report: &mut ImportReport) -> Result<(), (String, bool)> {
    let quiet = |why: &str| (why.to_string(), false);
    if !sure {
        let mut head = Vec::new();
        std::fs::File::open(p).and_then(|f| f.take(SNIFF).read_to_end(&mut head)).map_err(|e| quiet(&e.to_string()))?;
        if !decode(&head).trim_start_matches(is_bom_or_space).starts_with('<') { return Err(quiet("not XML")); }
    }
    let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    if size > MAX_FILE { return Err(quiet("the file is too big to be a song")); }
    let bytes = std::fs::read(p).map_err(|e| quiet(&format!("couldn't read it: {e}")))?;
    let text = decode(&bytes);
    // A damaged file that was meant to be a song is worth mentioning whatever its name.
    let doc = parse_xml(&text).map_err(|e| (e, text.contains("<song")))?;
    let stem = if is_xml(p) { p.file_stem() } else { p.file_name() }.map(|s| s.to_string_lossy().into_owned());
    let (title, song) = match format_of(&doc) {
        Some(Format::OpenLyrics) => from_document(&doc, stem.as_deref()),
        Some(Format::OpenSong) => opensong::from_document(&doc, stem.as_deref()),
        None => return Err(quiet("it isn't an OpenLyrics or OpenSong song")),
    };
    match song { Ok(s) => report.songs.push(s), Err(why) => report.skipped.push((title, why)) }
    Ok(())
}

/// Reads one OpenLyrics or OpenSong song file (any name), or every one in a folder and its subfolders: `.xml`
/// files, and files without a song extension whose content is song XML (OpenSong's `Songs` folder).
pub fn read_songs(path: &Path) -> Result<ImportReport, String> {
    let mut report = ImportReport::default();
    if path.is_file() {
        read_file(path, true, &mut report);
        if report.songs.is_empty() {
            let why = report.skipped.first().map(|(_, w)| w.clone()).unwrap_or_else(|| "no song in it".into());
            return Err(format!("No song could be read from {}: {why}.", path.display()));
        }
        return Ok(report);
    }
    if !path.is_dir() { return Err(format!("Couldn't find {}.", path.display())); }
    let (files, cut_short) = find_files(path, MAX_FILES, MAX_DEPTH, |_| true);
    for f in &files { read_file(f, is_xml(f), &mut report); }
    if report.songs.is_empty() && report.skipped.is_empty() {
        return Err(format!("No OpenLyrics or OpenSong songs were found in {}.", path.display()));
    }
    if cut_short {
        report.skipped.push((path.display().to_string(),
            format!("the folder has more than {MAX_FILES} files and folders in it, so only the first {MAX_FILES} were looked at")));
    }
    Ok(report)
}

/// OpenLyrics files (and any OpenSong songs beside them): see `read_songs`.
pub fn read_openlyrics(path: &Path) -> Result<ImportReport, String> { read_songs(path) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{classify, common::TempDir, Dedupe};
    use crate::slides::{parse_sections, song_order};

    // Amazing Grace (John Newton, 1779): public domain. The chorus is invented for the test.
    const GRACE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<song xmlns="http://openlyrics.info/namespace/2009/song" version="0.8" createdIn="OpenLP 3.0" modifiedIn="OpenLP 3.0">
  <properties>
    <titles><title>Amazing Grace</title><title>New Britain</title></titles>
    <authors><author type="words">John Newton</author><author type="music">Traditional</author><author type="words">john newton</author></authors>
    <copyright>Public Domain</copyright>
    <ccliNo>22025</ccliNo>
    <verseOrder>v1 c v2 c</verseOrder>
  </properties>
  <lyrics>
    <verse name="v1">
      <lines>
        <chord name="G"/>Amazing grace! how <chord name="C"/>sweet the sound<br/>
        That saved a wretch like me!
      </lines>
      <lines>I once was lost, but now am found,<br/>Was blind, but now I see.</lines>
    </verse>
    <verse name="c">
      <lines><comment>softly</comment>Sing it out, sing it out<br/>Grace that sets us free</lines>
    </verse>
    <verse name="v2">
      <lines>'Twas grace that taught my heart to fear,<br/>And grace my fears relieved;</lines>
    </verse>
  </lyrics>
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
        assert_eq!(labels(&s), ["Verse 1", "Chorus", "Verse 2"]);
        assert_eq!(s.arrangement, ["Verse 1", "Chorus", "Verse 2", "Chorus"]);
        let secs = parse_sections(&s.lyrics);
        let order: Vec<_> = song_order(&s, &secs).into_iter().map(|i| secs[i].label.clone()).collect();
        assert_eq!(order, s.arrangement);
        assert_eq!(secs[0].groups.len(), 2);
    }

    #[test]
    fn chords_wrapping_text_and_tags_keep_the_words() {
        let xml = r#"<song xmlns="http://openlyrics.info/namespace/2009/song" version="0.9"><properties><titles><title>T</title></titles></properties>
<lyrics><verse name="v1"><lines><chord root="G">A</chord>mazing <tag name="b">grace</tag><br/>Line two</lines></verse></lyrics></song>"#;
        assert_eq!(parse_str(xml).unwrap().lyrics, "Verse 1\nAmazing grace\nLine two");
    }

    #[test]
    fn written_order_gives_no_arrangement_and_names_map() {
        let xml = r#"<song xmlns="http://openlyrics.info/namespace/2009/song"><properties><titles><title>T</title></titles></properties>
<lyrics><verse name="i"><lines>a</lines></verse><verse name="p1"><lines>b</lines></verse><verse name="b"><lines>c</lines></verse>
<verse name="e"><lines>d</lines></verse><verse name="Outro"><lines>f</lines></verse><verse name="o"><lines>g</lines></verse></lyrics></song>"#;
        let s = parse_str(xml).unwrap();
        assert_eq!(labels(&s), ["Intro", "Pre-Chorus", "Bridge", "Ending", "Outro", "Other"]);
        assert!(s.arrangement.is_empty());
        assert_eq!(s.writers, "");
        assert_eq!(s.ccli, "");
    }

    #[test]
    fn translations_keep_the_first_language() {
        let xml = r#"<song xmlns="http://openlyrics.info/namespace/2009/song"><properties><titles><title lang="de">Erstaunliche Gnade</title><title lang="en">Amazing Grace</title></titles></properties>
<lyrics><verse name="v1" lang="en"><lines>Amazing grace</lines></verse><verse name="v1" lang="de"><lines>Erstaunliche Gnade</lines></verse>
<verse name="v1" lang="en" translit="la"><lines>Transliterated</lines></verse><verse name="v2" lang="EN"><lines>Through many dangers</lines></verse></lyrics></song>"#;
        let s = parse_str(xml).unwrap();
        assert_eq!(s.title, "Amazing Grace");
        assert_eq!(s.lyrics, "Verse 1\nAmazing grace\n\nVerse 2\nThrough many dangers");
    }

    #[test]
    fn errors_are_clear() {
        assert!(parse_str("").unwrap_err().contains("valid XML"));
        assert!(parse_str("<song><properties>").unwrap_err().contains("valid XML"));
        assert!(parse_str("<set><slide_groups/></set>").unwrap_err().contains("isn't an OpenLyrics"));
        assert!(parse_str("<song><title>X</title><lyrics>[V1]\n x</lyrics></song>").unwrap_err().contains("OpenSong"));
        let empty = r#"<song xmlns="http://openlyrics.info/namespace/2009/song"><properties><titles><title>E</title></titles></properties><lyrics/></song>"#;
        assert_eq!(parse_str(empty).unwrap_err(), "no lyrics");
        let blank = r#"<song xmlns="http://openlyrics.info/namespace/2009/song"><lyrics><verse name="v1"><lines> </lines></verse></lyrics></song>"#;
        assert!(parse_str(blank).is_err());
        let dtd = "<!DOCTYPE song [<!ENTITY a \"aaaa\">]><song xmlns=\"http://openlyrics.info/namespace/2009/song\"><lyrics><verse name=\"v1\"><lines>&a;</lines></verse></lyrics></song>";
        assert!(parse_str(dtd).is_err());
    }

    #[test]
    fn encodings() {
        let mut utf16 = vec![0xFF, 0xFE];
        for u in GRACE.replace("UTF-8", "UTF-16").encode_utf16() { utf16.extend(u.to_le_bytes()); }
        assert_eq!(parse_bytes(&utf16).unwrap().title, "Amazing Grace");
        let mut bom = b"\xEF\xBB\xBF".to_vec();
        bom.extend(GRACE.as_bytes());
        assert_eq!(parse_bytes(&bom).unwrap().title, "Amazing Grace");
        let latin = "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><song xmlns=\"http://openlyrics.info/namespace/2009/song\"><properties><titles><title>Caf\u{e9}</title></titles></properties><lyrics><verse name=\"v1\"><lines>x</lines></verse></lyrics></song>";
        let bytes: Vec<u8> = latin.chars().map(|c| c as u8).collect();
        assert_eq!(parse_bytes(&bytes).unwrap().title, "Caf\u{e9}");
    }

    #[test]
    fn split_tags() {
        let t = |s: &str| split_tag(s);
        assert_eq!(t("v1"), ("v".into(), "1".into(), String::new()));
        assert_eq!(t("C"), ("c".into(), String::new(), String::new()));
        assert_eq!(t("V2a"), ("v".into(), "2a".into(), String::new()));
        assert_eq!(t("Verse 3"), ("v".into(), "3".into(), String::new()));
        assert_eq!(t("Pre-Chorus"), ("p".into(), String::new(), String::new()));
        assert_eq!(t("Coda"), ("o".into(), String::new(), "Coda".into()));
        assert_eq!(t("Strophe 2"), ("o".into(), String::new(), "Strophe 2".into()));
        assert_eq!(t(""), ("v".into(), String::new(), String::new()));
        assert_eq!(ccli_number("#0123456"), "123456");
        assert_eq!(ccli_number("n/a"), "");
    }

    #[test]
    fn malformed_input_never_panics() {
        for i in 0..GRACE.len() {
            if GRACE.is_char_boundary(i) { let _ = parse_str(&GRACE[..i]); }
        }
        let deep = format!("<song xmlns=\"http://openlyrics.info/namespace/2009/song\"><lyrics><verse name=\"v1\"><lines>{}x{}</lines></verse></lyrics></song>",
            "<tag>".repeat(5000), "</tag>".repeat(5000));
        let _ = parse_str(&deep);
        let many = format!("<song xmlns=\"http://openlyrics.info/namespace/2009/song\"><lyrics>{}</lyrics></song>", "<verse name=\"v1\"><lines>x</lines></verse>".repeat(2000));
        assert!(parse_str(&many).is_err());
        for bytes in [&b"\xFF\xFE"[..], b"\0\0\0", b"<", b"\xEF\xBB\xBF<song", b"<song>\x80\x81</song>"] { let _ = parse_bytes(bytes); }
    }

    #[test]
    fn classify_finds_duplicates() {
        let a = parse_str(GRACE).unwrap();
        let b = parse_str(GRACE).unwrap();
        assert_eq!(classify(&[a.clone()], &[b]), vec![Dedupe::DuplicateCcli { existing: a.id }]);
    }

    #[test]
    fn reads_folders_and_files() {
        let tmp = TempDir::new().unwrap();
        let d = tmp.0.join("export");
        std::fs::create_dir_all(d.join("Songs").join("Hymns")).unwrap();
        std::fs::create_dir_all(d.join("Sets")).unwrap();
        std::fs::write(d.join("Amazing Grace (John Newton).xml"), GRACE).unwrap();
        // OpenSong keeps songs in files with no extension, named after the song; a dot in the name is not one.
        std::fs::write(d.join("Songs").join("Hymns").join("Morning Light (v. 2)"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><song><title></title><author>Nobody</author><lyrics>[V1]\n Morning light</lyrics></song>").unwrap();
        std::fs::write(d.join("Sets").join("Sunday"), "<?xml version=\"1.0\"?><set name=\"Sunday\"><slide_groups/></set>").unwrap();
        std::fs::write(d.join("broken.xml"), "<song><properties>").unwrap();
        std::fs::write(d.join("settings.xml"), "<config/>").unwrap();
        std::fs::write(d.join("background.jpg"), b"\xFF\xD8\xFF\xE0garbage").unwrap();
        std::fs::write(d.join("notes"), "just some notes").unwrap();
        std::fs::write(d.join("Empty.xml"), "<song xmlns=\"http://openlyrics.info/namespace/2009/song\"><properties><titles><title>Empty</title></titles></properties><lyrics/></song>").unwrap();

        let r = read_openlyrics(&d).unwrap();
        let mut titles: Vec<_> = r.songs.iter().map(|s| s.title.as_str()).collect();
        titles.sort();
        assert_eq!(titles, ["Amazing Grace", "Morning Light (v. 2)"]);
        let mut skipped: Vec<_> = r.skipped.iter().map(|(t, _)| t.as_str()).collect();
        skipped.sort();
        assert_eq!(skipped, ["Empty", "broken.xml", "settings.xml"], "{:?}", r.skipped);

        let one = read_openlyrics(&d.join("Amazing Grace (John Newton).xml")).unwrap();
        assert_eq!(one.songs.len(), 1);
        assert!(read_openlyrics(&d.join("broken.xml")).unwrap_err().contains("valid XML"));
        assert!(read_openlyrics(&d.join("notes")).is_err());
        assert!(read_openlyrics(&d.join("missing")).unwrap_err().contains("Couldn't find"));
        let empty = tmp.0.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(read_openlyrics(&empty).unwrap_err().contains("No OpenLyrics or OpenSong"));
    }
}
