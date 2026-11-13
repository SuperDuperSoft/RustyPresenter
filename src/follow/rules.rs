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

impl Mode {
    /// The Auto chip's next mode: Manual → Suggest → Auto → Manual.
    pub fn next(self) -> Mode {
        match self { Mode::Manual => Mode::Suggest, Mode::Suggest => Mode::Auto, Mode::Auto => Mode::Manual }
    }

    /// The A key: Auto on, or (when on) back to Manual.
    pub fn toggle_auto(self) -> Mode { if self == Mode::Auto { Mode::Manual } else { Mode::Auto } }

    /// 0 Manual, 1 Suggest, 2 Auto (the chip and Settings' choices).
    pub fn index(self) -> usize { match self { Mode::Manual => 0, Mode::Suggest => 1, Mode::Auto => 2 } }

    pub fn from_index(i: usize) -> Mode { [Mode::Manual, Mode::Suggest, Mode::Auto].get(i).copied().unwrap_or_default() }
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
    /// Blank the words after this many seconds with no singing; `None` = never (the default: songs have long
    /// pauses, and the next words wait on the screen through them).
    pub blank_after_s: Option<f32>,
    /// Blank the words after this many seconds of talking; `None` = never (the default).
    pub talk_blank_after_s: Option<f32>,
    /// Well-matched words in a row needed to follow the singers outside the planned order.
    pub jump_words: u8,
    /// Below this confidence (0–1) Automatic holds and Suggest only highlights.
    pub min_conf: f32,
    /// The sound input to listen to (a name from `audio::input_devices`); `None`: the first one.
    pub input: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { mode: Mode::Manual, lead_s: 2.0, blank_after_s: None, talk_blank_after_s: None, jump_words: super::align::JUMP_WORDS, min_conf: 0.6,
            input: None }
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
/// The song's last word was sung and nobody has sung for this long: the song is over, the words come off.
pub const SONG_END_QUIET: f64 = 5.0;
/// A slide's last word was sung and nobody has sung for this long: an instrumental slide after it comes up.
pub const INSTRUMENTAL_QUIET: f64 = 2.0;

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
    /// The pace the onsets give while singing, until words give one.
    beat: Option<f64>,
    /// Why the words were last taken off: talking (else no singing), and for how many seconds.
    blank_why: (bool, f32),
}

impl Rules {
    pub fn new(settings: Settings, song: &SongText) -> Rules {
        Rules { settings, song: song.clone(), pace: None, last_word: None, started: None, last_singing: None, speaking_since: None, blanked: false, blank_word: None, beat: None, blank_why: (false, 0.0) }
    }

    /// The singers' pace in syllables a second, once heard.
    pub fn pace(&self) -> Option<f64> { self.pace }

    /// The rules took the words off and wait for singing.
    pub fn blanked(&self) -> bool { self.blanked }

