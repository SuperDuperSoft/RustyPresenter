//! The ear: a CTC speech model (`model`, Candle) turns the sound into letters or phonemes every 20 ms; this part
//! runs it on a rolling window, keeps the most likely sound per frame (CTC: repeats and blanks collapse) and turns
//! them into heard words for the aligner.
//!
//! The letter model writes words with a "|" between them: each one is heard with its sound-alike key
//! (`lyrics::sound_key`), the aligner's own key. The phoneme model writes no word breaks, so the live song's words,
//! turned into phonemes (`g2p`), are looked for in what it hears (`Lexicon`); a word found is heard with that
//! word's key. Without a song it reports the phoneme groups between pauses.
use super::align::Heard;
use super::audio::Ear;
use super::g2p::{coarse, phonemes};
use super::lyrics::{SongText, sound_key};
use super::voice::RATE;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What a model writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Units { Letters, Phonemes }

/// A CTC speech model: the real one (`model::Ctc`) or a stand-in in tests.
pub trait Acoustic: Send {
    /// Per 20 ms frame of `pcm` (16 kHz, −1…1), the log-probability of each token of `vocab`.
    fn log_probs(&self, pcm: &[f32]) -> Result<Vec<Vec<f32>>, String>;
    fn vocab(&self) -> &[String];
    fn units(&self) -> Units;
    /// The CTC blank (the padding token).
    fn blank(&self) -> usize;
}

/// A song word as phonemes, and the aligner's key for it.
#[derive(Clone, Debug, PartialEq)]
pub struct LexWord { pub phones: Vec<char>, pub key: String }

/// The live song's words for the phoneme ear, shared with the follower (which sets them when the song changes).
#[derive(Clone, Default)]
pub struct Lexicon(Arc<Mutex<Arc<Vec<LexWord>>>>);

impl Lexicon {
    /// The words of `song`, each once. Words without phonemes (other scripts) can't be heard by the phoneme ear.
    pub fn set_song(&self, song: &SongText) {
        let mut seen = HashSet::new();
        let mut words = vec![];
        for w in &song.words {
            let Some(p) = phonemes(&w.text) else { continue };
            let phones = coarse(&p);
            if phones.is_empty() || !seen.insert((phones.clone(), w.key.clone())) { continue }
            words.push(LexWord { phones, key: w.key.clone() });
        }
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Arc::new(words);
    }

    pub fn words(&self) -> Arc<Vec<LexWord>> { self.0.lock().unwrap_or_else(|e| e.into_inner()).clone() }
}

/// Blank frames in a row that count as a pause (a word break for both models).
const PAUSE_FRAMES: u32 = 15;

/// Edits allowed between a song word's phonemes and what was heard.
fn tolerance(len: usize) -> usize { match len { 0..=2 => 0, 3..=5 => 1, _ => 2 } }

