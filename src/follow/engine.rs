//! The live hookup: one worker thread takes heard words and voice labels, keeps the aligner and rules for the
//! live song, and moves the slide through `App::command` like a click (so undo, stage, phones and the stream
//! follow). The operator always wins: a slide moved or cleared by hand pauses Automatic until the singers are
//! heard, sure, for several words in a row: from the operator's place on, or on another slide (never the words
//! that only finish the slide they were on at the click).
use super::align::{Aligner, Heard, Position};
use super::ear::Lexicon;
use super::log::{LogAction, LogEntry, ReviewLog};
use super::lyrics::SongText;
use super::rules::{FollowAction, Mode, Rules, Settings, Voice};
use crate::app::App;
use crate::live::{Command, Envelope, MediaLayer};
use crate::model::{Slide, SlideContent};
use std::collections::VecDeque;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// What the audio side tells the follower. Audio times are on the sound input's own
/// clock (seconds of sound since it started), not `Follower::now`: the engine lines the two up from the voice
/// labels, so a word is timed by when it was sung, not by when the ear delivered it.
#[derive(Clone, Debug, PartialEq)]
pub enum FollowEvent {
    Heard(Heard),
    Audio { voice: Voice, onsets: f32, at: f64, held_since: Option<f64> },
    Voice(Voice, f64),
    /// Note onsets a second (the beat tracker).
    Onsets(f32, f64),
    /// The sound input stopped (device gone, FFmpeg ended): back to Manual, with this reason.
    InputLost(String),
    /// The sound input started. `words`: the ear recognises words (false until the acoustic model, part B: then
    /// only blanking follows the room and no slide ever moves on its own).
    Listening { words: bool },
}

/// The Live column's listening light.
#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Light {
    #[default]
    Off,
    Listening,
    Sure,
    Unsure,
    Holding,
    Blank,
}

/// What the Live column, stage and phone show about auto-follow.
#[derive(serde::Serialize, Clone, Debug, PartialEq, Default)]
pub struct FollowState {
    pub mode: Mode,
    pub light: Light,
    /// The line it thinks is being sung.
    pub line: Option<String>,
    pub holding: bool,
    /// Suggest: the slide it thinks is next, to highlight.
    pub suggest: Option<usize>,
}

/// The live item as the follower sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveSong {
    pub item_id: Uuid,
    pub is_song: bool,
    /// The song's live slides (empty for other items).
    pub slides: Vec<Slide>,
    pub slide: usize,
    /// The slide's words are on screen (not cleared).
    pub words_up: bool,
    pub video_playing: bool,
    /// The live version, sent as the base of each change so an operator's change in between wins.
    pub version: u64,
    /// The item's own mode, else the church's.
    pub mode: Mode,
    pub settings: Settings,
}

/// What the follower needs of the live engine: the App, or a fake in tests.
pub trait Host: Send + Sync {
    fn live(&self) -> Option<LiveSong>;
    /// Takes `slide` of `item` live if the live version is still `base`. True when it changed.
    fn show(&self, item: Uuid, slide: usize, base: u64) -> bool;
    /// Takes the words off if the live version is still `base`. True when it changed.
    fn clear_slide(&self, base: u64) -> bool;
    /// A mark that moves whenever someone changes the screen by hand (a click, a key, a phone, a macro; the
    /// follower's own changes count too: they go in by the same door). After the follower's own blank it tells
    /// of a Clear or Logo pressed then, which changes neither the slide nor whether its words are up. A host
    /// that cannot tell hand-made changes from others gives the live version (as here): every change then
    /// counts as made by hand, which only ever leaves the words off.
    fn hand_version(&self) -> Option<u64> { self.live().map(|l| l.version) }
    fn notice(&self, text: &str);
    fn save_mode(&self, item: Uuid, mode: Mode);
    /// Lines for the listening log (`Engine::take_trace`): the App keeps them in a file, others drop them.
    fn trace(&self, _lines: &[String]) {}
    /// What the follower shows changed (the light, the line, Hold): screens and phones look again.
    fn changed(&self) {}
}

