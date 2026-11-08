//! Importing decks and media (Plan 4), and reading other programs' song libraries. Deck imports work in a private folder under `store.tmp()` and publish only
//! when every asset is in the store, so a failed or cancelled import adds nothing.
use crate::{app::App, platform::tools};
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub mod chordpro;
pub mod common;
pub mod deck;
pub mod easyworship;
pub mod libreoffice;
pub mod media;
pub mod notes;
pub mod openlp;
pub mod openlyrics;
pub mod opensong;
pub mod pdf;
pub mod powerpoint;
pub mod probe;
pub mod propresenter;
pub mod sniff;

#[path = "../strings/import.rs"]
pub mod strings;

pub use common::{classify, Dedupe, ImportReport};

/// At start-up of either binary: finds the converters and hands them to `App`. The returned future does what can
/// wait, and is for the runtime to run: it clears away a LibreOffice left by a dead app, then warms up a new one.
pub fn init(app: &Arc<App>) -> impl Future<Output = ()> + Send + 'static {
    let root = app.store.root.clone();
    let pdf = match pdf::PdfRenderer::start(&root) {
        Ok(pdf) => Some(Arc::new(pdf)),
        Err(e) => { eprintln!("Presentations will not import: {e}"); None }
    };
    let powerpoint = tools::powerpoint_installed().then(powerpoint::PowerPoint::new);
    let converters = Arc::new(deck::Converters::new(Box::new(tools::find_soffice), pdf, powerpoint, root.clone()));
    app.set_converters(converters.clone());
    async move {
        let deps = root.join("deps");
        let _ = tokio::task::spawn_blocking(move || libreoffice::kill_leftover(&deps)).await;
        if let Some(lo) = converters.libreoffice() {
            lo.warm_up().await;
        }
    }
}

/// What a cancelled import reports. No toast is shown for it.
pub const CANCELLED: &str = "Cancelled.";

/// A work folder for one import. Dropping it removes the folder and everything in it.
pub struct WorkDir(PathBuf);

impl WorkDir {
    pub fn new(tmp_root: &Path) -> io::Result<WorkDir> {
        std::fs::create_dir_all(tmp_root)?;
        let dir = tmp_root.join(format!("ppt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir)?;
        Ok(WorkDir(dir))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        // A converter killed a moment ago may still hold a file; what is left goes at the next start (`Store::clean_tmp`).
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The steps of a deck import, in order. Each owns a share of the job's progress bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Prepare,
    Convert,
    Render,
    Publish,
}

/// Overall progress for `frac` (0.0–1.0) of the way through `stage`.
pub fn percent(stage: Stage, frac: f32) -> u8 {
    let (from, to) = match stage {
        Stage::Prepare => (0.0, 5.0),
        Stage::Convert => (5.0, 70.0),
        Stage::Render => (70.0, 95.0),
        Stage::Publish => (95.0, 100.0),
    };
    let frac = if frac.is_nan() { 0.0 } else { frac.clamp(0.0, 1.0) };
    (from + (to - from) * frac).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_ranges() {
        assert_eq!(percent(Stage::Prepare, 0.0), 0);
        assert_eq!(percent(Stage::Prepare, 1.0), 5);
        assert_eq!(percent(Stage::Convert, 0.0), 5);
        assert_eq!(percent(Stage::Convert, 0.5), 38);
        assert_eq!(percent(Stage::Convert, 1.0), 70);
        assert_eq!(percent(Stage::Render, 0.0), 70);
        assert_eq!(percent(Stage::Render, 1.0), 95);
        assert_eq!(percent(Stage::Publish, 0.0), 95);
        assert_eq!(percent(Stage::Publish, 1.0), 100);
        // Out-of-range fractions stay inside the stage.
        assert_eq!(percent(Stage::Render, -1.0), 70);
        assert_eq!(percent(Stage::Render, 2.0), 95);
        assert_eq!(percent(Stage::Render, f32::NAN), 70);
    }

    #[test]
    fn workdir_removed_on_drop() {
        let tmp = crate::store::test_dir().join("tmp");
        let w = WorkDir::new(&tmp).unwrap();
        let p = w.path().to_path_buf();
        assert!(p.is_dir() && p.starts_with(&tmp), "{p:?}");
        assert!(p.file_name().unwrap().to_string_lossy().starts_with("ppt-"), "{p:?}");
        std::fs::create_dir(p.join("sub")).unwrap();
        std::fs::write(p.join("sub").join("slide-001.png"), b"x").unwrap();
        let other = WorkDir::new(&tmp).unwrap();
        assert_ne!(other.path(), p);
        drop(w);
        assert!(!p.exists());
        assert!(other.path().is_dir(), "only its own folder is removed");
    }
}
