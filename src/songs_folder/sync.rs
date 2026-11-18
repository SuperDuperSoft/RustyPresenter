//! Keeps the songs folder and the song library the same, both ways (design note of November 17, 2026). The data
//! file stays the app's own record: services point at song ids, and backups and Undo work as before. The folder is
//! the church's open copy.
//!
//! Each look (`Folder::scan`, about every two seconds, off the window's thread) finds the files that changed since
//! the last look and have stopped changing (a file is taken once its size and time stayed the same for two looks,
//! or it was last changed a few seconds ago, so a copy or a OneDrive download in progress is never read half
//! done). A file the app wrote itself is recognised by its contents and left alone. Then `Folder::write` writes
//! every song added or changed in the app since, renames the file of a song whose title changed, and moves the
//! file of a song removed from the library into the "Removed" folder (nothing is ever deleted).
//!
//! What `scan` found comes into the library through `apply`:
//! - a file of the app's (it has `{x_rustypresenter_id}`) changed outside, e.g. in Notepad: the song changes. When
//!   the song was changed in the app too since the last look, both are kept: the folder's version comes in as
//!   "<title> (from the folder)" and the app's own goes back into its file.
//! - a new file in any format the app reads (ChordPro, SongSelect `.txt` / `.usr`, OpenLyrics or OpenSong `.xml`,
//!   plain text with section names): a new song, unless the library already has it with the same words. The file
//!   stays as it is, and the app writes the song's own ChordPro file beside it; when the original changes later the
//!   song follows. With "Tidy imported files into ChordPro" ticked, the original is moved into the "Imported
//!   files" folder instead, so only the ChordPro file stays.
//! - a file that cannot be read is listed in Settings and never touched.
//!
//! A file deleted from the folder by hand is written again: songs are removed in the app (services may use them).
//! Every write is a whole new file renamed over the old one (`store::write_atomic`), so a crash leaves the old file;
//! a file that is busy (OneDrive, a network drive) is tried again at the next look.
use super::{format, names};
use crate::{model::Song, sha256::digest_hex};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

/// Where the files of songs removed from the library go.
pub const REMOVED: &str = "Removed";
/// Where imported files go when "Tidy imported files into ChordPro" is ticked.
pub const IMPORTED: &str = "Imported files";
/// A file bigger than this is not a song.
pub const MAX_BYTES: u64 = 1_000_000;
/// A file last changed longer ago than this is finished, even on the first look.
const SETTLED: Duration = Duration::from_secs(3);
/// The file endings read as songs (and files with no ending, which OpenSong uses).
pub const SONG_ENDINGS: &[&str] = &["chordpro", "cho", "chopro", "crd", "pro", "txt", "usr", "xml"];

/// A file's size and last change, to tell when it changed.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Stamp { pub len: u64, pub modified: Option<u128> }

fn stamp_of(meta: &fs::Metadata) -> Stamp {
    Stamp { len: meta.len(), modified: meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()) }
}

/// What the app knows of one song: its file and the song as it was when the two were last the same.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SongEntry { pub file: String, pub hash: String }

/// What the app knows of one file: the song it holds, whether the app wrote it (`own`: the song's ChordPro file;
/// else a file brought in), and the file as last seen.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileEntry { pub song: Uuid, pub own: bool, pub stamp: Stamp, pub hash: String }

/// Kept in the data folder (`songs-folder.json`), so a restart knows what it wrote.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct State {
    pub folder: PathBuf,
    pub songs: BTreeMap<Uuid, SongEntry>,
    pub files: BTreeMap<String, FileEntry>,
    /// Songs whose files were moved into "Removed": a file put back brings its song back.
    pub removed: BTreeSet<Uuid>,
}

/// What the songs folder did, for the short messages at the bottom of the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Note {
    CameIn(String),
    Updated(String),
    /// (title, the title the folder's version was kept under)
    KeptBoth(String, String),
    BroughtBack(String),
    /// (file, why)
    Unreadable(String, String),
}