/// Edit distance (insert, delete, change one sound).
fn edits(a: &[char], b: &[char]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(x != y)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// Finds the song's words in a stream of phonemes.
struct Spotter {
    lexicon: Lexicon,
    /// Heard phonemes not used yet: the sound, when, how sure.
    buf: Vec<(char, f64, f32)>,
}

struct Found { end: usize, key: String, conf: f32 }

impl Spotter {
    fn push(&mut self, c: char, at: f64, p: f32) -> Vec<Heard> {
        self.buf.push((c, at, p));
        let words = self.lexicon.words();
        let longest = words.iter().map(|w| w.phones.len()).max().unwrap_or(0);
        let mut out = vec![];
        // Wait until the longest word (and a sound either side) could be in the buffer, so "grace" isn't taken
        // for "grey" too soon.
        while !words.is_empty() && self.buf.len() >= longest + 2 {
            match self.find(&words) {
                Some(f) => out.push(self.take(f)),
                None => { self.buf.remove(0); }
            }
        }
        if words.is_empty() && self.buf.len() > 64 { self.buf.drain(..32); }
        out
    }

    /// A pause: everything still waiting is decided now.
    fn flush(&mut self) -> Vec<Heard> {
        let words = self.lexicon.words();
        let mut out = vec![];
        if words.is_empty() {
            // No song: the group of sounds itself.
            if !self.buf.is_empty() {
                let key: String = self.buf.iter().map(|b| b.0).collect();
                let conf = self.buf.iter().map(|b| b.2).sum::<f32>() / self.buf.len() as f32;
                out.push(Heard { key, at: self.buf[self.buf.len() - 1].1, conf });
                self.buf.clear();
            }
            return out;
        }
        while !self.buf.is_empty() {
            match self.find(&words) {
                Some(f) => out.push(self.take(f)),
                None => { self.buf.remove(0); }
            }
        }
        out
    }

    /// The best song word at the start of the buffer (one stray sound may come first).
    fn find(&self, words: &[LexWord]) -> Option<Found> {
        let heard: Vec<char> = self.buf.iter().map(|b| b.0).collect();
        let mut best: Option<(f32, Found)> = None;
        for start in 0..=1.min(heard.len().saturating_sub(1)) {
            for w in words {
                let len = w.phones.len();
                let tol = tolerance(len);
                for l in len.saturating_sub(tol).max(1)..=len + tol {
                    if start + l > heard.len() { break }
                    let d = edits(&w.phones, &heard[start..start + l]);
                    if d > tol { continue }
                    let score = len as f32 - d as f32 - start as f32 * 0.5;
                    if best.as_ref().is_none_or(|(s, _)| score > *s) {
                        let probs = &self.buf[start..start + l];
                        let p = probs.iter().map(|b| b.2).sum::<f32>() / l as f32;
                        let conf = (1.0 - d as f32 / len as f32) * p;
                        best = Some((score, Found { end: start + l, key: w.key.clone(), conf }));
                    }
                }
            }
        }
        best.map(|(_, f)| f)
    }

    fn take(&mut self, f: Found) -> Heard {
        let at = self.buf[f.end - 1].1;
        self.buf.drain(..f.end);
        Heard { key: f.key, at, conf: f.conf }
    }
}

/// Collapses the model's frames (CTC) into heard words.
pub struct Decoder {
    units: Units,
    vocab: Vec<String>,
    blank: usize,
    prev: usize,
    quiet: u32,
    word: String,
    probs: Vec<f32>,
    last: f64,
    spot: Spotter,
}

impl Decoder {
    pub fn new(units: Units, vocab: Vec<String>, blank: usize, lexicon: Lexicon) -> Decoder {
        Decoder { units, vocab, blank, prev: blank, quiet: 0, word: String::new(), probs: vec![], last: 0.0, spot: Spotter { lexicon, buf: vec![] } }
    }

    /// One frame's log-probabilities at `at` seconds; the words it finished.
    pub fn frame(&mut self, row: &[f32], at: f64) -> Vec<Heard> {
        let Some((id, lp)) = row.iter().copied().enumerate().max_by(|a, b| a.1.total_cmp(&b.1)) else { return vec![] };
        let p = lp.exp();
        if id == self.blank {
            self.prev = id;
            self.quiet += 1;
            return if self.quiet == PAUSE_FRAMES { self.pause() } else { vec![] };
        }
        self.quiet = 0;
        if id == self.prev { return vec![] }
        self.prev = id;
        let tok = self.vocab.get(id).map(String::as_str).unwrap_or("");
        if tok.is_empty() || (tok.starts_with('<') && tok.ends_with('>')) { return vec![] }
        match self.units {
            Units::Letters => {
                if tok == "|" { return self.end_word() }
                self.word.push_str(tok);
                self.probs.push(p);
                self.last = at;
                vec![]
            }
            Units::Phonemes => coarse(tok).into_iter().flat_map(|c| self.spot.push(c, at, p)).collect(),
        }
    }

    /// A pause in the singing ends the word.
    pub fn pause(&mut self) -> Vec<Heard> {
        match self.units {
            Units::Letters => self.end_word(),
            Units::Phonemes => self.spot.flush(),
        }
    }

    fn end_word(&mut self) -> Vec<Heard> {
        if self.word.is_empty() { return vec![] }
        let key = sound_key(&self.word.to_lowercase());
        let conf = self.probs.iter().sum::<f32>() / self.probs.len().max(1) as f32;
        self.word.clear();
        self.probs.clear();
        if key.is_empty() { return vec![] }
        vec![Heard { key, at: self.last, conf }]
    }
}

/// Seconds of sound the model sees at once (the left part is context).
pub const WINDOW_S: f64 = 1.5;
/// How often it runs.
pub const HOP_S: f64 = 0.5;
/// The model's frame step (320 samples).
const STEP_S: f64 = 0.02;

/// The model on a rolling window: each new half second is heard with a second before it as context.
pub struct Stream {
    model: Box<dyn Acoustic>,
    decoder: Decoder,
    buf: Vec<f32>,
    /// The time of `buf[0]`.
    start: f64,
    /// Frames before this time were decoded.
    done: f64,
}

impl Stream {
    pub fn new(model: Box<dyn Acoustic>, lexicon: Lexicon) -> Stream {
        let decoder = Decoder::new(model.units(), model.vocab().to_vec(), model.blank(), lexicon);
        Stream { model, decoder, buf: vec![], start: 0.0, done: 0.0 }
    }

    fn end(&self) -> f64 { self.start + self.buf.len() as f64 / RATE as f64 }

    /// Adds samples starting at `at` seconds. A gap (or a jump back) starts again.
    pub fn push(&mut self, pcm: &[i16], at: f64) {
        if self.buf.is_empty() || (at - self.end()).abs() > 0.1 {
            if !self.buf.is_empty() { let _ = self.decoder.pause(); }
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
        }
    }

    /// Runs the model when a hop of new sound is in; the words it finished.
    pub fn run(&mut self) -> Vec<Heard> {
        if self.end() - self.done < HOP_S - 1e-9 || self.buf.len() < 400 { return vec![] }
        let rows = match self.model.log_probs(&self.buf) {
            Ok(r) => r,
            Err(e) => { eprintln!("auto-follow ear: {e}"); self.done = self.end(); return vec![] }
        };
        let mut out = vec![];
        for (i, row) in rows.iter().enumerate() {
            let t = self.start + i as f64 * STEP_S;
            if t + 1e-9 < self.done { continue }
            out.extend(self.decoder.frame(row, t));
        }
        self.done = self.start + rows.len() as f64 * STEP_S;
        out
    }

    /// `push` then `run`.
    pub fn feed(&mut self, pcm: &[i16], at: f64) -> Vec<Heard> {
        self.push(pcm, at);
        self.run()
    }
}

/// For a recording (Practice): every bit of sound is heard, however long the model takes.
impl Ear for Stream {
    fn words(&self) -> bool { true }
    fn frames(&mut self, pcm: &[i16], at: f64) -> Vec<Heard> { self.feed(pcm, at) }
}

/// The real ear: the model runs on its own thread so the sound input never waits for it. When the model is
/// slower than the room it skips ahead to the newest sound.
pub struct CtcEar {
    tx: Option<Sender<(Vec<i16>, f64)>>,
    rx: Receiver<Heard>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl CtcEar {
    pub fn new(model: Box<dyn Acoustic>, lexicon: Lexicon) -> CtcEar {
        let (tx, frames) = channel::<(Vec<i16>, f64)>();
        let (heard, rx) = channel();
        let mut stream = Stream::new(model, lexicon);
        let thread = std::thread::Builder::new().name("follow-ear".into()).spawn(move || {
            while let Ok((pcm, at)) = frames.recv() {
                stream.push(&pcm, at);
                for (pcm, at) in frames.try_iter() { stream.push(&pcm, at) }
                for h in stream.run() { if heard.send(h).is_err() { return } }
            }
        }).ok();
        CtcEar { tx: Some(tx), rx, thread }
    }

    /// Waits up to `wait` for words still being worked on (tests, the end of a Practice file).
    pub fn settle(&mut self, wait: Duration) -> Vec<Heard> {
        let mut out = vec![];
        let until = std::time::Instant::now() + wait;
        while let Some(left) = until.checked_duration_since(std::time::Instant::now()) {
            match self.rx.recv_timeout(left) { Ok(h) => out.push(h), Err(_) => break }
        }
        out
    }
}

impl Ear for CtcEar {
    fn words(&self) -> bool { true }
    fn frames(&mut self, pcm: &[i16], at: f64) -> Vec<Heard> {
        if let Some(tx) = &self.tx { let _ = tx.send((pcm.to_vec(), at)); }
        self.rx.try_iter().collect()
    }
}

impl Drop for CtcEar {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(t) = self.thread.take() { let _ = t.join(); }
    }
}

/// Which model the ear uses on this computer (`speed_check`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Choice {
    /// `wav2vec2-xlsr-53-espeak-cv-ft`: phonemes, any language, large.
    Best,
    /// `wav2vec2-base-960h`: English letters, base size.
    Fast,
    /// No model, or none fast enough: the follower only blanks.
    Off,
}

