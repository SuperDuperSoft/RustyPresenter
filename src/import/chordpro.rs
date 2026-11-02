//! ChordPro text files (`.cho`, `.chordpro`, `.chopro`, `.crd`, `.pro`) → Songs.
//! Written from the ChordPro format description (facts, not code). A file holds one or more songs, separated by
//! `{new_song}`/`{ns}`. Lines in `{…}` are directives: `{title:}`/`{t:}`, `{artist:}`, `{composer:}`,
//! `{lyricist:}`, `{copyright:}`, `{ccli:}` (also as `{meta: name value}`) fill in the song; `{start_of_verse}`/
//! `{sov}` … `{end_of_verse}`/`{eov}`, `{soc}`/`{eoc}`, `{sob}`/`{eob}` mark sections, optionally labelled
//! (`{sov: Verse 2}`, `{start_of_verse label="Verse 2"}`); `{chorus}` plays the last chorus again; a
//! `{comment: Chorus}` (`{c:}`, `{ci:}`, `{cb:}`, `{highlight:}`) that reads as a section label starts that section.
//! Every other directive is ignored, as are `#` comment lines and tab/grid/abc/lilypond/svg blocks. Chords are
//! `[G]` inside lyric lines and are removed. Text outside any section is split at blank lines into verses.
//! Files are only read, never written.
use super::common::{label_line, section_name, ImportReport};
use super::easyworship::split_writers;
use crate::model::Song;
use crate::slides::section_label;
use encoding_rs::{UTF_16BE, UTF_16LE, WINDOWS_1252};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// File extensions picked up when a folder is imported (a single file is read whatever its name).
pub const EXTENSIONS: &[&str] = &["cho", "chordpro", "chopro", "crd", "pro"];

/// A text file bigger than this is not a song file.
const MAX_FILE: u64 = 4_000_000;
/// Files looked at, and folder depth walked, at most when a folder is imported.
const MAX_FILES: usize = 50_000;
const MAX_DEPTH: usize = 16;
/// More sections than this is not a song.
const MAX_SECTIONS: usize = 500;
/// Characters looked at for the `]` that closes a chord, so a stray `[` in a lyric costs little.
const MAX_CHORD: usize = 64;
/// A title made from the first lyric line is cut to this many characters.
const MAX_TITLE: usize = 80;

// ---------- text ----------

/// File bytes → text: UTF-8 (with or without BOM), UTF-16 with a BOM, else Windows-1252 (older Windows editors).
pub fn decode(bytes: &[u8]) -> String {
    if let Some(b) = bytes.strip_prefix(b"\xEF\xBB\xBF") { return String::from_utf8_lossy(b).into_owned(); }
    if let Some(b) = bytes.strip_prefix(b"\xFF\xFE") { return UTF_16LE.decode_without_bom_handling(b).0.into_owned(); }
    if let Some(b) = bytes.strip_prefix(b"\xFE\xFF") { return UTF_16BE.decode_without_bom_handling(b).0.into_owned(); }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => WINDOWS_1252.decode_without_bom_handling(bytes).0.into_owned(),
    }
}

/// `{name: value}`, `{name value}` or `{name}` → (name in lowercase without a `-selector`, value).
/// None when the line isn't a directive.
fn directive(line: &str) -> Option<(String, String)> {
    let inner = line.strip_prefix('{')?.strip_suffix('}')?.trim();
    let split = inner.find(|c: char| c == ':' || c.is_whitespace()).unwrap_or(inner.len());
    let name = inner[..split].to_ascii_lowercase();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '!')) { return None; }
    let value = inner[split..].trim_start();
    let value = value.strip_prefix(':').unwrap_or(value).trim();
    // `{title-guitar: …}` is the title for guitarists only: still the title.
    let name = name.split('-').next().unwrap_or("").to_string();
    Some((name, value.to_string()))
}

/// A section directive's label: `Verse 2` or `label="Verse 2"`.
fn env_label(value: &str) -> String {
    let v = value.trim();
    match v.strip_prefix("label").map(str::trim_start).and_then(|r| r.strip_prefix('=')) {
        Some(r) => {
            let r = r.trim();
            match r.chars().next() {
                Some(q @ ('"' | '\'')) => r[1..].split(q).next().unwrap_or("").trim().to_string(),
                _ => r.split_whitespace().next().unwrap_or("").to_string(),
            }
        }
        None => v.to_string(),
    }
}