/// A change to the library found in the folder.
#[derive(Clone, Debug)]
pub enum Change {
    /// The song with this id becomes `song`, if the app's song is still the one seen (`expect`, its hash);
    /// otherwise both are kept.
    Update { song: Song, expect: String },
    Add { song: Song, note: Note },
}

/// The hash a song is compared by: of its file's text, so two songs that write the same file are the same.
pub fn song_hash(song: &Song) -> String { digest_hex(format::write(song).as_bytes()) }

/// The title the folder's version of a song is kept under when both changed.
pub fn from_the_folder(title: &str) -> String { crate::tr!("{title} (from the folder)", title = title) }

/// Puts what `scan` found into the library. Runs with the library locked, so the check against `expect` is exact.
pub fn apply(songs: &mut Vec<Song>, changes: Vec<Change>) -> Vec<Note> {
    let mut notes = Vec::new();
    for c in changes {
        match c {
            Change::Update { song, expect } => match songs.iter_mut().find(|s| s.id == song.id) {
                Some(mine) if song_hash(mine) == expect => {
                    notes.push(Note::Updated(song.title.clone()));
                    *mine = song;
                }
                Some(mine) => {
                    let copy = Song { id: Uuid::new_v4(), title: from_the_folder(&song.title), ..song };
                    notes.push(Note::KeptBoth(mine.title.clone(), copy.title.clone()));
                    songs.push(copy);
                }
                None => {
                    notes.push(Note::BroughtBack(song.title.clone()));
                    songs.push(song);
                }
            },
            Change::Add { song, note } => {
                let song = if songs.iter().any(|s| s.id == song.id) { Song { id: Uuid::new_v4(), ..song } } else { song };
                notes.push(note);
                songs.push(song);
            }
        }
    }
    notes
}

/// A song read from a file someone else made: what that file cannot carry stays as the app has it.
fn keep_own_details(from_file: &mut Song, mine: &Song) {
    from_file.lines_per_slide = mine.lines_per_slide;
    from_file.text_size = mine.text_size;
    from_file.transpose = mine.transpose;
    if from_file.arrangement.is_empty() { from_file.arrangement = mine.arrangement.clone() }
    for (theirs, ours) in [(&mut from_file.key, &mine.key), (&mut from_file.artist, &mine.artist), (&mut from_file.publisher, &mine.publisher),
        (&mut from_file.year, &mine.year), (&mut from_file.writers, &mine.writers), (&mut from_file.copyright, &mine.copyright), (&mut from_file.ccli, &mine.ccli)] {
        if theirs.is_empty() { *theirs = ours.clone() }
    }
}

/// Reads a song file of any format the app reads. The bool says whether the songs folder wrote it.
pub fn read_any(name: &str, bytes: &[u8]) -> Result<(Song, bool), String> {
    use crate::{import::{chordpro, openlyrics, opensong}, songselect};
    let ext = Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let text = chordpro::decode(bytes);
    if format::is_ours(&text) { return format::read(&text).map(|s| (s, true)) }
    let looks_xml = text.trim_start_matches('\u{feff}').trim_start().starts_with('<');
    let song = match ext.as_str() {
        "usr" => songselect::usr::parse(&text),
        "xml" | "" if looks_xml => openlyrics::parse_str(&text).or_else(|e| opensong::parse_str(&text).map_err(|_| e)),
        "" => Err(crate::tr!("it is not a song file")),
        "txt" if !text.contains('{') => plain_text(&stem, &text),
        "pro" if !text.contains('{') && !text.contains('[') => Err(crate::tr!("it is not a song file")),
        _ => songselect::inbox::read_song_file(name, bytes).map(|mut s| { if s.title.trim().is_empty() { s.title = stem.clone() } s }),
    }?;
    Ok((song, false))
}

/// A plain text song: its title on the first line (SongSelect's "Lyrics" download), or, when the text starts with a
/// section name, the file's name as its title.
fn plain_text(stem: &str, text: &str) -> Result<Song, String> {
    let first = text.trim_start_matches('\u{feff}').lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if crate::slides::section_label(first).is_some() { crate::songselect::import_txt(&format!("{stem}\n\n{text}")) } else { crate::songselect::import_txt(text) }
}

