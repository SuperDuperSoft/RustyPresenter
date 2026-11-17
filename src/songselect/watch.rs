//! Watches the computer's Downloads folder for new song files the person downloads from SongSelect. It only looks:
//! files are read by the caller once finished, and never moved, renamed or deleted.
use super::inbox::{EXTENSIONS, MAX_BYTES};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::SystemTime,
};

/// The endings browsers give a download that is not finished yet (Chrome and Edge, Firefox, Safari, others).
const UNFINISHED: &[&str] = &["crdownload", "part", "partial", "download", "tmp"];

/// The computer's own Downloads folder: the Windows Known Folder (wherever the person moved it), `~/Downloads` on
/// macOS, the XDG folder on Linux.
pub fn downloads_folder() -> Option<PathBuf> {
    // Tests never watch the computer's own Downloads.
    #[cfg(test)]
    return testing::DOWNLOADS.with(|d| d.borrow().clone());
    #[cfg(not(test))]
    dirs::download_dir().filter(|d| d.is_dir())
}

/// A file's size and last change, to tell when it has stopped changing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp { len: u64, modified: Option<SystemTime> }

/// What is in the folder now: song-file names and their stamps, and the names still being downloaded.
fn look(dir: &Path) -> (HashMap<PathBuf, Stamp>, HashSet<String>) {
    let mut files = HashMap::new();
    let mut unfinished = HashSet::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return (files, unfinished) };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let lower = name.to_lowercase();
        if let Some((stem, ext)) = lower.rsplit_once('.') {
            if UNFINISHED.contains(&ext) { unfinished.insert(stem.to_string()); continue }
            if !EXTENSIONS.contains(&ext) { continue }
        } else { continue }
        // Office and editors' lock files ("~$name.txt") are never songs.
        if name.starts_with("~$") || name.starts_with(".~") { continue }
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() { continue }
        files.insert(e.path(), Stamp { len: meta.len(), modified: meta.modified().ok() });
    }
    (files, unfinished)
}

/// Finds the song files that appear in a folder after it starts, once each is finished.
pub struct Watcher {
    dir: PathBuf,
    /// What was there before, as it was: left alone.
    before: HashMap<PathBuf, Stamp>,
    /// New files seen once, waiting for a second look with the same size and time.
    waiting: HashMap<PathBuf, Stamp>,
    /// Files already handed out, as they were then. A file changed again later is handed out again.
    taken: HashMap<PathBuf, Stamp>,
}

impl Watcher {
    /// Starts watching `dir`: what is in it now is never taken.
    pub fn new(dir: PathBuf) -> Watcher {
        let (before, _) = look(&dir);
        Watcher { dir, before, waiting: HashMap::new(), taken: HashMap::new() }
    }

    pub fn dir(&self) -> &Path { &self.dir }

    /// One look (about once a second): the new song files that are finished, oldest name first.
    pub fn poll(&mut self) -> Vec<PathBuf> {
        let (now, unfinished) = look(&self.dir);
        let mut ready = Vec::new();
        let mut waiting = HashMap::new();
        for (path, stamp) in now {
            if self.before.get(&path) == Some(&stamp) || self.taken.get(&path) == Some(&stamp) { continue }
            let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            // Still being written: empty (Firefox makes the name first), too big, or its unfinished twin is there.
            if stamp.len == 0 || stamp.len > MAX_BYTES || unfinished.contains(&name) { continue }
            if self.waiting.get(&path) == Some(&stamp) {
                self.taken.insert(path.clone(), stamp);
                ready.push(path);
            } else {
                waiting.insert(path, stamp);
            }
        }
        self.waiting = waiting;
        ready.sort();
        ready
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::{cell::RefCell, path::PathBuf};
    thread_local! {
        /// The Downloads folder the tests use instead of the computer's.
        pub(crate) static DOWNLOADS: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    }

    /// A new empty folder standing in for Downloads on this thread.
    pub(crate) fn fake_downloads() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rp-songselect-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        DOWNLOADS.with(|d| *d.borrow_mut() = Some(dir.clone()));
        dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn only_new_finished_song_files_are_taken_once() {
        let dir = testing::fake_downloads();
        fs::write(dir.join("old song.txt"), "was here before").unwrap();
        let mut w = Watcher::new(dir.clone());
        assert!(w.poll().is_empty(), "what was there before is left alone");
        // A download under way: the browser's unfinished file, then the name with nothing in it yet.
        fs::write(dir.join("new song.txt.crdownload"), "half").unwrap();
        fs::write(dir.join("ff song.txt"), "").unwrap();
        fs::write(dir.join("ff song.txt.part"), "half").unwrap();
        fs::write(dir.join("picture.jpg"), "not a song name").unwrap();
        assert!(w.poll().is_empty());
        assert!(w.poll().is_empty(), "unfinished downloads are not taken");
        // Chrome renames when done; Firefox fills the name and removes its .part.
        fs::rename(dir.join("new song.txt.crdownload"), dir.join("new song.txt")).unwrap();
        fs::write(dir.join("ff song.txt"), "done").unwrap();
        fs::remove_file(dir.join("ff song.txt.part")).unwrap();
        assert!(w.poll().is_empty(), "first seen: waits for a second look");
        let got = w.poll();
        assert_eq!(got, [dir.join("ff song.txt"), dir.join("new song.txt")], "both, once they stay the same");
        assert!(w.poll().is_empty(), "each file once");
        // Still growing: waits until it stops changing.
        fs::write(dir.join("growing.usr"), "a").unwrap();
        assert!(w.poll().is_empty());
        fs::write(dir.join("growing.usr"), "ab").unwrap();
        assert!(w.poll().is_empty(), "changed since the last look");
        assert_eq!(w.poll(), [dir.join("growing.usr")]);
        // The old file changed (downloaded again over it): taken.
        fs::write(dir.join("old song.txt"), "downloaded again, longer").unwrap();
        w.poll();
        assert_eq!(w.poll(), [dir.join("old song.txt")]);
        // Nothing was moved or deleted.
        assert!(dir.join("old song.txt").is_file() && dir.join("picture.jpg").is_file() && dir.join("new song.txt").is_file());
        assert_eq!(w.dir(), dir);
    }

    #[test]
    fn a_missing_folder_gives_nothing() {
        let mut w = Watcher::new(std::env::temp_dir().join(format!("rp-songselect-{}-missing", uuid::Uuid::new_v4())));
        assert!(w.poll().is_empty());
    }
}