/// Removes `[chords]` (and `[*annotations]`) from a lyric line and tidies the spacing they leave.
/// Returns the line and whether a chord was removed. An unclosed `[` is kept as text.
fn strip_chords(line: &str) -> (String, bool) {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    let mut found = false;
    while let Some(i) = rest.find('[') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        match tail.char_indices().take(MAX_CHORD).find(|&(_, c)| c == ']' || c == '[') {
            Some((e, ']')) => { found = true; rest = &tail[e + 1..]; }
            _ => { out.push('['); rest = tail; }
        }
    }
    out.push_str(rest);
    (out.split_whitespace().collect::<Vec<_>>().join(" "), found)
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// The kind a label names ("verse", "chorus" …): its first word, lowercase.
fn kind_of(label: &str) -> String {
    label.split_whitespace().next().unwrap_or("").trim_end_matches(':').to_lowercase()
}

/// Section kinds whose content isn't lyrics.
const NOT_LYRICS: &[&str] = &["tab", "grid", "abc", "ly", "svg", "textblock", "musicxml"];

/// `sov` → `verse`, `start_of_chorus` → `chorus`; None if `name` doesn't open a section.
fn start_kind(name: &str) -> Option<String> {
    match name {
        "sov" => Some("verse".into()), "soc" => Some("chorus".into()), "sob" => Some("bridge".into()),
        "sot" => Some("tab".into()), "sog" => Some("grid".into()),
        _ => name.strip_prefix("start_of_").filter(|k| !k.is_empty()).map(str::to_string),
    }
}

fn is_end(name: &str) -> bool {
    matches!(name, "eov" | "eoc" | "eob" | "eot" | "eog") || name.starts_with("end_of_")
}

// ---------- one song ----------

#[derive(PartialEq)]
enum Mode {
    /// Plain text outside any section: each blank-line paragraph is a verse.
    Paragraph,
    /// After a `{comment: Chorus}` hint: runs to the next hint or section; blank lines are slide breaks.
    Hint,
    /// Inside `{start_of_…}` … `{end_of_…}`: blank lines are slide breaks.
    Env,
}

struct Open { label: String, kind: String, mode: Mode, groups: Vec<Vec<String>>, group: Vec<String> }

/// A song as read, before its title is settled.
#[derive(Default)]
struct Draft {
    title: Option<String>,
    writers: Vec<String>,
    copyright: String,
    ccli: String,
    /// (name, slides) in written order.
    sections: Vec<(String, Vec<Vec<String>>)>,
    /// Section names in the order they are played.
    order: Vec<String>,
    replayed: bool,
    /// key → written name, for `{chorus}` and repeated hints.
    names: HashMap<String, String>,
    next_number: HashMap<String, usize>,
    verses: usize,
    last_chorus: Option<String>,
    open: Option<Open>,
    skipping: bool,
    first_line: Option<String>,
    too_many: bool,
    /// Something only ChordPro has (a known directive or a chord) was seen.
    chordpro: bool,
}

fn key(name: &str) -> String { section_name(Some(name)).to_lowercase() }

impl Draft {
    fn is_empty(&self) -> bool {
        self.title.is_none() && self.writers.is_empty() && self.copyright.is_empty() && self.ccli.is_empty()
            && self.sections.is_empty() && self.open.is_none()
    }

    fn open(&mut self, label: String, kind: String, mode: Mode) {
        self.close();
        self.open = Some(Open { label, kind, mode, groups: vec![], group: vec![] });
    }

    /// Plays an already-written section again.
    fn replay(&mut self, name: &str) {
        let written = section_name(Some(name));
        self.order.push(written);
        self.replayed = true;
    }

