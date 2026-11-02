//! EasyWorship 6/7 song library (`Songs.db` + `SongWords.db`) → Songs. See spec §9.3.
//! Never writes to EasyWorship data: each database is opened read-only and copied into memory
//! with the SQLite backup API (a consistent snapshot that includes any `-wal`), and only the copy is read.
//! The whole library is read into one `ImportReport`; the caller saves all of it in one step or none of it.
//! Table layout comes from the OpenLP/FreeShow importers: `song` (rowid, title, author, copyright,
//! administrator, reference_number, vendor_id …) and `word` (song_id → song.rowid, words = one RTF document).
use crate::{model::Song, slides::section_label};
use rusqlite::{backup::Backup, types::ValueRef, Connection, OpenFlags};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

/// One `song` row joined with its `word` row, as read from the databases.
#[derive(Clone, Debug, Default)]
pub struct EwSong {
    pub title: String,
    pub author: String,
    pub copyright: String,
    pub administrator: String,
    pub reference_number: String,
    pub vendor_id: Option<i64>,
    pub words: Option<String>,
}

#[derive(Debug, Default)]
pub struct ImportReport {
    pub songs: Vec<Song>,
    /// (title, reason) for every row that did not become a song.
    pub skipped: Vec<(String, String)>,
}

/// How an imported song relates to the library (spec §9.2: a duplicate CCLI # asks update or keep both).
#[derive(Debug, Clone, PartialEq)]
pub enum Dedupe { New, DuplicateCcli { existing: Uuid } }

// ---------- RTF → lyrics ----------

/// Paragraph properties and skip state, saved at `{` and restored at `}`.
#[derive(Clone)]
struct Group { skip: bool, font_table: bool, hidden: bool, uc: usize, font: Option<i32> }

enum Event { Line { text: String, hidden: bool }, Break }

// Destinations whose text is never lyrics.
const SKIP_DESTINATIONS: &[&str] = &["colortbl", "stylesheet", "info", "pict", "object", "header", "footer", "headerl", "headerr", "footerl",
    "footerr", "listtable", "listoverridetable", "rsidtbl", "generator", "xmlnstbl", "themedata", "colorschememapping", "latentstyles",
    "datastore", "filetbl", "revtbl", "nonshppict", "shppict"];

struct Rtf {
    group: Group,
    stack: Vec<Group>,
    line: String,
    /// Consecutive `\'xx` bytes, decoded together in the active code page.
    bytes: Vec<u8>,
    high_surrogate: Option<u32>,
    /// `\ucN` fallback characters still to skip after a `\uN`.
    fallback: usize,
    /// After `\sdlistlevel\-1` the `1` would otherwise leak into the label as text.
    eat_level: bool,
    ansi_cp: u32,
    font_cp: HashMap<i32, u32>,
    defining_font: Option<i32>,
    /// EW6/7 `\sd` markup seen: labels and slide breaks are explicit.
    structured: bool,
    events: Vec<Event>,
}

impl Rtf {
    fn new() -> Self {
        Self { group: Group { skip: false, font_table: false, hidden: false, uc: 1, font: None }, stack: vec![], line: String::new(), bytes: vec![],
            high_surrogate: None, fallback: 0, eat_level: false, ansi_cp: 1252, font_cp: HashMap::new(), defining_font: None, structured: false, events: vec![] }
    }

    fn flush_bytes(&mut self) {
        if self.bytes.is_empty() { return; }
        let cp = self.group.font.and_then(|f| self.font_cp.get(&f).copied()).unwrap_or(self.ansi_cp);
        let bytes = std::mem::take(&mut self.bytes);
        self.line.extend(bytes.into_iter().map(|b| decode_byte(b, cp)));
    }

    fn text(&mut self, c: char) {
        if !self.group.skip { self.line.push(c); }
    }

    fn end_line(&mut self) {
        self.flush_bytes();
        if self.group.skip { self.line.clear(); return; }
        self.events.push(Event::Line { text: std::mem::take(&mut self.line), hidden: self.group.hidden });
    }

    fn unicode(&mut self, n: i32) {
        let u = if n < 0 { n + 65536 } else { n } as u32;
        match u {
            0xD800..=0xDBFF => { self.high_surrogate = Some(u); return; }
            0xDC00..=0xDFFF => {
                if let Some(h) = self.high_surrogate.take() {
                    if let Some(c) = char::from_u32(0x10000 + ((h - 0xD800) << 10) + (u - 0xDC00)) { self.text(c); }
                }
                return;
            }
            _ => {}
        }
        self.high_surrogate = None;
        if let Some(c) = char::from_u32(u) { self.text(c); }
    }

