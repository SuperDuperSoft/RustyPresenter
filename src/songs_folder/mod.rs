//! Settings → Songs → "Keep my songs in a folder" (Jake, November 17, 2026: "I need a toggleable folder where I can
//! just store all of my songs, where I know where RustyPresenter stores all the songs, but I can also upload songs
//! too"). When it is on, every song in the library is a ChordPro file in one folder (`format`, `names`), kept the
//! same as the library both ways (`sync`): songs added, changed, renamed or removed in the app change their files,
//! and song files dropped into the folder or edited there change the library. See
//! docs/superpowers/specs/2026-11-17-songs-folder-design.md.
//!
//! One thread (`start`) looks at the folder about every two seconds, and at once when Settings changes (`wake`). It
//! never runs on the window's thread. The window reads `status` for Settings and `take_notes` for its messages.
use crate::{app::App, tr, trn};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, Weak, mpsc},
    time::{Duration, SystemTime},
};

pub mod format;
pub mod names;
pub mod sync;

use sync::{Folder, Note};

/// How often the folder is looked at.
pub const EVERY: Duration = Duration::from_secs(2);

/// The choices in Settings → Songs (`Settings::songs_folder`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Prefs {
    /// Keep the songs in a folder (off by default).
    pub on: bool,
    /// The folder; None = `default_folder()`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
    /// "Tidy imported files into ChordPro": a song file of another kind dropped in the folder is moved into
    /// "Imported files" once it is in, so only ChordPro files stay.
    pub tidy: bool,
}

impl Prefs {
    /// The folder the songs are kept in.
    pub fn dir(&self) -> PathBuf { self.folder.clone().unwrap_or_else(default_folder) }
}

/// Documents\RustyPresenter\Songs (on macOS and Linux the same under the person's Documents folder). A portable
/// copy keeps them in its own data folder beside the app, so they travel with it.
pub fn default_folder() -> PathBuf {
    #[cfg(test)]
    { std::env::temp_dir().join("rp-test-documents").join("RustyPresenter").join("Songs") }
    #[cfg(not(test))]
    {
        if let Some(d) = crate::platform::data_folder::portable_dir() { return d.join("Songs") }
        dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(std::env::temp_dir).join("RustyPresenter").join("Songs")
    }
}

/// What Settings shows under the choice.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub on: bool,
    pub folder: PathBuf,
    /// Songs with their file in the folder.
    pub songs: usize,
    pub last_change: Option<SystemTime>,
    /// (file, why) for each file that could not be read.
    pub unreadable: Vec<(String, String)>,
    /// Files that are busy and will be tried again.
    pub retrying: usize,
    /// The folder itself could not be used: why.
    pub problem: Option<String>,
}

/// Shared by the thread and the window.
#[derive(Default)]
struct Shared {
    wake: Mutex<Option<mpsc::Sender<()>>>,
    status: Mutex<Status>,
    notes: Mutex<Vec<(String, bool)>>,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(Shared::default)
}

/// Starts the thread that keeps the folder (the app's binary does, once). It ends with the app.
pub fn start(app: &Arc<App>) {
    let (tx, rx) = mpsc::channel();
    *shared().wake.lock().unwrap() = Some(tx);
    let app: Weak<App> = Arc::downgrade(app);
    let _ = std::thread::Builder::new().name("songs-folder".into()).spawn(move || {
        let mut runner = Runner::default();
        loop {
            let Some(app) = app.upgrade() else { return };
            runner.tick(&app);
            drop(app);
            if matches!(rx.recv_timeout(EVERY), Err(mpsc::RecvTimeoutError::Disconnected)) { std::thread::sleep(EVERY) }
        }
    });
}

/// Looks at the folder now (Settings changed).
pub fn wake() { if let Some(tx) = shared().wake.lock().unwrap().as_ref() { let _ = tx.send(()); } }

/// What Settings shows.
pub fn status() -> Status { shared().status.lock().unwrap().clone() }

/// The messages since the last call: (text, is a problem).
pub fn take_notes() -> Vec<(String, bool)> { std::mem::take(&mut *shared().notes.lock().unwrap()) }

/// The thread's side: the folder in use, if the choice is on.
#[derive(Default)]
pub struct Runner {
    folder: Option<Folder>,
    /// The messages of the last look (also handed to the window through `take_notes`).
    pub last_notes: Vec<(String, bool)>,
}