/// "Amazing  Grace!" and "amazing grace" match.
fn title_key(title: &str) -> String {
    title.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect::<Vec<_>>().join(" ")
}

/// One songs folder and what the app knows of it.
pub struct Folder {
    pub dir: PathBuf,
    state_path: PathBuf,
    pub state: State,
    saved: State,
    /// Files seen changing, waiting for a second look.
    pending: HashMap<String, Stamp>,
    /// Files the app knew that are gone, and for how many looks.
    missing: HashMap<String, u8>,
    /// Files that could not be read, as they were then, and why.
    pub unreadable: BTreeMap<String, (Stamp, String)>,
    /// Files that could not be written or moved this look (busy): tried again at the next one.
    pub retrying: BTreeSet<String>,
    pub last_change: Option<SystemTime>,
}

impl Folder {
    /// The folder `dir`, with what the app knew of it from `state_path` (nothing when it knew another folder).
    pub fn open(dir: PathBuf, state_path: PathBuf) -> Folder {
        let state = fs::read(&state_path).ok().and_then(|b| serde_json::from_slice::<State>(&b).ok()).filter(|s| s.folder == dir)
            .unwrap_or_else(|| State { folder: dir.clone(), ..Default::default() });
        Folder { dir, state_path, saved: state.clone(), state, pending: HashMap::new(), missing: HashMap::new(), unreadable: BTreeMap::new(),
            retrying: BTreeSet::new(), last_change: None }
    }

    /// Writes what the app knows of the folder, when it changed.
    pub fn save_state(&mut self) {
        if self.state == self.saved { return }
        if let Ok(bytes) = serde_json::to_vec_pretty(&self.state) && crate::store::write_atomic(&self.state_path, &bytes).is_ok() {
            self.saved = self.state.clone();
        }
    }