    fn word(&mut self, name: &str, param: Option<i32>) {
        self.eat_level = name == "sdlistlevel" && param.is_none();
        if self.group.font_table {
            // Inside the font table only the font → charset map matters.
            match name {
                "f" => self.defining_font = param,
                "fcharset" => if let (Some(f), Some(cp)) = (self.defining_font, param.and_then(charset_cp)) { self.font_cp.insert(f, cp); },
                _ => {}
            }
            return;
        }
        if self.group.skip { return; }
        match name {
            "par" | "line" => self.end_line(),
            "pard" => self.group.hidden = false,
            // EW6/7: a hidden paragraph is the section name ("Verse 1"), not shown on screen.
            "sdparawysiwghidden" => { self.group.hidden = true; self.structured = true; }
            "sdslidemarker" => {
                if !self.line.trim().is_empty() { self.end_line(); }
                self.line.clear();
                self.events.push(Event::Break);
                self.structured = true;
            }
            "u" => if let Some(n) = param { self.unicode(n); self.fallback = self.group.uc; },
            "uc" => self.group.uc = param.unwrap_or(1).max(0) as usize,
            "ansicpg" => if let Some(cp) = param { self.ansi_cp = cp as u32; },
            "f" => self.group.font = param,
            "tab" => self.text(' '),
            "emdash" => self.text('\u{2014}'),
            "endash" => self.text('\u{2013}'),
            "lquote" => self.text('\u{2018}'),
            "rquote" => self.text('\u{2019}'),
            "ldblquote" => self.text('\u{201C}'),
            "rdblquote" => self.text('\u{201D}'),
            "bullet" => self.text('\u{2022}'),
            "fonttbl" => { self.group.skip = true; self.group.font_table = true; }
            d if SKIP_DESTINATIONS.contains(&d) => self.group.skip = true,
            // \sdewparatemplatestyleN, \qc, \plain, \fsN, \sdfsauto … are formatting.
            _ => {}
        }
    }

    fn symbol(&mut self, c: char) {
        if c != '-' { self.eat_level = false; }
        match c {
            '~' => self.text('\u{a0}'),
            '_' => self.text('\u{2011}'),
            '\\' | '{' | '}' => self.text(c),
            '*' => self.group.skip = true,
            '\n' | '\r' => if !self.group.skip { self.end_line(); },
            // `\-` (optional hyphen) and the rest carry nothing for lyrics.
            _ => {}
        }
    }

    fn plain(&mut self, c: char) {
        if self.eat_level && c.is_ascii_digit() { return; }
        self.eat_level = false;
        self.text(c);
    }

    fn parse(mut self, rtf: &str) -> Self {
        let chars: Vec<char> = rtf.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '\\' && chars.get(i + 1) == Some(&'\'') {
                // \'xx: a byte in the active code page; buffered so runs decode together.
                let Some(hex) = chars.get(i + 2..i + 4) else { break };
                i += 4;
                self.eat_level = false;
                if self.fallback > 0 { self.fallback -= 1; continue; }
                if let Ok(b) = u8::from_str_radix(&hex.iter().collect::<String>(), 16) { if !self.group.skip { self.bytes.push(b); } }
                continue;
            }
            self.flush_bytes();
            match c {
                '{' | '}' => {
                    i += 1;
                    self.fallback = 0;
                    self.eat_level = false;
                    if c == '{' { self.stack.push(self.group.clone()); continue; }
                    if let Some(outer) = self.stack.pop() {
                        // A hidden label group closed without \par: end its line before the flag changes.
                        if outer.hidden != self.group.hidden && !self.line.is_empty() { self.end_line(); }
                        self.group = outer;
                    }
                }
                '\\' => {
                    let Some(&n) = chars.get(i + 1) else { break };
                    if n.is_ascii_alphabetic() {
                        let mut j = i + 1;
                        while j < chars.len() && chars[j].is_ascii_alphabetic() { j += 1; }
                        let name: String = chars[i + 1..j].iter().collect();
                        let num_start = j;
                        if j < chars.len() && chars[j] == '-' && chars.get(j + 1).is_some_and(char::is_ascii_digit) { j += 1; }
                        while j < chars.len() && chars[j].is_ascii_digit() { j += 1; }
                        let param = chars[num_start..j].iter().collect::<String>().parse::<i32>().ok();
                        if chars.get(j) == Some(&' ') { j += 1; }
                        i = j;
                        if self.fallback > 0 { self.fallback -= 1; continue; }
                        self.word(&name, param);
                    } else {
                        i += 2;
                        if self.fallback > 0 { self.fallback -= 1; continue; }
                        self.symbol(n);
                    }
                }
                '\r' | '\n' => i += 1,
                _ => {
                    i += 1;
                    if self.fallback > 0 { self.fallback -= 1; continue; }
                    self.plain(c);
                }
            }
        }
        self.flush_bytes();
        if !self.line.is_empty() { self.end_line(); }
        self
    }
}

/// RTF `\fcharsetN` → Windows code page. 0 (ANSI) and unknown charsets use `\ansicpg`.
fn charset_cp(charset: i32) -> Option<u32> {
    match charset { 204 => Some(1251), 238 => Some(1250), 161 => Some(1253), 162 => Some(1254), 186 => Some(1257), _ => None }
}

