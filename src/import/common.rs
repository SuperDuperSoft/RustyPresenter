//! Shared by the song importers: the report every importer returns, duplicate detection, the label lines of the
//! app's lyric format, and a safe read-only snapshot of another program's SQLite database.
use crate::{model::Song, slides::section_label};
use rusqlite::{backup::{Backup, StepResult}, Connection, OpenFlags};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Default)]
pub struct ImportReport {
    pub songs: Vec<Song>,
    /// (title, reason) for every row that did not become a song.
    pub skipped: Vec<(String, String)>,
}

/// How an imported song relates to the library (spec §9.2: a duplicate CCLI # asks update or keep both).
/// Songs with no CCLI # fall back to the title, so importing the same library twice doesn't double every hymn.
#[derive(Debug, Clone, PartialEq)]
pub enum Dedupe { New, DuplicateCcli { existing: Uuid }, DuplicateTitle { existing: Uuid } }

// ---------- duplicates ----------

/// "Amazing  Grace!" and "amazing grace" match.
fn title_key(title: &str) -> String {
    title.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect::<Vec<_>>().join(" ")
}

/// One entry per imported song. Matches the library first, then earlier songs in the same import.
/// By CCLI # when the song has one; by title only when it has none (two songs can share a title).
pub fn classify(existing: &[Song], imported: &[Song]) -> Vec<Dedupe> {
    let mut by_ccli: HashMap<String, Uuid> = HashMap::new();
    let mut by_title: HashMap<String, Uuid> = HashMap::new();
    for s in existing {
        if !s.ccli.trim().is_empty() { by_ccli.entry(s.ccli.trim().to_string()).or_insert(s.id); }
        by_title.entry(title_key(&s.title)).or_insert(s.id);
    }
    imported.iter().map(|s| {
        let c = s.ccli.trim();
        let t = title_key(&s.title);
        let found = if c.is_empty() {
            by_title.get(&t).map(|&existing| Dedupe::DuplicateTitle { existing })
        } else {
            by_ccli.get(c).map(|&existing| Dedupe::DuplicateCcli { existing })
        };
        if !c.is_empty() { by_ccli.entry(c.to_string()).or_insert(s.id); }
        by_title.entry(t).or_insert(s.id);
        found.unwrap_or(Dedupe::New)
    }).collect()
}

// ---------- lyric format ----------

/// Label line in the app's lyric format: as-is when `section_label` knows it, else `[bracketed]`.
/// Cut to fit `section_label`'s 40-byte limit, or the label would read back as a lyric line.
pub(crate) fn label_line(label: &str) -> String {
    let mut l = String::new();
    for c in label.chars() { if l.len() + c.len_utf8() > 38 { break; } l.push(c); }
    let l = l.trim_end();
    if section_label(l).is_some() { l.to_string() } else { format!("[{l}]") }
}

/// "Chorus (2)", "Chorus (3)", …: the first numbered form of `base` whose section name isn't `taken`. The
/// base is shortened so the number always survives `label_line`'s cut (a long name would otherwise give the
/// same section name for every number, and the search would never end). Bounded; falls back to `base`.
pub(crate) fn numbered_label(base: &str, taken: impl Fn(&str) -> bool) -> String {
    for n in 2..=100_000usize {
        let suffix = format!(" ({n})");
        let mut cut = String::new();
        for c in base.chars() { if cut.len() + c.len_utf8() + suffix.len() > 38 { break; } cut.push(c); }
        let label = format!("{}{suffix}", cut.trim_end());
        if !taken(&section_name(Some(&label))) { return label; }
    }
    base.to_string()
}

/// A lyric line as written into the app's format: one `section_label` would read as a label ("Instrumental",
/// "Tag", "[Repeat]", common placeholder slides) is put in parentheses so it stays words on the slide.
pub(crate) fn lyric_line(line: &str) -> String {
    if section_label(line).is_none() { return line.to_string(); }
    let t = line.trim();
    let inner = if t.starts_with('[') && t.ends_with(']') { t.trim_matches(['[', ']']).trim() } else { t };
    format!("({inner})")
}

/// The name `parse_sections` gives a label, which is what `arrangement` must list.
pub(crate) fn section_name(label: Option<&str>) -> String {
    label.map(|l| section_label(&label_line(l)).unwrap_or_default()).unwrap_or_else(|| "Part 1".into())
}

// ---------- read-only database snapshot ----------

/// How long to wait for the other program to finish a save: `STABLE_TRIES` × `STABLE_PAUSE`.
pub(crate) const STABLE_TRIES: usize = 40;
pub(crate) const STABLE_PAUSE: Duration = Duration::from_millis(125);

/// The files SQLite needs for one database: the file, its WAL, and a rollback journal (a save in progress
/// in the old journal mode; SQLite rolls it back on our copy). `-shm` is only an index SQLite rebuilds.
pub(crate) fn database_files(db: &Path) -> Vec<PathBuf> {
    let name = db.file_name().unwrap_or_default().to_string_lossy().into_owned();
    vec![db.to_path_buf(), db.with_file_name(format!("{name}-wal")), db.with_file_name(format!("{name}-journal"))]
}

/// Reads every file until two reads in a row match, so a save can't tear the copy, and every file is from
/// the same moment. A missing file is None. `app` names the program in the error ("Close OpenLP …").
pub(crate) fn read_stable(paths: &[PathBuf], tries: usize, pause: Duration, app: &str) -> Result<Vec<Option<Vec<u8>>>, String> {
    let read_all = || -> Result<Vec<Option<Vec<u8>>>, String> {
        paths.iter().map(|p| match std::fs::read(p) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(crate::tr!("Couldn't read {path}: {error}.", path = p.display(), error = e)),
        }).collect()
    };
    let mut last = read_all()?;
    for _ in 0..tries {
        std::thread::sleep(pause);
        let now = read_all()?;
        if now == last { return Ok(now); }
        last = now;
    }
    Err(crate::tr!("{app} kept saving changes while the library was being read. Close {app} and try again.", app = app))
}

