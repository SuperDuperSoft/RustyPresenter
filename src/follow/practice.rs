//! Practice: auto-follow on a recording (last Sunday's, from record-to-disk) instead of the room, at home. The
//! song is followed on its own, never on the live screens, as fast as FFmpeg can read the file; the review log
//! says what Automatic would have done and when (seconds into the recording).
use super::audio::{Ear, End, Listener, ffmpeg, file_args, read_pcm};
use super::ear::{Lexicon, file_ear};
use super::engine::{Engine, Host, LiveSong};
use super::log::LogEntry;
use super::rules::{Mode, Settings};
use crate::model::Slide;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Runs Automatic over `file` for a song with `slides`, with `settings`. The log of what it would have done.
pub fn practice(file: &Path, slides: Vec<Slide>, settings: Settings) -> Result<Vec<LogEntry>, String> {
    practice_ear(&ffmpeg()?, file_args(file), slides, settings, file_ear)
}

/// `practice` with any program giving 16 kHz mono samples (FFmpeg, or a stand-in in tests) and any ear.
pub fn practice_with(program: &Path, args: Vec<OsString>, slides: Vec<Slide>, settings: Settings, ear: Box<dyn Ear>) -> Result<Vec<LogEntry>, String> {
    practice_ear(program, args, slides, settings, |_| ear)
}

/// The same, with the ear made for the song's words.
fn practice_ear(program: &Path, args: Vec<OsString>, slides: Vec<Slide>, settings: Settings, ear: impl FnOnce(Lexicon) -> Box<dyn Ear>)
    -> Result<Vec<LogEntry>, String> {
    let host = Practice(Mutex::new(LiveSong { item_id: Uuid::new_v4(), is_song: true, slides, slide: 0, words_up: true, video_playing: false, version: 1,
        mode: Mode::Auto, settings: Settings { mode: Mode::Auto, ..settings } }));
    let mut engine = Engine::new();
    let mut listener = Listener::new(ear(engine.lexicon()));
    let mut log = vec![];
    engine.step(&host, 0.0, Some(listener.hello()));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    let mut failed = None;
    let end = rt.block_on(read_pcm(program, args, CancellationToken::new(), |r| failed = r.err(), |pcm, at| {
        for ev in listener.frame(pcm, at) { engine.step(&host, at, Some(ev)) }
        log.extend(engine.take_log());
    }));
    if let Some(e) = failed { return Err(e) }
    match end {
        End::Ended { ok: false, why } => Err(why),
        _ => Ok(log),
    }
}

/// The song on a pretend screen.
struct Practice(Mutex<LiveSong>);

impl Host for Practice {
    fn live(&self) -> Option<LiveSong> { Some(self.0.lock().unwrap().clone()) }
    fn show(&self, _item: Uuid, slide: usize, _base: u64) -> bool {
        let mut l = self.0.lock().unwrap();
        l.slide = slide;
        l.words_up = true;
        l.version += 1;
        true
    }
    fn clear_slide(&self, _base: u64) -> bool {
        let mut l = self.0.lock().unwrap();
        l.words_up = false;
        l.version += 1;
        true
    }
    fn notice(&self, _text: &str) {}
    fn save_mode(&self, _item: Uuid, _mode: Mode) {}
}

#[cfg(test)]
#[path = "practice_tests.rs"]
mod tests;
