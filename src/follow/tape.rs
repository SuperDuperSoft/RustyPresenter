//! A practice tape (Jake, November 20, 2026: "Give it the ability to record DURING practice, so that you can
//! evaluate the recording right after practice and compare"). While one runs, the sound auto-follow hears is kept
//! as `sound.wav` (16 kHz mono, exactly what the ear gets) and everything that happened is written beside it in
//! `tape.jsonl`, one line each, stamped with its place in the sound (`a`, seconds): the room's label ten times a
//! second, every word the ear delivered, the song that went live with its slides' words, every slide change and
//! who made it (a hand or auto-follow), and the follower's own listening log.
//!
//! `review` plays a tape back through the follower as it is now and says how it compares with what was on the
//! screen. Nothing leaves this computer: a tape is a folder under `<data>/practice`.
use super::align::Heard;
use super::engine::FollowEvent;
use super::rules::{Mode, Voice};
use super::voice::RATE;
use serde_json::{Value, json};
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use uuid::Uuid;

pub const SOUND_FILE: &str = "sound.wav";
pub const LINES_FILE: &str = "tape.jsonl";
/// The folder under the app's data that holds the tapes, one folder each.
pub const FOLDER: &str = "practice";
/// The longest tape: a long rehearsal. A tape left running stops by itself (about 115 MB an hour).
pub const MAX_SECS: f64 = 4.0 * 3600.0;
/// No sound for this long (the input was changed, or stopped for a while): the gap is kept as silence, so the
/// sound and the lines stay in step.
const GAP_S: f64 = 0.5;
/// The sound file's sizes are written again this often, so a tape cut off (power, a crash) still plays.
const PATCH_SAMPLES: u64 = 2 * RATE as u64;
const HEADER: u64 = 44;

/// What the sound input hears, heard beside the follower: each 20 ms frame with what was made of it.
pub trait Tap: Send + Sync {
    fn frame(&self, pcm: &[i16], at: f64, events: &[FollowEvent]);
}

/// The screen as a tape keeps it: the live item and where it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub item: Uuid,
    pub title: String,
    /// A song (its slides' words go on the tape), or anything else (only that it is live).
    pub song: bool,
    /// Each slide's words.
    pub slides: Vec<String>,
    pub slide: usize,
    pub words_up: bool,
    pub mode: Mode,
    /// Moves whenever the screen is changed by hand (`Host::hand_version`).
    pub hand: u64,
    pub video: bool,
    pub hold: bool,
}

/// The app's one tape: off until `start`.
#[derive(Default)]
pub struct Tape {
    on: AtomicBool,
    open: Mutex<Option<Open>>,
}

struct Open {
    dir: PathBuf,
    wav: BufWriter<File>,
    lines: BufWriter<File>,
    samples: u64,
    patched: u64,
    started: Instant,
    /// When the last frame of sound came.
    last_frame: Option<Instant>,
    /// The tape's place minus the sound input's own clock, as of the last frame.
    offset: f64,
    /// The live item and its slides as last written, and the screen's state.
    song: Option<(Uuid, Vec<String>)>,
    shown: Option<Value>,
    hand: Option<u64>,
}

fn ms(t: f64) -> f64 { (t * 1000.0).round() / 1000.0 }

pub fn voice_name(v: Voice) -> &'static str {
    match v { Voice::Singing => "singing", Voice::Speaking => "speaking", Voice::Music => "music", Voice::Silence => "silence" }
}

pub fn mode_name(m: Mode) -> &'static str { match m { Mode::Manual => "manual", Mode::Suggest => "suggest", Mode::Auto => "auto" } }

fn wav_header(samples: u64) -> [u8; HEADER as usize] {
    let data = (samples * 2).min(u32::MAX as u64 - 36) as u32;
    let mut h = [0u8; HEADER as usize];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&1u16.to_le_bytes());
    h[24..28].copy_from_slice(&(RATE as u32).to_le_bytes());
    h[28..32].copy_from_slice(&(RATE as u32 * 2).to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}

