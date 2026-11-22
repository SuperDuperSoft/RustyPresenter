//! OpenLyrics song files (`.xml`) → Songs. OpenLyrics is the open song format OpenLP exports, and FreeShow,
//! Quelea, OpenSong converters and many worship apps read and write.
//! Written from the OpenLyrics format description (facts, not code): a file is one song,
//! `<song xmlns="http://openlyrics.info/namespace/2009/song">` with `<properties>` (`<titles><title>`,
//! `<authors><author type="words|music|translation">`, `<copyright>`, `<ccliNo>`, `<verseOrder>v1 c v2 c</verseOrder>`)
//! and `<lyrics>` holding `<verse name="v1">` elements. A verse's `<lines>` are its slides; `<br/>` breaks a line
//! and other whitespace is only spacing; `<chord>`s and `<comment>`s are not lyrics, and `<tag>` formatting keeps
//! its text. A song with translations (`<verse lang="de">`) is imported in the language its first verse is in
//! (which may be none given); transliterations are left out. Writers are the words, music and plain authors, and
//! the translator into the imported language; arrangers are not writers. Sections with a name of their own
//! (`name="Outro"`) are found by that name in `<verseOrder>`; `<instrument>` parts have no words and are left
//! out. The file's declared `encoding` is used. Files are only read, never written.
use super::chordpro::decode;
use super::openlp::{lyrics_from_verses, verse_text, Verse};
use crate::model::Song;
use crate::tr;
use std::collections::HashMap;
use uuid::Uuid;

/// `<verse>` elements read at most (the lyrics builder refuses more than 500 anyway).
const MAX_VERSES: usize = 1000;
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
    roxmltree::Document::parse(text.trim_start_matches(is_bom_or_space)).map_err(|e| tr!("it isn't valid XML ({error})", error = e))
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
    fallback.map(str::trim).filter(|f| !f.is_empty()).map(str::to_string).unwrap_or_else(|| tr!("Untitled song"))
}

// ---------- one song ----------

/// An element's `lang`, lowercase; None when it has none.
fn lang_of(n: roxmltree::Node) -> Option<String> {
    n.attribute("lang").map(|l| l.trim().to_ascii_lowercase()).filter(|l| !l.is_empty())
}

/// The song's title in language `lang` (or one with no language), else any title not transliterated, else any.
fn title_of(props: Option<roxmltree::Node>, lang: Option<&str>, fallback: Option<&str>) -> String {
    let titles: Vec<_> = props.and_then(|p| child(p, "titles"))
        .map(|t| t.children().filter(|c| c.is_element() && local(*c) == "title").collect()).unwrap_or_default();
    let plain = |t: &roxmltree::Node| t.attribute("translit").is_none();
    let first = |f: &dyn Fn(&roxmltree::Node) -> bool| titles.iter().filter(|t| f(t)).map(|t| text_of(*t)).find(|t| !t.is_empty());
    let own = first(&|t| plain(t) && lang_of(*t).is_none_or(|l| Some(l.as_str()) == lang))
        .or_else(|| first(&plain)).or_else(|| first(&|_| true)).unwrap_or_default();
    title_or(&own, fallback)
}