/// The test sound for the speed check.
pub const CHECK_S: f64 = 3.0;
/// A model must hear `CHECK_S` of sound in under this share of it.
pub const MAX_REAL_TIME: f64 = 0.5;

/// Picks the biggest model that hears 3 s of sound in under 1.5 s. `time` runs one (`None`: not installed or it
/// failed); the big one is tried first and the small one only when it is too slow.
pub fn speed_check_with(mut time: impl FnMut(Choice) -> Option<Duration>) -> Choice {
    for c in [Choice::Best, Choice::Fast] {
        if time(c).is_some_and(|t| t.as_secs_f64() < CHECK_S * MAX_REAL_TIME) { return c }
    }
    Choice::Off
}

/// The ear for the room: the model the speed check picked, if one is downloaded, on its own thread; else none
/// (the follower then only blanks).
pub fn room_ear(lexicon: Lexicon) -> Box<dyn Ear> {
    #[cfg(feature = "follow-model")]
    if let Some(m) = super::model::chosen(&super::fetch::deps_dir()) { return Box::new(CtcEar::new(Box::new(m), lexicon)) }
    #[cfg(not(feature = "follow-model"))]
    let _ = lexicon;
    Box::new(super::audio::NoEar)
}

/// The same for a recording (Practice), hearing all of it.
pub fn file_ear(lexicon: Lexicon) -> Box<dyn Ear> {
    #[cfg(feature = "follow-model")]
    if let Some(m) = super::model::chosen(&super::fetch::deps_dir()) { return Box::new(Stream::new(Box::new(m), lexicon)) }
    #[cfg(not(feature = "follow-model"))]
    let _ = lexicon;
    Box::new(super::audio::NoEar)
}

#[cfg(test)]
#[path = "ear_tests.rs"]
pub(crate) mod tests;