/// A private temp folder, removed on drop. Retries because Windows can hold a just-closed file for a moment.
pub(crate) struct TempDir(pub(crate) PathBuf);

impl TempDir {
    pub(crate) fn new() -> std::io::Result<Self> {
        let d = std::env::temp_dir().join(format!("rp-import-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&d)?;
        Ok(Self(d))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        for _ in 0..20 {
            if std::fs::remove_dir_all(&self.0).is_ok() || !self.0.exists() { return; }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Consistent in-memory copies of the databases, from a quiescent copy of their files (all from the same
/// moment). SQLite never opens the other program's own files, so nothing (`-wal`, `-shm`, `-journal`) is
/// created or changed next to them. File names must differ.
pub(crate) fn snapshot(dbs: &[&Path], app: &str) -> Result<Vec<Connection>, String> {
    let paths: Vec<PathBuf> = dbs.iter().flat_map(|d| database_files(d)).collect();
    let files = read_stable(&paths, STABLE_TRIES, STABLE_PAUSE, app)?;
    let tmp = TempDir::new().map_err(|e| crate::tr!("Couldn't make a temporary folder: {error}.", error = e))?;
    for (p, bytes) in paths.iter().zip(&files) {
        let Some(bytes) = bytes else { continue };
        std::fs::write(tmp.0.join(p.file_name().unwrap_or_default()), bytes).map_err(|e| crate::tr!("Couldn't copy {path}: {error}.", path = p.display(), error = e))?;
    }
    dbs.iter().map(|p| backup_into_memory(&tmp.0.join(p.file_name().unwrap_or_default()))
        .map_err(|e| crate::tr!("Couldn't open {path}: {error}.", path = p.display(), error = e))).collect()
}

/// How often to retry a backup step that finds the database locked: `BUSY_TRIES` × `BUSY_PAUSE`.
const BUSY_TRIES: u32 = 40;
const BUSY_PAUSE: Duration = Duration::from_millis(50);

/// Copies a database into memory in one step (one read lock, so one snapshot). Gives up with SQLITE_BUSY
/// when the file stays locked, instead of spinning forever.
pub(crate) fn backup_into_memory(path: &Path) -> rusqlite::Result<Connection> {
    let src = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    let mut dst = Connection::open_in_memory()?;
    {
        let backup = Backup::new(&src, &mut dst)?;
        let mut busy = 0;
        loop {
            match backup.step(-1)? {
                StepResult::Done => break,
                StepResult::More => {}
                _ => {
                    busy += 1;
                    if busy > BUSY_TRIES {
                        return Err(rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                            Some(crate::tr!("the database stayed locked"))));
                    }
                    std::thread::sleep(BUSY_PAUSE);
                }
            }
        }
    }
    Ok(dst)
}

/// The first 16 bytes are SQLite's magic. Err says the file is empty or damaged; `not_sqlite` is the message
/// for a file that has content but is something else.
pub(crate) fn check_sqlite(path: &Path, not_sqlite: &str) -> Result<(), String> {
    use std::io::Read;
    let mut head = Vec::new();
    std::fs::File::open(path).and_then(|f| f.take(16).read_to_end(&mut head)).map_err(|e| crate::tr!("Couldn't read {path}: {error}.", path = path.display(), error = e))?;
    if head == b"SQLite format 3\0" { return Ok(()); }
    if head.len() < 16 { return Err(crate::tr!("{path} is empty or damaged, so there is nothing to import.", path = path.display())); }
    Err(not_sqlite.into())
}

/// A file in `dir` whose name matches one of `names`, ignoring case.
pub(crate) fn find_file(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path())
        .find(|p| p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| names.iter().any(|x| n.eq_ignore_ascii_case(x))))
}

/// Files under `dir` that `keep` accepts, sorted, not following links (so a link loop can't trap the walk), at
/// most `max_depth` folders down. The bool says the walk stopped after `max` entries.
pub(crate) fn find_files(dir: &Path, max: usize, max_depth: usize, keep: impl Fn(&Path) -> bool) -> (Vec<PathBuf>, bool) {
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
            if seen > max { return (out, true); }
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            // macOS leaves "._name" resource-fork files next to the real ones on shared drives.
            if e.file_name().to_string_lossy().starts_with("._") { continue; }
            if ft.is_dir() && depth < max_depth { subdirs.push((p, depth + 1)); } else if ft.is_file() && keep(&p) { out.push(p); }
        }
        stack.extend(subdirs.into_iter().rev());
    }
    (out, false)
}

pub(crate) fn value_text(v: rusqlite::types::ValueRef) -> String {
    use rusqlite::types::ValueRef;
    match v {
        ValueRef::Null => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(b) | ValueRef::Blob(b) => String::from_utf8_lossy(b).into_owned(),
    }
}

pub(crate) fn value_int(v: rusqlite::types::ValueRef) -> Option<i64> {
    use rusqlite::types::ValueRef;
    match v { ValueRef::Integer(i) => Some(i), ValueRef::Text(b) => std::str::from_utf8(b).ok()?.trim().parse().ok(), _ => None }
}

/// Column names of `table`; empty when the table doesn't exist.
pub(crate) fn columns(db: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    db.prepare(&format!("PRAGMA table_info(\"{table}\")"))?.query_map([], |r| r.get::<_, String>(1))?.collect()
}
