//! The sound input: FFmpeg (the one the app already fetched) turns a microphone or line-in, or a recording in
//! Practice, into 16 kHz mono samples on its output. Each 20 ms frame goes through the voice tracker and the ear;
//! what they make of it goes to the follower. If FFmpeg ends by itself (device unplugged, driver gone), the
//! follower is told (`InputLost`) and goes back to Manual.
use super::align::Heard;
use super::engine::FollowEvent;
use super::tape::Tap;
use super::voice::{FRAME, RATE, VoiceTracker};
use crate::platform::proc::spawn_tree;
use crate::platform::tools::{self, Os};
use crate::record::{args, run::find_ffmpeg};
use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio_util::sync::CancellationToken;

/// What hears words in the sound (`ear::CtcEar`, the acoustic model).
pub trait Ear: Send {
    /// It recognises words. False for `NoEar`: then the follower only blanks and never moves a slide by itself.
    fn words(&self) -> bool;
    /// One 20 ms frame starting at `at` seconds; the words it finished hearing.
    fn frames(&mut self, pcm: &[i16], at: f64) -> Vec<Heard>;
    /// Flush the final buffered words when a Practice recording ends.
    fn finish(&mut self) -> Vec<Heard> { vec![] }
}

/// No model downloaded (or none fast enough here): hears no words.
pub struct NoEar;

impl Ear for NoEar {
    fn words(&self) -> bool { false }
    fn frames(&mut self, _pcm: &[i16], _at: f64) -> Vec<Heard> { vec![] }
}

/// The voice tracker and the ear on one frame at a time: the events for the follower.
pub struct Listener {
    tracker: VoiceTracker,
    ear: Box<dyn Ear>,
    frames: u64,
}

/// The voice label and the onset rate go out every 5 frames (100 ms).
const LABEL_EVERY: u64 = 5;

impl Listener {
    pub fn new(ear: Box<dyn Ear>) -> Listener { Listener { tracker: VoiceTracker::new(), ear, frames: 0 } }

    /// The first event: listening, with or without words.
    pub fn hello(&self) -> FollowEvent { FollowEvent::Listening { words: self.ear.words() } }

    pub fn frame(&mut self, pcm: &[i16], at: f64) -> Vec<FollowEvent> {
        let (voice, onsets) = self.tracker.frame(pcm, at);
        self.frames += 1;
        let mut out = Vec::new();
        if self.frames % LABEL_EVERY == 0 {
            out.push(FollowEvent::Audio { voice, onsets, at, held_since: self.tracker.held_since(at) });
        }
        out.extend(self.ear.frames(pcm, at).into_iter().map(FollowEvent::Heard));
        out
    }

    pub fn level(&self) -> f32 { self.tracker.level() }

    pub(crate) fn finish(&mut self) -> impl Iterator<Item = FollowEvent> {
        self.ear.finish().into_iter().map(FollowEvent::Heard)
    }
}

/// The FFmpeg arguments for live sound from input `device`: 16 kHz mono 16-bit samples on its output.
pub fn device_args(os: Os, device: &str) -> Vec<OsString> {
    let format = args::input_format(os);
    let mut a: Vec<OsString> = ["-hide_banner", "-loglevel", "error", "-nostdin"].map(OsString::from).into();
    // dshow holds half a second of sound by default; 50 ms keeps the follower close to the room.
    if format == "dshow" { a.extend(["-audio_buffer_size", "50"].map(OsString::from)) }
    a.extend(["-f", format, "-i"].map(OsString::from));
    a.push(args::device_input(format, device).into());
    a.extend(pcm_out());
    a
}

/// What listens to input `device`, and its arguments: FFmpeg (`device_args`), except on Linux when this FFmpeg
/// has no PulseAudio and `parec` is there (as the stream's sound inputs: `record::devices::reader_as`).
pub fn listen_with(ffmpeg: &Path, os: Os, device: &str) -> (PathBuf, Vec<OsString>) {
    let format = args::input_format(os);
    let ffmpeg_args = || device_args(os, device).into_iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let (program, a) = crate::record::devices::reader_as(ffmpeg, format, device, ffmpeg_args, RATE as u32, 1);
    (program, a.into_iter().map(OsString::from).collect())
}

/// The same from a recording (Practice).
pub fn file_args(file: &Path) -> Vec<OsString> {
    let mut a: Vec<OsString> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-i"].map(OsString::from).into();
    a.push(file.into());
    a.extend(pcm_out());
    a
}

fn pcm_out() -> Vec<OsString> { ["-vn", "-ac", "1", "-ar", "16000", "-f", "s16le", "-"].map(OsString::from).into() }

/// The FFmpeg to listen with (the app's own, else the computer's).
pub(crate) fn ffmpeg() -> Result<PathBuf, String> {
    let data = crate::server::data_dir().unwrap_or_default();
    find_ffmpeg(&data).ok_or_else(|| crate::tr!("Auto-follow needs FFmpeg to hear the room. Download it in Settings → Livestream → Recording."))
}

/// The microphones and line-ins auto-follow can listen to (names as FFmpeg opens them).
pub fn input_devices() -> Vec<String> {
    let Ok(ffmpeg) = ffmpeg() else { return vec![] };
    crate::record::run::list_inputs(&ffmpeg).into_iter().map(|i| i.name).collect()
}

/// How a run of FFmpeg ended.
pub(crate) enum End {
    /// Stopped on purpose.
    Stopped,
    /// It ended by itself: `ok` when it said so (the end of a file), `why` its last words.
    Ended { ok: bool, why: String },
}