/// The live song being followed.
struct Run {
    item: Uuid,
    /// A change in the lyrics or slide order rebuilds the run.
    sig: u64,
    /// Discard queued words from before this song or the operator's last move.
    audio_from: Option<f64>,
    /// Lyric decoding also recognizes spoken quotes. Hear a melodic phrase before using lyric evidence.
    vocals_confirmed: bool,
    text: SongText,
    aligner: Aligner,
    rules: Rules,
    pos: Position,
    /// The live slide and whether its words were up, as last seen or left by the follower itself.
    seen: (usize, bool),
    /// The operator moved here by hand: Automatic waits until the singers are heard from it on.
    paused: Option<usize>,
    relock: u8,
    /// The operator took the words off: nothing moves until they show a slide again.
    cleared: bool,
    /// The operator moved on to a later slide while the singers were still on an earlier one: the words that
    /// slide still had to come, each as (the slide it is on, its sound key). Heard after the move, they only
    /// finish the old slide. They are no proof for the operator's slide, even when it holds the very same words
    /// (the chorus on two slides in a row, verses that end on the same line): taken as proof, Automatic would
    /// lock on at that slide's last line and put the slide after it up.
    ///
    /// It only gets shorter by being heard. A word that is not in it (an ad-lib, a misheard word) does not end
    /// it, and neither does another hand move while Automatic is still waiting (Next, Next, Back; Next, Back,
    /// Next; a double-tapped Next): the singers are still where they were. A word only counts while its slide
    /// is before the operator's. All of it is forgotten when Automatic locks on again. Kept too long it only
    /// makes Automatic wait a few words longer; lost too soon it brings the wrong slide up.
    tail: Vec<(usize, String)>,
    /// The follower took the words off itself: `Host::hand_version` as that left it. The slide and "words up"
    /// stay the same when the operator then presses Clear or Logo, so only this mark tells of that click.
    off_at: Option<u64>,
}

fn sig(slides: &[Slide]) -> u64 {
    let mut hasher = DefaultHasher::new();
    slides.len().hash(&mut hasher);
    for slide in slides {
        match &slide.content {
            SlideContent::Text { text, .. } => Some(text.as_str()).hash(&mut hasher),
            _ => Option::<&str>::None.hash(&mut hasher),
        }
    }
    hasher.finish()
}

/// The decision core of the follower, without the thread: tests drive it with a fake host and clock.
pub struct Engine {
    run: Option<Run>,
    hold: bool,
    lost: bool,
    voice: Voice,
    onsets: f32,
    state: FollowState,
    /// The ear recognises words. Without them a hand move needs no re-lock: nothing would ever move by itself.
    words: bool,
    /// Review log entries not yet taken (`take_log`).
    fresh: Vec<LogEntry>,
    /// The listening log, not yet taken (`take_trace`): every word the ear delivered and where it put the
    /// singers, the operator's moves, and what the follower did. Not for the operator: it is how a wrong or
    /// missed change is explained afterwards (the review log only has the changes that were made).
    trace: Vec<String>,
    /// The live song's words for the phoneme ear.
    lexicon: Lexicon,
    /// The follower's clock minus the sound input's clock, from the latest voice label (they come ten times a
    /// second, almost as the sound does): added to the time a word was sung, it gives that time on the
    /// follower's clock. `None` until a label has come.
    skew: Option<f64>,
    voice_labels: VecDeque<(f64, Voice)>,
    /// The last live report on the follower's clock. An input can stall without closing its pipe.
    audio_reported: Option<f64>,
    audio_expected: bool,
    held_since: Option<f64>,
}

/// Discard words older than this instead of giving them a new timestamp and moving on stale evidence.
const MAX_EAR_LAG: f64 = 5.0;
const AUDIO_STALL: f64 = 0.5;
/// Reports come every 100 ms. Allow for fractional frame timestamps and scheduling jitter.
const LABEL_SLOP: f64 = 0.13;
/// Brief pitch plateaus occur in spoken syllables, too. Ask for a continuous span within the phrase.
const VOCAL_PROOF: f64 = 0.39;
/// The most lines of the listening log kept while nobody takes them (Practice): the oldest half goes.
const TRACE_MAX: usize = 4096;
/// The most words kept in `Run::tail`. What a slide still has to come is far fewer; more only pile up over many
/// hand moves in one wait, and then the oldest go first.
const TAIL_MAX: usize = 64;

impl Default for Engine {
    fn default() -> Self {
        Engine { run: None, hold: false, lost: false, voice: Voice::Silence, onsets: 0.0, state: FollowState::default(), words: true, fresh: vec![],
            trace: vec![], lexicon: Lexicon::default(), skew: None, voice_labels: VecDeque::new(),
            audio_reported: None, audio_expected: false, held_since: None }
    }
}