impl Open {
    /// The tape's place now: where the sound has got to, and a little on for the time since its last frame.
    /// Before any sound has come, the time since the tape started (the sound then starts after that much silence).
    fn place(&self) -> f64 {
        match self.last_frame {
            Some(t) => self.samples as f64 / RATE as f64 + t.elapsed().as_secs_f64().min(GAP_S),
            None => self.started.elapsed().as_secs_f64(),
        }
    }

    fn line(&mut self, at: f64, mut v: Value) {
        if let Value::Object(m) = &mut v { m.insert("a".into(), json!(ms(at))); }
        let _ = writeln!(self.lines, "{v}");
    }

    fn silence(&mut self, secs: f64) {
        let n = (secs.max(0.0) * RATE as f64) as u64;
        let zeros = [0u8; 3200];
        let mut left = n * 2;
        while left > 0 {
            let take = left.min(zeros.len() as u64) as usize;
            if self.wav.write_all(&zeros[..take]).is_err() { break }
            left -= take as u64;
        }
        self.samples += n;
    }

    /// Writes the sound file's sizes for what is in it so far.
    fn patch(&mut self) {
        let _ = self.lines.flush();
        if self.wav.flush().is_err() { return }
        let f = self.wav.get_mut();
        if f.seek(SeekFrom::Start(0)).is_ok() { let _ = f.write_all(&wav_header(self.samples)); }
        let _ = f.seek(SeekFrom::End(0));
        self.patched = self.samples;
    }

    fn sound(&mut self, pcm: &[i16], at: f64, events: &[FollowEvent]) {
        let now = Instant::now();
        let frame = pcm.len() as f64 / RATE as f64;
        match self.last_frame {
            None => self.silence(self.started.elapsed().as_secs_f64() - frame),
            Some(t) => {
                let gap = now.duration_since(t).as_secs_f64();
                if gap > GAP_S { self.silence(gap - frame) }
            }
        }
        let before = self.samples as f64 / RATE as f64;
        self.offset = before - at;
        let mut bytes = Vec::with_capacity(pcm.len() * 2);
        for s in pcm { bytes.extend_from_slice(&s.to_le_bytes()) }
        let _ = self.wav.write_all(&bytes);
        self.samples += pcm.len() as u64;
        self.last_frame = Some(now);
        for ev in events {
            match ev {
                FollowEvent::Audio { voice, onsets, at, held_since } => {
                    let held = held_since.map(|h| ms(h + self.offset));
                    self.line(at + self.offset, json!({ "voice": voice_name(*voice), "onsets": ms(*onsets as f64), "held": held }));
                }
                FollowEvent::Heard(h) => {
                    let sung = ms(h.at + self.offset);
                    self.line(before + frame, json!({ "heard": h.key, "conf": ms(h.conf as f64), "sung": sung }));
                }
                FollowEvent::Listening { words } => self.line(before, json!({ "listening": words })),
                FollowEvent::InputLost(why) => self.line(before, json!({ "lost": why })),
                FollowEvent::Voice(..) | FollowEvent::Onsets(..) => {}
            }
        }
        if self.samples - self.patched >= PATCH_SAMPLES { self.patch() }
    }

    fn screen(&mut self, s: &Screen) {
        let at = self.place();
        let song = s.song.then(|| (s.item, s.slides.clone()));
        if self.song != song || (song.is_none() && self.shown.as_ref().is_none_or(|v| v["item"] != json!(s.item.to_string()))) {
            match &song {
                Some((item, slides)) => self.line(at, json!({ "song": { "item": item.to_string(), "title": s.title, "slides": slides } })),
                None => self.line(at, json!({ "other": { "item": s.item.to_string(), "title": s.title } })),
            }
            self.song = song;
            self.shown = None;
        }
        // A change of the hand mark with nothing else changed is still a click (Clear after the follower's blank).
        let by = if self.hand.is_none_or(|h| h != s.hand) { "hand" } else { "auto" };
        let state = json!({ "item": s.item.to_string(), "slide": s.slide, "words": s.words_up, "mode": mode_name(s.mode), "video": s.video, "hold": s.hold });
        if self.shown.as_ref() != Some(&state) || by == "hand" && self.hand.is_some() {
            self.line(at, json!({ "live": { "slide": s.slide, "words": s.words_up, "mode": mode_name(s.mode), "video": s.video, "hold": s.hold, "by": by } }));
        }
        self.shown = Some(state);
        self.hand = Some(s.hand);
    }
}