impl Runner {
    /// One look: the folder's changes into the library, then the library into the folder.
    pub fn tick(&mut self, app: &App) -> Status {
        let (prefs, songs) = { let d = app.data(); (d.settings.songs_folder.clone(), d.songs.clone()) };
        let dir = prefs.dir();
        let mut status = Status { on: prefs.on, folder: dir.clone(), ..Default::default() };
        if !prefs.on || app.read_only() {
            // Off: the files stay where they are; nothing more is written.
            if let Some(mut f) = self.folder.take() { f.save_state() }
            return self.publish(status, vec![]);
        }
        if self.folder.as_ref().is_none_or(|f| f.dir != dir) {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                status.problem = Some(tr!("The songs folder could not be made: {error}", error = e));
                self.folder = None;
                return self.publish(status, vec![]);
            }
            self.folder = Some(Folder::open(dir, app.store.root.join("songs-folder.json")));
        }
        let Some(folder) = self.folder.as_mut() else { return self.publish(status, vec![]) };
        if !folder.dir.is_dir() {
            status.problem = Some(tr!("The songs folder cannot be found. Is the drive connected?"));
            return self.publish(status, vec![]);
        }
        let (changes, mut notes) = folder.scan(&songs, prefs.tidy);
        let mut ok = true;
        if !changes.is_empty() {
            match app.edit_untracked(|d| Ok(sync::apply(&mut d.songs, changes))) {
                Ok(n) => notes.extend(n),
                Err(e) => { ok = false; folder.forget_look(); status.problem = Some(e) }
            }
        }
        if ok {
            let songs = app.data().songs.clone();
            folder.write(&songs);
            folder.save_state();
        }
        status.songs = folder.count();
        status.last_change = folder.last_change.or_else(|| std::fs::metadata(&folder.dir).and_then(|m| m.modified()).ok());
        status.unreadable = folder.unreadable.iter().map(|(n, (_, why))| (n.clone(), why.clone())).collect();
        status.retrying = folder.retrying.len();
        self.publish(status, notes)
    }

    fn publish(&mut self, status: Status, notes: Vec<Note>) -> Status {
        *shared().status.lock().unwrap() = status.clone();
        self.last_notes = note_texts(notes);
        if !self.last_notes.is_empty() { shared().notes.lock().unwrap().extend(self.last_notes.clone()) }
        status
    }
}

/// The messages for what happened: one each, or one for many songs that came in at once.
pub fn note_texts(notes: Vec<Note>) -> Vec<(String, bool)> {
    let came = notes.iter().filter(|n| matches!(n, Note::CameIn(_))).count();
    let mut out = Vec::new();
    if came > 3 { out.push((trn!("{n} song came in from the songs folder.", "{n} songs came in from the songs folder.", came), false)) }
    for n in notes {
        out.push(match n {
            Note::CameIn(_) if came > 3 => continue,
            Note::CameIn(title) => (tr!("“{title}” came in from the songs folder.", title = title), false),
            Note::Updated(title) => (tr!("“{title}” was changed in the songs folder, so it changed here too.", title = title), false),
            Note::KeptBoth(title, copy) => (tr!("“{title}” was changed here and in the songs folder, so both were kept: the folder's version is “{copy}”.", title = title, copy = copy), false),
            Note::BroughtBack(title) => (tr!("“{title}” is in the songs folder, so it is back in the song library.", title = title), false),
            Note::Unreadable(file, why) => (tr!("{file} in the songs folder could not be read: {why}", file = file, why = why), true),
        });
    }
    out
}