impl Engine {
    pub fn new() -> Engine { Engine::default() }

    pub fn state(&self) -> FollowState { self.state.clone() }

    /// The live song's words, for the ear (`ear::room_ear`).
    pub fn lexicon(&self) -> Lexicon { self.lexicon.clone() }

    /// Hold ("don't move now") on or off.
    pub fn set_hold(&mut self, on: bool) { self.hold = on }

    /// The review log entries since the last call.
    pub fn take_log(&mut self) -> Vec<LogEntry> { std::mem::take(&mut self.fresh) }

    /// The listening log's lines since the last call.
    pub fn take_trace(&mut self) -> Vec<String> { std::mem::take(&mut self.trace) }

    fn note(&mut self, now: f64, line: String) { note(&mut self.trace, now, line) }

    fn voice_at(&mut self, voice: Voice, at: f64, now: f64) {
        self.voice = voice;
        self.skew = Some(now - at);
        self.voice_labels.push_back((at, voice));
        while self.voice_labels.len() > 64 || self.voice_labels.front().is_some_and(|&(t, _)| at - t > MAX_EAR_LAG + LABEL_SLOP) {
            self.voice_labels.pop_front();
        }
    }

    fn heard_voice(&self, at: f64) -> Option<Voice> {
        self.voice_labels.iter().rev()
            .filter(|&&(t, _)| (t - at).abs() <= LABEL_SLOP)
            .min_by(|&&(a, _), &&(b, _)| (a - at).abs().total_cmp(&(b - at).abs()))
            .map(|&(_, voice)| voice)
    }

    fn melody_near(labels: &VecDeque<(f64, Voice)>, at: f64, from: Option<f64>) -> bool {
        let mut start = at;
        let mut last = f64::NEG_INFINITY;
        let earliest = (at - 0.75).max(from.unwrap_or(f64::NEG_INFINITY));
        for &(t, voice) in labels {
            if t < earliest || t > at + LABEL_SLOP { continue }
            if voice != Voice::Singing {
                last = f64::NEG_INFINITY;
                continue;
            }
            if t - last > 0.15 { start = t; }
            if t - start >= VOCAL_PROOF { return true }
            last = t;
        }
        false
    }