impl Tape {
    /// Recording now.
    pub fn on(&self) -> bool { self.on.load(Ordering::Relaxed) }

    /// Starts a tape in a new folder under `root` (`<data>/practice`), named for `now`. `about`: what to know
    /// about this run (the app's version, the speech model, the settings). The folder.
    pub fn start(&self, root: &Path, now: chrono::NaiveDateTime, about: Value) -> Result<PathBuf, String> {
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(o) = open.as_ref() { return Ok(o.dir.clone()) }
        let dir = root.join(now.format("%Y-%m-%d_%H-%M-%S").to_string());
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut wav = BufWriter::new(File::create(dir.join(SOUND_FILE)).map_err(|e| e.to_string())?);
        wav.write_all(&wav_header(0)).map_err(|e| e.to_string())?;
        let lines = BufWriter::new(File::create(dir.join(LINES_FILE)).map_err(|e| e.to_string())?);
        let mut o = Open { dir: dir.clone(), wav, lines, samples: 0, patched: 0, started: Instant::now(), last_frame: None, offset: 0.0,
            song: None, shown: None, hand: None };
        o.line(0.0, json!({ "tape": about, "when": now.format("%Y-%m-%d %H:%M:%S").to_string() }));
        *open = Some(o);
        self.on.store(true, Ordering::Relaxed);
        Ok(dir)
    }

    /// Ends the tape. Its folder, when one was running.
    pub fn stop(&self) -> Option<PathBuf> {
        self.on.store(false, Ordering::Relaxed);
        let mut o = self.open.lock().unwrap_or_else(|e| e.into_inner()).take()?;
        let at = o.place();
        o.line(at, json!({ "end": true }));
        o.patch();
        Some(o.dir)
    }

    /// The running tape's folder and how long it is (seconds).
    pub fn running(&self) -> Option<(PathBuf, f64)> {
        let open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        open.as_ref().map(|o| (o.dir.clone(), o.place()))
    }

    /// The screen changed (or may have): the live item, its slides when it is a new song, and where it stands.
    pub fn screen(&self, s: &Screen) {
        if !self.on() { return }
        if let Some(o) = self.open.lock().unwrap_or_else(|e| e.into_inner()).as_mut() { o.screen(s) }
    }

    /// The follower's listening log (`Engine::take_trace`).
    pub fn trace(&self, lines: &[String]) {
        if !self.on() { return }
        if let Some(o) = self.open.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            let at = o.place();
            for l in lines { o.line(at, json!({ "trace": l })) }
        }
    }
}

impl Tap for Tape {
    fn frame(&self, pcm: &[i16], at: f64, events: &[FollowEvent]) {
        if !self.on() { return }
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        let Some(o) = open.as_mut() else { return };
        o.sound(pcm, at, events);
        let full = o.samples as f64 / RATE as f64 >= MAX_SECS;
        drop(open);
        if full { self.stop(); }
    }
}

/// The tapes under `root`, newest first.
pub fn tapes(root: &Path) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> = std::fs::read_dir(root).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.join(LINES_FILE).is_file()).collect();
    all.sort();
    all.reverse();
    all
}

/// A heard word as a tape line holds it, for `review`.
pub(crate) fn heard_of(v: &Value) -> Option<(f64, Heard)> {
    let key = v["heard"].as_str()?.to_string();
    Some((v["a"].as_f64()?, Heard { key, at: v["sung"].as_f64()?, conf: v["conf"].as_f64().unwrap_or(1.0) as f32 }))
}

#[cfg(test)]
#[path = "tape_tests.rs"]
mod tests;
