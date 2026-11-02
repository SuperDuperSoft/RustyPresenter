//! EasyWorship 6/7 song library (`Songs.db` + `SongWords.db`) → Songs. See spec §9.3.
//! Never writes to EasyWorship data, and never opens it with SQLite: the files (`.db`, `-wal`, `-journal`)
//! of both databases are read as bytes until two reads in a row match (a quiescent copy, not one torn by a
//! save in progress), written to a private temp folder, and SQLite reads only that copy.
//! The whole library is read into one `ImportReport`; the caller saves all of it in one step or none of it.
//! Table layout comes from the OpenLP/FreeShow importers: `song` (rowid, title, author, copyright,
//! administrator, reference_number, vendor_id …) and `word` (song_id → song.rowid, words = one RTF document).
use super::common::{check_sqlite, columns, find_file, label_line, section_name, snapshot, value_int, value_text};
pub use super::common::{classify, Dedupe, ImportReport};
use crate::{model::Song, slides::section_label};
use encoding_rs::Encoding;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
    /// Consecutive `\'xx` bytes, decoded together in the active code page (two-byte code pages need the pair).
    bytes: Vec<u8>,
    high_surrogate: Option<u32>,
    /// `\ucN` fallback characters still to skip after a `\uN`.
    fallback: usize,
    /// After `\sdlistlevel\-1` the `1` would otherwise leak into the label as text.
    eat_level: bool,
    ansi_cp: u32,
    /// `\deffN`: the font used where no `\fN` is given.
    deff: Option<i32>,
    font_cp: HashMap<i32, u32>,
    defining_font: Option<i32>,
    /// EW6/7 `\sd` markup seen: labels and slide breaks are explicit.
    structured: bool,
    events: Vec<Event>,
}

impl Rtf {
    fn new() -> Self {
        Self { group: Group { skip: false, font_table: false, hidden: false, uc: 1, font: None }, stack: vec![], line: String::new(), bytes: vec![],
            high_surrogate: None, fallback: 0, eat_level: false, ansi_cp: 1252, deff: None, font_cp: HashMap::new(), defining_font: None,
            structured: false, events: vec![] }
    }

