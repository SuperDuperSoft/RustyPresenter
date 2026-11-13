//! The live hookup: one worker thread takes heard words and voice labels, keeps the aligner and rules for the
//! live song, and moves the slide through `App::command` like a click (so undo, stage, phones and the stream
//! follow). The operator always wins: a slide moved or cleared by hand pauses Automatic until the singers are
//! heard, sure, from the operator's place on.
use super::align::{Aligner, Heard, Position};
use super::log::{LogAction, LogEntry, ReviewLog};
use super::lyrics::SongText;
use super::rules::{FollowAction, Mode, Rules, Settings, Voice};
use crate::app::App;
use crate::live::{Command, Envelope, MediaLayer};
use crate::model::{Slide, SlideContent};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// What the audio side tells the follower. Times are `Follower::now` seconds.
#[derive(Clone, Debug, PartialEq)]
pub enum FollowEvent {
    Heard(Heard),
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
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
#[derive(Clone, Debug, PartialEq, Default)]
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
    fn notice(&self, text: &str);
    fn save_mode(&self, item: Uuid, mode: Mode);
}

/// The live song being followed.
struct Run {
    item: Uuid,
    /// Slide count and text length: a change rebuilds the run (the song was edited).
    sig: (usize, usize),
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
}

fn sig(slides: &[Slide]) -> (usize, usize) {
    (slides.len(), slides.iter().map(|s| match &s.content { SlideContent::Text { text, .. } => text.len(), _ => 0 }).sum())
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
}

impl Default for Engine {
    fn default() -> Self {
        Engine { run: None, hold: false, lost: false, voice: Voice::Silence, onsets: 0.0, state: FollowState::default(), words: true, fresh: vec![] }
    }
}

impl Engine {
    pub fn new() -> Engine { Engine::default() }

    pub fn state(&self) -> FollowState { self.state.clone() }

    /// Hold ("don't move now") on or off.
    pub fn set_hold(&mut self, on: bool) { self.hold = on }

    /// The review log entries since the last call.
    pub fn take_log(&mut self) -> Vec<LogEntry> { std::mem::take(&mut self.fresh) }

