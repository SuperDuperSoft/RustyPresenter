//! A recording that never ended: while one runs, a note in the app's data folder says which file it writes and
//! whether it is also a livestream; it goes when the recording ends. Found at the next start, the app stopped
//! (crashed, the computer lost power) in the middle: the part written so far is finished into a video, and after
//! a livestream the operator is asked whether to go live again.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What the note says.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Unfinished {
    /// The part being written (`run::partial_of`).
    pub file: PathBuf,
    /// Also a livestream.
    pub live: bool,
}

/// The note, in the data folder `root`.
pub fn path(root: &Path) -> PathBuf { root.join("recording-under-way.json") }

/// Leaves the note: a recording has started.
pub fn mark(root: &Path, note: &Unfinished) {
    let written = serde_json::to_vec(note).map_err(std::io::Error::other).and_then(|b| crate::store::write_atomic(&path(root), &b));
    if let Err(e) = written { eprintln!("recording: the note that a recording is under way could not be written: {e}") }
}

/// Takes the note away: the recording ended.
pub fn clear(root: &Path) { let _ = std::fs::remove_file(path(root)); }

/// The note left by a recording that never ended, taken away (asked once).
pub fn take(root: &Path) -> Option<Unfinished> {
    let bytes = std::fs::read(path(root)).ok()?;
    clear(root);
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> PathBuf {
        let d = std::env::temp_dir().join(format!("rp-test-unfinished-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_recording_that_never_ended_is_found_once() {
        let root = folder();
        assert_eq!(take(&root), None, "nothing at first");
        let note = Unfinished { file: PathBuf::from("C:/Videos/Recording 2026-11-16 10-30.recording.mkv"), live: true };
        mark(&root, &note);
        assert_eq!(take(&root), Some(note.clone()));
        assert_eq!(take(&root), None, "taken");
        // Ended as it should: nothing left.
        mark(&root, &note);
        clear(&root);
        assert_eq!(take(&root), None);
        // A note that cannot be read is no note, and goes.
        std::fs::write(path(&root), b"{ not json").unwrap();
        assert_eq!(take(&root), None);
        assert!(!path(&root).exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
