//! What the room sounds like, every 20 ms: singing, talking, music without voice, or quiet, and how many notes
//! start each second. Pitch can oscillate quickly while a singer holds a note. Look for returns to the
//! same pitch over a longer span as well as slow changes between adjacent frames.
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
/// Pitch range 65-1000 Hz, as lags in samples. Covers a bass's low C.
const MIN_LAG: usize = RATE / 1000;
const MAX_LAG: usize = RATE / 65;
/// A frame whose best autocorrelation is below this has no pitch.
const PITCHED: f32 = 0.7;
/// Median pitch change between frames (semitones): below `MACHINE` is an instrument's dead-steady note, below
/// `SUNG` a held, sung note; above it, speech.
const MACHINE: f32 = 0.02;
const SUNG: f32 = 0.45;
/// An eight-frame span (140 ms between its first and last samples) can span a cycle of vibrato.
const CONTOUR: usize = 8;
const CONTOUR_RETURN: f32 = 1.25;
const CONTOUR_RANGE: f32 = 2.6;
const CONTOUR_TURN: f32 = 0.14;
const NOTE_RANGE: f32 = 2.6;
const NOTE_GAP: f64 = 0.08;
/// Onsets: a frame this much louder than the ones before, at least `ONSET_MIN` loud, `ONSET_GAP` s apart.
const ONSET_RISE: f32 = 2.0;
const ONSET_MIN: f32 = 0.01;
const ONSET_GAP: f64 = 0.1;
/// The onset rate is counted over this many seconds.
const ONSET_WINDOW: f64 = 4.0;

#[derive(Clone, Copy)]
struct Look { loud: bool, pitch: Option<f32> }

struct Note {
    start: f64,
    last: f64,
    low: f32,
    high: f32,
}

pub struct VoiceTracker {
    window: [f32; 2 * FRAME],
    history: VecDeque<Look>,
    levels: VecDeque<f32>,
    onsets: VecDeque<f64>,
    start: Option<f64>,
    level: f32,
    note: Option<Note>,
    voice: Voice,
}

impl Default for VoiceTracker {
    fn default() -> Self { VoiceTracker::new() }
}

impl VoiceTracker {
    pub fn new() -> VoiceTracker {
        VoiceTracker {
            window: [0.0; 2 * FRAME], history: VecDeque::with_capacity(HISTORY),
            levels: VecDeque::with_capacity(3), onsets: VecDeque::new(), start: None, level: 0.0,
            note: None, voice: Voice::Silence,
        }
    }

    /// The last frame's loudness (RMS, 0–1): the level meter.
    pub fn level(&self) -> f32 { self.level }

    /// Start of a pitch that has held for at least half a second without a new note or a breath.
    pub(crate) fn held_since(&self, at: f64) -> Option<f64> {
        let note = self.note.as_ref()?;
        (self.voice == Voice::Singing && at - note.start >= 0.5 && at - note.last <= NOTE_GAP).then_some(note.start)
    }

    /// One 20 ms frame of 16 kHz mono, starting at `at` seconds: the room's label and note onsets a second.
    pub fn frame(&mut self, pcm: &[i16], at: f64) -> (Voice, f32) {
        let mut sum = 0.0;
        let mut energy = 0.0;
        for &sample in pcm {
            let sample = sample as f32 / 32768.0;
            sum += sample;
            energy += sample * sample;
        }
        let len = pcm.len().max(1) as f32;
        let rms = (energy / len - (sum / len).powi(2)).max(0.0).sqrt();
        self.level = rms;
        let start = *self.start.get_or_insert(at);

        // Onsets: a jump in loudness over the last few frames.
        let before = if self.levels.is_empty() { 0.0 } else { self.levels.iter().sum::<f32>() / self.levels.len() as f32 };
        if rms >= ONSET_MIN && rms > ONSET_RISE * before + 1e-4 && self.onsets.back().is_none_or(|&t| at - t >= ONSET_GAP) { self.onsets.push_back(at) }
        if self.levels.len() == 3 { self.levels.pop_front(); }
        self.levels.push_back(rms);
        while self.onsets.front().is_some_and(|&t| t <= at - ONSET_WINDOW) { self.onsets.pop_front(); }
        let span = (at - start + FRAME as f64 / RATE as f64).clamp(1.0, ONSET_WINDOW);
        let rate = (self.onsets.len() as f64 / span) as f32;

        // Pitch over the last 40 ms.
        let loud = rms >= QUIET;
        let pitch = if pcm.len() == FRAME {
            self.window.copy_within(FRAME.., 0);
            for (dst, &sample) in self.window[FRAME..].iter_mut().zip(pcm) {
                *dst = sample as f32 / 32768.0;
            }
            if loud {
                let mean = self.window.iter().sum::<f32>() / self.window.len() as f32;
                pitch_hz(&self.window.map(|sample| sample - mean)).map(|hz| 12.0 * (hz / 440.0).log2())
            } else {
                None
            }
        } else {
            self.window.fill(0.0);
            None
        };
        if let Some(p) = pitch {
            match self.note.as_mut() {
                Some(n) if (0.0..=NOTE_GAP).contains(&(at - n.last))
                    && n.high.max(p) - n.low.min(p) <= NOTE_RANGE => {
                    n.last = at;
                    n.low = n.low.min(p);
                    n.high = n.high.max(p);
                }
                _ => self.note = Some(Note { start: at, last: at, low: p, high: p }),
            }
        } else if self.note.as_ref().is_some_and(|n| at - n.last > NOTE_GAP) {
            self.note = None;
        }
        if self.history.len() == HISTORY { self.history.pop_front(); }
        self.history.push_back(Look { loud, pitch });
        self.voice = self.label(at);
        (self.voice, rate)
    }

