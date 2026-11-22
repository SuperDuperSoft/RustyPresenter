//! Live captions: what is said into the sermon microphone, written out while live (livestreaming design §8,
//! "Later"; design: `docs/superpowers/specs/2026-11-17-livestream-extras-design.md`).
//!
//! The words come from auto-follow's English letter model (`follow::fetch::FAST`, wav2vec2-base-960h), the one
//! of its two models that writes letters and spaces; the other writes speech sounds, not words. It runs on this
//! computer, on its own thread, on a rolling window as the ear does (`follow::ear::Stream`), and its letters are
//! read as written (`Letters`), with no song to hold them against. A small FFmpeg of its own listens to the input
//! (as auto-follow does), so the stream's own sound path is not touched.
//!
//! The words go three ways, each chosen in Settings:
//! - **on the stream picture**: two lines at the bottom on a dark band, drawn here and laid over the picture by the
//!   recorder's writer, except while the stream shows song words, a verse or a card (`shows_over`);
//! - **a captions file** beside the recording (`<recording>.vtt`, WebVTT), which players and YouTube read;
//! - **YouTube's own captions** (viewers turn them on with the CC button), sent to the address YouTube Studio
//!   gives for "POST captions to URL" (`youtube_body`). FFmpeg cannot put CEA-608/708 captions into the stream
//!   from text (it has no encoder for them), so this is the way to real captions on YouTube; Facebook and the
//!   others get the words on the picture.
//!
//! The model was taught on read-aloud books: it writes no punctuation, and names, Bible places, quiet or echoing
//! speech and singing often come out wrong (`tr::how_good`).
use crate::follow::ear::{Acoustic, HOP_S, WINDOW_S};
use crate::follow::voice::RATE;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime};

/// Settings → Livestream → Live captions.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Prefs {
    /// Captions are made while live.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub on: bool,
    /// They are shown on the stream picture (`None`: yes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_picture: Option<bool>,
    /// They are kept in a file beside the recording (`None`: yes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<bool>,
    /// YouTube Studio's "captions ingestion URL" (Stream settings → Closed captions → POST captions to URL); empty:
    /// none. It lets anyone add captions to the church's stream, so it is never shown in a log.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub youtube: String,
    /// The microphone or line-in they listen to (a name as FFmpeg opens it); empty: the stream's own sound input.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub input: String,
}

impl Prefs {
    pub fn on_picture(&self) -> bool { self.on_picture.unwrap_or(true) }
    pub fn file(&self) -> bool { self.file.unwrap_or(true) }

    /// The input to listen to: the one chosen, else the stream's sound input (`stream_input`, from Settings'
    /// Sound choice); `None` when there is neither.
    pub fn listen_to(&self, stream_input: Option<&str>) -> Option<String> {
        let own = self.input.trim();
        if !own.is_empty() { return Some(own.to_string()) }
        stream_input.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
    }
}

/// The most letters on a caption line (CEA-608's 32, which also reads well on a phone).
pub const LINE: usize = 32;
/// Lines shown at once; a new line pushes the top one off (roll-up, as TV captions do).
pub const LINES: usize = 2;
/// No new word for this long: the captions are taken off.
pub const CLEAR_AFTER: f64 = 4.0;
/// A pause this long before a word starts a new sentence (a capital letter).
pub const SENTENCE_GAP: f64 = 1.0;
/// Silence frames (20 ms) that end a word the model has not ended with a space (the model writes long stretches
/// of nothing inside a held vowel, so this is longer than a breath between letters).
const PAUSE_FRAMES: u32 = 40;
/// The model's frame step.
const STEP_S: f64 = 0.02;

/// A word heard: its letters as written (lowercase), and when it started and ended (seconds since listening began).
#[derive(Clone, Debug, PartialEq)]
pub struct Spoken { pub text: String, pub start: f64, pub end: f64 }

/// Reads the letter model's frames (CTC): the most likely letter per frame, repeats and blanks collapsed, a "|"
/// (or a short pause) between words.
pub struct Letters {
    vocab: Vec<String>,
    blank: usize,
    prev: usize,
    quiet: u32,
    word: String,
    start: f64,
    end: f64,
}