/// The song in a parsed OpenLyrics document, and the title it is listed under even when it can't be read.
pub(crate) fn from_document(doc: &roxmltree::Document, fallback: Option<&str>) -> (String, Result<Song, String>) {
    let root = doc.root_element();
    let props = child(root, "properties");
    let prop = |name: &str| props.and_then(|p| child(p, name)).map(text_of).unwrap_or_default();

    let all: Vec<_> = child(root, "lyrics").map(|l| l.children().filter(|c| c.is_element() && local(*c) == "verse").collect()).unwrap_or_default();
    if all.len() > MAX_VERSES { return (title_of(props, None, fallback), Err(tr!("too many verses to be a song"))); }
    // Transliterations are left out, unless that is all there is.
    let plain: Vec<_> = all.iter().copied().filter(|v| v.attribute("translit").is_none()).collect();
    let all = if plain.is_empty() { all } else { plain };
    // Translations: the song is read in the language of its first verse (which may be none given). A verse name
    // missing in that language keeps the first language it appears in, so no verse is lost and none is doubled.
    let primary = all.first().and_then(|v| lang_of(*v));
    let name_of = |v: roxmltree::Node| v.attribute("name").unwrap_or("").trim().to_lowercase();
    let mut chosen: HashMap<String, Option<String>> = HashMap::new();
    for v in &all {
        let lang = lang_of(*v);
        let c = chosen.entry(name_of(*v)).or_insert_with(|| lang.clone());
        if *c != primary && lang == primary { *c = lang; }
    }
    let verses: Vec<_> = all.into_iter().filter(|v| chosen.get(&name_of(*v)).is_some_and(|c| *c == lang_of(*v))).collect();
    let title = title_of(props, primary.as_deref(), fallback);

    if verses.is_empty() { return (title, Err(tr!("no words"))); }

    // Writers are who wrote the words and music, and the translator of the language imported; not arrangers.
    let mut writers: Vec<String> = Vec::new();
    if let Some(authors) = props.and_then(|p| child(p, "authors")) {
        for a in authors.children().filter(|c| c.is_element() && local(*c) == "author") {
            let kind = a.attribute("type").unwrap_or("").trim().to_ascii_lowercase();
            if kind == "arrangement" { continue; }
            if kind == "translation" && lang_of(a).is_some_and(|l| Some(l) != primary) { continue; }
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
        copyright: prop("copyright"), lyrics, arrangement, ..Default::default()
    });
    (title, song)
}

/// The song in an OpenLyrics file's text. Err says why it can't be read.
pub fn parse_str(text: &str) -> Result<Song, String> {
    let doc = parse_xml(text)?;
    match format_of(&doc) {
        Some(Format::OpenLyrics) => from_document(&doc, None).1,
        Some(Format::OpenSong) => Err(tr!("it is an OpenSong song, not OpenLyrics")),
        None => Err(tr!("it isn't an OpenLyrics song")),
    }
}

/// The song in an OpenLyrics file's bytes, in whatever encoding it was saved.
pub fn parse_bytes(bytes: &[u8]) -> Result<Song, String> { parse_str(&decode_xml(bytes)) }

/// XML bytes → text. A byte-order mark or UTF-16 wins; then the `encoding` the XML declaration names (a damaged
/// byte in a UTF-8 file costs only that character); a file that names none, or one not known, is guessed.
pub(crate) fn decode_xml(bytes: &[u8]) -> String {
    let start = bytes.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(bytes.len());
    // Only a declaration written in single bytes is looked at: UTF-16, with or without a BOM, has zero bytes.
    let head = &bytes[start..bytes.len().min(start + 200)];
    if let Some(decl) = head.strip_prefix(b"<?xml") {
        let decl = &decl[..decl.windows(2).position(|w| w == b"?>").unwrap_or(decl.len())];
        if let Some(enc) = declared_encoding(decl).and_then(|l| encoding_rs::Encoding::for_label(l.as_bytes())) {
            if enc == encoding_rs::UTF_8 { return String::from_utf8_lossy(bytes).into_owned(); }
            if enc != encoding_rs::UTF_16LE && enc != encoding_rs::UTF_16BE {
                return enc.decode_without_bom_handling(bytes).0.into_owned();
            }
        }
    }
    decode(bytes)
}

