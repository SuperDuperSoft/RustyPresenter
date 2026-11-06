//! A one-time import of an EasyWorship 6/7 song library: titles, words, writers, copyright and CCLI numbers.
//!
//! EasyWorship keeps its library under `%PUBLIC%\Documents\Softouch\EasyWorship\<profile>\<version>\Databases\Data`
//! (EasyWorship 6 and 7 both use a `v6.1` folder). Two SQLite files matter:
//! - `Songs.db`, table `song`: `rowid`, `title`, `author`, `copyright`, `reference_number` (the CCLI song number)
//! - `SongWords.db`, table `word`: `song_id`, `words` (RTF)
//!
//! EasyWorship is never touched: the two files (and their `-wal`/`-shm` journals, so a library EasyWorship is still
//! writing reads consistently) are copied first, and only the copies are opened. Columns differ a little between
//! versions, so they are looked up at run time and a missing one reads as empty.
use crate::model::Song;
use rusqlite::{Connection, OpenFlags};
use std::{collections::HashMap, path::{Path, PathBuf}};

/// The words for when no library is found where EasyWorship keeps it.
pub const NOT_FOUND: &str = "EasyWorship's song library was not found on this computer.";

/// Folders where EasyWorship keeps its library, most likely first.
pub fn default_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let publics = [std::env::var_os("PUBLIC").map(PathBuf::from), Some(PathBuf::from(r"C:\Users\Public"))];
    for base in publics.into_iter().flatten() {
        let root = base.join("Documents").join("Softouch").join("EasyWorship");
        // The Default profile first, then any other profile folder.
        let mut profiles = vec![root.join("Default")];
        if let Ok(entries) = std::fs::read_dir(&root) {
            profiles.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir() && !p.ends_with("Default")));
        }
        for p in profiles {
            for v in ["v6.1", "v6", "v7", "v7.1"] { out.push(p.join(v).join("Databases").join("Data")); }
        }
    }
    out
}

/// The library folder: `given` when it (or its `Databases\Data` below it) holds `Songs.db`, else the first default.
pub fn find_dir(given: Option<&Path>) -> Option<PathBuf> {
    match given {
        Some(g) => [g.to_path_buf(), g.join("Databases").join("Data"), g.join("Data")].into_iter().find(|d| d.join("Songs.db").is_file()),
        None => default_dirs().into_iter().find(|d| d.join("Songs.db").is_file()),
    }
}

/// Reads every song from a library folder, through copies made in `scratch` (removed afterwards).
pub fn read_library(dir: &Path, scratch: &Path) -> Result<Vec<Song>, String> {
    std::fs::create_dir_all(scratch).map_err(|e| e.to_string())?;
    let copy = |name: &str| -> Result<PathBuf, String> {
        let to = scratch.join(name);
        std::fs::copy(dir.join(name), &to).map_err(|e| format!("Could not read {name} from EasyWorship: {e}"))?;
        for j in ["-wal", "-shm"] {
            let from = dir.join(format!("{name}{j}"));
            if from.is_file() { let _ = std::fs::copy(&from, scratch.join(format!("{name}{j}"))); }
        }
        Ok(to)
    };
    let result = (|| {
        let songs_db = open(&copy("Songs.db")?)?;
        let words_db = open(&copy("SongWords.db")?)?;
        read(&songs_db, &words_db).map_err(|e| format!("EasyWorship's library could not be read: {e}"))
    })();
    let _ = std::fs::remove_dir_all(scratch);
    result
}

fn open(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| format!("EasyWorship's library could not be opened: {e}"))
}

fn columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let cols = stmt.query_map([], |r| r.get::<_, String>(1))?.collect::<Result<Vec<_>, _>>()?;
    Ok(cols.into_iter().map(|c| c.to_lowercase()).collect())
}

/// A column as text, or '' when this version has no such column.
fn col(cols: &[String], name: &str) -> String {
    if cols.iter().any(|c| c == name) { format!("COALESCE(CAST({name} AS TEXT), '')") } else { "''".into() }
}

