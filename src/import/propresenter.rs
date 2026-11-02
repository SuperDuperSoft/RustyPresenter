//! ProPresenter 6 song files (`.pro6`, and the older `.pro5`) → Songs.
//! Written from the file format as ProPresenter saves it (facts, not code): one XML file per song,
//! `<RVPresentationDocument CCLISongTitle CCLIAuthor CCLICopyrightYear CCLIPublisher CCLISongNumber>` holding
//! `<RVSlideGrouping name="Verse 1" uuid>` groups of `<RVDisplaySlide>`s. A slide's words are in its
//! `displayElements`' `<RVTextElement>`s, as base64: `<NSString rvXMLIvarName="PlainText">` (used first), else
//! `RTFData` (RTF; an attribute in `.pro5`), else `WinFlowData` (a Windows XAML FlowDocument).
//! `<RVSongArrangement>`s list group uuids in the order they are sung; the one named by `selectedArrangementID`
//! (else the first) becomes the song's arrangement.
//!
//! Each group becomes a section labelled with the group's name, each slide a slide. Slides and groups with no
//! words (blank intro slides) are left out. A group name used twice is one section when the words match, and
//! "Chorus (2)" when they don't.
//!
//! ProPresenter 7 `.pro` files are protobuf, not XML, and aren't supported yet: they are reported with a clear
//! reason. `.pro6x` bundles are zip files holding a `.pro6`. Files are only read, never written.
use super::common::{find_files, label_line, section_name, ImportReport};
use super::easyworship::{rtf_text, split_writers};
use super::openlyrics::{ccli_number, decode_xml, parse_xml, title_or};
use crate::model::Song;
use roxmltree::Node;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use uuid::Uuid;

/// Why a ProPresenter 7 file can't be imported.
pub const PRO7_UNSUPPORTED: &str = "ProPresenter 7 files aren't supported yet; export as text or ProPresenter 6";
const BUNDLE_UNSUPPORTED: &str =
    "ProPresenter 6 bundles (.pro6x) aren't supported yet; rename it to .zip, unzip it, and import the .pro6 file inside";

/// A file bigger than this is not a song file (`.pro6` files carry RTF copies of every slide, so allow room).
const MAX_FILE: u64 = 32_000_000;
/// Files looked at, and folder depth walked, at most when a folder is imported.
const MAX_FILES: usize = 50_000;
const MAX_DEPTH: usize = 16;
/// Groups and slides read at most, and the longest lyrics accepted.
const MAX_GROUPS: usize = 500;
const MAX_SLIDES: usize = 5000;
const MAX_LYRICS: usize = 1_000_000;
/// Arrangement entries read at most.
const MAX_ORDER: usize = 2000;

// ---------- helpers ----------

fn is(n: Node, name: &str) -> bool { n.is_element() && n.tag_name().name().eq_ignore_ascii_case(name) }

/// An attribute, ignoring the case of its name.
fn attr<'a>(n: Node<'a, '_>, name: &str) -> Option<&'a str> {
    n.attributes().find(|a| a.name().eq_ignore_ascii_case(name)).map(|a| a.value())
}

/// `rvXMLIvarName`, ProPresenter's name for what a child element holds.
fn ivar<'a>(n: Node<'a, '_>) -> Option<&'a str> { attr(n, "rvXMLIvarName") }