impl Letters {
    pub fn new(vocab: Vec<String>, blank: usize) -> Letters { Letters { vocab, blank, prev: blank, quiet: 0, word: String::new(), start: 0.0, end: 0.0 } }

    /// One frame's log-probabilities at `at` seconds; the word it ended, if any.
    pub fn frame(&mut self, row: &[f32], at: f64) -> Option<Spoken> {
        let (id, _) = row.iter().copied().enumerate().max_by(|a, b| a.1.total_cmp(&b.1))?;
        if id == self.blank {
            self.prev = id;
            self.quiet += 1;
            return if self.quiet == PAUSE_FRAMES { self.end_word() } else { None };
        }
        self.quiet = 0;
        if id == self.prev { return None }
        self.prev = id;
        let tok = self.vocab.get(id).map(String::as_str).unwrap_or("");
        if tok == "|" || tok == " " { return self.end_word() }
        if tok.is_empty() || (tok.starts_with('<') && tok.ends_with('>')) { return None }
        let letters: String = tok.chars().filter(|c| c.is_alphanumeric() || *c == '\'').flat_map(char::to_lowercase).collect();
        if letters.is_empty() { return None }
        if self.word.is_empty() { self.start = at }
        self.word.push_str(&letters);
        self.end = at + STEP_S;
        None
    }

    /// Ends the word being written (a pause, or the sound stopped).
    pub fn end_word(&mut self) -> Option<Spoken> {
        let text = std::mem::take(&mut self.word);
        let text = text.trim_matches('\'').to_string();
        (!text.is_empty()).then(|| Spoken { text, start: self.start, end: self.end })
    }
}

/// The model on a rolling window (as `follow::ear::Stream`): each new half second is heard with a second before it.
pub struct Listen {
    model: Box<dyn Acoustic>,
    letters: Letters,
    buf: Vec<f32>,
    start: f64,
    done: f64,
}

impl Listen {
    pub fn new(model: Box<dyn Acoustic>) -> Listen {
        let letters = Letters::new(model.vocab().to_vec(), model.blank());
        Listen { model, letters, buf: vec![], start: 0.0, done: 0.0 }
    }

    fn end(&self) -> f64 { self.start + self.buf.len() as f64 / RATE as f64 }

    /// Adds 16 kHz samples starting at `at` seconds. A gap (or a jump back) ends the word and starts again.
    pub fn push(&mut self, pcm: &[i16], at: f64) -> Option<Spoken> {
        let mut out = None;
        if self.buf.is_empty() || (at - self.end()).abs() > 0.1 {
            if !self.buf.is_empty() { out = self.letters.end_word() }
            self.buf.clear();
            self.start = at;
            self.done = at;
        }
        self.buf.extend(pcm.iter().map(|&s| s as f32 / 32768.0));
        let keep = (WINDOW_S * RATE as f64) as usize;
        if self.buf.len() > keep {
            let cut = self.buf.len() - keep;
            self.buf.drain(..cut);
            self.start += cut as f64 / RATE as f64;
            // Sound the model never got to (a slow computer) is skipped: what it was writing ends there.
            if self.done < self.start { self.done = self.start; out = out.or_else(|| self.letters.end_word()) }
        }
        out
    }

    /// Runs the model when a hop of new sound is in; the words it finished.
    pub fn run(&mut self) -> Vec<Spoken> {
        if self.end() - self.done < HOP_S - 1e-9 || self.buf.len() < 400 { return vec![] }
        let rows = match self.model.log_probs(&self.buf) {
            Ok(r) => r,
            Err(e) => { eprintln!("captions: {e}"); self.done = self.end(); return vec![] }
        };
        let mut out = vec![];
        for (i, row) in rows.iter().enumerate() {
            let t = self.start + i as f64 * STEP_S;
            if t + 1e-9 < self.done { continue }
            out.extend(self.letters.frame(row, t));
        }
        self.done = self.start + rows.len() as f64 * STEP_S;
        out
    }

    /// What is still being written, ended (the end of a file, Stop).
    pub fn flush(&mut self) -> Option<Spoken> { self.letters.end_word() }
}