fn read(songs_db: &Connection, words_db: &Connection) -> rusqlite::Result<Vec<Song>> {
    let mut words: HashMap<i64, String> = HashMap::new();
    {
        let mut stmt = words_db.prepare("SELECT song_id, COALESCE(CAST(words AS TEXT), '') FROM word")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (id, w) = row?;
            words.entry(id).or_insert(w);
        }
    }
    let cols = columns(songs_db, "song")?;
    let sql = format!(
        "SELECT rowid, {}, {}, {}, {} FROM song ORDER BY title COLLATE NOCASE",
        col(&cols, "title"), col(&cols, "author"), col(&cols, "copyright"), col(&cols, "reference_number"),
    );
    let mut stmt = songs_db.prepare(&sql)?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (id, title, author, copyright, ccli) = row?;
        let lyrics = words.get(&id).map(|w| lyrics_text(&rtf_to_text(w))).unwrap_or_default();
        let ccli = ccli.trim().trim_start_matches('#').trim();
        out.push(Song {
            id: uuid::Uuid::new_v4(), title: title.trim().to_string(), writers: author.trim().to_string(),
            ccli: if ccli == "0" { String::new() } else { ccli.to_string() }, copyright: copyright.trim().to_string(),
            lyrics, arrangement: Vec::new(),
        });
    }
    Ok(out)
}

/// Tidies converted words: trailing spaces off, at most one blank line in a row, none at the ends.
fn lyrics_text(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in text.lines().map(str::trim_end) {
        if line.is_empty() && out.last().is_none_or(|l| l.is_empty()) { continue }
        out.push(line);
    }
    while out.last().is_some_and(|l| l.is_empty()) { out.pop(); }
    out.join("\n")
}

/// Groups whose text is never shown.
const SKIP_DESTINATIONS: &[&str] = &[
    "fonttbl", "colortbl", "stylesheet", "info", "pict", "header", "footer", "headerl", "headerr", "footerl",
    "footerr", "listtable", "listoverridetable", "rsidtbl", "generator", "xmlnstbl", "themedata",
    "colorschememapping", "latentstyles", "datastore",
];

/// RTF to plain text: paragraphs and line breaks become newlines; `\'hh` is Windows-1252, `\uN` Unicode (with its
/// fallback characters skipped); font tables, colour tables and `\*` groups are dropped. Text that is not RTF is
/// returned with its newlines normalised.
pub fn rtf_to_text(input: &str) -> String {
    if !input.trim_start().starts_with("{\\rtf") { return input.replace("\r\n", "\n").replace('\r', "\n") }
    #[derive(Clone, Copy)]
    struct Group { skip: bool, uc: usize }
    let chars: Vec<char> = input.chars().collect();
    let (mut out, mut stack) = (String::new(), Vec::new());
    let mut st = Group { skip: false, uc: 1 };
    let (mut pending_skip, mut i, mut group_start) = (0usize, 0usize, false);
    while i < chars.len() {
        match chars[i] {
            '{' => { stack.push(st); group_start = true; i += 1; }
            '}' => { st = stack.pop().unwrap_or(Group { skip: false, uc: 1 }); group_start = false; i += 1; }
            '\\' => {
                i += 1;
                let Some(&n) = chars.get(i) else { break };
                if n.is_ascii_alphabetic() {
                    let start = i;
                    while i < chars.len() && chars[i].is_ascii_alphabetic() { i += 1; }
                    let word: String = chars[start..i].iter().collect();
                    let pstart = i;
                    if i < chars.len() && (chars[i] == '-' || chars[i].is_ascii_digit()) {
                        i += 1;
                        while i < chars.len() && chars[i].is_ascii_digit() { i += 1; }
                    }
                    let param: Option<i32> = chars[pstart..i].iter().collect::<String>().parse().ok();
                    if chars.get(i) == Some(&' ') { i += 1; }
                    if group_start && SKIP_DESTINATIONS.contains(&word.as_str()) { st.skip = true; }
                    group_start = false;
                    if st.skip { continue }
                    match word.as_str() {
                        "par" | "line" | "sect" | "page" => out.push('\n'),
                        "tab" => out.push('\t'),
                        "emdash" => out.push('\u{2014}'),
                        "endash" => out.push('\u{2013}'),
                        "lquote" => out.push('\u{2018}'),
                        "rquote" => out.push('\u{2019}'),
                        "ldblquote" => out.push('\u{201C}'),
                        "rdblquote" => out.push('\u{201D}'),
                        "uc" => st.uc = param.unwrap_or(1).max(0) as usize,
                        "u" => if let Some(mut v) = param {
                            if v < 0 { v += 65536; }
                            if let Some(c) = char::from_u32(v as u32) { out.push(c); }
                            pending_skip = st.uc;
                        },
                        _ => {}
                    }
                } else {
                    i += 1;
                    group_start = false;
                    match n {
                        '*' => st.skip = true,
                        '\'' => if i + 1 < chars.len() {
                            let hex: String = chars[i..i + 2].iter().collect();
                            i += 2;
                            if pending_skip > 0 { pending_skip -= 1 }
                            else if !st.skip { if let Ok(b) = u8::from_str_radix(&hex, 16) { out.push(cp1252(b)) } }
                        },
                        '\\' | '{' | '}' if !st.skip => out.push(n),
                        '~' if !st.skip => out.push('\u{00A0}'),
                        '\n' | '\r' if !st.skip => out.push('\n'),
                        _ => {}
                    }
                }
            }
            '\r' | '\n' => i += 1,
            c => {
                group_start = false;
                i += 1;
                if pending_skip > 0 { pending_skip -= 1; continue }
                if !st.skip { out.push(c) }
            }
        }
    }
    out
}