/// Base64 (standard or URL-safe; whitespace and missing padding allowed). None when it isn't base64.
fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3 + 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            c if c.is_ascii_whitespace() => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Text → the lines a slide shows: any line break (including Mac's line separator) splits, spacing collapses,
/// and empty lines go (a blank line would split the slide in the app's format).
fn split_lines(text: &str) -> Vec<String> {
    text.split(['\r', '\n', '\u{2028}', '\u{2029}', '\u{0b}', '\u{0c}', '\u{85}'])
        .map(|l| l.chars().map(|c| if c.is_control() || c == '\u{fffc}' || c == '\u{feff}' { ' ' } else { c }).collect::<String>())
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect()
}

/// A Windows XAML FlowDocument's lines: one per `<Paragraph>`, split again at `<LineBreak/>`.
fn flow_text(xaml: &str) -> Vec<String> {
    let Ok(doc) = parse_xml(xaml) else { return vec![] };
    let mut text = String::new();
    for n in doc.root_element().descendants() {
        if is(n, "Paragraph") || is(n, "LineBreak") { text.push('\n'); }
        else if is(n, "Run") { if let Some(t) = attr(n, "Text") { text.push_str(t); } }
        else if n.is_text() && n.parent().is_some_and(|p| is(p, "Run") || is(p, "Span") || is(p, "Paragraph")) {
            text.push_str(n.text().unwrap_or(""));
        }
    }
    split_lines(&text)
}

/// The words in one `<RVTextElement>`: PlainText, else RTF, else the Windows FlowDocument.
fn element_lines(el: Node) -> Vec<String> {
    let field = |name: &str| -> Option<String> {
        let raw = el.children().find(|c| c.is_element() && ivar(*c).is_some_and(|v| v.eq_ignore_ascii_case(name)))
            .map(|c| c.text().unwrap_or("")).or_else(|| attr(el, name))?;
        base64(raw).map(|b| String::from_utf8_lossy(&b).into_owned())
    };
    let plain = field("PlainText").map(|t| split_lines(&t)).unwrap_or_default();
    if !plain.is_empty() { return plain; }
    let rtf = field("RTFData").map(|r| rtf_text(&r).iter().flat_map(|l| split_lines(l)).collect::<Vec<_>>()).unwrap_or_default();
    if !rtf.is_empty() { return rtf; }
    field("WinFlowData").map(|x| flow_text(&x)).unwrap_or_default()
}

/// The words on a slide: every text box on it, in order.
fn slide_lines(slide: Node) -> Vec<String> {
    slide.descendants()
        .filter(|n| is(*n, "RVTextElement") && n.parent().is_some_and(|p| is(p, "displayElements") || ivar(p).is_some_and(|v| v.eq_ignore_ascii_case("displayElements"))))
        .flat_map(element_lines).collect()
}

/// "© 1779 Public Domain" from the year and publisher, as SongSelect writes it.
fn copyright(year: &str, publisher: &str) -> String {
    let (year, publisher) = (year.trim(), publisher.trim());
    let lower = publisher.to_lowercase();
    if publisher.starts_with('©') || lower.starts_with("copyright") || lower.starts_with("(c)") {
        return if year.is_empty() || publisher.contains(year) { publisher.to_string() } else { format!("{publisher} {year}") };
    }
    match (year.is_empty(), publisher.is_empty()) {
        (true, true) => String::new(),
        (true, false) => format!("© {publisher}"),
        (false, true) => format!("© {year}"),
        (false, false) => format!("© {year} {publisher}"),
    }
}

// ---------- one song ----------

/// One group as read: its uuid, name and slides (each slide's lines).
struct Group { id: String, name: String, slides: Vec<Vec<String>> }

fn read_groups(root: Node) -> Result<Vec<Group>, String> {
    let groupings: Vec<Node> = root.descendants().filter(|n| is(*n, "RVSlideGrouping")).collect();
    if groupings.len() > MAX_GROUPS { return Err(format!("the song has too many groups (more than {MAX_GROUPS})")); }
    let mut slides_seen = 0usize;
    let mut words = 0usize;
    let mut read = |slides: Vec<Node>| -> Result<Vec<Vec<String>>, String> {
        slides_seen += slides.len();
        if slides_seen > MAX_SLIDES { return Err(format!("the song has too many slides (more than {MAX_SLIDES})")); }
        let out: Vec<Vec<String>> = slides.into_iter().map(slide_lines).filter(|l| !l.is_empty()).collect();
        words += out.iter().flatten().map(String::len).sum::<usize>();
        if words > MAX_LYRICS { return Err("the lyrics are too long to be a song".into()); }
        Ok(out)
    };
    if groupings.is_empty() {
        // Very old files have slides with no groups.
        let slides = read(root.descendants().filter(|n| is(*n, "RVDisplaySlide")).collect())?;
        return Ok(vec![Group { id: String::new(), name: String::new(), slides }]);
    }
    groupings.into_iter().map(|g| Ok::<_, String>(Group {
        id: attr(g, "uuid").unwrap_or("").trim().to_string(),
        name: attr(g, "name").unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" "),
        slides: read(g.descendants().filter(|n| is(*n, "RVDisplaySlide")).collect())?,
    })).collect()
}