/// One byte → char. cp1252 and cp1251 are mapped exactly; other code pages fall back to cp1252.
fn decode_byte(b: u8, cp: u32) -> char {
    const CP1252: [u16; 32] = [0x20AC, 0xFFFD, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0xFFFD, 0x017D, 0xFFFD,
        0xFFFD, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0xFFFD, 0x017E, 0x0178];
    const CP1251: [u16; 64] = [0x0402, 0x0403, 0x201A, 0x0453, 0x201E, 0x2026, 0x2020, 0x2021, 0x20AC, 0x2030, 0x0409, 0x2039, 0x040A, 0x040C, 0x040B, 0x040F,
        0x0452, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0xFFFD, 0x2122, 0x0459, 0x203A, 0x045A, 0x045C, 0x045B, 0x045F,
        0x00A0, 0x040E, 0x045E, 0x0408, 0x00A4, 0x0490, 0x00A6, 0x00A7, 0x0401, 0x00A9, 0x0404, 0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x0407,
        0x00B0, 0x00B1, 0x0406, 0x0456, 0x0491, 0x00B5, 0x00B6, 0x00B7, 0x0451, 0x2116, 0x0454, 0x00BB, 0x0458, 0x0405, 0x0455, 0x0457];
    let u = match (cp, b) {
        (_, 0..=0x7F) => b as u32,
        (1251, 0x80..=0xBF) => CP1251[(b - 0x80) as usize] as u32,
        (1251, _) => 0x0410 + (b - 0xC0) as u32,
        (_, 0x80..=0x9F) => CP1252[(b - 0x80) as usize] as u32,
        _ => b as u32,
    };
    char::from_u32(u).unwrap_or('\u{FFFD}')
}

/// Label line in the app's lyric format: as-is when `section_label` knows it, else `[bracketed]`.
fn label_line(label: &str) -> String {
    let l: String = label.trim_matches(['[', ']']).trim().chars().take(38).collect();
    if section_label(&l).is_some() { l } else { format!("[{l}]") }
}

/// Plain (EW2009-style) slide heading: "Verse 1", "Chorus (x2)", "Verse1" — not "Verse after verse".
fn plain_label(line: &str) -> bool {
    let base = match line.rfind('(') { Some(p) if line.ends_with(')') => line[..p].trim(), _ => line };
    if section_label(line).is_some() || section_label(base).is_some() { return true; }
    let lower = base.to_ascii_lowercase();
    ["pre-chorus", "chorus", "verse", "bridge", "tag", "ending", "intro"].iter().any(|w| lower.strip_prefix(w)
        .map(str::trim).is_some_and(|rest| rest.len() <= 3 && rest.chars().all(|c| c.is_ascii_alphanumeric())))
}

/// EasyWorship `words` (RTF, or plain text) → lyrics with section labels on their own lines and a blank
/// line between slides, the same shape SongSelect import produces.
pub fn rtf_to_lyrics(rtf: &str) -> String {
    let (events, structured) = if rtf.trim_start().starts_with("{\\rtf") {
        let p = Rtf::new().parse(rtf);
        (p.events, p.structured)
    } else {
        (rtf.replace("\r\n", "\n").replace('\r', "\n").lines().map(|l| Event::Line { text: l.to_string(), hidden: false }).collect(), false)
    };
    // Slides as (label, lines).
    let mut slides: Vec<(Option<String>, Vec<String>)> = vec![(None, vec![])];
    for e in events {
        let cur = slides.last_mut().expect("never empty");
        let started = cur.0.is_some() || !cur.1.is_empty();
        match e {
            Event::Break => if started { slides.push((None, vec![])); },
            Event::Line { text, hidden } => {
                let t = text.replace('\t', " ").trim().to_string();
                if t.is_empty() {
                    // Plain layout: a blank line starts the next slide. Structured: spacing only.
                    if !structured && started { slides.push((None, vec![])); }
                } else if hidden || (!structured && !started && plain_label(&t)) {
                    cur.0 = Some(t);
                } else {
                    cur.1.push(t);
                }
            }
        }
    }
    let mut out: Vec<String> = Vec::new();
    let mut prev_label: Option<String> = None;
    for (label, lines) in slides {
        if lines.is_empty() { continue; }
        if !out.is_empty() { out.push(String::new()); }
        // EW labels every slide; a repeated label just continues the section.
        if let Some(l) = label { if prev_label.as_deref() != Some(l.as_str()) { out.push(label_line(&l)); prev_label = Some(l); } }
        out.extend(lines);
    }
    out.join("\n")
}

// ---------- rows → Song ----------