fn cp1252(b: u8) -> char {
    const HIGH: [u32; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D,
        0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A,
        0x0153, 0x9D, 0x017E, 0x0178,
    ];
    if (0x80..0xA0).contains(&b) { char::from_u32(HIGH[(b - 0x80) as usize]).unwrap_or('?') } else { b as char }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("rp-ew-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn fixture(dir: &Path) {
        let s = Connection::open(dir.join("Songs.db")).unwrap();
        s.execute_batch("CREATE TABLE song (rowid INTEGER PRIMARY KEY, title TEXT, author TEXT, copyright TEXT, reference_number TEXT);
            INSERT INTO song VALUES (1, 'Amazing Grace ', 'John Newton', 'Public Domain', '22025');
            INSERT INTO song VALUES (2, 'Empty Song', NULL, NULL, '0');").unwrap();
        let w = Connection::open(dir.join("SongWords.db")).unwrap();
        w.execute_batch("CREATE TABLE word (rowid INTEGER PRIMARY KEY, song_id INTEGER, words TEXT);").unwrap();
        w.execute("INSERT INTO word (song_id, words) VALUES (1, ?1)",
            [r"{\rtf1\ansi{\fonttbl{\f0 Tahoma;}}\f0 Verse 1\par Amazing grace how sweet the sound\par That saved a wretch like me\par\par\par Chorus\par My chains are gone\par}"]).unwrap();
    }

    #[test]
    fn reads_words_credits_and_ccli_from_copies() {
        let dir = temp();
        fixture(&dir);
        let songs = read_library(&dir, &dir.join("copy")).unwrap();
        assert_eq!(songs.len(), 2);
        let ag = &songs[0];
        assert_eq!((ag.title.as_str(), ag.writers.as_str(), ag.ccli.as_str(), ag.copyright.as_str()), ("Amazing Grace", "John Newton", "22025", "Public Domain"));
        assert_eq!(ag.lyrics, "Verse 1\nAmazing grace how sweet the sound\nThat saved a wretch like me\n\nChorus\nMy chains are gone");
        assert_eq!((songs[1].ccli.as_str(), songs[1].lyrics.as_str()), ("", ""));
        assert!(!dir.join("copy").exists(), "the copies are removed");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn finds_the_library_under_a_chosen_folder() {
        let root = temp();
        let data = root.join("Databases").join("Data");
        std::fs::create_dir_all(&data).unwrap();
        fixture(&data);
        assert_eq!(find_dir(Some(&root)), Some(data));
        assert_eq!(find_dir(Some(Path::new("/nowhere/rp"))), None);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rtf_escapes_and_hidden_groups() {
        assert_eq!(rtf_to_text("a\r\nb"), "a\nb");
        assert_eq!(rtf_to_text(r"{\rtf1 caf\'e9 \u8217? \{x\} {\*\generator Riched20;}end}"), "café \u{2019} {x} end");
        assert_eq!(rtf_to_text(r"{\rtf1{\colortbl;\red0\green0\blue0;}Hi\line there\par}"), "Hi\nthere\n");
    }
}