    fn close(&mut self) {
        let Some(mut o) = self.open.take() else { return };
        if !o.group.is_empty() { o.groups.push(std::mem::take(&mut o.group)); }
        let base = if o.kind.is_empty() { "verse".to_string() } else { o.kind.clone() };
        let mut label = o.label.trim().to_string();
        // A bare number or letter ("2", "1a") belongs to the section kind: "Verse 2".
        if !label.is_empty() && section_label(&label).is_none() && label.chars().count() <= 3 && label.chars().all(char::is_alphanumeric) {
            label = format!("{} {label}", capitalize(&base));
        }
        if o.groups.is_empty() {
            // `{c: Chorus}` or `{soc: Chorus}` with nothing under it plays that section again.
            if let Some(name) = self.names.get(&key(&label)).cloned().filter(|_| !label.is_empty()) { self.replay(&name); }
            return;
        }
        if self.sections.len() >= MAX_SECTIONS { self.too_many = true; return; }
        let kind = if o.label.trim().is_empty() { base.clone() } else { kind_of(&label) };
        if label.is_empty() {
            label = match base.as_str() {
                "verse" => format!("Verse {}", self.verses + 1),
                k => capitalize(k),
            };
        }
        // The app finds sections by name, so the same name twice gets "(2)".
        let mut name = label.clone();
        if self.names.contains_key(&key(&name)) {
            let n = self.next_number.entry(key(&label)).or_insert(2);
            for _ in 0..=MAX_SECTIONS {
                name = format!("{label} ({n})");
                *n += 1;
                if !self.names.contains_key(&key(&name)) { break; }
            }
        }
        self.names.insert(key(&name), name.clone());
        // A label like "Chorus" also finds "Chorus (2)"'s original for a later `{c: Chorus}`.
        self.names.entry(key(&label)).or_insert_with(|| name.clone());
        if kind == "verse" { self.verses += 1; }
        if kind == "chorus" || kind == "refrain" { self.last_chorus = Some(name.clone()); }
        self.order.push(section_name(Some(&name)));
        self.sections.push((name, o.groups));
    }

    fn lyric(&mut self, raw: &str) {
        let (line, chord) = strip_chords(raw);
        self.chordpro |= chord;
        if line.is_empty() { return; } // a chord-only line
        if self.first_line.is_none() { self.first_line = Some(line.clone()); }
        if self.open.is_none() { self.open = Some(Open { label: String::new(), kind: "verse".into(), mode: Mode::Paragraph, groups: vec![], group: vec![] }); }
        if let Some(o) = &mut self.open { o.group.push(line); }
    }

    fn blank(&mut self) {
        if self.open.as_ref().is_some_and(|o| o.mode == Mode::Paragraph) { return self.close(); }
        if let Some(o) = &mut self.open {
            if !o.group.is_empty() { o.groups.push(std::mem::take(&mut o.group)); }
        }
    }

    fn add_writer(&mut self, value: &str) {
        for w in split_writers(value).split(" | ") {
            if !w.is_empty() && !self.writers.iter().any(|x| x.eq_ignore_ascii_case(w)) { self.writers.push(w.to_string()); }
        }
    }

    /// Err(()) is `{new_song}`.
    fn directive(&mut self, name: &str, value: &str) -> Result<(), ()> {
        if self.skipping {
            if is_end(name) { self.skipping = false; }
            return Ok(());
        }
        let known = |d: &mut Draft| d.chordpro = true;
        match name {
            "new_song" | "ns" => { known(self); return Err(()); }
            "title" | "t" => { known(self); if self.title.is_none() && !value.is_empty() { self.title = Some(value.to_string()); } }
            "artist" | "composer" | "lyricist" => { known(self); self.add_writer(value); }
            "copyright" => { known(self); if self.copyright.is_empty() { self.copyright = value.to_string(); } }
            "ccli" => { known(self); if self.ccli.is_empty() { self.ccli = ccli(value); } }
            "meta" => {
                known(self);
                let (n, v) = value.split_once(char::is_whitespace).unwrap_or((value, ""));
                let n = n.to_ascii_lowercase();
                if matches!(n.as_str(), "title" | "artist" | "composer" | "lyricist" | "copyright" | "ccli") { let _ = self.directive(&n, v.trim()); }
            }
            "comment" | "c" | "comment_italic" | "ci" | "comment_box" | "cb" | "highlight" => {
                known(self);
                let in_env = self.open.as_ref().is_some_and(|o| o.mode == Mode::Env);
                if let Some(label) = section_label(value).filter(|_| !in_env) {
                    self.open(label.clone(), kind_of(&label), Mode::Hint);
                }
            }
            "chorus" => {
                known(self);
                self.close();
                let label = env_label(value);
                let found = if label.is_empty() { None } else { self.names.get(&key(&label)).cloned() };
                if let Some(name) = found.or_else(|| self.last_chorus.clone()) { self.replay(&name); }
            }
            n if is_end(n) => { known(self); self.close(); }
            n => {
                if let Some(kind) = start_kind(n) {
                    known(self);
                    self.close();
                    if NOT_LYRICS.contains(&kind.as_str()) { self.skipping = true; } else { self.open(env_label(value), kind, Mode::Env); }
                }
                // Any other directive (key, capo, tempo, fonts …) has no place in the app's Song.
            }
        }
        Ok(())
    }