    fn label(&mut self, at: f64) -> Voice {
        let h = &self.history;
        let loud = h.iter().filter(|l| l.loud).count();
        if loud * 10 < HISTORY * 3 { return Voice::Silence }
        let pitched = h.iter().filter(|l| l.pitch.is_some()).count();
        if pitched * 10 < loud * 4 { return Voice::Music }
        let mut steps = [0.0; HISTORY - 1];
        let mut len = 0;
        for (a, b) in h.iter().zip(h.iter().skip(1)) {
            if let (Some(a), Some(b)) = (a.pitch, b.pitch) {
                steps[len] = (b - a).abs();
                len += 1;
            }
        }
        if len < 3 { return Voice::Music }
        let median = *steps[..len].select_nth_unstable_by(len / 2, f32::total_cmp).1;
        if median < MACHINE {
            // A straight note can be the continuation of a vocal that just lost its vibrato.
            let continuous = self.note.as_ref().is_some_and(|n| at - n.last <= NOTE_GAP && at - n.start >= 0.3);
            return if self.voice == Voice::Singing && continuous { Voice::Singing } else { Voice::Music };
        }
        // Legato across note changes stays pitched, even if the last half-second doesn't
        // contain a whole cycle around a single note.
        let legato = h.len() == HISTORY && pitched * 10 >= HISTORY * 9;
        let held = self.note.as_ref().is_some_and(|note| at - note.last <= NOTE_GAP && note.last - note.start >= 0.34);
        let steady = median < SUNG && (legato || held);
        if steady || returning_pitch(self.history.make_contiguous()) { Voice::Singing } else { Voice::Speaking }
    }
}

fn returning_pitch(history: &[Look]) -> bool {
    let mut voiced = 0;
    let mut returns = 0;
    for span in history.windows(CONTOUR) {
        let (Some(first), Some(last)) = (span[0].pitch, span[CONTOUR - 1].pitch) else { continue };
        let range = span.iter().try_fold((first, first), |(lo, hi), f| {
            let pitch = f.pitch?;
            Some((lo.min(pitch), hi.max(pitch)))
        });
        let Some((lo, hi)) = range else { continue };
        voiced += 1;
        let turn = (hi - first.max(last)).max(first.min(last) - lo);
        if (first - last).abs() <= CONTOUR_RETURN && hi - lo <= CONTOUR_RANGE && turn >= CONTOUR_TURN {
            returns += 1;
        }
    }
    returns >= 3 && returns * 2 >= voiced
}

/// The pitch of `w` (Hz) by normalised autocorrelation, if it has one: the shortest strong period, so a note is
/// not heard an octave low.
fn pitch_hz(w: &[f32]) -> Option<f32> {
    if w.len() < MAX_LAG + 2 * MIN_LAG { return None }
    let n = w.len() - MAX_LAG - 1;
    let a = &w[..n];
    let ea: f32 = a.iter().map(|v| v * v).sum();
    let mut r = [0.0; MAX_LAG + 2];
    for (t, value) in r.iter_mut().enumerate().skip(MIN_LAG - 1) {
        let b = &w[t..t + n];
        let eb: f32 = b.iter().map(|v| v * v).sum();
        let d = (ea * eb).sqrt();
        if d > 1e-9 { *value = a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>() / d; }
    }
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