    /// One step at `now`: take `ev` (or just look, on a timer), then act on the live song.
    pub fn step(&mut self, host: &dyn Host, now: f64, ev: Option<FollowEvent>) {
        let mut heard = None;
        match ev {
            Some(FollowEvent::Heard(h)) => heard = Some(h),
            Some(FollowEvent::Audio { voice, onsets, at, held_since }) => {
                if self.audio_reported.is_some_and(|last| now - last > AUDIO_STALL) {
                    self.voice_labels.clear();
                    if let Some(run) = self.run.as_mut() { run.recover_audio(Some(at)); }
                }
                self.voice_at(voice, at, now);
                self.onsets = onsets;
                self.audio_reported = Some(now);
                self.held_since = held_since.map(|start| now - (at - start).max(0.0));
                if self.voice_labels.iter().rev().take(3).filter(|(_, v)| *v == Voice::Speaking).count() == 3 {
                    if let Some(run) = self.run.as_mut() { run.vocals_confirmed = false; }
                }
            }
            Some(FollowEvent::Voice(v, at)) => { self.voice_at(v, at, now); self.audio_reported = Some(now) }
            Some(FollowEvent::Onsets(r, _)) => self.onsets = r,
            Some(FollowEvent::InputLost(why)) => {
                if !self.lost { self.note(now, format!("the sound input is gone: {why}")) }
                if !self.lost { host.notice(&crate::tr!("Auto-follow stopped: the sound input is gone ({error}). Move the slides by hand.", error = why)) }
                self.lost = true;
            }
            // A sound input that starts (again) starts its clock again: wait for its first voice label.
            Some(FollowEvent::Listening { words }) => {
                self.note(now, format!("listening; the ear hears words: {}", if words { "yes" } else { "no (no speech model here)" }));
                self.lost = false;
                self.words = words;
                self.skew = None;
                self.voice = Voice::Silence;
                self.onsets = 0.0;
                self.voice_labels.clear();
                self.audio_reported = None;
                self.audio_expected = true;
                self.held_since = None;
                if let Some(run) = self.run.as_mut() {
                    // The new device's sample clock starts over.
                    run.recover_audio(None);
                }
            }
            None => {}
        }
        // Place acoustic evidence on the follower's clock. A stale word never gets a fresh timestamp.
        let sung = match (&heard, self.skew) {
            (Some(h), Some(skew)) => (h.at + skew).min(now),
            _ => now,
        };
        self.state.holding = self.hold;
        let live = host.live().filter(|l| l.is_song);
        let Some(live) = live else {
            self.run = None;
            self.state = FollowState { holding: self.hold, ..FollowState::default() };
            return;
        };
        let mode = if self.lost { Mode::Manual } else { live.mode };
        self.state.mode = mode;
        let s = sig(&live.slides);
        let audio_now = self.voice_labels.back().map(|&(at, _)| at);
        if self.run.as_ref().is_none_or(|r| r.item != live.item_id || r.sig != s) {
            let audio_from = if self.run.is_some() || self.audio_expected {
                audio_now
            } else {
                None
            };
            let run = Run::new(&live, s, audio_from);
            self.lexicon.set_song(&run.text);
            if mode != Mode::Manual {
                note(&mut self.trace, now, format!("song: {} slides, {} words, {mode:?}, on slide {}", live.slides.len(), run.text.words.len(), live.slide + 1));
            }
            self.run = Some(run);
            self.state.suggest = None;
        }
        let audio_stalled = self.audio_expected && self.audio_reported.is_none_or(|at| now - at > AUDIO_STALL);
        let require_vocals = self.audio_expected && self.words;
        let ignored = heard.as_ref().and_then(|h| {
            if !h.at.is_finite() || !h.conf.is_finite() || now - sung > MAX_EAR_LAG {
                Some("expired or invalid audio")
            } else if audio_stalled {
                Some("waiting for live audio")
            } else if self.heard_voice(h.at).unwrap_or(self.voice) == Voice::Speaking {
                Some("speech at the time the word was heard")
            } else {
                None
            }
        });
        let Some(run) = self.run.as_mut() else { return };
        run.rules.set_settings(Settings { mode, ..live.settings.clone() });

        // Moved or cleared by someone else: the operator wins. The slide and whether its words are up tell of
        // most clicks. Not of a Clear or Logo pressed after the follower's own blank (the song's end, a Settings
        // timer): both stay as they were. There the mark of the last hand-made change is asked too, or the next
        // sung words would put the lyrics back over the operator's logo.
        let by_hand = !live.words_up && run.off_at.is_some_and(|mark| host.hand_version() != Some(mark));
        if (live.slide, live.words_up) != run.seen || by_hand {
            // The words the singers still have to sing on the slide they are on (see `Run::tail`). Automatic was
            // following: they are where the aligner has them, and an older tail is void. It was still waiting
            // after an earlier hand move: the singers may well be finishing the slide that tail is from, whatever
            // place the aligner has since made of their words (the old slide's own words can seat it on the
            // operator's slide). So the tail stays, and what the aligner's slide still has to come is added.
            // With no place since the last move (two clicks in a row) it stays as it is.
            if run.paused.is_none() { run.tail.clear() }
            let at = run.pos.word.and_then(|w| Some((w, run.text.words.get(w)?.slide)));
            if let Some((w, old)) = at {
                let rest = &run.text.words[w + 1..run.text.slides[old].end_word];
                run.tail.extend(rest.iter().map(|x| (old, x.key.clone())));
            }
            let over = run.tail.len().saturating_sub(TAIL_MAX);
            if over > 0 { run.tail = run.tail.split_off(over) }
            run.seen = (live.slide, live.words_up);
            run.audio_from = audio_now;
            run.off_at = None;
            run.aligner.reset_to(live.slide);
            // The operator may have gone back into the middle of a slide: the first word heard may be anywhere on it.
            run.aligner.seat_anywhere();
            run.pos = run.aligner.position().clone();
            run.rules.reset();
            run.paused = self.words.then_some(live.slide);
            run.relock = 0;
            run.cleared = !live.words_up;
            if mode != Mode::Manual {
                note(&mut self.trace, now, format!("by hand: slide {}{}", live.slide + 1, if live.words_up { "" } else { ", words off" }));
            }
        }
        if mode != Mode::Suggest || self.state.suggest == Some(live.slide) { self.state.suggest = None }

        let before_cue = heard.as_ref().zip(run.audio_from)
            .is_some_and(|(word, at)| word.at < at - LABEL_SLOP);
        let melody = heard.as_ref().is_some_and(|h| Self::melody_near(&self.voice_labels, h.at, run.audio_from));
        let ignored = ignored.or(before_cue.then_some("audio from before the current cue"));
        let waiting = require_vocals && !run.vocals_confirmed && !melody;
        if let Some((h, why)) = heard.as_ref().zip(ignored.or(waiting.then_some("waiting for a sung phrase"))) {
            if mode != Mode::Manual {
                note(&mut self.trace, now, format!("ignored lyric evidence {:<10} {:>3.0}%: {why}", h.key, h.conf * 100.0));
            }
            heard = None;
        }
        if let Some(h) = heard {
            let before = run.pos.word;
            run.pos = run.aligner.push(&h);
            if melody && run.pos.word.is_some() && run.pos.word != before
                && run.pos.confidence >= live.settings.min_conf && !run.pos.off_script {
                run.vocals_confirmed = true;
            }
            if let Some(from) = run.paused {
                if let Some(i) = run.tail.iter().position(|(slide, key)| *slide < from && *key == h.key) {
                    // A word a slide before the operator's still had to come (the ear may have missed some
                    // before it: they go too). The singers are finishing that slide. Wherever the aligner put
                    // the word, it is not counted, the count starts again, and Automatic stays paused.
                    run.tail = run.tail.split_off(i + 1);
                    run.relock = 0;
                } else {
                    // Any other word counts as it comes. It does not end the tail: one ad-lib or misheard word
                    // while the old slide is being finished must not turn the rest of that slide into proof for
                    // the operator's. A tail word heard later starts the count again.
                    //
                    // The singers may also be heard on a slide BEFORE the operator's (the leader goes back to the
                    // chorus as the operator presses Next; a click two slides ahead while they sing the one in
                    // between). The aligner only puts them there by a jump it has confirmed, and the words that
                    // finish the old slide never count (the tail), so a place there is real: its words count like
                    // any others, Automatic locks on, and the rules take the screen to the singers.
                    let on = run.pos.word.is_some() && run.pos.word != before && !run.pos.off_script;
                    run.relock = if on { run.relock.saturating_add(1) } else if run.pos.word != before || run.pos.off_script { 0 } else { run.relock };
                    // The operator's slide has no words (an intro, a break): waiting for `jump` words would leave
                    // the screen empty through most of the next slide's first line. From there the aligner can
                    // only place a single word on the first words of the next slide with words, so one sure word
                    // is enough. (The room's label alone is not: a last note still held at the click would undo
                    // the click at once. Nor is a word whose sound key is one letter, "Oh", "I": the ear writes
                    // the vowel of that held note down as just such a word. Those need `jump` words in a row
                    // like any other hand move; the next longer word brings the slide up.)
                    let wordless = run.text.slides.get(from).is_some_and(|s| s.instrumental) && run.pos.plan_index >= from;
                    let need = if wordless && h.key.chars().count() >= 2 { 1 } else { live.settings.jump() };
                    if run.relock >= need && run.pos.confidence >= live.settings.min_conf {
                        run.paused = None;
                        // Locked on again: the old slide is over.
                        run.tail.clear();
                    }
                }
            }
            if mode != Mode::Manual {
                let place = match run.pos.word.and_then(|w| run.text.words.get(w)) {
                    Some(w) => format!("slide {} \"{}\"", w.slide + 1, w.text),
                    None => format!("slide {}, no word yet", run.pos.plan_index + 1),
                };
                let line = format!("heard {:<10} {:>3.0}% (sung {:.1} s ago) -> {place}, sure {:.0}%{}{}{}", h.key, h.conf * 100.0, now - sung,
                    run.pos.confidence * 100.0, if run.pos.word == before { ", place unchanged" } else { "" },
                    if run.pos.off_script { ", off the song" } else { "" },
                    if run.paused.is_some() { ", waiting after a move by hand" } else { "" });
                note(&mut self.trace, now, line);
            }
        }

        let unconfirmed = require_vocals && !run.vocals_confirmed;
        if unconfirmed { self.state.suggest = None; }
        let blocked = self.hold || live.video_playing || run.paused.is_some() || run.cleared || audio_stalled;
        self.state.line = run.pos.word.and_then(|w| run.text.line_text(w));
        self.state.light = if mode == Mode::Manual {
            Light::Off
        } else if blocked {
            Light::Holding
        } else if run.rules.blanked() {
            Light::Blank
        } else if unconfirmed || run.pos.word.is_none() {
            Light::Listening
        } else if run.pos.confidence >= live.settings.min_conf && !run.pos.off_script {
            Light::Sure
        } else {
            Light::Unsure
        };
        if mode == Mode::Manual || blocked {
            // Not looking, but the ear goes on. The rules are told the place and when it was heard: by the time
            // they look again it is as old as it is, and a place heard long before moves nothing then.
            run.rules.missed(now, sung, &run.pos, self.voice);
            return;
        }

        let Some(action) = run.rules.tick_audio(now, sung, &run.pos, self.voice, self.onsets, live.slide, self.held_since) else { return };
        if unconfirmed && matches!(action,
            FollowAction::Show(n) | FollowAction::Suggest(n) | FollowAction::Unblank(n) if n != live.slide
        ) {
            if matches!(action, FollowAction::Unblank(_)) { run.rules.set_blanked(true); }
            return;
        }
        let said = match run.pos.word {
            Some(w) => crate::tr!("heard \"{words}\"", words = run.text.words[w.saturating_sub(3)..=w].iter().map(|x| x.text.as_str()).collect::<Vec<_>>().join(" ")),
            None => crate::tr!("singing again"),
        };
        let done = match action {
            FollowAction::Suggest(n) => {
                let new = self.state.suggest != Some(n);
                self.state.suggest = Some(n);
                new.then_some((LogAction::Suggest(n), said))
            }
            FollowAction::Show(n) | FollowAction::Unblank(n) => {
                let want = mode == Mode::Auto && (n != live.slide || !live.words_up);
                let moved = want && host.show(live.item_id, n, live.version);
                if moved {
                    run.seen = (n, true);
                    run.off_at = None;
                }
                // Refused (someone else changed the screen at that moment): still blank, try again next look.
                if want && !moved && matches!(action, FollowAction::Unblank(_)) { run.rules.set_blanked(true) }
                let what = if matches!(action, FollowAction::Show(_)) { LogAction::Show(n) } else { LogAction::Unblank(n) };
                moved.then_some((what, said))
            }
            FollowAction::Blank => {
                let want = mode == Mode::Auto && live.words_up;
                let cleared = want && host.clear_slide(live.version);
                if want && !cleared { run.rules.set_blanked(false) }
                if cleared {
                    run.seen = (live.slide, false);
                    // From here on a hand-made change with the words still off is the operator's (Clear, Logo).
                    run.off_at = host.hand_version();
                    self.state.light = Light::Blank;
                }
                let why = match run.rules.blank_why() {
                    (true, secs) => crate::tr!("talking for {secs} s", secs = secs),
                    (false, secs) => crate::tr!("no singing for {secs} s", secs = secs),
                };
                cleared.then_some((LogAction::Blank, why))
            }
        };
        if let Some((action, reason)) = done {
            let what = match action {
                LogAction::Show(n) => format!("SHOW slide {}", n + 1),
                LogAction::Suggest(n) => format!("SUGGEST slide {}", n + 1),
                LogAction::Blank => "WORDS OFF".to_string(),
                LogAction::Unblank(n) => format!("WORDS BACK on slide {}", n + 1),
            };
            note(&mut self.trace, now, format!("{what}: {reason}, sure {:.0}%", run.pos.confidence * 100.0));
            self.fresh.push(LogEntry { at: now, item: live.item_id, action, reason, confidence: run.pos.confidence });
        }
    }
}