/// Words that always start with a capital letter in a church's captions.
const NAMES: [&str; 10] = ["god", "jesus", "christ", "lord", "bible", "amen", "hallelujah", "israel", "jerusalem", "david"];

/// The word as it is shown: "i" and its contractions as "I", a few names with a capital, and the first word of a
/// sentence with a capital.
pub fn tidy(word: &str, first: bool) -> String {
    let w = word.to_lowercase();
    let capital = first || w == "i" || w.starts_with("i'") || NAMES.contains(&w.as_str()) || NAMES.iter().any(|n| w.strip_suffix("'s") == Some(n));
    if !capital { return w }
    let mut c = w.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// One caption line as it leaves the screen, for the captions file: its words, and from when to when it was shown.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue { pub start: f64, pub end: f64, pub text: String }

struct Line { text: String, start: f64 }

/// The captions on the screen: up to `LINES` lines of up to `LINE` letters, the newest at the bottom.
#[derive(Default)]
pub struct Board {
    lines: VecDeque<Line>,
    /// When the last word was said (its end), `None` before the first.
    last: Option<f64>,
}

impl Board {
    /// Adds a word said; the lines it pushed off the screen.
    pub fn word(&mut self, w: &Spoken) -> Vec<Cue> {
        let mut gone = vec![];
        let first = self.last.is_none_or(|l| w.start - l >= SENTENCE_GAP) || self.lines.is_empty();
        let text = tidy(&w.text, first);
        let fits = self.lines.back().is_some_and(|l| l.text.chars().count() + 1 + text.chars().count() <= LINE);
        if fits {
            let l = self.lines.back_mut().expect("a line");
            l.text.push(' ');
            l.text.push_str(&text);
        } else {
            self.lines.push_back(Line { text, start: w.start });
            while self.lines.len() > LINES {
                let l = self.lines.pop_front().expect("a line");
                gone.push(Cue { start: l.start, end: w.start, text: l.text });
            }
        }
        self.last = Some(w.end);
        gone
    }

    /// The time is `now`: after `CLEAR_AFTER` with no new word every line is taken off (and returned).
    pub fn tick(&mut self, now: f64) -> Vec<Cue> {
        match self.last { Some(l) if now - l >= CLEAR_AFTER && !self.lines.is_empty() => self.clear(l + 1.0), _ => vec![] }
    }

    /// Takes every line off at `at`.
    pub fn clear(&mut self, at: f64) -> Vec<Cue> {
        self.lines.drain(..).map(|l| Cue { start: l.start, end: at.max(l.start + 0.5), text: l.text }).collect()
    }

    /// The lines shown now, the top one first.
    pub fn text(&self) -> String { self.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n") }
}

/// "00:01:02.345", WebVTT's time.
pub fn vtt_time(secs: f64) -> String {
    let ms = (secs.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// One WebVTT cue (the text with any line break kept, "-->" never in it).
pub fn vtt_cue(c: &Cue) -> String {
    format!("{} --> {}\n{}\n\n", vtt_time(c.start), vtt_time(c.end), c.text.replace("-->", "->"))
}

/// The captions file beside `recording`: `Recording 2026-11-17 10-30-00.vtt`. Players open it with the video, and
/// YouTube Studio takes it as the replay's subtitles.
pub fn file_beside(recording: &Path) -> PathBuf { recording.with_extension("vtt") }

/// The captions file, written cue by cue so a crash keeps what was said until then.
pub struct VttFile { file: std::fs::File, pub path: PathBuf }

impl VttFile {
    pub fn create(path: &Path) -> std::io::Result<VttFile> {
        let mut file = std::fs::File::create(path)?;
        file.write_all(b"WEBVTT\n\n")?;
        Ok(VttFile { file, path: path.to_path_buf() })
    }

    pub fn cue(&mut self, c: &Cue) {
        if c.text.trim().is_empty() { return }
        if let Err(e) = self.file.write_all(vtt_cue(c).as_bytes()).and_then(|_| self.file.flush()) { eprintln!("captions: the captions file: {e}") }
    }
}

/// YouTube's address for caption `seq` (its count from 1): the address YouTube gave, with `seq` added.
pub fn youtube_url(base: &str, seq: u64) -> String {
    let base = base.trim();
    format!("{base}{}seq={seq}", if base.contains('?') { '&' } else { '?' })
}

/// What is sent to YouTube for one caption: the time it was said (UTC, to the millisecond) on the first line, then
/// its lines (YouTube's "<br>" between them).
pub fn youtube_body(at: SystemTime, text: &str) -> String {
    let t: chrono::DateTime<chrono::Utc> = at.into();
    format!("{}\n{}\n", t.format("%Y-%m-%dT%H:%M:%S%.3f"), text.replace('\n', "<br>"))
}

/// Whether the captions go over the stream picture now: not while it shows song words or a verse (they would sit
/// on each other, and singing captions badly) nor on a card (Starting soon, Ending, Hold).
pub fn shows_over(shown: &super::pictures::Shown) -> bool { shown.words.is_none() && !shown.scene.is_card() }

/// The captions picture: `text` at the bottom of a clear picture of `size`, white on a dark band, in a plain font
/// at about TV captions' size.
pub fn key(text: &str, size: (u32, u32)) -> crate::render::cache::RenderKey {
    use crate::model::Theme;
    let theme = Theme { size: 52, position: "lower".into(), band: true, band_color: "#000000".into(), band_opacity: 75, shadow: false, weight: 600,
        color: "#ffffff".into(), ..Theme::default() };
    crate::render::cache::RenderKey { text: text.to_string(), footer: None, reference: None, scripture: false, size: 0, passage: Default::default(), parallel: None,
        theme, canvas: size, design: None }
}

/// The captions picture the recorder's writer lays over the stream (premultiplied RGBA of the stream's size), and
/// `None` while there are none.
pub type Slot = Arc<Mutex<Option<Arc<Vec<u8>>>>>;

/// How the captions are doing, for the Go Live status.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum State {
    #[default]
    Starting,
    Listening,
    /// They stopped, with the words why.
    Failed(String),
}

/// Everything captions need for one stream.
pub struct Setup {
    pub ffmpeg: PathBuf,
    /// What FFmpeg reads: `follow::audio::device_args` for an input (a file in tests).
    pub args: Vec<OsString>,
    /// The picture's size; `None`: no captions on the picture.
    pub picture: Option<(u32, u32)>,
    pub slot: Slot,
    /// The captions file; `None`: none.
    pub file: Option<PathBuf>,
    /// YouTube's captions address; empty: none.
    pub youtube: String,
    /// The stream is held back this long (`stream::delay`): YouTube is told the captions are that much later.
    pub delay: Duration,
    /// The stream's Mute: nothing is written while it is on.
    pub mute: Arc<AtomicBool>,
    /// The input is opened again this long after it stops (an unplugged microphone); tests set it short.
    pub retry: Duration,
}

/// Captions being made. Dropping it stops them and finishes the file.
pub struct Captioner {
    stop: Arc<AtomicBool>,
    pub state: Arc<Mutex<State>>,
    /// The lines on the screen now (the Go Live status shows them).
    pub text: Arc<Mutex<String>>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Captioner {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) { let _ = t.join(); }
    }

    pub fn state(&self) -> State { self.state.lock().unwrap_or_else(|e| e.into_inner()).clone() }
}

impl Drop for Captioner {
    fn drop(&mut self) { self.stop() }
}

/// The English letter model, if it is downloaded (`follow::fetch::FAST`).
pub fn model() -> Result<Box<dyn Acoustic>, String> {
    #[cfg(feature = "follow-model")]
    {
        let deps = crate::follow::fetch::deps_dir();
        let dir = crate::follow::fetch::installed(&deps, &crate::follow::fetch::FAST).ok_or_else(tr::no_model)?;
        crate::follow::model::load(&dir).map(|m| Box::new(m) as Box<dyn Acoustic>).map_err(|e| tr::failed(&e))
    }
    #[cfg(not(feature = "follow-model"))]
    Err(tr::no_model())
}

/// What loads the model, on the captions' own thread (loading the real one takes a few seconds).
pub type Load = Box<dyn FnOnce() -> Result<Box<dyn Acoustic>, String> + Send>;

/// Starts captions with the model `load` gives (`model`, or a stand-in in tests). The captions file is made at
/// once; a model that cannot load leaves `State::Failed`.
pub fn start(setup: Setup, load: Load) -> Result<Captioner, String> {
    let mut vtt = match &setup.file {
        Some(p) => Some(VttFile::create(p).map_err(|e| tr::no_file(&p.display().to_string(), &e.to_string()))?),
        None => None,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let state: Arc<Mutex<State>> = Arc::default();
    let text: Arc<Mutex<String>> = Arc::default();
    let (pcm_tx, pcm_rx) = mpsc::channel::<(Vec<i16>, f64)>();
    let began = Instant::now();
    // The input: FFmpeg's 16 kHz samples, timed from when captions began; opened again when it stops.
    let (s, st, ffmpeg, args, retry) = (stop.clone(), state.clone(), setup.ffmpeg.clone(), setup.args.clone(), setup.retry);
    let reader = std::thread::Builder::new().name("captions-input".into()).spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
        while !s.load(Ordering::Relaxed) {
            let cancel = tokio_util::sync::CancellationToken::new();
            let c = cancel.clone();
            let s2 = s.clone();
            let watch = std::thread::spawn(move || { while !s2.load(Ordering::Relaxed) && !c.is_cancelled() { std::thread::sleep(Duration::from_millis(50)) } c.cancel() });
            let offset = began.elapsed().as_secs_f64();
            let tx = pcm_tx.clone();
            let st2 = st.clone();
            let end = rt.block_on(crate::follow::audio::read_pcm(&ffmpeg, args.clone(), cancel.clone(), |r| {
                if r.is_ok() { set(&st2, State::Listening) }
            }, |pcm, at| { let _ = tx.send((pcm.to_vec(), offset + at)); }));
            cancel.cancel();
            let _ = watch.join();
            if let crate::follow::audio::End::Ended { ok, why } = end {
                if s.load(Ordering::Relaxed) { break }
                // The end of a file (tests) ends the captions; an input that stopped is opened again.
                if ok { break }
                set(&st, State::Failed(tr::input_stopped(&why)));
                let until = Instant::now() + retry;
                while Instant::now() < until && !s.load(Ordering::Relaxed) { std::thread::sleep(Duration::from_millis(50)) }
            }
        }
    }).map_err(|e| e.to_string())?;
    let youtube = (!setup.youtube.trim().is_empty()).then(|| poster(setup.youtube.trim().to_string(), stop.clone()));
    let (s, tx2, slot, picture, mute, delay) = (stop.clone(), text.clone(), setup.slot.clone(), setup.picture, setup.mute.clone(), setup.delay);
    let st = state.clone();
    let worker = std::thread::Builder::new().name("captions".into()).spawn(move || {
        let model = match load() {
            Ok(m) => m,
            Err(e) => { set(&st, State::Failed(e)); s.store(true, Ordering::Relaxed); return }
        };
        let mut listen = Listen::new(model);
        let mut board = Board::default();
        let mut fonts = picture.map(|_| crate::render::Fonts::new());
        let mut shown = String::new();
        let mut sent = String::new();
        let mut last_sent = Instant::now() - Duration::from_secs(10);
        let wall0 = SystemTime::now() - began.elapsed();
        let mut ended = false;
        loop {
            let mut words = vec![];
            let first = pcm_rx.recv_timeout(Duration::from_millis(100));
            let mut got = match first {
                Ok(p) => vec![p],
                Err(mpsc::RecvTimeoutError::Timeout) => vec![],
                Err(mpsc::RecvTimeoutError::Disconnected) => { ended = true; vec![] }
            };
            got.extend(pcm_rx.try_iter());
            let muted = mute.load(Ordering::Relaxed);
            for (pcm, at) in &got {
                if muted { continue }
                words.extend(listen.push(pcm, *at));
            }
            if !muted { words.extend(listen.run()) }
            if ended || s.load(Ordering::Relaxed) { words.extend(listen.flush()) }
            let now = began.elapsed().as_secs_f64();
            let mut gone = vec![];
            for w in &words { gone.extend(board.word(w)) }
            gone.extend(if muted { board.clear(now) } else { board.tick(now) });
            let finishing = ended || s.load(Ordering::Relaxed);
            if finishing { gone.extend(board.clear(now)) }
            if let Some(f) = vtt.as_mut() { for c in &gone { f.cue(c) } }
            let text = board.text();
            if text != shown {
                shown = text.clone();
                *tx2.lock().unwrap_or_else(|e| e.into_inner()) = text.clone();
                if let (Some(size), Some(fonts)) = (picture, fonts.as_mut()) {
                    let pic = if text.is_empty() { None } else {
                        let r = crate::render::cache::draw_key(fonts, &key(&text, size));
                        ((r.width, r.height) == size).then(|| Arc::new(r.rgba_premul.buffer().as_bytes().to_vec()))
                    };
                    *slot.lock().unwrap_or_else(|e| e.into_inner()) = pic;
                }
            }
            // YouTube: the lines as they change, at most once a second, stamped with when the last word was said.
            if let Some(y) = &youtube && !shown.is_empty() && shown != sent && last_sent.elapsed() >= Duration::from_secs(1) {
                let said = board.last.unwrap_or(now);
                let at = wall0 + Duration::from_secs_f64(said.max(0.0)) + delay;
                let _ = y.send((at, shown.clone()));
                sent = shown.clone();
                last_sent = Instant::now();
            }
            if finishing { break }
        }
        *slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }).map_err(|e| e.to_string())?;
    Ok(Captioner { stop, state, text, threads: vec![reader, worker] })
}

fn set(state: &Mutex<State>, s: State) { *state.lock().unwrap_or_else(|e| e.into_inner()) = s }

/// Sends each caption to YouTube in order, on a thread of its own (a slow answer never holds up the words).
fn poster(base: String, stop: Arc<AtomicBool>) -> mpsc::Sender<(SystemTime, String)> {
    let (tx, rx) = mpsc::channel::<(SystemTime, String)>();
    let _ = std::thread::Builder::new().name("captions-youtube".into()).spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
        let Ok(client) = crate::bible::sources::client() else { return };
        let mut seq = 0u64;
        let mut told = false;
        while let Ok((at, text)) = rx.recv() {
            if stop.load(Ordering::Relaxed) { break }
            seq += 1;
            let r = rt.block_on(client.post(youtube_url(&base, seq)).header("content-type", "text/plain; charset=utf-8").body(youtube_body(at, &text)).send());
            match r {
                Ok(r) if r.status().is_success() => {}
                // Said once: the address has a key in it, so only the answer is.
                Ok(r) => if !told { told = true; eprintln!("captions: YouTube answered {}", r.status()) },
                Err(e) => if !told { told = true; eprintln!("captions: YouTube could not be reached: {}", e.without_url()) },
            }
        }
    });
    tx
}

pub mod tr {
    use crate::tr;
    pub fn how_good() -> String {
        tr!("Captions are made on this computer from the microphone, in English only. They are often wrong: names, Bible places, quiet or echoing speech and songs come out badly, and there is no punctuation. They show about two seconds after the words are said.")
    }
    pub fn no_model() -> String { tr!("Live captions need the English speech model. Download the fast model in Settings → Auto-follow → Speech model.") }
    pub fn failed(why: &str) -> String { tr!("Live captions could not start: {why}", why = why) }
    pub fn no_input() -> String { tr!("Live captions need a microphone or line-in: choose one in Settings → Livestream → Sound, or under Live captions.") }
    pub fn input_stopped(why: &str) -> String { tr!("Live captions stopped hearing the microphone ({why}). Trying again…", why = why) }
    pub fn no_file(path: &str, why: &str) -> String { tr!("The captions file {path} could not be made: {why}", path = path, why = why) }
    pub fn on() -> String { tr!("Live captions are on.") }
    pub fn saved(path: &str) -> String { tr!("Captions saved: {path}", path = path) }
}

#[cfg(test)]
#[path = "captions_tests.rs"]
mod tests;