/// The value of `encoding="…"` in the inside of an XML declaration.
fn declared_encoding(decl: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(decl).ok()?;
    let rest = text.split_once("encoding")?.1.trim_start().strip_prefix('=')?.trim_start();
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    Some(rest[1..].split(quote).next()?.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn ol(lyrics: &str, props: &str) -> String {
        format!("<song xmlns=\"http://openlyrics.info/namespace/2009/song\" version=\"0.9\"><properties>{props}</properties><lyrics>{lyrics}</lyrics></song>")
    }

    #[test]
    fn an_empty_line_inside_one_lines_block_does_not_split_the_slide() {
        let xml = ol(r#"<verse name="v1"><lines>Line one<br/><comment>softly</comment><br/>Line two<br/><chord root="G"/><br/>Line three<br/>
   <br/>Line four</lines><lines><line>Line five</line><line/><line> </line><line>Line six</line></lines>
   <lines break="optional">Line seven</lines><lines repeat="2" part="men">Line eight</lines><lines><comment>only a note</comment></lines></verse>"#,
            "<titles><title>T</title></titles>");
        let s = parse_str(&xml).unwrap();
        assert_eq!(s.lyrics, "Verse 1\nLine one\nLine two\nLine three\nLine four\n\nLine five\nLine six\n\nLine seven\n\nLine eight");
        assert_eq!(parse_sections(&s.lyrics)[0].groups.len(), 4);
    }

    #[test]
    fn originals_without_lang_are_not_mixed_with_translations() {
        let xml = ol(r#"<verse name="v1"><lines>Amazing grace</lines></verse><verse name="v1" lang="de"><lines>Erstaunliche Gnade</lines></verse>
<verse name="c"><lines>Sing it out</lines></verse><verse name="c" lang="de"><lines>Singt es laut</lines></verse>"#,
            r#"<titles><title lang="de">Erstaunliche Gnade</title><title>Amazing Grace</title></titles><verseOrder>v1 c v1</verseOrder>"#);
        let s = parse_str(&xml).unwrap();
        assert_eq!(s.title, "Amazing Grace");
        assert_eq!(s.lyrics, "Verse 1\nAmazing grace\n\nChorus\nSing it out");
        assert_eq!(s.arrangement, ["Verse 1", "Chorus", "Verse 1"]);
        // A verse that only exists without a language is kept with a song whose first verse has one.
        let xml = ol(r#"<verse name="v1" lang="en"><lines>Amazing grace</lines></verse><verse name="v1" lang="de"><lines>Gnade</lines></verse>
<verse name="c"><lines>Sing it out</lines></verse>"#, "");
        assert_eq!(parse_str(&xml).unwrap().lyrics, "Verse 1\nAmazing grace\n\nChorus\nSing it out");
    }

    #[test]
    fn named_sections_are_found_in_the_verse_order() {
        let xml = ol(r#"<verse name="v1"><lines>a</lines></verse><verse name="c"><lines>b</lines></verse><verse name="Outro"><lines>c</lines></verse>
<verse name="Coda"><lines>d</lines></verse><verse name="o"><lines>e</lines></verse>"#,
            "<titles><title>T</title></titles><verseOrder>v1 c outro c Coda o</verseOrder>");
        let s = parse_str(&xml).unwrap();
        assert_eq!(labels(&s), ["Verse 1", "Chorus", "Outro", "Coda", "Other"]);
        assert_eq!(s.arrangement, ["Verse 1", "Chorus", "Outro", "Chorus", "Coda", "Other"]);
    }

    #[test]
    fn order_shorthand_and_instruments() {
        let xml = ol(r#"<verse name="v"><lines>a</lines></verse><verse name="c1"><lines>b</lines></verse>
<instrument name="i1"><lines><beat><chord root="G"/></beat></lines></instrument><instrument name="solo"><lines><beat><chord root="C"/></beat></lines></instrument>"#,
            "<titles><title>T</title></titles><verseOrder>v1 i1 c solo v1</verseOrder>");
        let s = parse_str(&xml).unwrap();
        assert_eq!(labels(&s), ["Verse", "Chorus"]);
        assert_eq!(s.arrangement, ["Verse", "Chorus", "Verse"]);
    }

    #[test]
    fn verses_without_a_name_or_with_the_same_name() {
        let xml = ol(r#"<verse><lines>a</lines></verse><verse name="v2"><lines>b</lines></verse><verse name="v2"><lines>c</lines></verse>"#, "");
        let s = parse_str(&xml).unwrap();
        assert_eq!(labels(&s), ["Verse", "Verse 2", "Verse 2 (2)"]);
        assert_eq!(s.title, "Untitled song");
    }

    #[test]
    fn writers_are_words_music_and_this_languages_translator() {
        let xml = ol(r#"<verse name="v1" lang="en"><lines>a</lines></verse>"#, r#"<titles><title>T</title></titles><authors>
<author type="words">Ann Words</author><author type="music">Ben Music</author><author>Cy Plain</author>
<author type="translation" lang="de">Dora German</author><author type="translation" lang="en">Ed English</author>
<author type="arrangement">Fay Arranger</author></authors>"#);
        assert_eq!(parse_str(&xml).unwrap().writers, "Ann Words | Ben Music | Cy Plain | Ed English");
        // Original-language verses: a German translator didn't write them.
        let xml = ol(r#"<verse name="v1"><lines>a</lines></verse>"#,
            r#"<authors><author type="words">Ann Words</author><author type="translation" lang="de">Dora German</author><author type="translation">Gus Unknown</author></authors>"#);
        assert_eq!(parse_str(&xml).unwrap().writers, "Ann Words | Gus Unknown");
    }

    #[test]
    fn a_transliterated_title_is_not_the_title() {
        let xml = ol(r#"<verse name="v1"><lines>a</lines></verse>"#,
            r#"<titles><title translit="en">Ha Chesed</title><title>Grace</title></titles>"#);
        assert_eq!(parse_str(&xml).unwrap().title, "Grace");
    }

    #[test]
    fn the_declared_encoding_is_used() {
        let cyr = "<?xml version=\"1.0\" encoding=\"windows-1251\"?><song xmlns=\"http://openlyrics.info/namespace/2009/song\"><properties><titles><title>\u{411}\u{43b}\u{430}\u{433}\u{43e}\u{434}\u{430}\u{442}\u{44c}</title></titles></properties><lyrics><verse name=\"v1\"><lines>x</lines></verse></lyrics></song>";
        let (bytes, _, bad) = encoding_rs::WINDOWS_1251.encode(cyr);
        assert!(!bad);
        assert_eq!(parse_bytes(&bytes).unwrap().title, "\u{411}\u{43b}\u{430}\u{433}\u{43e}\u{434}\u{430}\u{442}\u{44c}");
        // UTF-8 with one damaged byte keeps its other accented letters.
        let mut utf8 = "<?xml version='1.0' encoding='UTF-8'?><song xmlns=\"http://openlyrics.info/namespace/2009/song\"><properties><titles><title>Caf\u{e9} \u{fc}ber</title></titles></properties><lyrics><verse name=\"v1\"><lines>x".as_bytes().to_vec();
        utf8.push(0xE9);
        utf8.extend(b"</lines></verse></lyrics></song>");
        assert_eq!(parse_bytes(&utf8).unwrap().title, "Caf\u{e9} \u{fc}ber");
        // UTF-16: big-endian with a BOM, and both byte orders without one.
        let be: Vec<u8> = GRACE.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        let mut be_bom = vec![0xFE, 0xFF];
        be_bom.extend(&be);
        let le: Vec<u8> = GRACE.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        for b in [be_bom, be, le] { assert_eq!(parse_bytes(&b).unwrap().title, "Amazing Grace"); }
    }

    #[test]
    fn errors_are_clear() {
        assert!(parse_str("").unwrap_err().contains("valid XML"));
        assert!(parse_str("<song><properties>").unwrap_err().contains("valid XML"));
        assert!(parse_str("<set><slide_groups/></set>").unwrap_err().contains("isn't an OpenLyrics"));
        assert!(parse_str("<song><title>X</title><lyrics>[V1]\n x</lyrics></song>").unwrap_err().contains("OpenSong"));
        let empty = r#"<song xmlns="http://openlyrics.info/namespace/2009/song"><properties><titles><title>E</title></titles></properties><lyrics/></song>"#;
        assert_eq!(parse_str(empty).unwrap_err(), "no words");
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

}