/// "A / B", "A; B", "A, B, and C" → "A | B | C" (the SongSelect writers style).
/// Like OpenLP: try `/`, then `;`, then `,`; the first that gives two or more names wins.
pub fn split_writers(author: &str) -> String {
    let t = author.trim();
    let mut parts = vec![t];
    for sep in ['/', ';', ','] {
        let p: Vec<&str> = t.split(sep).map(str::trim).filter(|s| !s.is_empty()).collect();
        if p.len() >= 2 { parts = p; break; }
    }
    parts.into_iter().map(|p| {
        let lower = p.to_ascii_lowercase();
        if lower.starts_with("and ") { p[4..].trim() } else if lower.starts_with("& ") { p[2..].trim() } else { p }
    }).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" | ")
}

/// CCLI song #: `reference_number` when it is all digits (FreeShow), else `vendor_id` (OpenLP).
pub fn ccli_number(reference: &str, vendor: Option<i64>) -> String {
    let r = reference.trim();
    if !r.is_empty() && r.chars().all(|c| c.is_ascii_digit()) && !r.trim_start_matches('0').is_empty() { return r.to_string(); }
    vendor.filter(|&v| v > 0).map(|v| v.to_string()).unwrap_or_default()
}

/// Err is the reason the row is skipped.
pub fn to_song(row: &EwSong) -> Result<Song, String> {
    let words = row.words.as_deref().filter(|w| !w.trim().is_empty()).ok_or("no lyrics (probably a video or announcement item)")?;
    let lyrics = rtf_to_lyrics(words);
    if lyrics.trim().is_empty() { return Err("the lyrics are empty".into()); }
    let admin = row.administrator.trim();
    let mut copyright = row.copyright.trim().to_string();
    if !admin.is_empty() {
        if !copyright.is_empty() { copyright.push_str(", "); }
        copyright.push_str(&format!("Administered by {admin}"));
    }
    Ok(Song { id: Uuid::new_v4(), title: display_title(row), writers: split_writers(&row.author), ccli: ccli_number(&row.reference_number, row.vendor_id),
        copyright, lyrics, arrangement: vec![] })
}

fn display_title(row: &EwSong) -> String {
    let t = row.title.trim();
    if t.is_empty() { "Untitled song".into() } else { t.to_string() }
}

// ---------- duplicates ----------

/// One entry per imported song. Matches the library first, then earlier songs in the same import.
/// Songs without a CCLI # are always New.
pub fn classify(existing: &[Song], imported: &[Song]) -> Vec<Dedupe> {
    let mut seen: HashMap<String, Uuid> = HashMap::new();
    for s in existing { if !s.ccli.trim().is_empty() { seen.entry(s.ccli.trim().to_string()).or_insert(s.id); } }
    imported.iter().map(|s| {
        let c = s.ccli.trim();
        if c.is_empty() { return Dedupe::New; }
        match seen.get(c) {
            Some(&existing) => Dedupe::DuplicateCcli { existing },
            None => { seen.insert(c.to_string(), s.id); Dedupe::New }
        }
    }).collect()
}

// ---------- reader ----------

/// Reads a whole EW6/7 library. `folder` may be the profile folder, its `Databases\Data`, or Songs.db itself.
pub fn read_library(folder: &Path) -> Result<ImportReport, String> {
    let (songs_path, words_path) = find_databases(folder)?;
    let songs_db = snapshot(&songs_path)?;
    let words_db = snapshot(&words_path)?;
    let rows = read_rows(&songs_db, &words_db)?;
    let mut report = ImportReport::default();
    for row in &rows {
        match to_song(row) { Ok(s) => report.songs.push(s), Err(why) => report.skipped.push((display_title(row), why)) }
    }
    Ok(report)
}

/// Same search as OpenLP: `<root>/Databases/Data`, `<root>/Data`, `<root>`. File names match case-insensitively.
fn find_databases(path: &Path) -> Result<(PathBuf, PathBuf), String> {
    let root = if path.is_file() { path.parent().unwrap_or(path).to_path_buf() } else { path.to_path_buf() };
    for dir in [root.join("Databases").join("Data"), root.join("Data"), root.clone()] {
        let Some(songs) = find_file(&dir, "Songs.db") else { continue };
        if !is_sqlite(&songs) {
            return Err("This looks like an EasyWorship 2009 (or older) library. Only EasyWorship 6 and 7 libraries can be imported.".into());
        }
        let words = find_file(&dir, "SongWords.db").ok_or_else(|| format!("Found Songs.db but not SongWords.db next to it in {}.", dir.display()))?;
        return Ok((songs, words));
    }
    Err("No EasyWorship Songs.db was found here. Pick the profile's Databases\\Data folder \
        (usually C:\\Users\\Public\\Documents\\Softouch\\Easyworship\\Default\\v6.1\\Databases\\Data).".into())
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path())
        .find(|p| p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(name)))
}

fn is_sqlite(path: &Path) -> bool {
    let mut head = [0u8; 16];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut head)).is_ok() && &head == b"SQLite format 3\0"
}

