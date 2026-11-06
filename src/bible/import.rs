//! Offline Bibles the church supplies: public-domain texts (WEB, BSB, ASV…) or a file a publisher licensed to the
//! church directly. Reads Zefania, OSIS, USX and OpenSong XML into the same shape as the bundled KJV and keeps each
//! one as `bibles/{ABBR}.json` in the data folder. Nothing here is bundled or fetched. A Bible with a prefixed id
//! (`bible::id`) keeps the same file format, under its encoded id.
use super::{BOOKS, blocklist, canonical, display_name, find_book, id, normalise_id, parse_reference, sources::tidy};
use crate::{model::VerseText, scripture::strings::BLOCKED};
use quick_xml::{Reader, events::{BytesStart, Event}};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, fs, io, path::{Path, PathBuf}, sync::{Arc, Mutex}};

/// (book index into `BOOKS`, chapter, verse) → text, exactly like `Kjv`.
pub type Verses = BTreeMap<(u8, u16, u16), String>;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportedBible {
    pub abbreviation: String,
    pub name: String,
    pub copyright: String,
    pub verses: Verses,
}

const SUPPORTED: &str = "Use a Zefania, OSIS, USX or OpenSong Bible file (.xml or .usx).";

/// OSIS book ids in `BOOKS` order (OSIS 2.1.1 User Manual, Appendix C.1:
/// https://crosswire.org/osis/OSIS%202.1.1%20User%20Manual%2006March2006.pdf).
const OSIS: [&str; 66] = [
    "Gen", "Exod", "Lev", "Num", "Deut", "Josh", "Judg", "Ruth", "1Sam", "2Sam", "1Kgs", "2Kgs", "1Chr", "2Chr", "Ezra",
    "Neh", "Esth", "Job", "Ps", "Prov", "Eccl", "Song", "Isa", "Jer", "Lam", "Ezek", "Dan", "Hos", "Joel", "Amos", "Obad",
    "Jonah", "Mic", "Nah", "Hab", "Zeph", "Hag", "Zech", "Mal",
    "Matt", "Mark", "Luke", "John", "Acts", "Rom", "1Cor", "2Cor", "Gal", "Eph", "Phil", "Col", "1Thess", "2Thess",
    "1Tim", "2Tim", "Titus", "Phlm", "Heb", "Jas", "1Pet", "2Pet", "1John", "2John", "3John", "Jude", "Rev",
];

use super::USFM;

/// Elements that separate words; their edges become a space so "line one</para><para>line two" stays readable.
const BLOCK: &[&str] = &["br", "lb", "l", "lg", "p", "para", "div", "row", "cell", "table", "item", "list"];

/// Detects the format from the root element and reads the whole file. The abbreviation is empty when the file
/// does not name one (USX, OpenSong); the caller asks the operator.
pub fn parse(bytes: &[u8]) -> Result<ImportedBible, String> {
    let src = decode(bytes);
    let mut b = match root(&src)?.as_str() {
        "xmlbible" => zefania(&src)?,
        "osis" | "osiscorpus" => osis(&src)?,
        "usx" => usx(&src)?,
        "bible" => opensong(&src)?,
        _ => return Err(SUPPORTED.into()),
    };
    if b.verses.is_empty() { return Err(format!("No Bible verses found in this file. {SUPPORTED}")); }
    b.abbreviation = sanitize_abbr(&b.abbreviation).unwrap_or_default();
    b.name = tidy(&b.name);
    b.copyright = tidy(&b.copyright);
    Ok(b)
}