    fn encoding(&self) -> &'static Encoding {
        code_page(self.group.font.or(self.deff).and_then(|f| self.font_cp.get(&f).copied()).unwrap_or(self.ansi_cp))
    }

    fn flush_bytes(&mut self) {
        if self.bytes.is_empty() { return; }
        let bytes = std::mem::take(&mut self.bytes);
        self.line.push_str(&self.encoding().decode_without_bom_handling(&bytes).0);
    }

    /// The buffered bytes end with the first half of a two-byte character. Writers put a trail byte in the
    /// ASCII range as a plain letter (`\'83A` is Shift-JIS "ア"), so the next character belongs to it.
    fn awaiting_trail(&self) -> bool {
        if self.bytes.is_empty() { return false; }
        let enc = self.encoding();
        let lead: fn(u8) -> bool = if enc == encoding_rs::SHIFT_JIS { |b| matches!(b, 0x81..=0x9F | 0xE0..=0xFC) }
            else if [encoding_rs::GBK, encoding_rs::EUC_KR, encoding_rs::BIG5].contains(&enc) { |b| matches!(b, 0x81..=0xFE) }
            else { return false };
        let mut i = 0;
        while i < self.bytes.len() { i += if lead(self.bytes[i]) { 2 } else { 1 }; }
        i > self.bytes.len()
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
            // Inside the font table only the font → code page map matters.
            match name {
                "f" => self.defining_font = param,
                "fcharset" => if let (Some(f), Some(cp)) = (self.defining_font, param.and_then(charset_cp)) { self.font_cp.insert(f, cp); },
                "cpg" => if let (Some(f), Some(cp)) = (self.defining_font, param) { self.font_cp.insert(f, cp as u32); },
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
            "deff" => self.deff = param,
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
            if self.fallback == 0 && c.is_ascii_graphic() && !matches!(c, '\\' | '{' | '}') && self.awaiting_trail() {
                self.bytes.push(c as u8);
                i += 1;
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

/// RTF `\fcharsetN` → Windows code page. 0 (ANSI), 1 (default) and unknown charsets use `\ansicpg`.
fn charset_cp(charset: i32) -> Option<u32> {
    Some(match charset {
        77 => 10000, 128 => 932, 129 => 949, 134 => 936, 136 => 950, 161 => 1253, 162 => 1254, 163 => 1258, 177 => 1255,
        178 => 1256, 186 => 1257, 204 => 1251, 222 => 874, 238 => 1250,
        _ => return None,
    })
}

/// Windows code page → decoder. Anything unknown reads as cp1252, the EasyWorship default.
fn code_page(cp: u32) -> &'static Encoding {
    use encoding_rs::*;
    match cp {
        874 => WINDOWS_874, 932 => SHIFT_JIS, 936 => GBK, 949 => EUC_KR, 950 => BIG5, 1250 => WINDOWS_1250, 1251 => WINDOWS_1251,
        1253 => WINDOWS_1253, 1254 => WINDOWS_1254, 1255 => WINDOWS_1255, 1256 => WINDOWS_1256, 1257 => WINDOWS_1257, 1258 => WINDOWS_1258,
        10000 => MACINTOSH, 20866 => KOI8_R, 65001 => UTF_8,
        _ => WINDOWS_1252,
    }
}

/// "[Chorus (x2)]" → "Chorus": brackets and a trailing repeat note go, like OpenLP.
fn clean_label(label: &str) -> String {
    let l = label.trim().trim_matches(['[', ']']).trim();
    let l = match l.rfind('(') { Some(p) if p > 0 && l.ends_with(')') => l[..p].trim(), _ => l };
    l.to_string()
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
    words_to_lyrics(rtf).0
}

/// The text an RTF document shows, one entry per paragraph or line break; hidden text is left out. For
/// importers that keep one slide's words as RTF (ProPresenter), where there are no labels to find.
pub(crate) fn rtf_text(rtf: &str) -> Vec<String> {
    Rtf::new().parse(rtf).events.into_iter()
        .filter_map(|e| match e { Event::Line { text, hidden: false } => Some(text), _ => None }).collect()
}

/// Lyrics plus the arrangement: empty unless a section comes back later (Chorus, Verse, Chorus).
fn words_to_lyrics(rtf: &str) -> (String, Vec<String>) {
    let (events, structured) = if rtf.trim_start().starts_with("{\\rtf") {
        let p = Rtf::new().parse(rtf);
        (p.events, p.structured)
    } else {
        (rtf.replace("\r\n", "\n").replace('\r', "\n").lines().map(|l| Event::Line { text: l.to_string(), hidden: false }).collect(), false)
    };
    // Slides as (label, lines).
    let mut slides: Vec<(Option<String>, Vec<String>)> = vec![(None, vec![])];
    for e in events {
        let (has_label, has_lines) = slides.last().map(|s| (s.0.is_some(), !s.1.is_empty())).expect("never empty");
        let started = has_label || has_lines;
        let cur = slides.len() - 1;
        match e {
            Event::Break => if started { slides.push((None, vec![])); },
            Event::Line { text, hidden } => {
                let t = text.replace('\t', " ").trim().to_string();
                if t.is_empty() {
                    // Plain layout: a blank line starts the next slide. Structured: spacing only.
                    if !structured && started { slides.push((None, vec![])); }
                } else if hidden && has_lines {
                    // A label after lyric lines with no slide marker between: it names the next slide, not these lines.
                    slides.push((Some(t), vec![]));
                } else if hidden || (!structured && !started && plain_label(&t)) {
                    slides[cur].0 = Some(t);
                } else {
                    slides[cur].1.push(t);
                }
            }
        }
    }
    // Sections as (label, slide groups). EW labels every slide; a repeated label just continues the section,
    // and an unlabeled slide continues the one before. A label-only slide has nothing to show, so its label
    // passes to the next slide when that one has none.
    let mut sections: Vec<(Option<String>, Vec<Vec<String>>)> = Vec::new();
    let mut pending: Option<String> = None;
    for (label, lines) in slides {
        if lines.is_empty() { if label.is_some() { pending = label; } continue; }
        let label = label.or(pending.take()).map(|l| clean_label(&l)).filter(|l| !l.is_empty());
        pending = None;
        match sections.last_mut() {
            Some(s) if label.is_none() || s.0 == label => s.1.push(lines),
            _ => sections.push((label, vec![lines])),
        }
    }
    // The app finds sections by label, so a label used again later (Chorus, Verse 2, Chorus) can't be a second
    // section: the same words become an arrangement entry, different words get their own name ("Verse 1 (2)").
    let mut kept: Vec<(Option<String>, Vec<Vec<String>>)> = Vec::new();
    let mut order: Vec<String> = Vec::new();
    let mut repeats = false;
    for (mut label, groups) in sections {
        let name = section_name(label.as_deref());
        if let Some(earlier) = kept.iter().find(|k| section_name(k.0.as_deref()).eq_ignore_ascii_case(&name)) {
            if earlier.1 == groups { order.push(section_name(earlier.0.as_deref())); repeats = true; continue; }
            let base = label.clone().unwrap_or_else(|| name.clone());
            let taken = |n: &str| kept.iter().any(|k| section_name(k.0.as_deref()).eq_ignore_ascii_case(n));
            label = (2..).map(|n| format!("{base} ({n})")).find(|l| !taken(&section_name(Some(l))));
        }
        order.push(section_name(label.as_deref()));
        kept.push((label, groups));
    }
    let mut out: Vec<String> = Vec::new();
    for (label, groups) in kept {
        if !out.is_empty() { out.push(String::new()); }
        if let Some(l) = label { out.push(label_line(&l)); }
        out.extend(groups.join(&String::new()));
    }
    (out.join("\n"), if repeats { order } else { vec![] })
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

/// CCLI song #: `vendor_id` (filled in from SongSelect), else `reference_number` when it looks like one.
/// `reference_number` is the "Song Number" box, where churches often type hymnal numbers ("123"), so it
/// needs at least 4 digits.
pub fn ccli_number(reference: &str, vendor: Option<i64>) -> String {
    if let Some(v) = vendor.filter(|&v| v > 0) { return v.to_string(); }
    let r = reference.trim().trim_start_matches('0');
    if r.len() >= 4 && r.chars().all(|c| c.is_ascii_digit()) { r.to_string() } else { String::new() }
}

/// Err is the reason the row is skipped.
pub fn to_song(row: &EwSong) -> Result<Song, String> {
    let words = row.words.as_deref().filter(|w| !w.trim().is_empty()).ok_or("no lyrics (probably a video or announcement item)")?;
    let (lyrics, arrangement) = words_to_lyrics(words);
    if lyrics.trim().is_empty() { return Err("the lyrics are empty".into()); }
    let admin = row.administrator.trim();
    let mut copyright = row.copyright.trim().to_string();
    if !admin.is_empty() {
        if !copyright.is_empty() { copyright.push_str(", "); }
        copyright.push_str(&format!("Administered by {admin}"));
    }
    Ok(Song { id: Uuid::new_v4(), title: display_title(row), writers: split_writers(&row.author), ccli: ccli_number(&row.reference_number, row.vendor_id),
        copyright, lyrics, arrangement })
}

fn display_title(row: &EwSong) -> String {
    let t = row.title.trim();
    if t.is_empty() { "Untitled song".into() } else { t.to_string() }
}

// ---------- reader ----------

/// Reads a whole EW6/7 library. `folder` may be the Easyworship folder, a profile folder, its `Databases\Data`,
/// or Songs.db itself.
pub fn read_library(folder: &Path) -> Result<ImportReport, String> {
    let (songs_path, words_path) = find_databases(folder)?;
    let dbs = snapshot(&[&songs_path, &words_path], "EasyWorship")?;
    let rows = read_rows(&dbs[0], &dbs[1])?;
    let mut report = ImportReport::default();
    for row in &rows {
        match to_song(row) { Ok(s) => report.songs.push(s), Err(why) => report.skipped.push((display_title(row), why)) }
    }
    Ok(report)
}

/// OpenLP's search (`<root>/Databases/Data`, `<root>/Data`, `<root>`) plus the EW7 layouts under a profile
/// (`Default\v6.1\Databases\Data`). File names match case-insensitively.
fn find_databases(path: &Path) -> Result<(PathBuf, PathBuf), String> {
    let root = if path.is_file() { path.parent().unwrap_or(path).to_path_buf() } else { path.to_path_buf() };
    let data = |p: PathBuf| p.join("Databases").join("Data");
    let dirs = [data(root.clone()), data(root.join("v6.1")), data(root.join("Default").join("v6.1")), root.join("Data"), root.clone()];
    for dir in dirs {
        let Some(songs) = find_file(&dir, &["Songs.db", "Songs.dat"]) else { continue };
        check_sqlite(&songs, "This looks like an EasyWorship 2009 (or older) library. Only EasyWorship 6 and 7 libraries can be imported.")?;
        let words = find_file(&dir, &["SongWords.db", "SongWords.dat"])
            .ok_or_else(|| format!("Found Songs.db but not SongWords.db next to it in {}.", dir.display()))?;
        return Ok((songs, words));
    }
    Err("No EasyWorship Songs.db was found here. Pick the profile's Databases\\Data folder \
        (usually C:\\Users\\Public\\Documents\\Softouch\\Easyworship\\Default\\v6.1\\Databases\\Data).".into())
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
    use super::super::common::{backup_into_memory, read_stable, TempDir};
    use crate::slides::parse_sections;
    use std::time::Duration;
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
            "Verse 1\nFirst verse line\nSecond verse line\n\nChorus\nChorus line\n\nPre-Chorus 2\nPre line\n\nMore pre line\n\nTag\nTag line\nSame slide");
        let labels: Vec<_> = parse_sections(&rtf_to_lyrics(PLAIN)).into_iter().map(|s| s.label).collect();
        assert_eq!(labels, ["Verse 1", "Chorus", "Pre-Chorus 2", "Tag"]);
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
    fn ccli_from_vendor_id_then_reference_number() {
        assert_eq!(ccli_number(" 12345 ", Some(999)), "999");
        assert_eq!(ccli_number(" 012345 ", None), "12345");
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
        assert_eq!(got, vec![Dedupe::DuplicateCcli { existing: existing[0].id }, Dedupe::New, Dedupe::DuplicateCcli { existing: imported[1].id }, Dedupe::New, Dedupe::DuplicateTitle { existing: imported[3].id }]);
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

    // ---------- review findings ----------

    /// Runs `f` on a thread; fails the test instead of hanging when it takes longer than `secs`.
    fn within<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || { tx.send(f()).ok(); });
        rx.recv_timeout(Duration::from_secs(secs)).expect("timed out: the import hung")
    }

    fn listing(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut v: Vec<_> = std::fs::read_dir(dir).unwrap().flatten()
            .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap_or_default())).collect();
        v.sort();
        v
    }

    #[test]
    fn writer_holding_a_lock_mid_save_does_not_hang_the_import() {
        let dir = temp_folder();
        write_library(&dir, "Songs.db", &[("Committed", "", "", None, Some(PLAIN))]);
        // EasyWorship mid-save in the old journal mode, holding the lock: Songs.db-journal is hot.
        let ew = Connection::open(dir.join("Songs.db")).unwrap();
        ew.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA locking_mode=EXCLUSIVE; BEGIN EXCLUSIVE;
            INSERT INTO song (title) VALUES ('Uncommitted');").unwrap();
        assert!(dir.join("Songs.db-journal").exists());
        let before = listing(&dir);
        let d = dir.clone();
        let report = within(60, move || read_library(&d)).unwrap();
        assert_eq!(report.songs.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Committed"]);
        assert_eq!(listing(&dir), before);
        drop(ew);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reading_a_wal_library_leaves_the_easyworship_folder_untouched() {
        let dir = temp_folder();
        write_library(&dir, "Songs.db", &[("First", "", "", None, Some(PLAIN))]);
        let ew = Connection::open(dir.join("Songs.db")).unwrap();
        ew.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;").unwrap();
        ew.execute("INSERT INTO song (title) VALUES ('Second')", []).unwrap();
        // EasyWorship open: -wal and -shm exist and belong to it.
        let before = listing(&dir);
        assert_eq!(read_library(&dir).unwrap().songs.len(), 1);
        assert_eq!(listing(&dir), before);
        // EasyWorship closed: a WAL-mode database with no -wal or -shm next to it.
        drop(ew);
        let before = listing(&dir);
        assert_eq!(before.len(), 2, "{:?}", before.iter().map(|f| &f.0).collect::<Vec<_>>());
        assert_eq!(read_library(&dir).unwrap().songs.len(), 1);
        assert_eq!(listing(&dir), before);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn central_european_greek_turkish_hebrew_and_japanese_code_pages() {
        // \fcharset238 = cp1250 (Polish), reached through \deff when no \f is given.
        assert_eq!(rtf_to_lyrics(r"{\rtf1\ansi\deff1{\fonttbl{\f0 Arial;}{\f1\fcharset238 Arial;}}Za\'bf\'f3\'b3\'e6\par}"), "Za\u{17c}\u{f3}\u{142}\u{107}");
        assert_eq!(rtf_to_lyrics(r"{\rtf1\ansi\ansicpg1253{\fonttbl{\f0\fcharset0 Arial;}}\'e1\'e3\'e1\'f0\'e7\par}"), "\u{3b1}\u{3b3}\u{3b1}\u{3c0}\u{3b7}");
        assert_eq!(rtf_to_lyrics(r"{\rtf1\ansi{\fonttbl{\f0\fcharset162 Arial;}}\f0 \'f0\'fd\par}"), "\u{11f}\u{131}");
        assert_eq!(rtf_to_lyrics(r"{\rtf1\ansi{\fonttbl{\f0\fcharset177 Arial;}}\f0 \'f9\'ec\'e5\'ed\par}"), "\u{5e9}\u{5dc}\u{5d5}\u{5dd}");
        // Shift-JIS: two-byte characters, one with an ASCII trail byte written as a plain letter.
        assert_eq!(rtf_to_lyrics(r"{\rtf1\ansi{\fonttbl{\f0\fcharset128 MS Gothic;}}\f0 \'82\'a0\'82\'a2\'83A\par}"), "\u{3042}\u{3044}\u{30a2}");
    }

    #[test]
    fn hymnal_number_is_not_taken_as_the_ccli_number() {
        assert_eq!(ccli_number("123", Some(22025)), "22025");
        assert_eq!(ccli_number("123", None), "");
        assert_eq!(ccli_number("4567", None), "4567");
    }

    #[test]
    fn hidden_label_after_lines_starts_a_new_slide() {
        let rtf = r"{\rtf1\ansi{\pard\sdparawysiwghidden Verse 1\par}{\pard Line one\par}{\pard\sdparawysiwghidden Chorus\par}{\pard Line two\par}}";
        assert_eq!(rtf_to_lyrics(rtf), "Verse 1\nLine one\n\nChorus\nLine two");
    }

    #[test]
    fn repeated_chorus_becomes_an_arrangement() {
        let slide = |label: &str, line: &str| format!(r"{{\pard\sdslidemarker\plain}}{{\pard\sdparawysiwghidden {label}\par}}{{\pard {line}\par}}");
        let rtf = format!(r"{{\rtf1\ansi{}{}{}{}{}}}", slide("Chorus", "Same chorus"), slide("Verse 1", "Verse text"),
            slide("Chorus", "Same chorus"), slide("Verse 2", "More text"), slide("Verse 1", "Different words"));
        let row = EwSong { title: "Repeats".into(), words: Some(rtf), ..Default::default() };
        let s = to_song(&row).unwrap();
        assert_eq!(s.lyrics, "Chorus\nSame chorus\n\nVerse 1\nVerse text\n\nVerse 2\nMore text\n\n[Verse 1 (2)]\nDifferent words");
        assert_eq!(s.arrangement, ["Chorus", "Verse 1", "Chorus", "Verse 2", "Verse 1 (2)"]);
        let secs = parse_sections(&s.lyrics);
        let order: Vec<_> = crate::slides::song_order(&s, &secs).into_iter().map(|i| secs[i].label.clone()).collect();
        assert_eq!(order, s.arrangement);
        // No repeats: no arrangement, as from SongSelect.
        assert!(to_song(&EwSong { words: Some(STRUCTURED.into()), ..Default::default() }).unwrap().arrangement.is_empty());
    }

    #[test]
    fn label_only_slide_names_the_next_unlabeled_slide() {
        let rtf = r"{\rtf1\ansi{\pard\sdparawysiwghidden Verse 1\par}{\pard A\par}{\pard\sdslidemarker\plain}{\pard\sdparawysiwghidden Bridge\par}{\pard\sdslidemarker\plain}{\pard B\par}}";
        assert_eq!(rtf_to_lyrics(rtf), "Verse 1\nA\n\nBridge\nB");
    }

    #[test]
    fn repeat_notes_are_dropped_from_labels() {
        assert_eq!(rtf_to_lyrics(r"{\rtf1\ansi Chorus (x2)\par Line\par}"), "Chorus\nLine");
        assert_eq!(rtf_to_lyrics(r"{\rtf1\ansi{\pard\sdparawysiwghidden Verse 1 (repeat)\par}{\pard Line\par}}"), "Verse 1\nLine");
    }

    #[test]
    fn uc_skip_counts_and_their_restore_at_group_end() {
        assert_eq!(rtf_to_lyrics(r"{\rtf1\ansi\uc0\u20320 x\par}"), "\u{4f60}x");
        assert_eq!(rtf_to_lyrics(r"{\rtf1\ansi{\uc2\u20320\'a0\'a0}\u20320?y\par}"), "\u{4f60}\u{4f60}y");
    }

    #[test]
    fn malformed_rtf_does_not_panic() {
        for rtf in [r"{\rtf1\ansi {\b bold\par", r"{\rtf1\ansi x}}}} y\par", r"{\rtf1\ansi z\'4", r"{\rtf1\ansi\u", r"{\rtf1\ansi\u-99999999999 q\par}", r"{\rtf1\ansi\'zz w\par}"] {
            let _ = rtf_to_lyrics(rtf);
        }
        assert_eq!(rtf_to_lyrics(r"{\rtf1\ansi {\b bold\par"), "bold");
    }

    #[test]
    fn profile_folder_with_the_v6_1_layout_is_found() {
        let root = temp_folder();
        let data = root.join("Default").join("v6.1").join("Databases").join("Data");
        std::fs::create_dir_all(&data).unwrap();
        write_library(&data, "Songs.db", &[("Found", "", "", None, Some(PLAIN))]);
        assert_eq!(read_library(&root.join("Default")).unwrap().songs.len(), 1);
        assert_eq!(read_library(&root).unwrap().songs.len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn empty_songs_db_is_not_called_easyworship_2009() {
        let dir = temp_folder();
        std::fs::write(dir.join("Songs.db"), b"").unwrap();
        std::fs::write(dir.join("SongWords.db"), b"").unwrap();
        let e = read_library(&dir).unwrap_err();
        assert!(!e.contains("2009") && e.contains("empty or damaged"), "{e}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hymnal_numbers_and_shared_ccli_through_the_reader() {
        let dir = temp_folder();
        write_library(&dir, "Songs.db", &[
            ("Hymn A", "", "123", Some(1111), Some(PLAIN)),
            ("Hymn B", "", "123", Some(2222), Some(PLAIN)),
            ("Same Song", "", "", Some(1111), Some(PLAIN)),
        ]);
        let report = read_library(&dir).unwrap();
        assert_eq!(report.songs.iter().map(|s| s.ccli.as_str()).collect::<Vec<_>>(), ["1111", "2222", "1111"]);
        let got = classify(&[], &report.songs);
        assert_eq!(got, vec![Dedupe::New, Dedupe::New, Dedupe::DuplicateCcli { existing: report.songs[0].id }]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn songs_without_a_ccli_number_match_by_title() {
        let song = |t: &str, c: &str| Song { id: Uuid::new_v4(), title: t.into(), ccli: c.into(), lyrics: "x".into(), ..Default::default() };
        let existing = vec![song("Amazing Grace", "22025"), song("Holy, Holy, Holy", "")];
        let imported = vec![song("amazing  grace", ""), song("Holy Holy Holy!", ""), song("Holy Holy Holy", "1234567"), song("New Hymn", ""), song("New hymn", "")];
        assert_eq!(classify(&existing, &imported), vec![Dedupe::DuplicateTitle { existing: existing[0].id }, Dedupe::DuplicateTitle { existing: existing[1].id },
            Dedupe::New, Dedupe::New, Dedupe::DuplicateTitle { existing: imported[3].id }]);
    }

    #[test]
    fn copy_gives_up_while_a_file_keeps_changing() {
        let dir = temp_folder();
        let f = dir.join("Songs.db");
        std::fs::write(&f, b"0").unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (s, path) = (stop.clone(), f.clone());
        let writer = std::thread::spawn(move || {
            let mut n = 0u64;
            while !s.load(std::sync::atomic::Ordering::Relaxed) { n += 1; std::fs::write(&path, n.to_string()).ok(); std::thread::sleep(Duration::from_millis(2)); }
        });
        while std::fs::read(&f).unwrap_or_default() == b"0" { std::thread::sleep(Duration::from_millis(1)); }
        let got = read_stable(&[f.clone()], 5, Duration::from_millis(20), "EasyWorship");
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        writer.join().unwrap();
        assert!(got.unwrap_err().contains("kept saving"));
        // Quiet again: the copy goes through, and a missing -wal is just None.
        let got = read_stable(&[f.clone(), dir.join("Songs.db-wal")], 5, Duration::from_millis(5), "EasyWorship").unwrap();
        assert!(got[0].is_some() && got[1].is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn backup_gives_up_when_the_database_stays_locked() {
        let dir = temp_folder();
        let path = dir.join("Locked.db");
        let w = Connection::open(&path).unwrap();
        w.execute_batch("CREATE TABLE t (x); PRAGMA locking_mode=EXCLUSIVE; BEGIN EXCLUSIVE; INSERT INTO t VALUES (1);").unwrap();
        let p = path.clone();
        assert!(within(60, move || backup_into_memory(&p).map(|_| ())).is_err());
        drop(w);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn temp_copy_is_removed() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.0.clone();
        std::fs::write(path.join("Songs.db"), b"lyrics").unwrap();
        let open = std::fs::File::open(path.join("Songs.db")).unwrap();
        let closer = std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(100)); drop(open); });
        drop(tmp);
        closer.join().unwrap();
        assert!(!path.exists());
    }
}