    /// Why the words were last taken off: `(talking, seconds)`; not talking means nobody sang for that long.
    pub fn blank_why(&self) -> (bool, f32) { self.blank_why }

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
    /// `onsets_per_s` (note onsets while singing) stands in for the pace until words have been heard.
    ///
    /// The next words go up as the slide's last line is sung and then wait there, however long the pause. No
    /// timer takes words off unless Settings asks for one: they come off when the song's last word has been
    /// sung and it stays quiet, and where the song has an instrumental slide. The room's sound alone (no heard
    /// word) only leaves an instrumental slide as singing starts, and brings blanked words back.
    pub fn tick(&mut self, now: f64, pos: &Position, voice: Voice, onsets_per_s: f32, live_slide: usize) -> Option<FollowAction> {
        let started = *self.started.get_or_insert(now);
        match voice {
            Voice::Singing => {
                self.last_singing = Some(now);
                self.speaking_since = None;
                let r = onsets_per_s as f64;
                if (0.5..=12.0).contains(&r) { self.beat = Some(r) }
            }
            Voice::Speaking => { self.speaking_since.get_or_insert(now); }
            Voice::Music | Voice::Silence => self.speaking_since = None,
        }
        let sure = pos.confidence >= self.settings.min_conf && !pos.off_script;
        // A new word of the song is singing, whatever the room's label says (a loud band hides the voice from it).
        if self.note_word(now, pos) && sure && voice != Voice::Speaking { self.last_singing = Some(now) }
        let mode = self.settings.mode;
        if mode == Mode::Manual { return None }
        let live_has_words = self.song.slides.get(live_slide).is_some_and(|s| !s.instrumental);

        // Blanking (Automatic only).
        if mode == Mode::Auto {
            if self.blanked {
                // Back when singing moves on from where it stopped; with no words heard at all (the ear can't
                // tell words yet), as singing starts, on the live slide.
                let on = match pos.word { Some(w) => sure && pos.word != self.blank_word && w < self.song.words.len(), None => live_has_words };
                if voice != Voice::Singing || !on { return None }
                self.blanked = false;
                let slide = pos.word.map_or(live_slide, |w| self.song.words[w].slide);
                return Some(FollowAction::Unblank(slide));
            }
            let quiet = now - self.last_singing.unwrap_or(started);
            // Timers, only when Settings asks for them.
            let no_singing = matches!(voice, Voice::Music | Voice::Silence) && self.settings.blank_after_s.is_some_and(|b| quiet >= b as f64);
            let talking = self.speaking_since.is_some_and(|s| self.settings.talk_blank_after_s.is_some_and(|b| now - s >= b as f64));
            // The end of the song: its last word was sung on the last slide, and it has stayed quiet.
            let last_word_sung = sure && pos.word.is_some_and(|w| w + 1 == self.song.words.len() && self.song.words[w].slide == live_slide);
            let ended = last_word_sung && live_slide + 1 == self.song.slides.len() && voice != Voice::Singing && quiet >= SONG_END_QUIET;
            if live_has_words && (no_singing || talking || ended) {
                self.blanked = true;
                self.blank_word = pos.word;
                self.blank_why = if no_singing {
                    (false, self.settings.blank_after_s.unwrap_or(0.0))
                } else if talking {
                    (true, self.settings.talk_blank_after_s.unwrap_or(0.0))
                } else {
                    (false, SONG_END_QUIET as f32)
                };
                return Some(FollowAction::Blank);
            }
        }

        let act = |n: usize| Some(if mode == Mode::Auto { FollowAction::Show(n) } else { FollowAction::Suggest(n) });
        // Where the singers surely are: (word, its slide). Talk, an ad-lib or an unsure place gives none.
        let placed = pos.word.filter(|_| sure && voice != Voice::Speaking).and_then(|w| Some((w, self.song.words.get(w)?.slide)));

        // An instrumental slide is live (an intro, a break): as singing starts the next words come up, unless
        // the singers are already heard somewhere else.
        if !live_has_words && voice == Voice::Singing && !placed.is_some_and(|(w, at)| at != live_slide && !self.trailing(w, at, live_slide)) {
            if let Some(n) = (live_slide + 1..self.song.slides.len()).find(|&s| !self.song.slides[s].instrumental) { return act(n) }
        }

        // Talk, an ad-lib or an unsure place: hold.
        let (w, at) = placed?;

        if at != live_slide {
            // The singers are on another slide. Just behind the live one, on its last line, is where they are
            // after an early change: hold.
            return if self.trailing(w, at, live_slide) { None } else { act(at) };
        }

        // On the live slide: change `lead` seconds before the next slide's first word, never before the last
        // line has started, never past the last slide. Once the last word is heard the next words are up and
        // wait there, however long it is held or the band plays on.
        let span = &self.song.slides[live_slide];
        if w < span.last_line_first_word { return None }
        let next = live_slide + 1;
        let after = self.song.slides.get(next)?;
        if after.instrumental {
            // An instrumental slide would take the words off: only once the last word is sung and it is quiet.
            let quiet = now - self.last_singing.unwrap_or(started);
            return if w + 1 == span.end_word && voice != Voice::Singing && quiet >= INSTRUMENTAL_QUIET { act(next) } else { None };
        }
        let singing = self.last_singing.is_some_and(|t| now - t <= STILL_SINGING);
        if !singing { return None }
        let (_, heard_at) = self.last_word?;
        let left: u32 = self.song.words[w + 1..span.end_word].iter().map(|x| x.syllables as u32).sum();
        let predicted = heard_at + left as f64 / self.pace.or(self.beat).unwrap_or(DEFAULT_PACE);
        if predicted - now <= self.settings.lead() as f64 + 1e-6 { act(next) } else { None }
    }

    /// The singers at word `w` of slide `at` are just behind the live slide, on `at`'s last line: where they are
    /// after an early change.
    fn trailing(&self, w: usize, at: usize, live_slide: usize) -> bool {
        at < live_slide && (at + 1..live_slide).all(|s| self.song.slides[s].instrumental) && w >= self.song.slides[at].last_line_first_word
    }

    /// A new word moves the pace on (syllables sung since the last word over the time between). True when the
    /// word is a new one.
    fn note_word(&mut self, now: f64, pos: &Position) -> bool {
        let Some(w) = pos.word else { return false };
        if w >= self.song.words.len() { return false }
        match self.last_word {
            Some((last, _)) if last == w => return false,
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
        true
    }
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