    /// The song, or the reason it's skipped. `fallback` titles a song without `{title}`.
    fn finish(mut self, fallback: Option<&str>) -> (String, Result<Song, String>) {
        self.close();
        let title = self.title.clone().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
            .or_else(|| fallback.map(str::to_string).filter(|t| !t.trim().is_empty()))
            .or_else(|| self.first_line.as_ref().map(|l| l.chars().take(MAX_TITLE).collect::<String>().trim_end_matches([',', ';', ':', '.']).to_string()))
            .unwrap_or_else(|| "Untitled song".into());
        if self.too_many { return (title, Err(format!("more than {MAX_SECTIONS} sections, so it isn't a song"))); }
        if self.sections.is_empty() { return (title, Err("no lyrics".into())); }
        let lyrics = self.sections.iter()
            .map(|(name, groups)| format!("{}\n{}", label_line(name), groups.iter().map(|g| g.join("\n")).collect::<Vec<_>>().join("\n\n")))
            .collect::<Vec<_>>().join("\n\n");
        let arrangement = if self.replayed { self.order.clone() } else { vec![] };
        let song = Song { id: Uuid::new_v4(), title: title.clone(), writers: self.writers.join(" | "), ccli: self.ccli.clone(),
            copyright: self.copyright.trim().to_string(), lyrics, arrangement };
        (title, Ok(song))
    }
}

/// `{ccli}` is free text: kept only when it is a number ("#1234567" → "1234567").
fn ccli(raw: &str) -> String {
    let t = raw.trim().trim_start_matches('#').trim().trim_start_matches('0');
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) { t.to_string() } else { String::new() }
}

// ---------- files ----------

/// The songs in one file's text; `name` (the file name without extension) titles a lone song with no `{title}`.
/// The bool says whether anything ChordPro-only was seen.
fn parse_named(text: &str, name: Option<&str>) -> (ImportReport, bool) {
    let mut drafts = vec![Draft::default()];
    for raw in text.lines() {
        let line = raw.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
        let Some(d) = drafts.last_mut() else { break };
        if line.starts_with('#') && !d.skipping { continue; }
        if let Some((n, v)) = directive(line) {
            if d.directive(&n, &v).is_err() { drafts.push(Draft::default()); }
        } else if d.skipping {
            continue;
        } else if line.is_empty() {
            d.blank();
        } else {
            d.lyric(line);
        }
    }
    let chordpro = drafts.iter().any(|d| d.chordpro);
    drafts.retain(|d| !d.is_empty());
    let fallback = if drafts.len() == 1 { name } else { None };
    let mut report = ImportReport::default();
    for d in drafts {
        match d.finish(fallback) { (_, Ok(s)) => report.songs.push(s), (title, Err(why)) => report.skipped.push((title, why)) }
    }
    (report, chordpro)
}

/// The songs in a ChordPro text (one or more, split at `{new_song}`).
pub fn parse_str(text: &str) -> ImportReport { parse_named(text, None).0 }

/// The songs in a ChordPro file's bytes, in whatever encoding it was saved (see `decode`).
pub fn parse_bytes(bytes: &[u8]) -> ImportReport { parse_str(&decode(bytes)) }