/// Uppercase A-Z/0-9, 2-10 characters, never a Windows device name. This is also the file name, so nothing that
/// survives it can leave the `bibles` folder.
pub fn sanitize_abbr(s: &str) -> Option<String> {
    let a: String = s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect();
    let device = matches!(a.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (a.len() == 4 && (a.starts_with("COM") || a.starts_with("LPT")) && a.as_bytes()[3].is_ascii_digit());
    ((2..=10).contains(&a.len()) && !device).then_some(a)
}

/// The id of a Bible on disk: a prefixed id as it stands, an old-style abbreviation by `sanitize_abbr`.
fn stored_id(s: &str) -> Option<String> {
    let s = normalise_id(s);
    if s.contains(':') { id::is_prefixed(&s).then_some(s) } else { sanitize_abbr(&s) }
}

fn file(root: &Path, id: &str) -> PathBuf { root.join("bibles").join(format!("{}.json", id::file_stem(id))) }

/// Adds `new` to an earlier import with the same abbreviation, book by book: each book in `new` (USX is one book
/// per file) replaces that whole book; other old books stay. A non-empty new name or copyright wins.
pub fn combine(old: Option<ImportedBible>, new: ImportedBible) -> ImportedBible {
    let Some(mut old) = old else { return new };
    let books: BTreeSet<u8> = new.verses.keys().map(|k| k.0).collect();
    old.verses.retain(|k, _| !books.contains(&k.0));
    old.verses.extend(new.verses);
    if !new.name.is_empty() { old.name = new.name; }
    if !new.copyright.is_empty() { old.copyright = new.copyright; }
    old
}

impl ImportedBible {
    pub fn len(&self) -> usize { self.verses.len() }
    pub fn is_empty(&self) -> bool { self.verses.is_empty() }

    fn last_verse(&self, book: u8, chapter: u16) -> Option<u16> {
        self.verses.range((book, chapter, 0)..=(book, chapter, u16::MAX)).next_back().map(|(k, _)| k.2)
    }

    /// Same references, labels and errors as `Kjv::lookup`. Missing chapters inside a range are an error
    /// (`Jude 1:1-3:1`), never an endless loop.
    pub fn lookup(&self, input: &str) -> Result<(String, Vec<VerseText>), String> {
        let r = parse_reference(input)?;
        let display = display_name(r.book);
        let mut out = Vec::new();
        for part in &r.parts {
            let (sc, sv) = part.start;
            let (ec, ev) = part.end;
            let ev_resolved = match ev { Some(v) => v, None => self.last_verse(r.book, ec).ok_or_else(|| format!("{display} {ec} does not exist."))? };
            let mut c = sc;
            let mut v = sv.unwrap_or(1);
            loop {
                let last = self.last_verse(r.book, c).ok_or_else(|| format!("{display} {c} does not exist."))?;
                match self.verses.get(&(r.book, c, v)) {
                    Some(t) => out.push(VerseText { reference: format!("{display} {c}:{v}"), text: t.clone() }),
                    None if v == sv.unwrap_or(1) && c == sc => return Err(format!("{display} {c}:{v} does not exist.")),
                    None => {}
                }
                if c == ec && v >= ev_resolved { break; }
                if v >= last { c += 1; v = 1; if c > ec { break; } } else { v += 1; }
                if out.len() > 400 { return Err("That passage is too long. Choose up to about 400 verses.".into()); }
            }
            if ev.is_some() && !out.iter().any(|x| x.reference == format!("{display} {ec}:{ev_resolved}")) {
                return Err(format!("{display} {ec}:{ev_resolved} does not exist."));
            }
        }
        if out.is_empty() { return Err("No verses found.".into()); }
        Ok((canonical(&r, display), out))
    }
}

// ---- persistence ----

#[derive(Serialize)]
struct StoredRef<'a> { abbreviation: &'a str, name: &'a str, copyright: &'a str, verses: Vec<(u8, u16, u16, &'a str)> }

#[derive(Deserialize)]
struct Stored { abbreviation: String, name: String, copyright: String, verses: Vec<(u8, u16, u16, String)> }

fn bad_abbr() -> io::Error { io::Error::new(io::ErrorKind::InvalidInput, "Give the Bible a short name of 2-10 letters or digits, like WEB.") }

/// Writes `root/bibles/{ABBR}.json`, or the encoded id for a prefixed one.
pub fn save(root: &Path, b: &ImportedBible) -> io::Result<()> {
    let abbr = stored_id(&b.abbreviation).ok_or_else(bad_abbr)?;
    fs::create_dir_all(root.join("bibles"))?;
    let s = StoredRef {
        abbreviation: &abbr, name: &b.name, copyright: &b.copyright,
        verses: b.verses.iter().map(|(k, t)| (k.0, k.1, k.2, t.as_str())).collect(),
    };
    crate::store::write_atomic(&file(root, &abbr), &serde_json::to_vec(&s).map_err(io::Error::other)?)
}

/// One saved Bible, if present and readable.
pub fn load(root: &Path, abbr: &str) -> Option<ImportedBible> {
    let abbr = stored_id(abbr)?;
    read(&file(root, &abbr), &abbr)
}

/// Every saved Bible, sorted by abbreviation. Unreadable or misnamed files are skipped.
pub fn load_all(root: &Path) -> Vec<ImportedBible> {
    let Ok(rd) = fs::read_dir(root.join("bibles")) else { return Vec::new() };
    let mut out: Vec<ImportedBible> = rd.flatten().filter_map(|e| {
        let p = e.path();
        if p.extension()? != "json" { return None; }
        read(&p, &id::from_file_stem(p.file_stem()?.to_str()?)?)
    }).collect();
    out.sort_by(|a, b| a.abbreviation.cmp(&b.abbreviation));
    out
}

fn read(p: &Path, abbr: &str) -> Option<ImportedBible> {
    let s: Stored = serde_json::from_slice(&fs::read(p).ok()?).ok()?;
    if stored_id(abbr).as_deref() != Some(abbr) || s.abbreviation != abbr { return None; }
    let b = ImportedBible {
        abbreviation: s.abbreviation, name: s.name, copyright: s.copyright,
        verses: s.verses.into_iter().filter(|v| (v.0 as usize) < BOOKS.len()).map(|(b, c, v, t)| ((b, c, v), t)).collect(),
    };
    (!is_blocked(&b)).then_some(b)
}

/// What the file says about itself and what its verses read. The abbreviation may be the operator's own.
fn is_blocked(b: &ImportedBible) -> bool {
    blocklist::blocked(&b.abbreviation, &[&b.name], &[&b.copyright], &[&b.abbreviation])
        || blocklist::blocked_text(b.verses.iter().map(|(k, t)| (*k, t.as_str())))
}

/// Deletes the Bible's file; already gone is fine. Holds `LOADED` throughout so no reader can reload the
/// file between the delete and the eviction.
pub fn remove(root: &Path, abbr: &str) -> io::Result<()> {
    let abbr = stored_id(abbr).ok_or_else(bad_abbr)?;
    let mut all = LOADED.lock().unwrap();
    let r = match fs::remove_file(file(root, &abbr)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        r => r,
    };
    all.remove(root);
    r
}

// ---- loaded Bibles ----

type Loaded = BTreeMap<String, Arc<ImportedBible>>;

/// Imported Bibles are megabytes of JSON: read once per data folder, then kept until one is added or removed.
static LOADED: Mutex<BTreeMap<PathBuf, Arc<Loaded>>> = Mutex::new(BTreeMap::new());

fn loaded(root: &Path) -> Arc<Loaded> {
    let mut all = LOADED.lock().unwrap();
    all.entry(root.to_path_buf())
        .or_insert_with(|| Arc::new(load_all(root).into_iter().map(|b| (b.abbreviation.clone(), Arc::new(b))).collect()))
        .clone()
}

/// The imported Bible with this abbreviation, if any.
pub fn get(root: &Path, abbr: &str) -> Option<Arc<ImportedBible>> { loaded(root).get(abbr).cloned() }

/// Every imported Bible, sorted by abbreviation.
pub fn all(root: &Path) -> Vec<Arc<ImportedBible>> { loaded(root).values().cloned().collect() }

/// Parses a file and saves it under `abbr` (or the file's own abbreviation when `abbr` is empty), merging books
/// into an earlier import of the same name. Built-in names (KJV, WEB, BSB) are refused; copyrighted names such as
/// HCSB are allowed, because a licensed file is how a church gets a translation no online source offers.
pub fn import(root: &Path, bytes: &[u8], abbr: &str) -> Result<Arc<ImportedBible>, String> {
    let mut b = parse(bytes)?;
    if is_blocked(&b) { return Err(BLOCKED.into()); }
    if !abbr.trim().is_empty() { b.abbreviation = abbr.into(); }
    let abbr = sanitize_abbr(&b.abbreviation).ok_or_else(|| bad_abbr().to_string())?;
    if super::built_in(&abbr).is_some() { return Err(format!("{abbr} is already built in. Give this Bible another short name.")); }
    b.abbreviation = abbr.clone();
    let mut b = combine(load(root, &abbr), b);
    if is_blocked(&b) { return Err(BLOCKED.into()); }
    if b.name.is_empty() { b.name = abbr.clone(); }
    save(root, &b).map_err(|e| e.to_string())?;
    // Patch the cache in place; a cold cache stays cold and reads the saved file when first needed.
    let b = Arc::new(b);
    if let Some(m) = LOADED.lock().unwrap().get_mut(root) {
        let mut next = (**m).clone();
        next.insert(abbr, b.clone());
        *m = Arc::new(next);
    }
    Ok(b)
}

// ---- XML plumbing ----

/// The lowercase `encoding` of the XML declaration in the first 200 bytes, if any.
fn declared_encoding(bytes: &[u8]) -> Option<String> {
    let head: String = bytes.iter().take(200).map(|&b| b.to_ascii_lowercase() as char).collect();
    let decl = &head[head.find("<?xml")?..];
    let decl = &decl[..decl.find("?>")?];
    let rest = decl[decl.find("encoding")? + 8..].trim_start().strip_prefix('=')?.trim_start();
    let q = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    Some(rest[1..].split(q).next()?.trim().to_string())
}

/// UTF-16 with BOM; Windows-1252 when the XML declaration names a single-byte Latin encoding (older Zefania
/// files); otherwise UTF-8, where only invalid bytes become U+FFFD.
fn decode(bytes: &[u8]) -> String {
    const CP1252: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}',
        '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    let utf16 = |b: &[u8], f: fn([u8; 2]) -> u16| String::from_utf16_lossy(&b.chunks_exact(2).map(|c| f([c[0], c[1]])).collect::<Vec<_>>());
    match bytes {
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
        _ => match declared_encoding(bytes).as_deref() {
            Some("windows-1252" | "cp1252" | "iso-8859-1" | "iso8859-1" | "latin1" | "latin-1" | "us-ascii" | "ascii") =>
                bytes.iter().map(|&b| if (0x80..0xA0).contains(&b) { CP1252[(b - 0x80) as usize] } else { b as char }).collect(),
            _ => String::from_utf8_lossy(bytes).into_owned(),
        },
    }
}

/// The five XML entities, `&nbsp;` and numeric character references.
fn entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&', "lt" => '<', "gt" => '>', "quot" => '"', "apos" => '\'', "nbsp" => ' ',
        _ => {
            let n = name.strip_prefix('#')?;
            let code = match n.strip_prefix(['x', 'X']) { Some(h) => u32::from_str_radix(h, 16).ok()?, None => n.parse().ok()? };
            char::from_u32(code)?
        }
    })
}