/// The group uuids of the chosen arrangement: the selected one, else the first. Empty when there is none.
fn arrangement_ids(root: Node) -> Vec<String> {
    let arrangements: Vec<Node> = root.descendants().filter(|n| is(*n, "RVSongArrangement")).collect();
    let selected = attr(root, "selectedArrangementID").map(str::trim).filter(|s| !s.is_empty());
    let chosen = selected.and_then(|s| arrangements.iter().find(|a| attr(**a, "uuid").is_some_and(|u| u.trim().eq_ignore_ascii_case(s))))
        .or(arrangements.first());
    let Some(a) = chosen else { return vec![] };
    a.descendants().filter(|n| is(*n, "NSString")).filter_map(|n| n.text()).map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty()).take(MAX_ORDER).collect()
}

/// Groups → lyrics in the app's format and the arrangement (empty when it is just the written order).
fn lyrics_of(groups: Vec<Group>, order_ids: &[String]) -> Result<(String, Vec<String>), String> {
    // Sections as (label, slides); `name_of[i]` is the section group i was put in.
    let mut kept: Vec<(String, Vec<Vec<String>>)> = Vec::new();
    let mut name_of: Vec<Option<String>> = Vec::new();
    let mut merged = false;
    for g in &groups {
        if g.slides.is_empty() { name_of.push(None); continue; }
        let base = if g.name.is_empty() { "Group".to_string() } else { g.name.clone() };
        let name = section_name(Some(&base));
        let taken = |n: &str| kept.iter().any(|k| section_name(Some(&k.0)).eq_ignore_ascii_case(n));
        let mut label = base.clone();
        if let Some(earlier) = kept.iter().find(|k| section_name(Some(&k.0)).eq_ignore_ascii_case(&name)) {
            if earlier.1 == g.slides { name_of.push(Some(section_name(Some(&earlier.0)))); merged = true; continue; }
            label = (2..).map(|n| format!("{base} ({n})")).find(|l| !taken(&section_name(Some(l)))).unwrap_or_else(|| base.clone());
        }
        name_of.push(Some(section_name(Some(&label))));
        kept.push((label, g.slides.clone()));
    }
    if kept.is_empty() { return Err("the song has no words on its slides".into()); }

    let mut out: Vec<String> = Vec::new();
    for (label, slides) in &kept {
        if !out.is_empty() { out.push(String::new()); }
        out.push(label_line(label));
        out.push(slides.iter().map(|s| s.join("\n")).collect::<Vec<_>>().join("\n\n"));
    }
    let written: Vec<String> = kept.iter().map(|k| section_name(Some(&k.0))).collect();
    let by_id: HashMap<&str, &str> = groups.iter().zip(&name_of)
        .filter_map(|(g, n)| Some((g.id.as_str(), n.as_deref()?))).filter(|(id, _)| !id.is_empty()).collect();
    let mut order: Vec<String> = order_ids.iter().filter_map(|id| by_id.get(id.as_str()).map(|n| n.to_string())).collect();
    if order.is_empty() && merged { order = name_of.into_iter().flatten().collect(); }
    if order == written { order.clear(); }
    Ok((out.join("\n"), order))
}

/// The song in a parsed ProPresenter document, and the title it is listed under even when it can't be read.
fn from_document(doc: &roxmltree::Document, fallback: Option<&str>) -> (String, Result<Song, String>) {
    let root = doc.root_element();
    let get = |name: &str| attr(root, name).unwrap_or("").trim().to_string();
    let title = title_or(&get("CCLISongTitle"), fallback);
    let song = read_groups(root).and_then(|groups| lyrics_of(groups, &arrangement_ids(root))).map(|(lyrics, arrangement)| Song {
        id: Uuid::new_v4(), title: title.clone(), writers: split_writers(&get("CCLIAuthor")), ccli: ccli_number(&get("CCLISongNumber")),
        copyright: copyright(&get("CCLICopyrightYear"), &get("CCLIPublisher")), lyrics, arrangement,
    });
    (title, song)
}

/// Is a parsed document a ProPresenter song? Err says what it is instead.
fn check_root(doc: &roxmltree::Document) -> Result<(), String> {
    let name = doc.root_element().tag_name().name();
    if name.eq_ignore_ascii_case("RVPresentationDocument") { return Ok(()); }
    if name.eq_ignore_ascii_case("RVPlaylistDocument") { return Err("it is a ProPresenter playlist, not a song".into()); }
    Err("it isn't a ProPresenter 6 song".into())
}