    /// The song files in the folder now (not in its subfolders), with their stamps.
    fn list(&self) -> BTreeMap<String, Stamp> {
        let mut out = BTreeMap::new();
        let Ok(entries) = fs::read_dir(&self.dir) else { return out };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            // Hidden files, the app's half-written ones (`.rp-….tmp`) and editors' lock files ("~$name") are not songs.
            if name.starts_with('.') || name.starts_with('~') { continue }
            let ext = Path::new(&name).extension().map(|x| x.to_string_lossy().to_lowercase());
            if ext.as_deref().is_some_and(|x| !SONG_ENDINGS.contains(&x)) { continue }
            let Ok(meta) = e.metadata() else { continue };
            if !meta.is_file() { continue }
            out.insert(name, stamp_of(&meta));
        }
        out
    }

    fn stamp_now(&self, name: &str) -> Option<Stamp> { fs::metadata(self.dir.join(name)).ok().filter(|m| m.is_file()).map(|m| stamp_of(&m)) }

    /// Whether a changed file has stopped changing: the same as at the last look, or last changed a while ago.
    fn settled(&self, name: &str, stamp: Stamp) -> bool {
        if self.pending.get(name) == Some(&stamp) { return true }
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        stamp.modified.is_some_and(|m| now.saturating_sub(m) > SETTLED.as_nanos())
    }

    /// One look at the folder: the changes to bring into the library (`apply`) and the files that could not be read.
    /// `songs` is the library now.
    pub fn scan(&mut self, songs: &[Song], tidy: bool) -> (Vec<Change>, Vec<Note>) {
        let now = self.list();
        let by_id: HashMap<Uuid, &Song> = songs.iter().map(|s| (s.id, s)).collect();
        let mut changes = Vec::new();
        let mut notes = Vec::new();
        let mut pending = HashMap::new();
        for (name, &stamp) in &now {
            if self.state.files.get(name).is_some_and(|e| e.stamp == stamp) { continue }
            if self.unreadable.get(name).is_some_and(|(s, _)| *s == stamp) { continue }
            if stamp.len > MAX_BYTES {
                self.unreadable.insert(name.clone(), (stamp, crate::tr!("it is too big to be a song file")));
                continue;
            }
            if !self.settled(name, stamp) { pending.insert(name.clone(), stamp); continue }
            // A file that is busy, or changed while being read, is looked at again next time.
            let bytes = match fs::read(self.dir.join(name)) { Ok(b) => b, Err(_) => { pending.insert(name.clone(), stamp); continue } };
            if self.stamp_now(name) != Some(stamp) { pending.insert(name.clone(), stamp); continue }
            let hash = digest_hex(&bytes);
            if let Some(e) = self.state.files.get_mut(name) && e.hash == hash { e.stamp = stamp; continue }
            self.unreadable.remove(name);
            match read_any(name, &bytes) {
                Err(why) => {
                    notes.push(Note::Unreadable(name.clone(), why.clone()));
                    self.unreadable.insert(name.clone(), (stamp, why));
                }
                Ok((song, ours)) => {
                    self.last_change = Some(SystemTime::now());
                    self.take(name, stamp, hash, song, ours, &by_id, &now, tidy, &mut changes);
                }
            }
        }
        self.pending = pending;
        self.unreadable.retain(|n, _| now.contains_key(n));
        // Files the app knew that are gone: after two looks with nothing else under way (a rename shows as a new
        // file first), forgotten. A song whose own file went is written again by `write`.
        let gone: Vec<String> = self.state.files.keys().filter(|n| !now.contains_key(*n)).cloned().collect();
        self.missing.retain(|n, _| gone.contains(n));
        for name in gone {
            let looks = self.missing.entry(name.clone()).or_insert(0);
            *looks = looks.saturating_add(1);
            if *looks < 2 || !self.pending.is_empty() { continue }
            self.missing.remove(&name);
            if let Some(e) = self.state.files.remove(&name) && self.state.songs.get(&e.song).is_some_and(|s| s.file == name) {
                self.state.songs.remove(&e.song);
            }
        }
        (changes, notes)
    }

    /// What one changed or new file means for the library.
    #[allow(clippy::too_many_arguments)]
    fn take(&mut self, name: &str, stamp: Stamp, hash: String, mut song: Song, ours: bool, by_id: &HashMap<Uuid, &Song>, now: &BTreeMap<String, Stamp>,
        tidy: bool, changes: &mut Vec<Change>) {
        if let Some(e) = self.state.files.get(name).cloned() {
            // A file the app knew, changed outside.
            let id = e.song;
            song.id = id;
            self.state.files.insert(name.to_string(), FileEntry { hash, stamp, ..e.clone() });
            let Some(mine) = by_id.get(&id) else {
                // Its song was removed meanwhile: it comes back rather than lose the change.
                if e.own && ours { self.state.songs.insert(id, SongEntry { file: name.to_string(), hash: song_hash(&song) }); }
                changes.push(Change::Add { note: Note::BroughtBack(song.title.clone()), song });
                return;
            };
            if !ours { keep_own_details(&mut song, mine) }
            let (mine_hash, theirs) = (song_hash(mine), song_hash(&song));
            if mine_hash == theirs { return }
            let synced = self.state.songs.get(&id).map(|s| s.hash.clone());
            if synced.is_some() && synced.as_deref() != Some(mine_hash.as_str()) {
                // Changed here and there: both kept. `write` puts the app's version back into this song's file.
                let copy = Song { id: Uuid::new_v4(), title: from_the_folder(&song.title), ..song.clone() };
                changes.push(Change::Add { note: Note::KeptBoth(mine.title.clone(), copy.title.clone()), song: copy });
                return;
            }
            // The song's own file, as the app writes it: nothing to write back (unless a new title renames it).
            if e.own && ours && mine.title == song.title && let Some(s) = self.state.songs.get_mut(&id) { s.hash = theirs }
            changes.push(Change::Update { song, expect: mine_hash });
            return;
        }
        if ours {
            let id = song.id;
            if let Some(mine) = by_id.get(&id) {
                let mapped = self.state.songs.get(&id).map(|s| s.file.clone());
                if mapped.as_ref().is_some_and(|f| f != name && now.contains_key(f)) {
                    // A copy of a song's file: a new song. It is written again under its own id.
                    let copy = Song { id: Uuid::new_v4(), ..song };
                    self.own(name, copy.id, stamp, hash, String::new());
                    changes.push(Change::Add { note: Note::CameIn(copy.title.clone()), song: copy });
                    return;
                }
                // The song's file under a new name (renamed, or the folder used from another computer).
                if let Some(old) = mapped.filter(|f| f != name) { self.state.files.remove(&old); }
                let (mine_hash, theirs) = (song_hash(mine), song_hash(&song));
                let synced = self.state.songs.get(&id).map(|s| s.hash.clone());
                if mine_hash == theirs {
                    self.own(name, id, stamp, hash, theirs);
                } else if synced.as_deref() == Some(mine_hash.as_str()) {
                    self.own(name, id, stamp, hash, theirs);
                    changes.push(Change::Update { song, expect: mine_hash });
                } else {
                    // Nothing says which is newer: both kept, and the app's version written into the file.
                    self.own(name, id, stamp, hash, String::new());
                    let copy = Song { id: Uuid::new_v4(), title: from_the_folder(&song.title), ..song };
                    changes.push(Change::Add { note: Note::KeptBoth(mine.title.clone(), copy.title.clone()), song: copy });
                }
                return;
            }
            // A song the library does not have (removed here, put back from "Removed", or from another library).
            let back = self.state.removed.remove(&id) || self.state.songs.contains_key(&id);
            let song = if id.is_nil() { Song { id: Uuid::new_v4(), ..song } } else { song };
            let theirs = song_hash(&song);
            self.own(name, song.id, stamp, hash, theirs);
            let note = if back { Note::BroughtBack(song.title.clone()) } else { Note::CameIn(song.title.clone()) };
            changes.push(Change::Add { note, song });
            return;
        }
        // A file from elsewhere. The library may have this song already, with the same words.
        let same = by_id.values().find(|s| {
            let same_song = if song.ccli.trim().is_empty() { title_key(&s.title) == title_key(&song.title) } else { s.ccli.trim() == song.ccli.trim() };
            same_song && crate::songselect::inbox::same_words(&s.lyrics, &song.lyrics)
        });
        let id = match same {
            Some(s) => s.id,
            None => {
                let id = Uuid::new_v4();
                changes.push(Change::Add { note: Note::CameIn(song.title.clone()), song: Song { id, ..song } });
                id
            }
        };
        if tidy && self.move_into(name, IMPORTED) { return }
        self.state.files.insert(name.to_string(), FileEntry { song: id, own: false, stamp, hash });
    }

    /// Notes `name` as song `id`'s own file. An empty `hash` makes `write` write the song into it again.
    fn own(&mut self, name: &str, id: Uuid, stamp: Stamp, file_hash: String, song_hash: String) {
        self.state.files.insert(name.to_string(), FileEntry { song: id, own: true, stamp, hash: file_hash });
        self.state.songs.insert(id, SongEntry { file: name.to_string(), hash: song_hash });
        self.state.removed.remove(&id);
    }

    /// Moves `name` into the subfolder `sub` under a name not used there. False when it could not be moved now.
    fn move_into(&mut self, name: &str, sub: &str) -> bool {
        let to_dir = self.dir.join(sub);
        if fs::create_dir_all(&to_dir).is_err() { self.retrying.insert(name.to_string()); return false }
        let p = Path::new(name);
        let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = p.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let target = if ext.is_empty() {
            (1..).map(|n| if n == 1 { stem.clone() } else { format!("{stem} ({n})") }).find(|n| !to_dir.join(n).exists()).unwrap_or(stem)
        } else {
            let there: BTreeSet<String> = fs::read_dir(&to_dir).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect()).unwrap_or_default();
            names::unique(&stem, &ext, |n| there.contains(n))
        };
        match fs::rename(self.dir.join(name), to_dir.join(target)) {
            Ok(()) => { self.retrying.remove(name); true }
            Err(_) => { self.retrying.insert(name.to_string()); false }
        }
    }

    /// Writes the library into the folder: songs added or changed since the last look, renamed songs' files, and
    /// removed songs' files into "Removed". A file changed outside and not yet looked at waits for `scan`.
    pub fn write(&mut self, songs: &[Song]) {
        if fs::create_dir_all(&self.dir).is_err() { return }
        let mut on_disk: BTreeSet<String> = fs::read_dir(&self.dir).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect()).unwrap_or_default();
        let changed_outside = |me: &Folder, file: &str| me.pending.contains_key(file)
            || me.state.files.get(file).is_some_and(|e| me.stamp_now(file).is_some_and(|s| s != e.stamp));
        for song in songs {
            let text = format::write(song);
            let hash = digest_hex(text.as_bytes());
            let entry = self.state.songs.get(&song.id).cloned();
            if entry.as_ref().is_some_and(|e| e.hash == hash) { continue }
            // New songs wait while files are still arriving: one of them may be this song's file.
            if entry.is_none() && !self.pending.is_empty() { continue }
            if let Some(e) = &entry && changed_outside(self, &e.file) { continue }
            let keep = entry.as_ref().map(|e| e.file.clone()).filter(|f| names::fits(f, &song.title, format::EXT));
            let target = keep.clone().unwrap_or_else(|| names::unique(&names::stem(&song.title), format::EXT, |n| on_disk.contains(n)));
            if crate::store::write_atomic(&self.dir.join(&target), text.as_bytes()).is_err() { self.retrying.insert(target); continue }
            self.retrying.remove(&target);
            on_disk.insert(target.to_lowercase());
            if let Some(old) = entry.map(|e| e.file).filter(|f| *f != target) {
                // Renamed: the old file held this song as the app wrote it, and the new one has it now.
                if self.state.files.get(&old).is_some_and(|e| e.own) && fs::remove_file(self.dir.join(&old)).is_ok() { on_disk.remove(&old.to_lowercase()); }
                self.state.files.remove(&old);
            }
            let stamp = self.stamp_now(&target).unwrap_or_default();
            self.own(&target, song.id, stamp, hash.clone(), hash);
            self.last_change = Some(SystemTime::now());
        }
        // Songs removed from the library: their files go into "Removed", with any file they were brought in from.
        let ids: BTreeSet<Uuid> = songs.iter().map(|s| s.id).collect();
        let gone: Vec<Uuid> = self.state.songs.keys().filter(|id| !ids.contains(id)).copied().collect();
        for id in gone {
            let files: Vec<String> = self.state.files.iter().filter(|(_, e)| e.song == id).map(|(n, _)| n.clone()).collect();
            if files.iter().any(|f| changed_outside(self, f)) { continue }
            let mut all = true;
            for f in &files {
                if self.dir.join(f).exists() && !self.move_into(f, REMOVED) { all = false; continue }
                self.state.files.remove(f);
            }
            if all {
                self.state.songs.remove(&id);
                self.state.removed.insert(id);
                self.last_change = Some(SystemTime::now());
            }
        }
        // Files brought in for songs that are gone and had no file of their own.
        let orphans: Vec<String> = self.state.files.iter().filter(|(_, e)| !e.own && !ids.contains(&e.song) && !self.state.songs.contains_key(&e.song)).map(|(n, _)| n.clone()).collect();
        for f in orphans {
            if !self.dir.join(&f).exists() || self.move_into(&f, REMOVED) { self.state.files.remove(&f); }
        }
    }

    /// Forgets what the last look found (the library could not take it): the next look finds it again.
    pub fn forget_look(&mut self) {
        self.state = self.saved.clone();
        self.pending.clear();
    }

    /// How many songs have their own file in the folder.
    pub fn count(&self) -> usize { self.state.songs.len() }
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