/// Adds a line to the listening log, stamped with the follower's clock.
fn note(trace: &mut Vec<String>, now: f64, line: String) {
    if trace.len() >= TRACE_MAX { trace.drain(..TRACE_MAX / 2); }
    trace.push(format!("{now:9.2}  {line}"));
}

impl Run {
    fn recover_audio(&mut self, from: Option<f64>) {
        self.audio_from = from;
        self.vocals_confirmed = false;
        self.aligner.reset_to(self.seen.0);
        self.aligner.seat_anywhere();
        self.pos = self.aligner.position().clone();
        self.relock = 0;
        let blanked = self.rules.blanked();
        self.rules.reset();
        self.rules.set_blanked(blanked);
    }

    fn new(live: &LiveSong, sig: u64, audio_from: Option<f64>) -> Run {
        let text = SongText::from_slides(&live.slides);
        let plan: Vec<usize> = (0..live.slides.len()).collect();
        let mut aligner = Aligner::new(&text, &plan).with_jump_words(live.settings.jump());
        aligner.reset_to(live.slide);
        let rules = Rules::new(live.settings.clone(), &text);
        Run { item: live.item_id, sig, audio_from, vocals_confirmed: false, pos: aligner.position().clone(), text, aligner, rules, seen: (live.slide, live.words_up), paused: None, relock: 0,
            cleared: !live.words_up, tail: vec![], off_at: None }
    }
}