/// Bytes → XML text, or why they can't be a ProPresenter 6 song (a zip bundle, a ProPresenter 7 file).
fn xml_text(bytes: &[u8], pro7_name: bool) -> Result<String, String> {
    if bytes.starts_with(b"PK\x03\x04") { return Err(BUNDLE_UNSUPPORTED.into()); }
    let text = decode_xml(bytes);
    if text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}').starts_with('<') { return Ok(text); }
    // ProPresenter 7 files are protobuf; their first field (application info) starts with byte 0x0A.
    if pro7_name || bytes.first() == Some(&0x0A) { return Err(PRO7_UNSUPPORTED.into()); }
    Err("it isn't a ProPresenter 6 song".into())
}

fn parse_named(bytes: &[u8], pro7_name: bool, fallback: Option<&str>) -> (Option<String>, Result<Song, String>) {
    let text = match xml_text(bytes, pro7_name) { Ok(t) => t, Err(e) => return (None, Err(e)) };
    let doc = match parse_xml(&text) { Ok(d) => d, Err(e) => return (None, Err(e)) };
    if let Err(e) = check_root(&doc) { return (None, Err(e)); }
    let (title, song) = from_document(&doc, fallback);
    (Some(title), song)
}

/// The song in a ProPresenter 6 file's text. Err says why it can't be read.
pub fn parse_str(text: &str) -> Result<Song, String> { parse_bytes(text.as_bytes()) }

/// The song in a ProPresenter 6 file's bytes. A ProPresenter 7 file gives `PRO7_UNSUPPORTED`.
pub fn parse_bytes(bytes: &[u8]) -> Result<Song, String> { parse_named(bytes, false, None).1 }

// ---------- files ----------

fn file_label(p: &Path) -> String { p.file_name().unwrap_or_default().to_string_lossy().into_owned() }

fn extension(p: &Path) -> String { p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase() }

/// Song file extensions: ProPresenter 6 and 5, ProPresenter 7 (reported as unsupported), and 6 bundles.
fn is_song_file(p: &Path) -> bool { matches!(extension(p).as_str(), "pro6" | "pro5" | "pro4" | "pro" | "pro6x") }

/// A song, or a skipped entry, for one file.
fn read_file(p: &Path, report: &mut ImportReport) {
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned());
    let mut bytes = Vec::new();
    // Read at most one byte past the limit, so the size is checked even when the file's size can't be asked.
    if let Err(e) = std::fs::File::open(p).and_then(|f| f.take(MAX_FILE + 1).read_to_end(&mut bytes)) {
        report.skipped.push((file_label(p), format!("couldn't read it: {e}")));
        return;
    }
    if bytes.len() as u64 > MAX_FILE { report.skipped.push((file_label(p), "the file is too big to be a song".into())); return; }
    match parse_named(&bytes, extension(p) == "pro", stem.as_deref()) {
        (_, Ok(s)) => report.songs.push(s),
        (title, Err(why)) => report.skipped.push((title.unwrap_or_else(|| file_label(p)), why)),
    }
}