fn has_extension(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// ChordPro files under `dir`, sorted, not following links (so a link loop can't trap the walk).
fn find_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    let mut seen = 0usize;
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        let mut subdirs = Vec::new();
        for e in entries {
            seen += 1;
            if seen > MAX_FILES { return out; }
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            // macOS leaves "._name" resource-fork files next to the real ones on shared drives.
            if e.file_name().to_string_lossy().starts_with("._") { continue; }
            if ft.is_dir() && depth < MAX_DEPTH { subdirs.push((p, depth + 1)); } else if ft.is_file() && has_extension(&p) { out.push(p); }
        }
        stack.extend(subdirs.into_iter().rev());
    }
    out
}

fn file_label(p: &Path) -> String { p.file_name().unwrap_or_default().to_string_lossy().into_owned() }

/// One file's songs added to `report`; problems become skipped entries named after the file.
fn read_file(p: &Path, report: &mut ImportReport) {
    let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    if size > MAX_FILE { report.skipped.push((file_label(p), "the file is too big to be a song".into())); return; }
    let bytes = match std::fs::read(p) {
        Ok(b) => b,
        Err(e) => { report.skipped.push((file_label(p), format!("couldn't read it: {e}"))); return; }
    };
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned());
    let (r, chordpro) = parse_named(&decode(&bytes), stem.as_deref());
    if !chordpro { report.skipped.push((file_label(p), "it doesn't look like a ChordPro file".into())); return; }
    if r.songs.is_empty() && r.skipped.is_empty() { report.skipped.push((file_label(p), "no songs in it".into())); return; }
    report.songs.extend(r.songs);
    report.skipped.extend(r.skipped);
}

