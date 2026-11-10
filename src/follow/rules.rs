//! When to change, blank and bring back the lyrics: the singers' pace predicts when the next slide's first
//! line starts, and the slide changes `lead` seconds before it. Unsure, talking or off-script: hold.
use super::align::Position;
use super::lyrics::SongText;
use serde::{Deserialize, Serialize};

/// How a song follows the singers.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The operator moves every slide (as before 1.3).
    #[default]
    Manual,
    /// The app highlights the slide it thinks is next; the operator takes it.
    Suggest,
    /// The app moves the slides itself; the operator can always take over.
    Auto,
}

/// What the room sounds like right now (the voice tracker's label).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voice { Singing, Speaking, Music, Silence }

/// Settings → Auto-follow.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// The mode songs use unless an item has its own.
    pub mode: Mode,
    /// Seconds before the next slide's first word that it comes up (0.5–4).
    pub lead_s: f32,
    /// Blank the words after this many seconds with no singing; `None` = never.
    pub blank_after_s: Option<f32>,
    /// Blank the words after this many seconds of talking; `None` = never.
    pub talk_blank_after_s: Option<f32>,
    /// Well-matched words in a row needed to follow the singers outside the planned order.
    pub jump_words: u8,
    /// Below this confidence (0–1) Automatic holds and Suggest only highlights.
    pub min_conf: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { mode: Mode::Manual, lead_s: 2.0, blank_after_s: Some(3.0), talk_blank_after_s: Some(8.0), jump_words: super::align::JUMP_WORDS, min_conf: 0.6 }
    }
}

pub const LEAD_MIN: f32 = 0.5;
pub const LEAD_MAX: f32 = 4.0;

impl Settings {
    /// The lead time kept to 0.5–4 s, whatever was saved.
    pub fn lead(&self) -> f32 { if self.lead_s.is_nan() { 2.0 } else { self.lead_s.clamp(LEAD_MIN, LEAD_MAX) } }
    /// The jump strictness, at least 2 words.
    pub fn jump(&self) -> u8 { self.jump_words.max(2) }
}

/// What the rules want done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowAction {
    /// Take this slide live (Automatic).
    Show(usize),
    /// Highlight this slide for the operator (Suggest).
    Suggest(usize),
    /// Take the words off (keep the background).
    Blank,
    /// Singing is back: put the words up again on this slide.
    Unblank(usize),
}

/// The pace used until the singers have been heard: a calm hymn's syllables a second.
const DEFAULT_PACE: f64 = 2.5;
/// How fast the pace follows new words (moving average weight).
const PACE_WEIGHT: f64 = 0.3;
/// A gap longer than this between words is a breath or a break, not the pace.
const PACE_GAP: f64 = 3.0;
/// Singing heard this recently still counts as singing (the voice label flickers between words).
const STILL_SINGING: f64 = 1.0;

pub struct Rules {
    settings: Settings,
    song: SongText,
    pace: Option<f64>,
    /// The last word heard and when.
    last_word: Option<(usize, f64)>,
    started: Option<f64>,
    last_singing: Option<f64>,
    speaking_since: Option<f64>,
    blanked: bool,
    /// The word sung last when the words were taken off; singing must move on from it to bring them back.
    blank_word: Option<usize>,
}

impl Rules {
    pub fn new(settings: Settings, song: &SongText) -> Rules {
        Rules { settings, song: song.clone(), pace: None, last_word: None, started: None, last_singing: None, speaking_since: None, blanked: false, blank_word: None }
    }

    /// The singers' pace in syllables a second, once heard.
    pub fn pace(&self) -> Option<f64> { self.pace }

    /// The rules took the words off and wait for singing.
    pub fn blanked(&self) -> bool { self.blanked }

    /// The operator moved or cleared: forget the last word and the blank (the pace stays, it is the same band).
    pub fn reset(&mut self) {
        self.last_word = None;
        self.blanked = false;
        self.blank_word = None;
        self.speaking_since = None;
    }

    /// Settings → Auto-follow changed, or the item's mode.
    pub fn set_settings(&mut self, settings: Settings) { self.settings = settings }