/// A consistent in-memory copy. First choice: read-only connection + backup API (includes committed WAL
/// pages even while EasyWorship is open). Fallback: copy the file and its `-wal` into a private temp folder
/// and back up from that. The `-shm` index is not copied: SQLite rebuilds it from the `-wal`, and a copy
/// taken mid-write could be stale.
fn snapshot(path: &Path) -> Result<Connection, String> {
    let first = match backup_into_memory(path, OpenFlags::SQLITE_OPEN_READ_ONLY) { Ok(c) => return Ok(c), Err(e) => e };
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let tmp = std::env::temp_dir().join(format!("rp-ew-{}", Uuid::new_v4()));
    let copied = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&tmp)?;
        std::fs::copy(path, tmp.join(&name))?;
        let wal = path.with_file_name(format!("{name}-wal"));
        if wal.exists() { std::fs::copy(&wal, tmp.join(format!("{name}-wal")))?; }
        Ok(())
    })();
    let result = match copied {
        Ok(()) => backup_into_memory(&tmp.join(&name), OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    std::fs::remove_dir_all(&tmp).ok();
    result.map_err(|e| format!("Couldn't open {} ({first}; copying it also failed: {e}).", path.display()))
}

fn backup_into_memory(path: &Path, mode: OpenFlags) -> rusqlite::Result<Connection> {
    let src = Connection::open_with_flags(path, mode | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    src.busy_timeout(Duration::from_secs(5))?;
    let mut dst = Connection::open_in_memory()?;
    Backup::new(&src, &mut dst)?.run_to_completion(i32::MAX, Duration::ZERO, None)?;
    Ok(dst)
}

fn value_text(v: ValueRef) -> String {
    match v {
        ValueRef::Null => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(b) | ValueRef::Blob(b) => String::from_utf8_lossy(b).into_owned(),
    }
}

fn value_int(v: ValueRef) -> Option<i64> {
    match v { ValueRef::Integer(i) => Some(i), ValueRef::Text(b) => std::str::from_utf8(b).ok()?.trim().parse().ok(), _ => None }
}

fn columns(db: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    db.prepare(&format!("PRAGMA table_info({table})"))?.query_map([], |r| r.get::<_, String>(1))?.collect()
}

/// Builds the SELECT from the columns that exist: EasyWorship builds differ.
fn read_rows(songs: &Connection, words: &Connection) -> Result<Vec<EwSong>, String> {
    let err = |e: rusqlite::Error| format!("Couldn't read the EasyWorship library: {e}");
    let cols = columns(songs, "song").map_err(err)?;
    if !cols.iter().any(|c| c.eq_ignore_ascii_case("title")) { return Err("Songs.db has no song table. Is this an EasyWorship library?".into()); }
    let pick = |c: &str| if cols.iter().any(|x| x.eq_ignore_ascii_case(c)) { format!("\"{c}\"") } else { "NULL".into() };
    let sql = format!("SELECT rowid, {}, {}, {}, {}, {}, {} FROM song ORDER BY rowid",
        pick("title"), pick("author"), pick("copyright"), pick("administrator"), pick("reference_number"), pick("vendor_id"));

    let wcols = columns(words, "word").map_err(err)?;
    if !["song_id", "words"].iter().all(|c| wcols.iter().any(|x| x.eq_ignore_ascii_case(c))) {
        return Err("SongWords.db has no word table. Is this an EasyWorship library?".into());
    }
    let mut lyrics: HashMap<i64, String> = HashMap::new();
    let mut stmt = words.prepare("SELECT song_id, words FROM word ORDER BY rowid").map_err(err)?;
    let mut q = stmt.query([]).map_err(err)?;
    while let Some(r) = q.next().map_err(err)? {
        let Some(id) = value_int(r.get_ref(0).map_err(err)?) else { continue };
        let text = value_text(r.get_ref(1).map_err(err)?);
        let slot = lyrics.entry(id).or_default();
        if slot.trim().is_empty() { *slot = text; }
    }

    let mut out = Vec::new();
    let mut stmt = songs.prepare(&sql).map_err(err)?;
    let mut q = stmt.query([]).map_err(err)?;
    while let Some(r) = q.next().map_err(err)? {
        let t = |i: usize| r.get_ref(i).map(value_text).map_err(err);
        let rowid: i64 = r.get(0).map_err(err)?;
        out.push(EwSong { title: t(1)?, author: t(2)?, copyright: t(3)?, administrator: t(4)?, reference_number: t(5)?,
            vendor_id: value_int(r.get_ref(6).map_err(err)?), words: lyrics.remove(&rowid) });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slides::parse_sections;
    use rusqlite::Connection;
    use std::path::PathBuf;

    // EW6/7 "structured" RTF: hidden label paragraphs, slide markers, the `\sdlistlevel\-1` trap.
    const STRUCTURED: &str = r"{\rtf1\ansi\deff0\sdeasyworship2
{\fonttbl{\f0 Tahoma;}}{\colortbl ;}
{\pard\sdparawysiwghidden\sdlistlevel\-1\qc\qdef\sdewparatemplatestyle102{\*\sdasfactor 1}{\*\sdasbaseline 72.9}\sdastextstyle102\plain\sdewtemplatestyle102\fs146{\*\sdfsreal 72.9}{\*\sdfsdef 72.9}\sdfsauto Verse 1\par}
{\pard\sdlistlevel0\qc\qdef\sdewparatemplatestyle101{\*\sdasfactor 1}{\*\sdasbaseline 72.9}\sdastextstyle101\plain
\sdewtemplatestyle101\fs146{\*\sdfsreal 72.9}{\*\sdfsdef 72.9}\sdfsauto First line here\par}
{\pard\sdlistlevel0\qc\qdef\sdewparatemplatestyle101\plain\sdfsauto Second line\par}
{\pard\sdslidemarker\qc\qdef\sdewparatemplatestyle101\plain}
{\pard\sdparawysiwghidden\sdlistlevel\-1\qc\sdewparatemplatestyle102\plain\sdfsauto Verse 1\par}
{\pard\sdlistlevel0\qc\plain\sdfsauto Third line\par}
{\pard\sdslidemarker\qc\qdef\sdewparatemplatestyle101\plain\sdfsauto Fourth line\par}
{\pard\sdslidemarker\qc\plain}
{\pard\sdparawysiwghidden\sdlistlevel\-1\qc\sdewparatemplatestyle102\plain Chorus\par}
{\pard\sdlistlevel0\qc\plain Chorus line\par}
{\pard\sdslidemarker\qc\plain}
{\pard\sdparawysiwghidden\sdlistlevel\-1\qc\plain Slide1Label\par}
{\pard\sdlistlevel0\qc\plain Odd label line\par}
}";

    // Songs carried over from EW2009 / bundled hymns: no \sd marks, blank lines between slides.
    const PLAIN: &str = r"{\rtf1\ansi\deff0{\fonttbl{\f0 Tahoma;}}\f0\fs20 Verse 1\par First verse line\par Second verse line\par\par Chorus (x2)\par Chorus line\par\par\par Pre-Chorus 2\par Pre line\par \par More pre line\par\par Bridge\par\par Tag\par Tag line\line Same slide\par}";

    const ENCODINGS: &str = r"{\rtf1\ansi\ansicpg1252\deff0{\fonttbl{\f0 Tahoma;}{\f1\fcharset204 Arial;}}It\u8217?s caf\'e9 \u-10179?\u-8704?\par \uc1 na\u239\'efve\par Nbsp\~here a\_b \{brace\} back\\slash\par\f1\'cf\'f0\'e8\'e2\'e5\'f2\par{\*\generator Riched20;}\emdash\tab x\par}";

    const CYRILLIC: &str = r"{\rtf1\ansi\ansicpg1251\deff0{\fonttbl{\f0\fcharset0 Arial;}}\'cf\'f0\'e8\'e2\'e5\'f2\par}";

    #[test]
    fn structured_rtf_uses_hidden_labels_and_slide_markers() {
        assert_eq!(rtf_to_lyrics(STRUCTURED),
            "Verse 1\nFirst line here\nSecond line\n\nThird line\n\nFourth line\n\nChorus\nChorus line\n\n[Slide1Label]\nOdd label line");
    }

    #[test]
    fn listlevel_minus_one_does_not_leak_into_labels() {
        let out = rtf_to_lyrics(STRUCTURED);
        assert!(!out.contains('\u{ad}'));
        assert!(!out.contains("1Verse") && !out.contains("1Chorus"));
    }

    #[test]
    fn structured_output_parses_into_app_sections() {
        let secs = parse_sections(&rtf_to_lyrics(STRUCTURED));
        let labels: Vec<_> = secs.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["Verse 1", "Chorus", "Slide1Label"]);
        assert_eq!(secs[0].groups, vec![vec!["First line here".to_string(), "Second line".into()], vec!["Third line".into()], vec!["Fourth line".into()]]);
    }

    #[test]
    fn plain_rtf_splits_on_blank_lines_and_finds_labels() {
        assert_eq!(rtf_to_lyrics(PLAIN),
            "Verse 1\nFirst verse line\nSecond verse line\n\n[Chorus (x2)]\nChorus line\n\nPre-Chorus 2\nPre line\n\nMore pre line\n\nTag\nTag line\nSame slide");
        let labels: Vec<_> = parse_sections(&rtf_to_lyrics(PLAIN)).into_iter().map(|s| s.label).collect();
        assert_eq!(labels, ["Verse 1", "Chorus (x2)", "Pre-Chorus 2", "Tag"]);
    }

    #[test]
    fn plain_lyric_that_starts_like_a_label_stays_a_lyric() {
        assert_eq!(rtf_to_lyrics(r"{\rtf1\ansi Verse after verse I sing\par Second\par}"), "Verse after verse I sing\nSecond");
    }

    #[test]
    fn non_rtf_words_are_read_as_plain_text() {
        assert_eq!(rtf_to_lyrics("Verse 1\r\nLine a\r\n\r\nChorus\r\nLine b"), "Verse 1\nLine a\n\nChorus\nLine b");
    }

    #[test]
    fn unicode_escapes_code_pages_and_symbols() {
        assert_eq!(rtf_to_lyrics(ENCODINGS), "It\u{2019}s caf\u{e9} \u{1F600}\nna\u{ef}ve\nNbsp\u{a0}here a\u{2011}b {brace} back\\slash\n\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}\n\u{2014} x");
        assert_eq!(rtf_to_lyrics(CYRILLIC), "\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}");
    }

    #[test]
    fn empty_rtf_gives_empty_lyrics() {
        assert_eq!(rtf_to_lyrics(r"{\rtf1\ansi{\fonttbl{\f0 Tahoma;}}\par\par {\pard\sdslidemarker\plain}\par}"), "");
    }

    #[test]
    fn writers_split_like_openlp() {
        assert_eq!(split_writers("Ann Lee / Bob Ray"), "Ann Lee | Bob Ray");
        assert_eq!(split_writers("Ann Lee; Bob Ray"), "Ann Lee | Bob Ray");
        assert_eq!(split_writers("Ann Lee, Bob Ray, and Cy Day"), "Ann Lee | Bob Ray | Cy Day");
        assert_eq!(split_writers("Ann Lee & Bob Ray"), "Ann Lee & Bob Ray");
        assert_eq!(split_writers("  "), "");
    }

    #[test]
    fn ccli_from_reference_number_then_vendor_id() {
        assert_eq!(ccli_number(" 12345 ", Some(999)), "12345");
        assert_eq!(ccli_number("", Some(777)), "777");
        assert_eq!(ccli_number("Hymn 12", Some(777)), "777");
        assert_eq!(ccli_number("", Some(0)), "");
        assert_eq!(ccli_number("0", None), "");
        assert_eq!(ccli_number("", None), "");
    }

    #[test]
    fn row_maps_to_song_with_administrator_in_copyright() {
        let row = EwSong { title: " Test Song ".into(), author: "A / B".into(), copyright: "© 2020 Test Music".into(), administrator: "Admin Co".into(),
            reference_number: "12345".into(), vendor_id: None, words: Some(PLAIN.into()) };
        let s = to_song(&row).unwrap();
        assert_eq!(s.title, "Test Song");
        assert_eq!(s.writers, "A | B");
        assert_eq!(s.copyright, "© 2020 Test Music, Administered by Admin Co");
        assert_eq!(s.ccli, "12345");
        assert!(s.lyrics.starts_with("Verse 1\nFirst verse line"));
        assert!(s.arrangement.is_empty());
        let only_admin = EwSong { administrator: "Admin Co".into(), copyright: String::new(), ..row.clone() };
        assert_eq!(to_song(&only_admin).unwrap().copyright, "Administered by Admin Co");
    }

    #[test]
    fn rows_without_lyrics_are_skipped_with_a_reason() {
        let none = EwSong { title: "Video".into(), ..Default::default() };
        assert!(to_song(&none).unwrap_err().contains("no lyrics"));
        let blank = EwSong { title: "Blank".into(), words: Some(r"{\rtf1\ansi\par}".into()), ..Default::default() };
        assert!(to_song(&blank).unwrap_err().contains("empty"));
    }

    #[test]
    fn duplicates_by_ccli_against_library_and_within_import() {
        let song = |t: &str, c: &str| Song { id: Uuid::new_v4(), title: t.into(), ccli: c.into(), lyrics: "x".into(), ..Default::default() };
        let existing = vec![song("Old", "111")];
        let imported = vec![song("A", "111"), song("B", "222"), song("B again", "222"), song("No number", ""), song("No number", "")];
        let got = classify(&existing, &imported);
        assert_eq!(got, vec![Dedupe::DuplicateCcli { existing: existing[0].id }, Dedupe::New, Dedupe::DuplicateCcli { existing: imported[1].id }, Dedupe::New, Dedupe::New]);
    }

    // ---------- reader, against SQLite fixtures shaped like EW6/7 ----------

    fn temp_folder() -> PathBuf {
        let d = std::env::temp_dir().join(format!("rp-ew-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const SONG_TABLE: &str = "CREATE TABLE song (title TEXT, author TEXT, copyright TEXT, administrator TEXT, description TEXT, tags TEXT, reference_number TEXT, vendor_id INTEGER, song_uid TEXT)";
    const WORD_TABLE: &str = "CREATE TABLE word (song_id INTEGER, words TEXT, slide_uids TEXT)";

    /// Writes Songs.db + SongWords.db. Each song: (title, author, reference_number, vendor_id, words).
    fn write_library(dir: &Path, songs_name: &str, songs: &[(&str, &str, &str, Option<i64>, Option<&str>)]) {
        let s = Connection::open(dir.join(songs_name)).unwrap();
        s.execute_batch(SONG_TABLE).unwrap();
        let w = Connection::open(dir.join("SongWords.db")).unwrap();
        w.execute_batch(WORD_TABLE).unwrap();
        for (title, author, refno, vendor, words) in songs {
            s.execute("INSERT INTO song (title, author, copyright, administrator, reference_number, vendor_id) VALUES (?1, ?2, '© 2020 Test Music', 'Admin Co', ?3, ?4)",
                rusqlite::params![title, author, refno, vendor]).unwrap();
            if let Some(words) = words { w.execute("INSERT INTO word (song_id, words) VALUES (?1, ?2)", rusqlite::params![s.last_insert_rowid(), words]).unwrap(); }
        }
    }

    #[test]
    fn reader_finds_databases_reads_songs_and_reports_skips() {
        let root = temp_folder();
        let data = root.join("Databases").join("Data");
        std::fs::create_dir_all(&data).unwrap();
        write_library(&data, "Songs.DB", &[
            ("Structured", "A / B", "12345", None, Some(STRUCTURED)),
            ("Video Item", "", "", None, None),
            ("Blank", "", "", None, Some(r"{\rtf1\ansi\par\par}")),
            ("Plain", "C; D", "", Some(777), Some(PLAIN)),
        ]);
        let before: Vec<_> = ["Songs.DB", "SongWords.db"].iter().map(|n| std::fs::read(data.join(n)).unwrap()).collect();
        let report = read_library(&root).unwrap();
        assert_eq!(report.songs.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Structured", "Plain"]);
        assert_eq!(report.songs[0].ccli, "12345");
        assert_eq!(report.songs[0].writers, "A | B");
        assert_eq!(report.songs[0].copyright, "© 2020 Test Music, Administered by Admin Co");
        assert_eq!(report.songs[1].ccli, "777");
        assert_eq!(report.skipped.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["Video Item", "Blank"]);
        assert!(report.skipped[0].1.contains("no lyrics"));
        // Never write to EasyWorship data.
        let after: Vec<_> = ["Songs.DB", "SongWords.db"].iter().map(|n| std::fs::read(data.join(n)).unwrap()).collect();
        assert_eq!(before, after);
        assert!(!data.join("Songs.DB-journal").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn reader_sees_uncheckpointed_wal_while_easyworship_is_open() {
        let dir = temp_folder();
        write_library(&dir, "Songs.db", &[("First", "", "1", None, Some(PLAIN))]);
        // EasyWorship still running: WAL mode, an open writer, nothing checkpointed.
        let ew = Connection::open(dir.join("Songs.db")).unwrap();
        ew.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;").unwrap();
        ew.execute("INSERT INTO song (title, reference_number) VALUES ('Second', '2')", []).unwrap();
        let words = Connection::open(dir.join("SongWords.db")).unwrap();
        words.execute("INSERT INTO word (song_id, words) VALUES (?1, ?2)", rusqlite::params![ew.last_insert_rowid(), PLAIN]).unwrap();
        assert!(dir.join("Songs.db-wal").exists());
        let report = read_library(&dir).unwrap();
        assert_eq!(report.songs.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["First", "Second"]);
        drop((ew, words));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reader_copes_with_missing_columns() {
        let dir = temp_folder();
        let s = Connection::open(dir.join("Songs.db")).unwrap();
        s.execute_batch("CREATE TABLE song (title TEXT, author TEXT, vendor_id INTEGER); INSERT INTO song VALUES ('Old Build', 'X', 4242);").unwrap();
        let w = Connection::open(dir.join("SongWords.db")).unwrap();
        w.execute_batch(WORD_TABLE).unwrap();
        w.execute("INSERT INTO word (song_id, words) VALUES (1, ?1)", [PLAIN]).unwrap();
        drop((s, w));
        let report = read_library(&dir.join("Songs.db")).unwrap();
        assert_eq!(report.songs.len(), 1);
        assert_eq!(report.songs[0].ccli, "4242");
        assert_eq!(report.songs[0].copyright, "");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reader_errors_are_clear() {
        let dir = temp_folder();
        assert!(read_library(&dir).unwrap_err().contains("Songs.db"));
        Connection::open(dir.join("Songs.db")).unwrap().execute_batch(SONG_TABLE).unwrap();
        assert!(read_library(&dir).unwrap_err().contains("SongWords.db"));
        // EasyWorship 2009 keeps a Paradox table under the same name.
        let old = temp_folder();
        std::fs::write(old.join("Songs.DB"), b"\x00\x08\x00\x00 not sqlite at all").unwrap();
        std::fs::write(old.join("SongWords.db"), b"").unwrap();
        assert!(read_library(&old).unwrap_err().contains("2009"));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&old).ok();
    }
}