enum Msg { Event(FollowEvent), Look, Hold(bool), Mode(Uuid, Mode) }

/// The follower's worker thread. Ends when the `Follower` is dropped.
pub struct Follower {
    tx: Sender<Msg>,
    state: Arc<Mutex<FollowState>>,
    log: Arc<Mutex<ReviewLog>>,
    hold: AtomicBool,
    clock: Instant,
    lexicon: Lexicon,
}

/// How often the worker looks without new events (blanking needs the clock to move).
const LOOK_EVERY: Duration = Duration::from_millis(100);
/// With no events for this long (no sound input), the worker stops looking by itself.
const IDLE_AFTER: f64 = 5.0;

impl Follower {
    pub fn start(app: Weak<App>) -> Follower { Follower::start_with(Arc::new(AppHost(app))) }

    pub fn start_with(host: Arc<dyn Host>) -> Follower {
        let (tx, rx) = channel();
        let state = Arc::new(Mutex::new(FollowState::default()));
        let log = Arc::new(Mutex::new(ReviewLog::default()));
        let clock = Instant::now();
        let (shared, kept) = (state.clone(), log.clone());
        let engine = Engine::new();
        let lexicon = engine.lexicon();
        std::thread::Builder::new().name("follow".into()).spawn(move || work(engine, host, rx, shared, kept, clock)).expect("follow thread");
        Follower { tx, state, log, hold: AtomicBool::new(false), clock, lexicon }
    }