/// Reads a ChordPro file (any name), or every ChordPro file in a folder and its subfolders.
pub fn read_chordpro(path: &Path) -> Result<ImportReport, String> {
    let mut report = ImportReport::default();
    if path.is_file() {
        read_file(path, &mut report);
        if report.songs.is_empty() {
            let why = report.skipped.first().map(|(_, w)| w.clone()).unwrap_or_else(|| "no songs in it".into());
            return Err(format!("No songs could be read from {}: {why}.", path.display()));
        }
        return Ok(report);
    }
    if !path.is_dir() { return Err(format!("Couldn't find {}.", path.display())); }
    let files = find_files(path);
    if files.is_empty() {
        return Err(format!("No ChordPro files (.cho, .chordpro, .chopro, .crd, .pro) were found in {}.", path.display()));
    }
    for f in &files { read_file(f, &mut report); }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::common::TempDir;
    use crate::slides::{parse_sections, song_order};

    // Amazing Grace (John Newton, 1779): public domain. Chords added for the test.
    const GRACE: &str = "{title: Amazing Grace}
{artist: John Newton}
{copyright: Public Domain}
{ccli: 22025}
{key: G}

{start_of_verse}
A[G]mazing [G7]grace! how [C]sweet the [G]sound
That saved a wretch like [D]me!
{end_of_verse}

{sov: Verse 2}
'Twas [G]grace that taught my heart to fear,
And grace my fears relieved;
{eov}
";

    fn one(text: &str) -> Song {
        let r = parse_str(text);
        assert_eq!(r.songs.len(), 1, "skipped: {:?}", r.skipped);
        r.songs.into_iter().next().unwrap()
    }

    fn labels(s: &Song) -> Vec<String> { parse_sections(&s.lyrics).into_iter().map(|x| x.label).collect() }

    #[test]
    fn metadata_and_sections() {
        let s = one(GRACE);
        assert_eq!(s.title, "Amazing Grace");
        assert_eq!(s.writers, "John Newton");
        assert_eq!(s.copyright, "Public Domain");
        assert_eq!(s.ccli, "22025");
        assert_eq!(s.lyrics, "Verse 1\nAmazing grace! how sweet the sound\nThat saved a wretch like me!\n\nVerse 2\n'Twas grace that taught my heart to fear,\nAnd grace my fears relieved;");
        assert_eq!(labels(&s), ["Verse 1", "Verse 2"]);
        assert!(s.arrangement.is_empty());
    }

    #[test]
    fn short_directives_and_writers() {
        let s = one("{t:Short}\n{composer: A / B}\n{lyricist: B}\n{meta: ccli #0123456}\n{meta: copyright 1900 Nobody}\nLine one\n");
        assert_eq!(s.title, "Short");
        assert_eq!(s.writers, "A | B");
        assert_eq!(s.ccli, "123456");
        assert_eq!(s.copyright, "1900 Nobody");
    }

    #[test]
    fn chorus_bridge_and_replay() {
        let s = one("{title: Song}\n{sov}\nVerse one line\n{eov}\n{soc}\nChorus [D]line\n{eoc}\n{sov}\nVerse two line\n{eov}\n{chorus}\n{start_of_bridge}\nBridge line\n{end_of_bridge}\n{chorus}\n");
        assert_eq!(labels(&s), ["Verse 1", "Chorus", "Verse 2", "Bridge"]);
        assert_eq!(s.arrangement, ["Verse 1", "Chorus", "Verse 2", "Chorus", "Bridge", "Chorus"]);
        let secs = parse_sections(&s.lyrics);
        let order: Vec<_> = song_order(&s, &secs).into_iter().map(|i| secs[i].label.clone()).collect();
        assert_eq!(order, s.arrangement);
    }

    #[test]
    fn comments_as_section_hints() {
        let s = one("{title: Hinted}\n{c: Verse 1}\n[G]Line a\nLine b\n\nLine c\n{comment: Chorus}\nChorus a\n{c: Play softly}\n{c: Verse 2}\nLine d\n{c: Chorus}\n");
        assert_eq!(s.lyrics, "Verse 1\nLine a\nLine b\n\nLine c\n\nChorus\nChorus a\n\nVerse 2\nLine d");
        assert_eq!(s.arrangement, ["Verse 1", "Chorus", "Verse 2", "Chorus"]);
    }

    #[test]
    fn plain_paragraphs_are_verses_and_chord_lines_drop() {
        let s = one("{title: Plain}\n# a comment\n[G]  [C]  [D]\nFirst line\nSecond line\n\n\nThird [Em]line\n");
        assert_eq!(s.lyrics, "Verse 1\nFirst line\nSecond line\n\nVerse 2\nThird line");
    }

    #[test]
    fn labels_on_sections() {
        let s = one("{title: L}\n{start_of_verse label=\"Verse 3\"}\nx\n{end_of_verse}\n{sov: 4}\ny\n{eov}\n{soc: Refrain}\nz\n{eoc}\n{start_of_chorus: My Odd Label}\nw\n{end_of_chorus}\n{start_of_intro}\ni\n{end_of_intro}\n");
        assert_eq!(labels(&s), ["Verse 3", "Verse 4", "Refrain", "My Odd Label", "Intro"]);
    }

    #[test]
    fn duplicate_names_get_numbers() {
        let s = one("{title: D}\n{soc}\na\n{eoc}\n{soc}\nb\n{eoc}\n");
        assert_eq!(labels(&s), ["Chorus", "Chorus (2)"]);
    }

    #[test]
    fn tabs_and_grids_skipped() {
        let s = one("{title: T}\n{sot}\ne|---0---|\n# not a comment\n{eot}\n{start_of_grid}\n| G . | C . |\n{end_of_grid}\nWords\n");
        assert_eq!(s.lyrics, "Verse 1\nWords");
    }

    #[test]
    fn several_songs_per_file() {
        let r = parse_str("{ns}\n{title: One}\na\n{new_song}\n{title: Two}\nb\n{ns}\n{title: Empty}\n{ns}\n");
        assert_eq!(r.songs.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["One", "Two"]);
        assert_eq!(r.skipped, [("Empty".to_string(), "no lyrics".to_string())]);
    }

    #[test]
    fn untitled_uses_first_line() {
        let s = one("[G]Hello there, world,\nmore\n");
        assert_eq!(s.title, "Hello there, world");
    }

    #[test]
    fn selectors_and_brackets() {
        let s = one("{title-guitar: Sel}\nan [unclosed bracket\n[*riff]Line\n");
        assert_eq!(s.title, "Sel");
        assert_eq!(s.lyrics, "Verse 1\nan [unclosed bracket\nLine");
    }

    #[test]
    fn decodes_encodings() {
        assert_eq!(decode(b"\xEF\xBB\xBFcaf\xC3\xA9"), "café");
        assert_eq!(decode(b"caf\xE9 \x93q\x94"), "café \u{201c}q\u{201d}");
        assert_eq!(decode(b"\xFF\xFEh\0i\0"), "hi");
        assert_eq!(decode(b"\xFE\xFF\0h\0i"), "hi");
        let s = parse_bytes(b"\xEF\xBB\xBF{title: Caf\xC3\xA9}\r\nLine\r\n").songs.remove(0);
        assert_eq!(s.title, "Café");
        assert_eq!(s.lyrics, "Verse 1\nLine");
    }

    #[test]
    fn malformed_input_never_panics() {
        for t in ["", "{", "}", "{}", "{:}", "[", "]", "[[[[", "{sov", "{eov}\n{eoc}\n{chorus}", "{c:}", "{meta}", "{meta: }",
            "{start_of_}\nx", "{sot}\nnever ends", "{t:}\n\n\n", "\u{feff}", "{ns}{ns}", "{chorus: Nope}", "[\u{e9}\u{301}",
            "{sov: \u{1F600}\u{1F600}\u{1F600}}\nx\n{eov}", "{start_of_verse label=\"unterminated}\nx"] {
            let _ = parse_str(t);
        }
        let big = "{c: Chorus}\nx\n".repeat(MAX_SECTIONS + 5);
        let r = parse_str(&big);
        assert!(r.songs.is_empty() && r.skipped.len() == 1);
        let many = "{soc}\nx\n{eoc}\n".repeat(300);
        assert_eq!(parse_str(&many).songs[0].lyrics.matches("Chorus (").count(), 299);
    }

    #[test]
    fn classify_finds_duplicates() {
        use crate::import::{classify, Dedupe};
        let a = one(GRACE);
        let b = one(GRACE);
        assert!(matches!(classify(&[a], &[b])[0], Dedupe::DuplicateCcli { .. }));
    }

    #[test]
    fn reads_folders_and_files() {
        let tmp = TempDir::new().unwrap();
        let d = &tmp.0;
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("Amazing Grace.cho"), GRACE).unwrap();
        std::fs::write(d.join("sub").join("Untitled One.chopro"), "[G]Some words\n").unwrap();
        std::fs::write(d.join("sub").join("two.CRD"), "{t: A}\nx\n{ns}\n{t: B}\ny\n").unwrap();
        std::fs::write(d.join("project.pro"), "QT += core\nSOURCES = main.cpp\n").unwrap();
        std::fs::write(d.join("notes.txt"), "{t: ignored}\nx\n").unwrap();
        std::fs::write(d.join("._Amazing Grace.cho"), b"\0\x05\x16\x07").unwrap();
        std::fs::write(d.join("empty.cho"), "{t: Nothing}\n").unwrap();

        let r = read_chordpro(d).unwrap();
        let mut titles: Vec<_> = r.songs.iter().map(|s| s.title.as_str()).collect();
        titles.sort();
        assert_eq!(titles, ["A", "Amazing Grace", "B", "Untitled One"]);
        let mut skipped: Vec<_> = r.skipped.iter().map(|(t, _)| t.as_str()).collect();
        skipped.sort();
        assert_eq!(skipped, ["Nothing", "project.pro"]);

        let one = read_chordpro(&d.join("Amazing Grace.cho")).unwrap();
        assert_eq!(one.songs.len(), 1);
        assert!(read_chordpro(&d.join("project.pro")).unwrap_err().contains("doesn't look like a ChordPro file"));
        assert!(read_chordpro(&d.join("missing")).unwrap_err().contains("Couldn't find"));
        let empty = TempDir::new().unwrap();
        assert!(read_chordpro(&empty.0).unwrap_err().contains("No ChordPro files"));
        // Files are only read.
        assert_eq!(std::fs::read_to_string(d.join("Amazing Grace.cho")).unwrap(), GRACE);
    }
}