/// Attribute values are raw bytes in quick-xml; resolve entities ourselves (works with or without its `encoding` feature).
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let hit = rest.find(';').filter(|&j| j <= 12).and_then(|j| entity(&rest[1..j]).map(|c| (c, j)));
        match hit {
            Some((c, j)) => { out.push(c); rest = &rest[j + 1..]; }
            None => { out.push('&'); rest = &rest[1..]; }
        }
    }
    out.push_str(rest);
    out
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes().flatten()
        .find(|a| a.key.local_name().as_ref().eq_ignore_ascii_case(name.as_bytes()))
        .map(|a| unescape(&String::from_utf8_lossy(&a.value)))
}

/// Leading digits: "16" → 16, "16-17" (combined verses) → 16, "16a" → 16. Zero is not a verse.
fn num(s: impl AsRef<str>) -> Option<u16> {
    let s = s.as_ref().trim();
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    s[..end].parse().ok().filter(|n| *n > 0)
}

fn lname(b: &[u8]) -> String { String::from_utf8_lossy(b).to_ascii_lowercase() }

fn bad_xml(e: impl std::fmt::Display, at: u64) -> String { format!("This file isn't valid XML (near byte {at}: {e}). {SUPPORTED}") }

/// Lowercase local name of the first element.
fn root(src: &str) -> Result<String, String> {
    let mut r = Reader::from_str(src);
    loop {
        match r.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => return Ok(lname(e.local_name().as_ref())),
            Ok(Event::Eof) => return Err(SUPPORTED.into()),
            Err(e) => return Err(bad_xml(e, r.error_position())),
            _ => {}
        }
    }
}