    /// The live song's words, for the ear (`ear::room_ear`).
    pub fn lexicon(&self) -> Lexicon { self.lexicon.clone() }

    /// A sender for the sound input (`audio::Capture`): what it sends reaches the follower until it is dropped.
    pub fn sender(&self) -> Sender<FollowEvent> {
        let (tx, rx) = channel::<FollowEvent>();
        let to = self.tx.clone();
        let _ = std::thread::Builder::new().name("follow-in".into()).spawn(move || {
            for ev in rx { if to.send(Msg::Event(ev)).is_err() { break } }
        });
        tx
    }

    /// This session's review log, oldest first.
    pub fn log(&self) -> Vec<LogEntry> { self.log.lock().unwrap().entries() }

    /// Seconds since the follower started: the clock its looks and the review log use. (Events carry the sound
    /// input's own clock; any clock that runs at the same speed will do, this one too.)
    pub fn now(&self) -> f64 { self.clock.elapsed().as_secs_f64() }

    fn send(&self, m: Msg) { let _ = self.tx.send(m); }

    pub fn feed(&self, ev: FollowEvent) { self.send(Msg::Event(ev)) }

    /// The live item or slide changed (from `App::broadcast_live`): look again now.
    pub fn on_live_change(&self) { self.send(Msg::Look) }

    pub fn state(&self) -> FollowState { self.state.lock().unwrap().clone() }

    pub fn holding(&self) -> bool { self.hold.load(Ordering::Relaxed) }

    pub fn set_hold(&self, on: bool) {
        self.hold.store(on, Ordering::Relaxed);
        self.send(Msg::Hold(on));
    }

    /// The song item's own mode (saved with the service).
    pub fn set_mode(&self, item: Uuid, mode: Mode) { self.send(Msg::Mode(item, mode)) }
}