/// Reads one ProPresenter song file (any name), or every `.pro6`/`.pro5` in a folder and its subfolders (the
/// ProPresenter 6 library is a folder of them). ProPresenter 7 `.pro` files and `.pro6x` bundles found in a
/// folder are listed as skipped with the reason; other files are left out.
pub fn read_propresenter(path: &Path) -> Result<ImportReport, String> {
    let mut report = ImportReport::default();
    if path.is_file() {
        read_file(path, &mut report);
        if report.songs.is_empty() {
            let why = report.skipped.first().map(|(_, w)| w.clone()).unwrap_or_else(|| "no song in it".into());
            return Err(format!("No song could be read from {}: {why}.", path.display()));
        }
        return Ok(report);
    }
    if !path.is_dir() { return Err(format!("Couldn't find {}.", path.display())); }
    let (files, cut_short) = find_files(path, MAX_FILES, MAX_DEPTH, is_song_file);
    for f in &files { read_file(f, &mut report); }
    if report.songs.is_empty() && report.skipped.is_empty() {
        return Err(format!("No ProPresenter songs were found in {}.", path.display()));
    }
    if cut_short {
        report.skipped.push((path.display().to_string(),
            format!("the folder has more than {MAX_FILES} files and folders in it, so only the first {MAX_FILES} were looked at")));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{classify, common::TempDir, Dedupe};
    use crate::slides::parse_sections;

    // Amazing Grace (John Newton, 1779): public domain. The chorus is invented for the test. Verse 2's first
    // slide is RTF only, its second a Windows FlowDocument only; one slide and one group are blank.
    const GRACE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/import/propresenter/Amazing Grace.pro6"));

    fn b64(bytes: &[u8]) -> String {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in bytes.chunks(3) {
            let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
            for i in 0..4 { s.push(if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' }); }
        }
        s
    }

    fn slide(text: &str) -> String {
        format!("<RVDisplaySlide enabled=\"true\"><array rvXMLIvarName=\"cues\"/><array rvXMLIvarName=\"displayElements\"><RVTextElement displayName=\"Default\"><NSString rvXMLIvarName=\"PlainText\">{}</NSString></RVTextElement></array></RVDisplaySlide>", b64(text.as_bytes()))
    }

    fn group(name: &str, id: &str, slides: &[&str]) -> String {
        format!("<RVSlideGrouping name=\"{name}\" uuid=\"{id}\"><array rvXMLIvarName=\"slides\">{}</array></RVSlideGrouping>",
            slides.iter().map(|s| slide(s)).collect::<String>())
    }

    fn doc(attrs: &str, groups: &[String], arrangement: &[&str]) -> String {
        let arr = if arrangement.is_empty() { String::new() } else {
            format!("<array rvXMLIvarName=\"arrangements\"><RVSongArrangement name=\"A\" uuid=\"a1\"><array rvXMLIvarName=\"groupIDs\">{}</array></RVSongArrangement></array>",
                arrangement.iter().map(|i| format!("<NSString>{i}</NSString>")).collect::<String>())
        };
        format!("<?xml version=\"1.0\" encoding=\"utf-8\"?><RVPresentationDocument versionNumber=\"600\" {attrs}><array rvXMLIvarName=\"groups\">{}</array>{arr}</RVPresentationDocument>", groups.concat())
    }

    fn labels(s: &Song) -> Vec<String> { parse_sections(&s.lyrics).into_iter().map(|x| x.label).collect() }

    #[test]
    fn fixture_metadata_lyrics_and_arrangement() {
        let s = parse_str(GRACE).unwrap();
        assert_eq!(s.title, "Amazing Grace");
        assert_eq!(s.writers, "John Newton");
        assert_eq!(s.ccli, "22025");
        assert_eq!(s.copyright, "© 1779 Public Domain");
        assert_eq!(s.lyrics, "Verse 1\nAmazing grace! how sweet the sound\nThat saved a wretch like me!\n\nI once was lost, but now am found,\nWas blind, but now I see.\n\n\
Chorus\nSing it out, sing it out\nGrace that sets us free\n\n\
Verse 2\n\u{2018}Twas grace that taught my heart to fear,\nAnd grace my fears relieved;\n\nHow precious did that grace appear\nThe hour I first believed.");
        assert_eq!(labels(&s), ["Verse 1", "Chorus", "Verse 2"]);
        // The selected arrangement, not the first; the blank group and an unknown uuid are dropped.
        assert_eq!(s.arrangement, ["Verse 1", "Chorus", "Verse 2", "Chorus"]);
        assert_eq!(parse_sections(&s.lyrics)[0].groups.len(), 2);
    }

    #[test]
    fn written_order_arrangement_is_empty_and_unnamed_groups_get_a_label() {
        let d = doc("CCLISongTitle=\"Morning Song\"", &[group("", "g1", &["Morning light"]), group("Chorus", "g2", &["Sing"])], &["g1", "g2"]);
        let s = parse_str(&d).unwrap();
        assert_eq!(s.lyrics, "[Group]\nMorning light\n\nChorus\nSing");
        assert!(s.arrangement.is_empty());
        assert_eq!(labels(&s), ["Group", "Chorus"]);
    }

    #[test]
    fn repeated_groups_merge_or_get_numbered() {
        // No arrangement: groups written out in sung order, the chorus copied.
        let d = doc("", &[group("Verse 1", "a", &["One"]), group("Chorus", "b", &["Sing"]), group("Verse 2", "c", &["Two"]),
            group("Chorus", "d", &["Sing"]), group("Chorus", "e", &["Different words"])], &[]);
        let s = parse_str(&d).unwrap();
        assert_eq!(labels(&s), ["Verse 1", "Chorus", "Verse 2", "Chorus (2)"]);
        assert_eq!(s.arrangement, ["Verse 1", "Chorus", "Verse 2", "Chorus", "Chorus (2)"]);
        // With an arrangement, the copy's uuid plays the first chorus.
        let s = parse_str(&doc("", &[group("Chorus", "b", &["Sing"]), group("Chorus", "d", &["Sing"])], &["b", "d", "b"])).unwrap();
        assert_eq!(s.lyrics, "Chorus\nSing");
        assert_eq!(s.arrangement, ["Chorus", "Chorus", "Chorus"]);
    }

    #[test]
    fn slide_text_is_cleaned() {
        let d = doc("", &[group("Verse 1", "a", &["  Line\tone \r\n\r\n\u{2029}Line two\u{0b}three  ", "", "   "])], &[]);
        assert_eq!(parse_str(&d).unwrap().lyrics, "Verse 1\nLine one\nLine two\nthree");
    }

    #[test]
    fn metadata_variants() {
        assert_eq!(copyright("", ""), "");
        assert_eq!(copyright("2001", ""), "© 2001");
        assert_eq!(copyright("", "Hymns Ltd"), "© Hymns Ltd");
        assert_eq!(copyright("2001", "© 2001 Hymns Ltd"), "© 2001 Hymns Ltd");
        let d = doc("CCLIAuthor=\"Ann Writer, Bob Writer\" CCLISongNumber=\"0\"", &[group("Verse", "a", &["x"])], &[]);
        let s = parse_str(&d).unwrap();
        assert_eq!(s.writers, "Ann Writer | Bob Writer");
        assert_eq!(s.ccli, "");
        assert_eq!(s.title, "Untitled song");
    }

    #[test]
    fn pro5_rtf_attribute_and_groupless_slides() {
        let rtf = b64(br"{\rtf1\ansi{\fonttbl\f0 Arial;}\f0 First line\par Second\line Third}");
        let d = format!("<RVPresentationDocument CCLISongTitle=\"Old\"><groups><RVSlideGrouping name=\"Verse 1\" uuid=\"x\"><slides><RVDisplaySlide><displayElements><RVTextElement RTFData=\"{rtf}\"/></displayElements></RVDisplaySlide></slides></RVSlideGrouping></groups></RVPresentationDocument>");
        assert_eq!(parse_str(&d).unwrap().lyrics, "Verse 1\nFirst line\nSecond\nThird");
        let d = format!("<RVPresentationDocument><slides>{}</slides></RVPresentationDocument>", slide("Just words"));
        assert_eq!(parse_str(&d).unwrap().lyrics, "[Group]\nJust words");
    }

    #[test]
    fn errors_are_clear() {
        assert_eq!(parse_bytes(b"\x0a\x12\x0aProPresenter\x10\x07").unwrap_err(), PRO7_UNSUPPORTED);
        assert!(parse_bytes(b"PK\x03\x04rest").unwrap_err().contains(".pro6x"));
        assert!(parse_str("just text").unwrap_err().contains("isn't a ProPresenter 6 song"));
        assert!(parse_str("<song><title>x</title></song>").unwrap_err().contains("isn't a ProPresenter 6 song"));
        assert!(parse_str("<RVPlaylistDocument/>").unwrap_err().contains("playlist"));
        assert!(parse_str("<RVPresentationDocument").unwrap_err().contains("valid XML"));
        assert!(parse_str(&doc("", &[group("Verse", "a", &[""])], &[])).unwrap_err().contains("no words"));
        let bad = "<RVPresentationDocument><RVSlideGrouping name=\"V\"><RVDisplaySlide><array rvXMLIvarName=\"displayElements\"><RVTextElement><NSString rvXMLIvarName=\"PlainText\">!!not base64!!</NSString></RVTextElement></array></RVDisplaySlide></RVSlideGrouping></RVPresentationDocument>";
        assert!(parse_str(bad).unwrap_err().contains("no words"));
        let many = doc("", &(0..=MAX_GROUPS).map(|i| group("V", &i.to_string(), &["x"])).collect::<Vec<_>>(), &[]);
        assert!(parse_str(&many).unwrap_err().contains("too many groups"));
    }

    #[test]
    fn malformed_input_never_panics() {
        for i in 0..GRACE.len() {
            if GRACE.is_char_boundary(i) { let _ = parse_str(&GRACE[..i]); }
        }
        let deep = format!("<RVPresentationDocument>{}x{}</RVPresentationDocument>", "<a>".repeat(5000), "</a>".repeat(5000));
        let _ = parse_str(&deep);
        for bytes in [&b"\xFF\xFE"[..], b"\0\0\0", b"<", b"\x0a", b"PK", b"\xEF\xBB\xBF<RVPresentationDocument/>", b"<RVPresentationDocument>\x80</RVPresentationDocument>"] {
            let _ = parse_bytes(bytes);
        }
        for s in ["", "=", "A", "AB", "A=B", "QQ", "////"] { let _ = base64(s); }
        assert_eq!(base64("SGk=").unwrap(), b"Hi");
        assert_eq!(base64("SGk").unwrap(), b"Hi");
        assert!(flow_text("<not xml").is_empty());
        assert!(rtf_text("{\\rtf1 {{{{").len() <= 1);
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
        let d = tmp.0.join("ProPresenter6");
        std::fs::create_dir_all(d.join("Hymns")).unwrap();
        std::fs::write(d.join("Amazing Grace.pro6"), GRACE).unwrap();
        std::fs::write(d.join("Hymns").join("Morning Light.pro6"), doc("", &[group("Verse 1", "a", &["Morning light"])], &[])).unwrap();
        std::fs::write(d.join("New Song.pro"), b"\x0a\x12\x0aProPresenter\x10\x07").unwrap();
        std::fs::write(d.join("Bundle.pro6x"), b"PK\x03\x04zip").unwrap();
        std::fs::write(d.join("broken.pro6"), "<RVPresentationDocument CCLISongTitle=").unwrap();
        std::fs::write(d.join("Blank.pro6"), doc("CCLISongTitle=\"Blank Song\"", &[group("Verse", "a", &[""])], &[])).unwrap();
        std::fs::write(d.join("Sunday.pro6plx"), "<RVPlaylistDocument/>").unwrap();
        std::fs::write(d.join("notes.txt"), "not a song").unwrap();

        let r = read_propresenter(&d).unwrap();
        let mut titles: Vec<_> = r.songs.iter().map(|s| s.title.as_str()).collect();
        titles.sort();
        assert_eq!(titles, ["Amazing Grace", "Morning Light"]);
        let mut skipped: Vec<_> = r.skipped.iter().map(|(t, w)| (t.as_str(), w.as_str())).collect();
        skipped.sort();
        assert_eq!(skipped.len(), 4, "{skipped:?}");
        assert_eq!(skipped[0].0, "Blank Song");
        assert_eq!(skipped[1].0, "Bundle.pro6x");
        assert_eq!(skipped[2].0, "New Song.pro");
        assert_eq!(skipped[2].1, PRO7_UNSUPPORTED);
        assert_eq!(skipped[3].0, "broken.pro6");

        assert_eq!(read_propresenter(&d.join("Amazing Grace.pro6")).unwrap().songs.len(), 1);
        let pro7 = read_propresenter(&d.join("New Song.pro")).unwrap_err();
        assert!(pro7.contains("ProPresenter 7 files aren't supported yet"), "{pro7}");
        // A .pro file that is text, not XML, is still called a ProPresenter 7 file.
        std::fs::write(d.join("Text.pro"), "hello").unwrap();
        assert!(read_propresenter(&d.join("Text.pro")).unwrap_err().contains("ProPresenter 7"));
        assert!(read_propresenter(&d.join("notes.txt")).unwrap_err().contains("isn't a ProPresenter 6 song"));
        assert!(read_propresenter(&d.join("broken.pro6")).unwrap_err().contains("valid XML"));
        assert!(read_propresenter(&d.join("missing")).unwrap_err().contains("Couldn't find"));
        let empty = tmp.0.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(read_propresenter(&empty).unwrap_err().contains("No ProPresenter songs"));
    }
}