/// What the format readers see: names are lowercase local names (no namespace prefix); text is unescaped.
/// An empty element (`<br/>`) is one `Open { empty: true }` with no `Close`.
enum Tok<'t> {
    Open { name: &'t str, el: &'t BytesStart<'t>, empty: bool },
    Close(&'t str),
    Text(&'t str),
}

fn walk(src: &str, mut f: impl FnMut(Tok<'_>)) -> Result<(), String> {
    let mut r = Reader::from_str(src);
    loop {
        let ev = match r.read_event() { Ok(e) => e, Err(e) => return Err(bad_xml(e, r.error_position())) };
        match &ev {
            Event::Start(e) => f(Tok::Open { name: &lname(e.local_name().as_ref()), el: e, empty: false }),
            Event::Empty(e) => f(Tok::Open { name: &lname(e.local_name().as_ref()), el: e, empty: true }),
            Event::End(e) => f(Tok::Close(&lname(e.local_name().as_ref()))),
            Event::Text(t) => f(Tok::Text(&String::from_utf8_lossy(t))),
            Event::CData(t) => f(Tok::Text(&String::from_utf8_lossy(t))),
            Event::GeneralRef(g) => if let Some(c) = entity(&String::from_utf8_lossy(g)) { f(Tok::Text(c.encode_utf8(&mut [0; 4]))) },
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

/// Collects the text of the current verse; `skip` > 0 while inside a note or heading.
#[derive(Default)]
struct Acc { verses: Verses, cur: Option<(u8, u16, u16)>, buf: String, skip: usize }

impl Acc {
    fn start(&mut self, key: (u8, u16, u16)) { self.end(); if key.1 > 0 { self.cur = Some(key); } }
    fn end(&mut self) {
        if let Some(k) = self.cur.take() {
            let t = tidy(&self.buf);
            if !t.is_empty() { self.verses.entry(k).and_modify(|s| { s.push(' '); s.push_str(&t) }).or_insert(t); }
        }
        self.buf.clear();
    }
    fn text(&mut self, s: &str) { if self.cur.is_some() { self.buf.push_str(s); } }
    /// Swallows tokens inside a skipped element; true when the token was consumed.
    fn skipping(&mut self, t: &Tok) -> bool {
        if self.skip == 0 { return false; }
        match t { Tok::Open { empty: false, .. } => self.skip += 1, Tok::Close(_) => self.skip -= 1, _ => {} }
        true
    }
    /// Spaces at block edges and empty elements (`<BR/>`, `<lb/>`, `<optbreak/>`).
    fn edge(&mut self, name: &str, empty: bool) { if empty || BLOCK.contains(&name) { self.text(" "); } }
}

/// Zefania XML (zef2005.xsd; element reference https://leimer.name/zefania/dokumentation/ and
/// https://github.com/biblenerd/Zefania-XML-Preservation): `<XMLBIBLE biblename>` `<INFORMATION>` (Dublin Core
/// title/identifier/rights) `<BIBLEBOOK bnumber="1-66">` `<CHAPTER cnumber>` `<VERS vnumber>`; book numbers above 66
/// are the Apocrypha.
fn zefania(src: &str) -> Result<ImportedBible, String> {
    let mut b = ImportedBible::default();
    let mut acc = Acc::default();
    let (mut book, mut chapter) = (None::<u8>, 0u16);
    let (mut in_info, mut meta) = (false, None::<usize>);
    let mut fields: [String; 3] = Default::default(); // title, identifier, rights
    walk(src, |t| {
        if acc.skipping(&t) { return; }
        match t {
            Tok::Open { name, el, empty } => match name {
                "xmlbible" => b.name = attr(el, "biblename").unwrap_or_default(),
                "information" => in_info = !empty,
                "title" | "identifier" | "rights" if in_info && !empty => meta = Some(["title", "identifier", "rights"].iter().position(|x| *x == name).unwrap_or(0)),
                "biblebook" => { acc.end(); book = attr(el, "bnumber").and_then(num).filter(|n| *n <= 66).map(|n| n as u8 - 1); }
                "chapter" => { acc.end(); chapter = attr(el, "cnumber").and_then(num).unwrap_or(0); }
                "vers" => {
                    acc.end();
                    if let (Some(bk), Some(v), false) = (book, attr(el, "vnumber").and_then(num), empty) { acc.start((bk, chapter, v)); }
                }
                "note" | "xref" | "caption" | "remark" | "prolog" | "media" if !empty => acc.skip = 1,
                _ => acc.edge(name, empty),
            },
            Tok::Close(name) => match name {
                "vers" | "chapter" | "biblebook" => acc.end(),
                "information" => in_info = false,
                "title" | "identifier" | "rights" => meta = None,
                _ => acc.edge(name, false),
            },
            Tok::Text(s) => match meta { Some(i) => fields[i].push_str(s), None => acc.text(s) },
        }
    })?;
    acc.end();
    let [title, identifier, rights] = fields;
    if !title.trim().is_empty() { b.name = title; }
    Ok(ImportedBible { abbreviation: identifier, copyright: rights, verses: acc.verses, ..b })
}

/// "John.3.16", "KJV:John.3.16", "John.3.16!a", "John.3.16 John.3.17" (first wins).
fn osis_ref(id: &str) -> Option<(u8, u16, u16)> {
    let id = id.split_whitespace().next()?.split('!').next()?;
    let id = id.rsplit_once(':').map_or(id, |(_, r)| r);
    let mut it = id.split('.');
    let (b, c, v) = (it.next()?, it.next()?, it.next()?);
    Some((OSIS.iter().position(|x| *x == b)? as u8, num(c)?, num(v)?))
}

/// OSIS 2.1.1 (manual linked on `OSIS`): `<osisText osisIDWork>`, `<header><work><title/><rights/></work>`, verses
/// either as containers `<verse osisID="John.3.16">…</verse>` or milestones `<verse sID osisID/>…<verse eID/>`.
/// Notes and non-canonical titles (section headings) are skipped; canonical titles (Psalm superscriptions) are kept.
fn osis(src: &str) -> Result<ImportedBible, String> {
    let mut acc = Acc::default();
    let (mut in_header, mut in_work, mut work_done) = (false, false, false);
    let mut meta = None::<usize>;
    let mut fields: [String; 3] = Default::default(); // title, OSIS identifier, rights
    let mut abbr = String::new();
    walk(src, |t| {
        if acc.skipping(&t) { return; }
        match t {
            Tok::Open { name, el, empty } => match name {
                "osistext" => abbr = attr(el, "osisIDWork").unwrap_or_default(),
                "header" => in_header = !empty,
                "work" if in_header && !work_done && !empty => in_work = true,
                "title" | "rights" if in_work && !empty => meta = Some(if name == "title" { 0 } else { 2 }),
                "identifier" if in_work && !empty && attr(el, "type").is_some_and(|t| t.eq_ignore_ascii_case("OSIS")) => meta = Some(1),
                "verse" => {
                    if attr(el, "eID").is_some() { acc.end(); return; }
                    acc.end();
                    // osisID names the verse; sID is often an opaque id ("v1"), so it is only a fallback on milestones.
                    let k = attr(el, "osisID").as_deref().and_then(osis_ref)
                        .or_else(|| attr(el, "sID").filter(|_| empty).as_deref().and_then(osis_ref));
                    if let Some(k) = k { acc.start(k); }
                }
                "chapter" if attr(el, "eID").is_some() => acc.end(),
                "note" if !empty => acc.skip = 1,
                "title" if !in_header && !empty && attr(el, "canonical").as_deref() != Some("true") => acc.skip = 1,
                _ => acc.edge(name, empty),
            },
            Tok::Close(name) => match name {
                "header" => in_header = false,
                "work" if in_work => { in_work = false; work_done = true; }
                "title" | "rights" | "identifier" => meta = None,
                "verse" | "chapter" => acc.end(),
                _ => acc.edge(name, false),
            },
            Tok::Text(s) => match meta { Some(i) => fields[i].push_str(s), None => acc.text(s) },
        }
    })?;
    acc.end();
    let [title, identifier, rights] = fields;
    // "Bible.en.WEB" → "WEB" when osisIDWork is missing.
    if abbr.trim().is_empty() { abbr = identifier.rsplit('.').next().unwrap_or_default().to_string(); }
    Ok(ImportedBible { abbreviation: abbr, name: title, copyright: rights, verses: acc.verses })
}

/// Para styles that are not verse text: identification, headings, titles, introductions, labels, remarks
/// (https://ubsicap.github.io/usx/parastyles.html). Trailing level digits are ignored (`s1` → `s`).
fn usx_heading(style: &str) -> bool {
    let s = style.trim_end_matches(|c: char| c.is_ascii_digit());
    s.starts_with('i') || s.starts_with("toc")
        || matches!(s, "h" | "mt" | "mte" | "ms" | "mr" | "s" | "sr" | "r" | "d" | "sp" | "sd" | "cl" | "cd" | "cp" | "rem" | "qa" | "lit" | "restore" | "lh" | "lf")
}

fn usx_book(code: &str) -> Option<u8> {
    let code = code.trim().to_ascii_uppercase();
    USFM.iter().position(|c| *c == code).or_else(|| BOOKS.iter().position(|b| b.0 == code)).map(|i| i as u8)
}

/// USX 3 (https://ubsicap.github.io/usx/elements.html): one book per file, `<book code="JHN">description</book>`,
/// `<chapter number="3" sid="JHN 3"/>`, `<verse number="16" sid="JHN 3:16"/>…<verse eid="JHN 3:16"/>`. USX 2 has
/// no `eid`; a verse then runs to the next verse or chapter. Notes, figures and heading paragraphs are skipped.
fn usx(src: &str) -> Result<ImportedBible, String> {
    let mut acc = Acc::default();
    let (mut book, mut chapter) = (None::<u8>, 0u16);
    walk(src, |t| {
        if acc.skipping(&t) { return; }
        match t {
            Tok::Open { name, el, empty } => match name {
                "book" => { acc.end(); book = attr(el, "code").as_deref().and_then(usx_book); if !empty { acc.skip = 1; } }
                "chapter" => { acc.end(); if let Some(n) = attr(el, "number").and_then(num) { chapter = n; } }
                "verse" => {
                    acc.end();
                    if attr(el, "eid").is_some() { return; }
                    if let (Some(bk), Some(v)) = (book, attr(el, "number").and_then(num)) { acc.start((bk, chapter, v)); }
                }
                "note" | "figure" | "sidebar" | "periph" if !empty => acc.skip = 1,
                "para" if !empty && attr(el, "style").is_some_and(|s| usx_heading(&s)) => acc.skip = 1,
                _ => acc.edge(name, empty),
            },
            Tok::Close(name) => acc.edge(name, false),
            Tok::Text(s) => acc.text(s),
        }
    })?;
    acc.end();
    Ok(ImportedBible { verses: acc.verses, ..Default::default() })
}

/// OpenSong (no published schema; shape from the Bible files distributed for OpenSong):
/// `<bible><b n="John"><c n="3"><v n="16">`. Book names go through `find_book`; a 66-book file whose names don't
/// all match (another language) is mapped by position. Also reads the similar Beblia layout
/// `<bible translation><testament><book number="43"><chapter number="3"><verse number="16">`.
fn opensong(src: &str) -> Result<ImportedBible, String> {
    let mut acc = Acc::default();
    let mut name = String::new();
    let mut books: Vec<Option<u8>> = Vec::new(); // per book in file order: its BOOKS index, if known
    let mut named: Vec<bool> = Vec::new(); // OpenSong `<b>`: matched by name?
    let mut chapter = 0u16;
    walk(src, |t| {
        if acc.skipping(&t) { return; }
        match t {
            Tok::Open { name: tag, el, empty } => match tag {
                "bible" => name = attr(el, "translation").or_else(|| attr(el, "n")).unwrap_or_default(),
                "b" => { acc.end(); let n = attr(el, "n").unwrap_or_default(); named.push(find_book(&n).is_some()); books.push(find_book(&n)); }
                "book" => { acc.end(); books.push(attr(el, "number").and_then(num).filter(|n| *n <= 66).map(|n| n as u8 - 1)); }
                "c" | "chapter" => { acc.end(); chapter = attr(el, "n").or_else(|| attr(el, "number")).and_then(num).unwrap_or(0); }
                "v" | "verse" => {
                    acc.end();
                    let v = attr(el, "n").or_else(|| attr(el, "number")).and_then(num);
                    // Key by position in the file for now; mapped to BOOKS below.
                    if let (Some(v), false, Some(pos)) = (v, empty, books.len().checked_sub(1)) { if pos < 256 { acc.start((pos as u8, chapter, v)); } }
                }
                _ => acc.edge(tag, empty),
            },
            Tok::Close(tag) => match tag { "v" | "verse" | "c" | "chapter" | "b" | "book" => acc.end(), _ => acc.edge(tag, false) },
            Tok::Text(s) => acc.text(s),
        }
    })?;
    acc.end();
    if books.len() == 66 && named.len() == 66 && named.contains(&false) {
        books = (0..66).map(Some).collect();
    }
    let verses = acc.verses.into_iter()
        .filter_map(|((pos, c, v), t)| Some(((books.get(pos as usize).copied().flatten()?, c, v), t)))
        .collect();
    Ok(ImportedBible { name, verses, ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(b: &ImportedBible) -> Vec<(u8, u16, u16)> { b.verses.keys().copied().collect() }

    // Zefania shape per zef2005.xsd (https://github.com/biblenerd/Zefania-XML-Preservation).
    const ZEFANIA: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<XMLBIBLE xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="zef2005.xsd" biblename="WEB" status="v" version="2.0.1.18" type="x-bible" revision="0">
 <INFORMATION><title>World English Bible</title><identifier>WEB</identifier><rights>Public Domain</rights><language>ENG</language></INFORMATION>
 <BIBLEBOOK bnumber="43" bname="John" bsname="Joh">
  <CHAPTER cnumber="3">
   <CAPTION vref="16">Heading</CAPTION>
   <VERS vnumber="16">For God so loved the world,<NOTE type="x-studynote">a note</NOTE> that he gave<BR art="x-nl"/>his one and only <gr str="5207">Son</gr>.</VERS>
   <VERS vnumber="17">For God didn&apos;t send his Son &amp; more.</VERS>
  </CHAPTER>
 </BIBLEBOOK>
 <BIBLEBOOK bnumber="67" bname="Tobit"><CHAPTER cnumber="1"><VERS vnumber="1">apocrypha</VERS></CHAPTER></BIBLEBOOK>
</XMLBIBLE>"#;

    #[test]
    fn zefania() {
        let b = parse(ZEFANIA.as_bytes()).unwrap();
        assert_eq!((b.abbreviation.as_str(), b.name.as_str(), b.copyright.as_str()), ("WEB", "World English Bible", "Public Domain"));
        assert_eq!(keys(&b), vec![(42, 3, 16), (42, 3, 17)]);
        assert_eq!(b.verses[&(42, 3, 16)], "For God so loved the world, that he gave his one and only Son.");
        assert_eq!(b.verses[&(42, 3, 17)], "For God didn't send his Son & more.");
    }

    // OSIS milestone form as in CrossWire modules (OSIS 2.1.1 manual §§ "verse", "chapter").
    const OSIS_MILESTONE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<osis xmlns="http://www.bibletechnologies.net/2003/OSIS/namespace"><osisText osisIDWork="WEB" osisRefWork="Bible" xml:lang="en">
<header><work osisWork="WEB"><title>World English Bible</title><identifier type="OSIS">Bible.en.WEB</identifier><rights>Public Domain</rights></work><work osisWork="KJV"><title>Other</title></work></header>
<div type="book" osisID="John"><chapter osisID="John.3" sID="John.3"/>
<title type="section">Heading</title>
<p><verse osisID="John.3.16" sID="John.3.16"/>For God so loved<note type="study" osisRef="John.3.16">n</note> the world.<verse eID="John.3.16"/>
<verse osisID="John.3.17" sID="John.3.17"/>For God didn’t send<lb/>his Son.<verse eID="John.3.17"/></p>
<chapter eID="John.3"/></div>
<div type="book" osisID="Tob"><chapter osisID="Tob.1"><verse osisID="Tob.1.1">apocrypha</verse></chapter></div>
</osisText></osis>"#;

    const OSIS_CONTAINER: &str = r#"<osis><osisText osisIDWork="KJV"><div type="book" osisID="Ps"><chapter osisID="Ps.23">
<verse osisID="Ps.23.1"><title type="psalm" canonical="true">A Psalm of David.</title> The LORD <transChange type="added">is</transChange> my <w lemma="strong:H7462">shepherd</w>; I shall not want.</verse>
<verse osisID="Ps.23.2 Ps.23.3">Combined.</verse></chapter></div></osisText></osis>"#;

    #[test]
    fn osis_milestones() {
        let b = parse(OSIS_MILESTONE.as_bytes()).unwrap();
        assert_eq!((b.abbreviation.as_str(), b.name.as_str(), b.copyright.as_str()), ("WEB", "World English Bible", "Public Domain"));
        assert_eq!(keys(&b), vec![(42, 3, 16), (42, 3, 17)]);
        assert_eq!(b.verses[&(42, 3, 16)], "For God so loved the world.");
        assert_eq!(b.verses[&(42, 3, 17)], "For God didn’t send his Son.");
    }

    #[test]
    fn osis_containers() {
        let b = parse(OSIS_CONTAINER.as_bytes()).unwrap();
        assert_eq!(b.abbreviation, "KJV");
        assert_eq!(b.verses[&(18, 23, 1)], "A Psalm of David. The LORD is my shepherd; I shall not want.");
        assert_eq!(b.verses[&(18, 23, 2)], "Combined.");
        assert_eq!(osis_ref("KJV:John.3.16!a"), Some((42, 3, 16)));
    }

    // USX 3 shape per https://ubsicap.github.io/usx/elements.html.
    const USX: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<usx version="3.0"><book code="JHN" style="id">World English Bible</book>
<para style="h">John</para><para style="toc1">The Good News According to John</para><para style="mt1">John</para>
<chapter number="3" style="c" sid="JHN 3"/>
<para style="s1">Heading</para>
<para style="p"><verse number="16" style="v" sid="JHN 3:16"/>For God so loved the world<note caller="+" style="f"><char style="fr" closed="false">3:16 </char><char style="ft" closed="false">fn</char></note>, that he gave<verse eid="JHN 3:16"/></para>
<para style="q1"><verse number="17-18" style="v" sid="JHN 3:17-18"/>Poetry line one</para><para style="q2">line two.<verse eid="JHN 3:17-18"/></para>
<chapter eid="JHN 3"/></usx>"#;

    #[test]
    fn usx() {
        let b = parse(USX.as_bytes()).unwrap();
        assert_eq!(b.abbreviation, "");
        assert_eq!(keys(&b), vec![(42, 3, 16), (42, 3, 17)]);
        assert_eq!(b.verses[&(42, 3, 16)], "For God so loved the world, that he gave");
        assert_eq!(b.verses[&(42, 3, 17)], "Poetry line one line two.");
    }

    #[test]
    fn usx2_without_end_milestones() {
        let src = r#"<usx version="2.0"><book code="PSA" style="id"/><chapter number="23" style="c"/><para style="d">A Psalm of David.</para>
<para style="q1"><verse number="1" style="v"/>The LORD is my shepherd;</para><para style="q2">I shall not want.</para>
<para style="q1"><verse number="2" style="v"/>He makes me lie down</para></usx>"#;
        let b = parse(src.as_bytes()).unwrap();
        assert_eq!(b.verses[&(18, 23, 1)], "The LORD is my shepherd; I shall not want.");
        assert_eq!(b.verses[&(18, 23, 2)], "He makes me lie down");
    }

    #[test]
    fn opensong() {
        let src = r#"<?xml version="1.0" encoding="UTF-8"?><bible><b n="Genesis"><c n="1"><v n="1">In the beginning</v></c></b>
<b n="1 John"><c n="2"><v n="1">a</v><v n="2">b</v></c></b><b n="Tobit"><c n="1"><v n="1">x</v></c></b><b n="Jude"><c n="1"><v n="1">j</v></c></b></bible>"#;
        let b = parse(src.as_bytes()).unwrap();
        assert_eq!(keys(&b), vec![(0, 1, 1), (61, 2, 1), (61, 2, 2), (64, 1, 1)]);
    }

    #[test]
    fn opensong_other_language_maps_by_position() {
        let books: String = (1..=66).map(|i| format!(r#"<b n="Livre{i}"><c n="1"><v n="1">t{i}</v></c></b>"#)).collect();
        let b = parse(format!("<bible>{books}</bible>").as_bytes()).unwrap();
        assert_eq!(b.verses[&(42, 1, 1)], "t43");
        assert_eq!(b.len(), 66);
    }

    #[test]
    fn beblia_numbers() {
        let src = r#"<bible translation="King James Version"><testament name="New"><book number="43"><chapter number="3"><verse number="16">For God</verse></chapter></book><book number="70"><chapter number="1"><verse number="1">x</verse></chapter></book></testament></bible>"#;
        let b = parse(src.as_bytes()).unwrap();
        assert_eq!(b.name, "King James Version");
        assert_eq!(keys(&b), vec![(42, 3, 16)]);
    }

    #[test]
    fn book_tables() {
        assert_eq!(OSIS.len(), BOOKS.len());
        assert_eq!(OSIS.iter().position(|x| *x == "John"), Some(42));
        assert_eq!(usx_book("JHN"), Some(42));
        assert_eq!(usx_book("PHP"), Some(49));
        assert_eq!(usx_book("SNG"), Some(21));
        assert_eq!(usx_book("REV"), Some(65));
        assert_eq!(usx_book("TOB"), None);
        assert_eq!(osis_ref("Tob.1.1"), None);
        assert_eq!(osis_ref("Rev.22.21"), Some((65, 22, 21)));
    }

    #[test]
    fn encodings() {
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend(ZEFANIA.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(parse(&utf16).unwrap().len(), 2);
        let latin = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><bible><b n=\"John\"><c n=\"3\"><v n=\"16\">\x93God\x94 caf\xe9</v></c></b></bible>";
        assert_eq!(parse(latin).unwrap().verses[&(42, 3, 16)], "“God” café");
        assert_eq!(decode(b"<?xml version='1.0' encoding = 'windows-1252' ?><a>\xe9</a>"), "<?xml version='1.0' encoding = 'windows-1252' ?><a>é</a>");
    }

    #[test]
    fn one_bad_byte_stays_utf8() {
        let mut src = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><a>café “x” ".as_bytes().to_vec();
        src.extend(b"\xff</a>");
        assert_eq!(decode(&src), "<?xml version=\"1.0\" encoding=\"UTF-8\"?><a>café “x” \u{FFFD}</a>");
        let mut bare = "<a>café".as_bytes().to_vec();
        bare.extend(b"\x93</a>");
        assert_eq!(decode(&bare), "<a>café\u{FFFD}</a>");
        assert_eq!(declared_encoding(b"<a encoding=\"latin1\"/>"), None);
    }

    #[test]
    fn osis_milestone_keys_by_osis_id() {
        let src = r#"<osis><osisText osisIDWork="WEB"><div type="book" osisID="John"><chapter osisID="John.3" sID="c3"/>
<verse osisID="John.3.16" sID="v1"/>For God so loved the world.<verse eID="v1"/><chapter eID="c3"/></div></osisText></osis>"#;
        let b = parse(src.as_bytes()).unwrap();
        assert_eq!(keys(&b), vec![(42, 3, 16)]);
        assert_eq!(b.verses[&(42, 3, 16)], "For God so loved the world.");
    }

    fn temp_root(tag: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("rp-import-{tag}-{stamp}"))
    }

    #[test]
    fn remove_evicts_cache() {
        let root = temp_root("evict");
        let mut b = sample();
        b.abbreviation = "TSTA".into();
        save(&root, &b).unwrap();
        assert!(get(&root, "TSTA").is_some());
        remove(&root, "TSTA").unwrap();
        assert!(get(&root, "TSTA").is_none());
        assert!(all(&root).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn import_updates_loaded_cache_in_place() {
        let root = temp_root("cache");
        assert!(all(&root).is_empty()); // warm, empty cache
        let john = import(&root, USX.as_bytes(), "tstb").unwrap();
        assert_eq!(john.len(), 2);
        assert_eq!(get(&root, "TSTB").unwrap().len(), 2);
        let psalm = r#"<usx version="2.0"><book code="PSA" style="id"/><chapter number="23" style="c"/><para style="q1"><verse number="1" style="v"/>The LORD</para></usx>"#;
        assert_eq!(import(&root, psalm.as_bytes(), "tstb").unwrap().len(), 3);
        let cached = get(&root, "TSTB").unwrap();
        assert_eq!(keys(&cached), vec![(18, 23, 1), (42, 3, 16), (42, 3, 17)]);
        assert_eq!(cached.name, "TSTB");
        assert_eq!(load(&root, "TSTB").unwrap(), *cached);
        let _ = fs::remove_dir_all(&root);
    }

    const NWT_FILE: &str = r#"<XMLBIBLE biblename="New World Translation"><INFORMATION><title>New World Translation</title><rights>Watch Tower</rights></INFORMATION>
        <BIBLEBOOK bnumber="43"><CHAPTER cnumber="3"><VERS vnumber="16">text</VERS></CHAPTER></BIBLEBOOK></XMLBIBLE>"#;

    #[test]
    fn a_blocked_file_is_refused_under_any_abbreviation() {
        let root = temp_root("blocked");
        assert_eq!(import(&root, NWT_FILE.as_bytes(), "").unwrap_err(), BLOCKED);
        assert_eq!(import(&root, NWT_FILE.as_bytes(), "TEST").unwrap_err(), BLOCKED);
        assert_eq!(import(&root, USX.as_bytes(), "TPT").unwrap_err(), BLOCKED, "the abbreviation alone");
        assert!(all(&root).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn verses_give_a_nameless_file_away() {
        let root = temp_root("fingerprint");
        let psalm = r#"<usx version="2.0"><book code="PSA" style="id"/><chapter number="23" style="c"/><para style="q1"><verse number="1" style="v"/>The Lord is my best friend</para></usx>"#;
        assert_eq!(import(&root, psalm.as_bytes(), "ABCD").unwrap_err(), BLOCKED);
        assert!(all(&root).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_signs_of_the_nwt_are_read_across_files_one_book_at_a_time() {
        let root = temp_root("books");
        let usx = |code: &str, chapter: u16, verses: String| format!(r#"<usx version="2.0"><book code="{code}" style="id"/><chapter number="{chapter}" style="c"/><para style="p">{verses}</para></usx>"#);
        let jehovah: String = (1..=20).map(|n| format!(r#"<verse number="{n}" style="v"/>Jehovah spoke "#)).collect();
        import(&root, usx("MAT", 1, jehovah).as_bytes(), "ABCD").unwrap();
        let john = usx("JHN", 1, r#"<verse number="1" style="v"/>and the Word was a god"#.into());
        assert_eq!(import(&root, john.as_bytes(), "ABCD").unwrap_err(), BLOCKED);
        assert_eq!(get(&root, "ABCD").unwrap().len(), 20, "the first file stays as it was");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_blocked_file_already_on_disk_is_skipped_and_can_be_removed() {
        let root = temp_root("on-disk");
        let bible = ImportedBible { abbreviation: "TPT".into(), name: "x".into(), verses: [((0, 1, 1), "a".to_string())].into(), ..Default::default() };
        save(&root, &bible).unwrap();
        assert!(load(&root, "TPT").is_none());
        assert!(all(&root).is_empty());
        remove(&root, "TPT").unwrap();
        assert!(!root.join("bibles").join("TPT.json").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse(b"<html><body>hi</body></html>").is_err());
        assert!(parse(b"<XMLBIBLE><BIBLEBOOK bnumber=\"1\">").is_err());
        assert!(parse(b"<bible></bible>").is_err());
        assert!(parse(b"not xml").is_err());
    }

    fn sample() -> ImportedBible {
        let mut verses = Verses::new();
        for (b, c, n) in [(42u8, 1u16, 51u16), (42, 2, 25), (42, 3, 36), (64, 1, 25), (40, 1, 3), (40, 3, 2)] {
            for v in (1..=n).filter(|v| (b, c) != (42, 1) || *v >= 50) { verses.insert((b, c, v), format!("{b}-{c}-{v}")); }
        }
        ImportedBible { abbreviation: "TST".into(), verses, ..Default::default() }
    }

    #[test]
    fn lookup_ranges() {
        let b = sample();
        let (label, v) = b.lookup("John 1:51-2:2").unwrap();
        assert_eq!(v.iter().map(|x| x.reference.as_str()).collect::<Vec<_>>(), vec!["John 1:51", "John 2:1", "John 2:2"]);
        assert_eq!(label, "John 1:51–2:2");
        assert_eq!(b.lookup("John 3:16-18").unwrap().1.len(), 3);
        assert_eq!(b.lookup("John 3").unwrap().1.len(), 36);
        assert_eq!(b.lookup("John 2-3").unwrap().1.len(), 61);
        assert_eq!(b.lookup("John 3:16, 18").unwrap().1.len(), 2);
    }

    #[test]
    fn lookup_errors_instead_of_looping() {
        let b = sample();
        assert_eq!(b.lookup("Jude 1:1-3:1").unwrap_err(), "Jude 2 does not exist.");
        assert_eq!(b.lookup("Mark 1:1-3:1").unwrap_err(), "Mark 2 does not exist.");
        assert_eq!(b.lookup("John 3:35-40").unwrap_err(), "John 3:40 does not exist.");
        assert_eq!(b.lookup("John 9").unwrap_err(), "John 9 does not exist.");
        assert_eq!(b.lookup("John 9:1").unwrap_err(), "John 9 does not exist.");
        assert!(b.lookup("Tobit 1:1").is_err());
        let mut big = Verses::new();
        for c in 1..=20u16 { for v in 1..=30u16 { big.insert((18, c, v), "x".into()); } }
        let big = ImportedBible { verses: big, ..Default::default() };
        assert!(big.lookup("Ps 1-20").unwrap_err().contains("too long"));
    }

    #[test]
    fn abbreviations_cannot_escape_the_folder() {
        assert_eq!(sanitize_abbr("web"), Some("WEB".into()));
        assert_eq!(sanitize_abbr("../../etc/passwd"), Some("ETCPASSWD".into()));
        assert_eq!(sanitize_abbr("..\\x"), None);
        assert_eq!(sanitize_abbr("C:"), None);
        assert_eq!(sanitize_abbr("con"), None);
        assert_eq!(sanitize_abbr("COM1"), None);
        assert_eq!(sanitize_abbr("ABCDEFGHIJK"), None);
        assert_eq!(sanitize_abbr("Ελληνικά"), None);
    }

    #[test]
    fn a_prefixed_id_keeps_its_case_and_gets_an_encoded_file() {
        let root = crate::store::test_dir();
        let mut b = sample();
        b.abbreviation = "ebible:spaRV1909".into();
        b.name = "Reina Valera 1909".into();
        save(&root, &b).unwrap();
        assert!(root.join("bibles/ebible~spaRV1909.json").exists());
        assert_eq!(load(&root, "ebible:spaRV1909").unwrap().name, "Reina Valera 1909");
        assert!(load(&root, "ebible:sparv1909").is_none(), "ids are case-sensitive");
        let all = load_all(&root);
        assert_eq!(all.iter().map(|b| b.abbreviation.as_str()).collect::<Vec<_>>(), ["ebible:spaRV1909"]);
        assert!(get(&root, "ebible:spaRV1909").is_some());
        remove(&root, "ebible:spaRV1909").unwrap();
        assert!(!root.join("bibles/ebible~spaRV1909.json").exists());
        assert!(get(&root, "ebible:spaRV1909").is_none());
    }

    #[test]
    fn a_bad_id_is_refused_and_a_file_must_match_its_name() {
        let root = crate::store::test_dir();
        let mut b = sample();
        b.abbreviation = "EBIBLE:x".into();
        assert!(save(&root, &b).is_err());
        b.abbreviation = "ebible:x".into();
        save(&root, &b).unwrap();
        fs::rename(root.join("bibles/ebible~x.json"), root.join("bibles/ebible~y.json")).unwrap();
        assert!(load_all(&root).is_empty());
    }

    #[test]
    fn an_operators_abbreviation_never_becomes_a_prefixed_id() {
        let root = crate::store::test_dir();
        let file = br#"<XMLBIBLE biblename="Test"><BIBLEBOOK bnumber="43"><CHAPTER cnumber="3"><VERS vnumber="16">For God loved.</VERS></CHAPTER></BIBLEBOOK></XMLBIBLE>"#;
        assert_eq!(import(&root, file, "ab:cd").unwrap().abbreviation, "ABCD");
    }

    #[test]
    fn save_load_remove() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("rp-import-test-{stamp}"));
        let mut b = sample();
        b.abbreviation = "../web".into();
        b.name = "World English Bible".into();
        save(&root, &b).unwrap();
        assert!(root.join("bibles").join("WEB.json").exists());
        fs::write(root.join("bibles").join("junk.json"), b"{}").unwrap();
        let all = load_all(&root);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].abbreviation, "WEB");
        assert_eq!(all[0].verses, b.verses);
        assert_eq!(load(&root, "web").unwrap().name, "World English Bible");
        b.abbreviation = "x".into();
        assert!(save(&root, &b).is_err());
        remove(&root, "WEB").unwrap();
        remove(&root, "WEB").unwrap();
        assert!(load_all(&root).is_empty());
        assert!(remove(&root, "..").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn combine_merges_new_books_and_replaces_repeats() {
        let one = |b: u8| ImportedBible { verses: [((b, 1, 1), "x".to_string())].into(), ..Default::default() };
        let merged = combine(Some(one(0)), one(1));
        assert_eq!(merged.len(), 2);
        // A repeated book replaces only that book: John had 1:1 and 1:2, the new John has only 1:1.
        let mut old = merged.clone();
        old.verses.insert((1, 1, 2), "old".into());
        old.name = "Old".into();
        old.copyright = "C".into();
        let mut new = one(1);
        new.verses.insert((1, 1, 1), "new".into());
        new.name = "New".into();
        let c = combine(Some(old), new);
        assert_eq!(keys(&c), vec![(0, 1, 1), (1, 1, 1)]);
        assert_eq!(c.verses[&(1, 1, 1)], "new");
        assert_eq!((c.name.as_str(), c.copyright.as_str()), ("New", "C"));
        // A whole re-import replaces every book it contains.
        let whole = ImportedBible { verses: [((0, 1, 1), "a".to_string()), ((1, 1, 1), "b".to_string())].into(), ..Default::default() };
        assert_eq!(combine(Some(c), whole.clone()).verses, whole.verses);
        assert_eq!(combine(None, one(3)).len(), 1);
    }
}