/// Runs `program` and hands each whole 20 ms frame of its output to `each`, with its time from the start.
/// `started` hears whether it could start at all.
pub(crate) async fn read_pcm(program: &Path, args: Vec<OsString>, cancel: CancellationToken, started: impl FnOnce(Result<(), String>),
    mut each: impl FnMut(&[i16], f64)) -> End {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut tree = match spawn_tree(cmd) {
        Ok(t) => t,
        Err(e) => { started(Err(e.to_string())); return End::Ended { ok: false, why: e.to_string() } }
    };
    let (Some(mut out), Some(err)) = (tree.stdout(), tree.stderr()) else {
        started(Err(ended()));
        return End::Ended { ok: false, why: ended() };
    };
    started(Ok(()));
    let said: Arc<Mutex<VecDeque<String>>> = Arc::default();
    let said2 = said.clone();
    let talk = tokio::spawn(async move {
        let mut lines = BufReader::new(err).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            eprintln!("ffmpeg (auto-follow): {l}");
            let mut s = said2.lock().unwrap_or_else(|e| e.into_inner());
            s.push_back(l);
            if s.len() > 5 { s.pop_front(); }
        }
    });
    let mut buf = vec![0u8; FRAME * 2];
    let mut pcm = vec![0i16; FRAME];
    let mut n: u64 = 0;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return End::Stopped,
            r = out.read_exact(&mut buf) => {
                if r.is_err() { break }
                for (s, b) in pcm.iter_mut().zip(buf.chunks_exact(2)) { *s = i16::from_le_bytes([b[0], b[1]]) }
                each(&pcm, n as f64 * FRAME as f64 / RATE as f64);
                n += 1;
            }
        }
    }
    let status = tree.wait(Duration::from_secs(5), &cancel).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), talk).await;
    if cancel.is_cancelled() { return End::Stopped }
    let ok = status.is_ok_and(|s| s.success());
    let last = said.lock().unwrap_or_else(|e| e.into_inner()).back().cloned();
    End::Ended { ok, why: last.filter(|l| !l.trim().is_empty()).unwrap_or_else(ended) }
}

fn ended() -> String { crate::tr!("FFmpeg stopped") }

/// Listening to a sound input until dropped. Sends `Listening`, then voice labels, onsets and heard words, and
/// `InputLost` if the input goes away by itself.
pub struct Capture {
    cancel: CancellationToken,
    level: Arc<AtomicU32>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Capture {
    /// Starts listening to `device` (a name from `input_devices`) with `ear` (`ear::room_ear`).
    pub fn start(device: &str, ear: Box<dyn Ear>, tx: Sender<FollowEvent>) -> Result<Capture, String> { Capture::start_tapped(device, ear, tx, None) }

    /// `start`, with `tap` hearing every frame beside the follower (a practice tape, `tape::Tape`).
    pub fn start_tapped(device: &str, ear: Box<dyn Ear>, tx: Sender<FollowEvent>, tap: Option<Arc<dyn Tap>>) -> Result<Capture, String> {
        let (program, args) = listen_with(&ffmpeg()?, tools::OS, device);
        Capture::run_tapped(program, args, ear, tx, tap)
    }

    /// Starts `program` with `args` (FFmpeg, or a stand-in in tests) and listens to its output with `ear`.
    pub fn run(program: PathBuf, args: Vec<OsString>, ear: Box<dyn Ear>, tx: Sender<FollowEvent>) -> Result<Capture, String> {
        Capture::run_tapped(program, args, ear, tx, None)
    }

    pub fn run_tapped(program: PathBuf, args: Vec<OsString>, ear: Box<dyn Ear>, tx: Sender<FollowEvent>, tap: Option<Arc<dyn Tap>>) -> Result<Capture, String> {
        let cancel = CancellationToken::new();
        let level = Arc::new(AtomicU32::new(0));
        let (started_tx, started_rx) = channel();
        let (c, lv) = (cancel.clone(), level.clone());
        let thread = std::thread::Builder::new().name("follow-audio".into()).spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => { let _ = started_tx.send(Err(e.to_string())); return }
            };
            let mut listener = Listener::new(ear);
            let hello = listener.hello();
            let sent = tx.clone();
            let stop = c.clone();
            let end = rt.block_on(read_pcm(&program, args, c, |r| {
                if r.is_ok() { let _ = sent.send(hello); }
                let _ = started_tx.send(r);
            }, |pcm, at| {
                let events = listener.frame(pcm, at);
                if let Some(tap) = &tap { tap.frame(pcm, at, &events) }
                for ev in events { if sent.send(ev).is_err() { stop.cancel() } }
                lv.store(listener.level().to_bits(), Ordering::Relaxed);
            }));
            lv.store(0, Ordering::Relaxed);
            if let End::Ended { why, .. } = end { let _ = tx.send(FollowEvent::InputLost(why)); }
        }).map_err(|e| e.to_string())?;
        match started_rx.recv() {
            Ok(Ok(())) => Ok(Capture { cancel, level, thread: Some(thread) }),
            Ok(Err(e)) => { let _ = thread.join(); Err(e) }
            Err(_) => { let _ = thread.join(); Err(ended()) }
        }
    }

    /// The input's loudness now (RMS, 0–1), for a level meter.
    pub fn level(&self) -> f32 { f32::from_bits(self.level.load(Ordering::Relaxed)) }

    /// Still listening (the input has not gone away).
    pub fn running(&self) -> bool { self.thread.as_ref().is_some_and(|t| !t.is_finished()) }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(t) = self.thread.take() { let _ = t.join(); }
    }
}

#[cfg(test)]
#[path = "audio_tests.rs"]
mod tests;