    /// One look at `now` (seconds): where the aligner is, what the room sounds like, and the live slide.
    /// `onsets_per_s` is kept for the beat tracker; the decisions use the words' pace for now.
    pub fn tick(&mut self, now: f64, pos: &Position, voice: Voice, onsets_per_s: f32, live_slide: usize) -> Option<FollowAction> {
        let _ = onsets_per_s;
        let started = *self.started.get_or_insert(now);
        match voice {
            Voice::Singing => { self.last_singing = Some(now); self.speaking_since = None }
            Voice::Speaking => { self.speaking_since.get_or_insert(now); }
            Voice::Music | Voice::Silence => self.speaking_since = None,
        }
        self.note_word(now, pos);
        let mode = self.settings.mode;
        if mode == Mode::Manual { return None }
        let sure = pos.confidence >= self.settings.min_conf && !pos.off_script;
        let live_has_words = self.song.slides.get(live_slide).is_some_and(|s| !s.instrumental);

        // Blanking (Automatic only).
        if mode == Mode::Auto {
            if self.blanked {
                let back = voice == Voice::Singing && sure && pos.word.is_some() && pos.word != self.blank_word;
                if !back { return None }
                self.blanked = false;
                let slide = self.song.words[pos.word?].slide;
                return Some(FollowAction::Unblank(slide));
            }
            let quiet = now - self.last_singing.unwrap_or(started);
            let no_singing = matches!(voice, Voice::Music | Voice::Silence) && self.settings.blank_after_s.is_some_and(|b| quiet >= b as f64);
            let talking = self.speaking_since.is_some_and(|s| self.settings.talk_blank_after_s.is_some_and(|b| now - s >= b as f64));
            if live_has_words && (no_singing || talking) {
                self.blanked = true;
                self.blank_word = pos.word;
                return Some(FollowAction::Blank);
            }
        }

        // Talk, an ad-lib or an unsure place: hold.
        if voice == Voice::Speaking || !sure { return None }
        let w = pos.word?;
        let at = self.song.words.get(w)?.slide;
        let act = |n: usize| Some(if mode == Mode::Auto { FollowAction::Show(n) } else { FollowAction::Suggest(n) });

        if at != live_slide {
            // The singers are on another slide. Just behind the live one, on its last line, is where they are
            // after an early change: hold.
            let trailing = at < live_slide
                && (at + 1..live_slide).all(|s| self.song.slides[s].instrumental)
                && w >= self.song.slides[at].last_line_first_word;
            return if trailing { None } else { act(at) };
        }

        // On the live slide: change `lead` seconds before the next slide's first word, never before the last
        // line has started, never past the last slide, never to an instrumental.
        let span = &self.song.slides[live_slide];
        if w < span.last_line_first_word { return None }
        let next = live_slide + 1;
        if self.song.slides.get(next).is_none_or(|s| s.instrumental) { return None }
        let singing = self.last_singing.is_some_and(|t| now - t <= STILL_SINGING);
        if !singing { return None }
        let (_, heard_at) = self.last_word?;
        let left: u32 = self.song.words[w + 1..span.end_word].iter().map(|x| x.syllables as u32).sum();
        let predicted = heard_at + left as f64 / self.pace.unwrap_or(DEFAULT_PACE);
        if predicted - now <= self.settings.lead() as f64 + 1e-6 { act(next) } else { None }
    }

    /// A new word moves the pace on (syllables sung since the last word over the time between).
    fn note_word(&mut self, now: f64, pos: &Position) {
        let Some(w) = pos.word else { return };
        if w >= self.song.words.len() { return }
        match self.last_word {
            Some((last, _)) if last == w => return,
            Some((last, t)) if w > last && now > t && now - t <= PACE_GAP => {
                let syl: u32 = self.song.words[last + 1..=w].iter().map(|x| x.syllables as u32).sum();
                let rate = syl as f64 / (now - t);
                if (0.5..=12.0).contains(&rate) {
                    self.pace = Some(match self.pace { Some(p) => p + PACE_WEIGHT * (rate - p), None => rate });
                }
            }
            _ => {}
        }
        self.last_word = Some((w, now));
    }
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
