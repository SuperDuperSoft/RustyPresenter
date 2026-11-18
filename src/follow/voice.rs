//! What the room sounds like, every 20 ms: singing, talking, music without voice, or quiet, and how many notes
//! start each second. Own small code: loudness, an autocorrelation pitch, and how steady that pitch is (a sung
//! note holds its pitch with a little vibrato; speech glides all the time; drums and noise have no pitch).
use super::rules::Voice;
use std::collections::VecDeque;

/// Samples a second (FFmpeg is asked for 16 kHz mono).
pub const RATE: usize = 16_000;
/// One frame: 20 ms.
pub const FRAME: usize = RATE / 50;

/// Quieter than this (RMS, full scale 1.0) is silence.
const QUIET: f32 = 0.005;
/// Frames the label looks back over (0.5 s).
const HISTORY: usize = 25;
/// Pitch range 80–1000 Hz, as lags in samples.
const MIN_LAG: usize = RATE / 1000;
const MAX_LAG: usize = RATE / 80;
/// A frame whose best autocorrelation is below this has no pitch.
const PITCHED: f32 = 0.7;
/// Median pitch change between frames (semitones): below `MACHINE` is an instrument's dead-steady note, below
/// `SUNG` a held, sung note; above it, speech.
const MACHINE: f32 = 0.02;
const SUNG: f32 = 0.45;
/// Onsets: a frame this much louder than the ones before, at least `ONSET_MIN` loud, `ONSET_GAP` s apart.
const ONSET_RISE: f32 = 2.0;
const ONSET_MIN: f32 = 0.01;
const ONSET_GAP: f64 = 0.1;
/// The onset rate is counted over this many seconds.
const ONSET_WINDOW: f64 = 4.0;

#[derive(Clone, Copy)]
struct Look { loud: bool, pitch: Option<f32> }

pub struct VoiceTracker {
    prev: Vec<f32>,
    history: VecDeque<Look>,
    levels: VecDeque<f32>,
    onsets: VecDeque<f64>,
    start: Option<f64>,
    level: f32,
}

impl Default for VoiceTracker {
    fn default() -> Self { VoiceTracker::new() }
}

impl VoiceTracker {
    pub fn new() -> VoiceTracker {
        VoiceTracker { prev: vec![0.0; FRAME], history: VecDeque::new(), levels: VecDeque::new(), onsets: VecDeque::new(), start: None, level: 0.0 }
    }

    /// The last frame's loudness (RMS, 0–1): the level meter.
    pub fn level(&self) -> f32 { self.level }

    /// One 20 ms frame of 16 kHz mono, starting at `at` seconds: the room's label and note onsets a second.
    pub fn frame(&mut self, pcm: &[i16], at: f64) -> (Voice, f32) {
        let x: Vec<f32> = pcm.iter().map(|&s| s as f32 / 32768.0).collect();
        let rms = if x.is_empty() { 0.0 } else { (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt() };
        self.level = rms;
        let start = *self.start.get_or_insert(at);

        // Onsets: a jump in loudness over the last few frames.
        let before = if self.levels.is_empty() { 0.0 } else { self.levels.iter().sum::<f32>() / self.levels.len() as f32 };
        if rms >= ONSET_MIN && rms > ONSET_RISE * before + 1e-4 && self.onsets.back().is_none_or(|&t| at - t >= ONSET_GAP) { self.onsets.push_back(at) }
        self.levels.push_back(rms);
        if self.levels.len() > 3 { self.levels.pop_front(); }
        while self.onsets.front().is_some_and(|&t| t <= at - ONSET_WINDOW) { self.onsets.pop_front(); }
        let span = (at - start + FRAME as f64 / RATE as f64).clamp(1.0, ONSET_WINDOW);
        let rate = (self.onsets.len() as f64 / span) as f32;

        // Pitch over the last 40 ms.
        let loud = rms >= QUIET;
        let pitch = if loud && x.len() == FRAME {
            let mut w = std::mem::take(&mut self.prev);
            w.extend_from_slice(&x);
            let p = pitch_hz(&w).map(|hz| 12.0 * (hz / 440.0).log2());
            self.prev = x;
            p
        } else {
            if x.len() == FRAME { self.prev = x } else { self.prev = vec![0.0; FRAME] }
            None
        };
        self.history.push_back(Look { loud, pitch });
        if self.history.len() > HISTORY { self.history.pop_front(); }
        (self.label(), rate)
    }

    fn label(&self) -> Voice {
        let h = &self.history;
        let loud = h.iter().filter(|l| l.loud).count();
        if loud * 10 < HISTORY * 3 { return Voice::Silence }
        let pitched = h.iter().filter(|l| l.pitch.is_some()).count();
        if pitched * 10 < loud * 4 { return Voice::Music }
        let mut steps: Vec<f32> = h.iter().zip(h.iter().skip(1)).filter_map(|(a, b)| Some((b.pitch? - a.pitch?).abs())).collect();
        if steps.len() < 3 { return Voice::Music }
        steps.sort_by(f32::total_cmp);
        let median = steps[steps.len() / 2];
        if median < MACHINE { Voice::Music } else if median < SUNG { Voice::Singing } else { Voice::Speaking }
    }
}

/// The pitch of `w` (Hz) by normalised autocorrelation, if it has one: the shortest strong period, so a note is
/// not heard an octave low.
fn pitch_hz(w: &[f32]) -> Option<f32> {
    if w.len() < MAX_LAG + 2 * MIN_LAG { return None }
    let n = w.len() - MAX_LAG - 1;
    let a = &w[..n];
    let ea: f32 = a.iter().map(|v| v * v).sum();
    let r: Vec<f32> = (0..=MAX_LAG + 1).map(|t| {
        if t + 1 < MIN_LAG { return 0.0 }
        let b = &w[t..t + n];
        let eb: f32 = b.iter().map(|v| v * v).sum();
        let d = (ea * eb).sqrt();
        if d <= 1e-9 { 0.0 } else { a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>() / d }
    }).collect();
    let best = r[MIN_LAG..=MAX_LAG].iter().copied().fold(f32::MIN, f32::max);
    if best < PITCHED { return None }
    let t = (MIN_LAG..=MAX_LAG).find(|&t| r[t] >= 0.9 * best && r[t] >= r[t - 1] && r[t] >= r[t + 1])?;
    let (y0, y1, y2) = (r[t - 1], r[t], r[t + 1]);
    let den = y0 - 2.0 * y1 + y2;
    let off = if den.abs() > 1e-9 { 0.5 * (y0 - y2) / den } else { 0.0 };
    Some(RATE as f32 / (t as f32 + off))
}

#[cfg(test)]
#[path = "voice_tests.rs"]
pub(crate) mod tests;