/// The line under the choice in Settings: how many songs, the last change, files that could not be read.
pub fn status_line(s: &Status, now: SystemTime) -> String {
    if !s.on { return String::new() }
    if let Some(p) = &s.problem { return p.clone() }
    let mut parts = vec![trn!("{n} song in the folder", "{n} songs in the folder", s.songs)];
    if let Some(t) = s.last_change {
        let ago = now.duration_since(t).unwrap_or_default().as_secs();
        parts.push(if ago < 60 { tr!("last change just now") } else if ago < 3600 {
            trn!("last change {n} minute ago", "last change {n} minutes ago", ago / 60)
        } else {
            let local: chrono::DateTime<chrono::Local> = t.into();
            tr!("last change {when}", when = local.format("%Y-%m-%d %H:%M"))
        });
    }
    if !s.unreadable.is_empty() {
        let files = s.unreadable.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>().join(", ");
        parts.push(trn!("{n} file could not be read: {files}", "{n} files could not be read: {files}", s.unreadable.len(), files = files));
    }
    if s.retrying > 0 { parts.push(trn!("{n} file is busy and will be tried again", "{n} files are busy and will be tried again", s.retrying)) }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, model::Song};
    use uuid::Uuid;

    fn temp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("rp-test-songs-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn files(dir: &std::path::Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn turning_it_on_writes_every_song_and_off_leaves_them() {
        let app = test_app();
        let dir = temp().join("Songs");
        {
            let mut d = app.data();
            d.songs.push(Song { id: Uuid::new_v4(), title: "Amazing Grace".into(), lyrics: "Verse 1\n[G]Amazing grace".into(), ..Default::default() });
            d.songs.push(Song { id: Uuid::new_v4(), title: "Be Still".into(), lyrics: "Verse 1\nBe still".into(), ..Default::default() });
            d.settings.songs_folder = Prefs { on: false, folder: Some(dir.clone()), tidy: false };
        }
        let mut r = Runner::default();
        assert!(!r.tick(&app).on);
        assert!(!dir.exists(), "off: nothing is made");
        app.data().settings.songs_folder.on = true;
        let s = r.tick(&app);
        assert_eq!((s.on, s.songs, s.problem.clone()), (true, 2, None));
        assert_eq!(files(&dir), ["Amazing Grace.chordpro", "Be Still.chordpro"]);
        assert!(status_line(&s, SystemTime::now()).starts_with("2 songs in the folder · last change just now"), "{}", status_line(&s, SystemTime::now()));
        // What the app knows is kept in the data folder, not in the songs folder.
        assert!(app.store.root.join("songs-folder.json").is_file());
        // A song edited in the app changes its file at the next look.
        app.data().songs[0].lyrics = "Verse 1\n[G]Amazing grace!".into();
        r.tick(&app);
        assert!(std::fs::read_to_string(dir.join("Amazing Grace.chordpro")).unwrap().contains("[G]Amazing grace!"));
        // Off: the files stay; a later edit is not written.
        app.data().settings.songs_folder.on = false;
        r.tick(&app);
        app.data().songs[1].title = "Be Still My Soul".into();
        r.tick(&app);
        assert_eq!(files(&dir), ["Amazing Grace.chordpro", "Be Still.chordpro"]);
        // On again: the change made meanwhile is written (the file renamed to the new title).
        app.data().settings.songs_folder.on = true;
        r.tick(&app);
        assert_eq!(files(&dir), ["Amazing Grace.chordpro", "Be Still My Soul.chordpro"]);
    }

    #[test]
    fn a_dropped_file_comes_into_the_library_and_says_so() {
        let app = test_app();
        let dir = temp();
        app.data().settings.songs_folder = Prefs { on: true, folder: Some(dir.clone()), tidy: false };
        let mut r = Runner::default();
        r.tick(&app);
        std::fs::write(dir.join("river.txt"), crate::songselect::inbox::tests::LYRICS_TXT).unwrap();
        r.tick(&app);
        r.tick(&app);
        let notes = r.last_notes.clone();
        let d = app.data();
        let song = d.songs.iter().find(|s| s.title == "River Of Test Songs").expect("it came in");
        assert_eq!(song.ccli, "1234567");
        drop(d);
        r.tick(&app);
        assert!(files(&dir).contains(&"River Of Test Songs.chordpro".to_string()), "its ChordPro file is beside it: {:?}", files(&dir));
        assert!(dir.join("river.txt").is_file(), "the original stays");
        assert!(notes.iter().any(|(t, err)| t == "“River Of Test Songs” came in from the songs folder." && !err), "{notes:?}");
    }

    #[test]
    fn a_folder_that_cannot_be_made_is_said() {
        let app = test_app();
        let blocker = temp().join("a file");
        std::fs::write(&blocker, "x").unwrap();
        app.data().settings.songs_folder = Prefs { on: true, folder: Some(blocker.join("Songs")), tidy: false };
        let s = Runner::default().tick(&app);
        assert!(s.problem.as_deref().is_some_and(|p| p.starts_with("The songs folder could not be made")), "{s:?}");
        assert!(status_line(&s, SystemTime::now()).starts_with("The songs folder could not be made"));
    }

    #[test]
    fn many_new_songs_say_so_once() {
        let notes = (0..5).map(|i| Note::CameIn(format!("S{i}"))).chain([Note::Unreadable("x.txt".into(), "why".into())]).collect();
        assert_eq!(note_texts(notes), [("5 songs came in from the songs folder.".to_string(), false), ("x.txt in the songs folder could not be read: why".to_string(), true)]);
        let s = Status { on: true, songs: 1, unreadable: vec![("a.docx".into(), "no".into())], retrying: 2, ..Default::default() };
        assert_eq!(status_line(&s, SystemTime::now()), "1 song in the folder · 1 file could not be read: a.docx · 2 files are busy and will be tried again");
        assert_eq!(status_line(&Status::default(), SystemTime::now()), "");
    }
}