fn work(mut engine: Engine, host: Arc<dyn Host>, rx: Receiver<Msg>, state: Arc<Mutex<FollowState>>, log: Arc<Mutex<ReviewLog>>, clock: Instant) {
    let mut last_event = f64::NEG_INFINITY;
    loop {
        let now = || clock.elapsed().as_secs_f64();
        let ev = match rx.recv_timeout(LOOK_EVERY) {
            Ok(Msg::Event(ev)) => { last_event = now(); Some(ev) }
            Ok(Msg::Look) => None,
            Ok(Msg::Hold(on)) => { engine.set_hold(on); None }
            Ok(Msg::Mode(item, mode)) => { host.save_mode(item, mode); None }
            Err(RecvTimeoutError::Timeout) => {
                if now() - last_event > IDLE_AFTER { continue }
                None
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        engine.step(host.as_ref(), now(), ev);
        let new = engine.state();
        let changed = *state.lock().unwrap() != new;
        for e in engine.take_log() { log.lock().unwrap().push(e) }
        let trace = engine.take_trace();
        if !trace.is_empty() { host.trace(&trace) }
        if changed {
            *state.lock().unwrap() = new;
            host.changed();
        }
    }
}

/// The App as the follower's host.
pub struct AppHost(pub Weak<App>);

impl Host for AppHost {
    fn live(&self) -> Option<LiveSong> {
        let app = self.0.upgrade()?;
        let data = app.data.lock().unwrap();
        let live = app.live.lock().unwrap();
        let (item, cursor) = (live.item.as_ref()?, live.cursor.as_ref()?);
        let is_song = item.kind == "song";
        Some(LiveSong {
            item_id: item.item_id,
            is_song,
            slides: if is_song { item.slides.clone() } else { vec![] },
            slide: cursor.slide,
            words_up: live.frame.slide.is_some(),
            video_playing: live.frame.media.as_ref().is_some_and(|m| matches!(m.v, MediaLayer::Video { .. })),
            version: live.version,
            mode: data.automation.follow.get(&item.item_id).copied().unwrap_or(data.settings.follow.mode),
            settings: data.settings.follow.clone(),
        })
    }

    fn show(&self, item_id: Uuid, slide: usize, base: u64) -> bool {
        let Some(app) = self.0.upgrade() else { return false };
        let out = app.command(Envelope { id: format!("follow-{}", Uuid::new_v4()), base: Some(base), cmd: Command::Show { item_id, slide } });
        out.ok && !out.duplicate
    }

    fn clear_slide(&self, base: u64) -> bool {
        let Some(app) = self.0.upgrade() else { return false };
        // A clear names no base: check it here, so a change made in between wins.
        if app.live.lock().unwrap().version != base { return false }
        let out = app.command(Envelope { id: format!("follow-{}", Uuid::new_v4()), base: None, cmd: Command::ClearSlide });
        out.ok && !out.duplicate
    }

    /// The live version of the last visual change made by hand (not an overlay, not an automatic slide turn).
    fn hand_version(&self) -> Option<u64> {
        let app = self.0.upgrade()?;
        let mark = app.live.lock().unwrap().manual_version;
        Some(mark)
    }

    fn notice(&self, text: &str) { if let Some(app) = self.0.upgrade() { app.set_notice(text) } }

    /// The listening log: `<data>/logs/follow-YYYYMMDD.log`, each line with the time of day.
    fn trace(&self, lines: &[String]) {
        let Some(app) = self.0.upgrade() else { return };
        write_trace(&app.store.root.join("logs"), chrono::Local::now().naive_local(), lines);
    }

    fn changed(&self) { if let Some(app) = self.0.upgrade() { app.broadcast_live() } }

    fn save_mode(&self, item: Uuid, mode: Mode) {
        let Some(app) = self.0.upgrade() else { return };
        let _ = app.edit_untracked(|d| { d.automation.follow.insert(item, mode); Ok(()) });
        // The first song switched on: start listening (FFmpeg, off this thread).
        if mode != Mode::Manual { let _ = std::thread::Builder::new().name("follow-start".into()).spawn(move || app.start_follow_if_wanted()); }
    }
}

/// The listening logs kept: one a day, the newest of them.
const TRACE_DAYS: usize = 10;

/// Adds `lines` to the day's listening log in `dir`. A new day's first line drops the oldest logs. Nothing here
/// may stop the follower: a disk that can't be written is left alone.
fn write_trace(dir: &std::path::Path, now: chrono::NaiveDateTime, lines: &[String]) {
    use std::io::Write;
    let path = dir.join(format!("follow-{}.log", now.format("%Y%m%d")));
    if !path.exists() {
        let _ = std::fs::create_dir_all(dir);
        let mut old: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path())
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("follow-") && n.ends_with(".log"))).collect();
        old.sort();
        let extra = (old.len() + 1).saturating_sub(TRACE_DAYS);
        for p in old.into_iter().take(extra) { let _ = std::fs::remove_file(p); }
    }
    let time = now.format("%H:%M:%S");
    let text: String = lines.iter().map(|l| format!("{time} {l}\n")).collect();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) { let _ = f.write_all(text.as_bytes()); }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