    /// One step at `now`: take `ev` (or just look, on a timer), then act on the live song.
    pub fn step(&mut self, host: &dyn Host, now: f64, ev: Option<FollowEvent>) {
        let mut heard = None;
        match ev {
            Some(FollowEvent::Heard(h)) => heard = Some(h),
            Some(FollowEvent::Voice(v, _)) => self.voice = v,
            Some(FollowEvent::Onsets(r, _)) => self.onsets = r,
            Some(FollowEvent::InputLost(why)) => {
                if !self.lost { host.notice(&crate::tr!("Auto-follow stopped: the sound input is gone ({error}). Move the slides by hand.", error = why)) }
                self.lost = true;
            }
            Some(FollowEvent::Listening { words }) => { self.lost = false; self.words = words }
            None => {}
        }
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
        if self.run.as_ref().is_none_or(|r| r.item != live.item_id || r.sig != s) {
            self.run = Some(Run::new(&live, s));
            self.state.suggest = None;
        }
        let Some(run) = self.run.as_mut() else { return };
        run.rules.set_settings(Settings { mode, ..live.settings.clone() });

        // Moved or cleared by someone else: the operator wins.
        if (live.slide, live.words_up) != run.seen {
            run.seen = (live.slide, live.words_up);
            run.aligner.reset_to(live.slide);
            run.pos = run.aligner.position().clone();
            run.rules.reset();
            run.paused = self.words.then_some(live.slide);
            run.relock = 0;
            run.cleared = !live.words_up;
        }
        if mode != Mode::Suggest || self.state.suggest == Some(live.slide) { self.state.suggest = None }

        if let Some(h) = heard {
            let before = run.pos.word;
            run.pos = run.aligner.push(&h);
            if let Some(from) = run.paused {
                let on = run.pos.word.is_some() && run.pos.word != before && !run.pos.off_script && run.pos.plan_index >= from;
                run.relock = if on { run.relock.saturating_add(1) } else if run.pos.word != before || run.pos.off_script { 0 } else { run.relock };
                if run.relock >= live.settings.jump() && run.pos.confidence >= live.settings.min_conf { run.paused = None }
            }
        }

        let blocked = self.hold || live.video_playing || run.paused.is_some() || run.cleared;
        self.state.line = run.pos.word.and_then(|w| run.text.line_text(w));
        self.state.light = if mode == Mode::Manual {
            Light::Off
        } else if blocked {
            Light::Holding
        } else if run.rules.blanked() {
            Light::Blank
        } else if run.pos.word.is_none() {
            Light::Listening
        } else if run.pos.confidence >= live.settings.min_conf && !run.pos.off_script {
            Light::Sure
        } else {
            Light::Unsure
        };
        if mode == Mode::Manual || blocked { return }

        let Some(action) = run.rules.tick(now, &run.pos, self.voice, self.onsets, live.slide) else { return };
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
                let moved = mode == Mode::Auto && (n != live.slide || !live.words_up) && host.show(live.item_id, n, live.version);
                if moved { run.seen = (n, true) }
                let what = if matches!(action, FollowAction::Show(_)) { LogAction::Show(n) } else { LogAction::Unblank(n) };
                moved.then_some((what, said))
            }
            FollowAction::Blank => {
                let cleared = mode == Mode::Auto && live.words_up && host.clear_slide(live.version);
                if cleared {
                    run.seen = (live.slide, false);
                    self.state.light = Light::Blank;
                }
                let why = match self.voice {
                    Voice::Speaking => crate::tr!("talking for {secs} s", secs = live.settings.talk_blank_after_s.unwrap_or(0.0)),
                    _ => crate::tr!("no singing for {secs} s", secs = live.settings.blank_after_s.unwrap_or(0.0)),
                };
                cleared.then_some((LogAction::Blank, why))
            }
        };
        if let Some((action, reason)) = done {
            self.fresh.push(LogEntry { at: now, item: live.item_id, action, reason, confidence: run.pos.confidence });
        }
    }
}

impl Run {
    fn new(live: &LiveSong, sig: (usize, usize)) -> Run {
        let text = SongText::from_slides(&live.slides);
        let plan: Vec<usize> = (0..live.slides.len()).collect();
        let mut aligner = Aligner::new(&text, &plan).with_jump_words(live.settings.jump());
        aligner.reset_to(live.slide);
        let rules = Rules::new(live.settings.clone(), &text);
        Run { item: live.item_id, sig, pos: aligner.position().clone(), text, aligner, rules, seen: (live.slide, live.words_up), paused: None, relock: 0,
            cleared: !live.words_up }
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
        std::thread::Builder::new().name("follow".into()).spawn(move || work(host, rx, shared, kept, clock)).expect("follow thread");
        Follower { tx, state, log, hold: AtomicBool::new(false), clock }
    }

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

    /// Seconds since the follower started: the clock event times use.
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

fn work(host: Arc<dyn Host>, rx: Receiver<Msg>, state: Arc<Mutex<FollowState>>, log: Arc<Mutex<ReviewLog>>, clock: Instant) {
    let mut engine = Engine::new();
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
        *state.lock().unwrap() = engine.state();
        for e in engine.take_log() { log.lock().unwrap().push(e) }
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

    fn notice(&self, text: &str) { if let Some(app) = self.0.upgrade() { app.set_notice(text) } }

    fn save_mode(&self, item: Uuid, mode: Mode) {
        let Some(app) = self.0.upgrade() else { return };
        let _ = app.edit_untracked(|d| { d.automation.follow.insert(item, mode); Ok(()) });
        // The first song switched on: start listening (FFmpeg, off this thread).
        if mode != Mode::Manual { let _ = std::thread::Builder::new().name("follow-start".into()).spawn(move || app.start_follow_if_wanted()); }
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
