//! The live engine: what the room should show, which commands are allowed,
//! and which reports from the room are still valid.
//!
//! Rules implemented here (spec §5–§7, review R2-01..R2-03):
//! * Every change to the output bumps `version`.
//! * Each command carries a unique `id`. Delivering the same id twice has no
//!   further effect; a new id for the same action (e.g. retriggering a video)
//!   applies again.
//! * Relative commands (Next/Prev) and remote absolute commands carry `base`,
//!   the version the sender saw. A stale base is rejected, so a Next sent before
//!   Clear All cannot undo the clear, and two Nexts based on the same state
//!   advance once.
//! * Every playing video/audio gets a fresh playback instance id. Reports from
//!   the room are accepted while their instance is in the requested frame or
//!   the committed frame (still on air), and ignored once retired.
use crate::{model::*, recovery::state::LiveState, sections::{JumpTarget, LiveChange, SectionKind}};
use crate::theme::{overlay::{self, Overlay, OverlaySlot, slots}, strings as theme_strings};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use uuid::Uuid;

pub mod drawing;
mod song;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Inst<T> {
    pub inst: u64,
    #[serde(flatten)]
    pub v: T,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaLayer {
    Video { asset: AssetId, end: EndAction, paused: bool },
    Still { asset: AssetId },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AudioLayer {
    pub asset: AssetId,
    pub title: String,
    pub looped: bool,
    pub paused: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SlideLayer {
    Text {
        text: String,
        footer: Option<String>,
        reference: Option<String>,
        scripture: bool,
        /// The slide's largest text size (`SlideContent::Text::size`); 0 = the theme's.
        #[serde(default, skip_serializing_if = "is_zero")]
        size: u32,
        /// Every slide of the passage this slide belongs to: they all share one text size. Empty for songs.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        passage: Vec<PassageSlide>,
        /// More Bibles on this slide (`SlideContent::Text::parallel`); `text` is the main one's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parallel: Option<crate::scripture::Parallel>,
        /// The live item's theme as it was taken live (`LiveItem::theme`). `None` only in a `live-state.json` from
        /// before themes: then `LiveView.theme` (the church default's style).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        theme: Option<Theme>,
    },
    Image {
        asset: AssetId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transition: Option<crate::design::Transition>,
    },
    /// A designed slide (N26). `build`: it adds the next click to the slide just before it, so the room changes
    /// only what is new. `play`: that click's animations (or, as the slide comes up, its own), for the room to play.
    Design {
        design: crate::design::Design,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        build: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        play: Option<Box<crate::design::Play>>,
    },
}

fn is_zero(n: &u32) -> bool { *n == 0 }

/// The full layer set the room shows. Bottom → top: background, media, slide, logo, overlays. Audio is separate.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Frame {
    pub background: Option<Inst<Background>>,
    pub media: Option<Inst<MediaLayer>>,
    pub slide: Option<SlideLayer>,
    pub logo: bool,
    pub audio: Option<Inst<AudioLayer>>,
    /// Overlays that stay up while slides change (a logo bug, a clock, a sign), sorted by `(z, key)`, one per key.
    /// Each is a copy of what was shown (FEAT-1 spec §4.5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overlays: Vec<OverlaySlot>,
    /// The newest jump asked of the playing video or audio (N4). An instant, not a state: never saved or sent, so a
    /// restart never replays it. The room applies each `n` once, and only to the player with `inst`.
    #[serde(skip)]
    pub seek: Option<Seek>,
}

/// A jump in a playing video or audio (N4: back 15 s, forward 15 s, 10 s before the end).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Seek { pub inst: u64, pub n: u64, pub to: SeekTo }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SeekTo {
    /// Forward (positive) or back (negative) from where it is.
    By { secs: f64 },
    /// This many seconds before the end, to check what happens when it finishes.
    BeforeEnd { secs: f64 },
}

/// Which layers a frame shows, for clear buttons that light up while their layer is on screen.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Showing { pub background: bool, pub media: bool, pub text: bool, pub logo: bool, pub overlays: bool, pub audio: bool }

impl Frame {
    /// Which layers have something on them.
    pub fn showing(&self) -> Showing {
        Showing {
            background: self.background.is_some(), media: self.media.is_some(), text: self.slide.is_some(), logo: self.logo,
            overlays: !self.overlays.is_empty(), audio: self.audio.is_some(),
        }
    }

    /// Is there video or audio here, rather than only slides and pictures?
    pub fn plays(&self) -> bool {
        self.media.as_ref().is_some_and(|m| matches!(m.v, MediaLayer::Video { .. }))
            || self.audio.is_some()
            || self.background.as_ref().is_some_and(|b| b.v.moving())
    }
    pub(crate) fn instances(&self) -> impl Iterator<Item = u64> + '_ {
        self.background.iter().map(|b| b.inst).chain(self.media.iter().map(|m| m.inst)).chain(self.audio.iter().map(|a| a.inst))
    }
    fn retire(&mut self, inst: u64) -> bool {
        let mut changed = false;
        if self.background.as_ref().is_some_and(|b| b.inst == inst) { self.background = None; changed = true; }
        if self.media.as_ref().is_some_and(|m| m.inst == inst) { self.media = None; changed = true; }
        if self.audio.as_ref().is_some_and(|a| a.inst == inst) { self.audio = None; changed = true; }
        changed
    }
}

/// Snapshot of the live item, taken when it goes live (spec §7.2).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LiveItem {
    pub item_id: Uuid,
    pub title: String,
    pub kind: String,
    pub slides: Vec<Slide>,
    pub background: Background,
    /// The resolved text style (item > service > church default), captured when the item was taken live, so editing
    /// a theme never changes the room by itself (FEAT-1 spec §5.3).
    #[serde(default)]
    pub theme: Theme,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Cursor {
    pub item_id: Uuid,
    /// Position of the item in the service when it went live (for removed items).
    pub item_index: usize,
    pub slide: usize,
}

#[derive(Clone, Debug)]
struct Snapshot {
    version: u64,
    frame: Frame,
    cursor: Option<Cursor>,
    item: Option<LiveItem>,
    changed: Option<LiveChange>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Next,
    Prev,
    /// Take a slide of a service item live (also used to retrigger).
    Show { item_id: Uuid, slide: usize },
    ClearAll,
    ClearSlide,
    ClearMedia,
    ClearAudio,
    Logo,
    /// Pause/resume the foreground video (`paused`), naming the playback instance.
    MediaPause { inst: u64, paused: bool },
    AudioPause { inst: u64, paused: bool },
    /// Jump in the playing video or audio with this instance (N4).
    Seek { inst: u64, to: SeekTo },
    /// Turn saved overlay `id` on or off. Turning on carries `seen_clear`, the `clear_epoch` of the sender's newest
    /// view: a show sent before a clear the sender had not seen is refused (FEAT-1 spec §5.2). No `base`. In JSON the
    /// overlay's id is `"overlay"`, as the envelope's `"id"` is the command's own.
    Overlay { #[serde(rename = "overlay")] id: Uuid, on: bool, #[serde(default)] seen_clear: Option<u64> },
    /// F6: take every overlay down.
    ClearOverlays,
    /// Jump the live song to a section (FEAT-4). Relative: needs `base`. With `occurrence`, `kind` is its kind.
    Jump { item_id: Uuid, kind: SectionKind, #[serde(default)] number: Option<u8>, #[serde(default)] occurrence: Option<u16> },
    /// Show the library's version of the live item at the same place (Update screen). Operator only.
    Refresh { item_id: Uuid },
    /// FEAT-2: put a timer on the screen. Carries `seen_clear` like an overlay.
    ShowTimer { timer: Uuid, #[serde(default)] seen_clear: Option<u64> },
    HideTimer { timer: Uuid },
    /// FEAT-2: show a message filled from a template (one at a time; a new one replaces it).
    ShowMessage { template: Uuid, #[serde(default)] values: BTreeMap<String, String>, #[serde(default)] seen_clear: Option<u64> },
    HideMessage,
    /// FEAT-2: run a macro. The App runs it (it needs timers and data); `base` is required when it changes slides.
    RunMacro { #[serde(rename = "macro")] macro_id: Uuid, #[serde(default)] seen_clear: Option<u64> },
    /// N18: take these layers down at once (a clear group, or F7 / F8), as one change.
    ClearLayers { layers: Vec<ClearLayer> },
    /// N29: show slide `slide` of `item_id` on the stage screen only; no `item_id` takes it down. The App keeps it
    /// (`App::set_stage_only`); the room never changes.
    StageOnly { #[serde(default)] item_id: Option<Uuid>, #[serde(default)] slide: usize },
    /// N15: play service item `item_id`'s video, picture or sound on its layer. The live slide stays where it is.
    PlayMedia { item_id: Uuid },
    /// N15: show the live item in saved theme `theme` until another item goes live.
    UseTheme { theme: Uuid },
    /// 1.3: how song item `item_id` follows the singers; no `mode` goes back to Settings → Auto-follow's. The App
    /// keeps it (`App::follow_command`); the screen doesn't change.
    FollowMode { item_id: Uuid, #[serde(default)] mode: Option<crate::follow::Mode> },
    /// 1.3: Hold ("don't move now") on or off for auto-follow. The App's follower keeps it.
    FollowHold { on: bool },
}

/// One layer a clear key or clear group takes down (N18). `Overlays` is the saved ones only (pictures, clocks,
/// signs); the message, the timers and the phone drawing are layers of their own. Text takes the drawing with it, as F2 does.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ClearLayer { Slide, Media, Audio, Logo, Overlays, Message, Timers, Drawing }

impl Command {
    fn relative(&self) -> bool { matches!(self, Command::Next | Command::Prev | Command::Jump { .. }) }
    /// Every intentional clear, whether or not it removes anything: it moves `Live::clear_epoch` on.
    fn clears(&self) -> bool {
        matches!(self, Command::ClearAll | Command::ClearSlide | Command::ClearMedia | Command::ClearAudio | Command::Logo | Command::ClearOverlays
            | Command::ClearLayers { .. })
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Envelope {
    pub id: String,
    #[serde(default)]
    pub base: Option<u64>,
    #[serde(flatten)]
    pub cmd: Command,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Outcome {
    pub ok: bool,
    pub version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub duplicate: bool,
}

/// What the service plan looks like right now.
pub trait Plan {
    fn order(&self) -> Vec<Uuid>;
    fn build(&self, item_id: Uuid) -> Option<LiveItem>;
    /// Saved overlay `id`: its library index (its drawing order) and a copy.
    fn saved_overlay(&self, _id: Uuid) -> Option<(u32, Overlay)> { None }
    /// The church logo, which a "church logo" picture overlay shows.
    fn logo(&self) -> Option<AssetId> { None }
    /// FEAT-2's timers and message templates.
    fn automation(&self) -> Option<&crate::automation::Automation> { None }
    /// A theme's text style, for `Command::UseTheme` (N15).
    fn theme_style(&self, _id: Uuid) -> Option<Theme> { None }
}

/// What a change touched, weakest first (FEAT-2 spec §5.2). Only `commit_change` bumps the version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Change {
    None,
    /// A new version with the room unchanged (a quiet adoption): in-flight relative commands go stale.
    Silent,
    /// Overlays only: slide timing (auto-advance) carries on.
    Overlay,
    /// Slides, media, logo or audio.
    Visual,
}

/// Who made a visual change: the operator (or phone, or a macro) or an automatic slide turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin { User, Auto }

/// One step of a batch: a command (with whether it puts something new on screen) or a runtime step the
/// caller applies (its index into the caller's list).
#[derive(Clone, Debug)]
pub enum BatchStep { Cmd { cmd: Command, show: bool }, Runtime(usize) }

/// A batch's outcome and the 0-based steps that failed (with why) or were skipped (shows after a clear).
#[derive(Clone, Debug)]
pub struct BatchOutcome { pub outcome: Outcome, pub failed: Vec<(usize, String)>, pub skipped_shows: Vec<usize> }

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlayState { Playing, Paused, Ended, Error }

/// Reports from the room output.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub enum RoomEvent {
    Committed { version: u64 },
    Failed { version: u64, reason: String },
    Media { inst: u64, state: PlayState, #[serde(default)] position: f64, #[serde(default)] duration: f64, #[serde(default)] error: Option<String> },
    /// An overlay could not be drawn (its picture is gone). The rest of the frame was shown.
    OverlayFailed { key: String, inst: u64, reason: String },
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PlayStatus {
    pub inst: u64,
    pub state: PlayState,
    pub position: f64,
    pub duration: f64,
}

/// Inside a batch the shows were already checked against the clear at its start.
fn with_seen_clear(cmd: &Command, epoch: u64) -> Command {
    let mut cmd = cmd.clone();
    match &mut cmd {
        Command::Overlay { seen_clear, .. } | Command::ShowTimer { seen_clear, .. } | Command::ShowMessage { seen_clear, .. } => *seen_clear = Some(epoch),
        _ => {}
    }
    cmd
}

pub struct Live {
    pub version: u64,
    pub frame: Frame,
    pub cursor: Option<Cursor>,
    pub item: Option<LiveItem>,
    pub committed_version: u64,
    pub media_status: Option<PlayStatus>,
    pub audio_status: Option<PlayStatus>,
    pub notice: Option<String>,
    /// The version `restore` made, once there has been one.
    pub restored_version: Option<u64>,
    /// Moves on with every clear (F1–F6), so a show sent before the sender saw a clear is refused.
    pub clear_epoch: u64,
    /// N13: moves on with every Clear audio and Clear everything, which stop the walk-in music too.
    pub audio_clears: u64,
    /// The live item no longer matches the library and the room would look different (FEAT-4 banner).
    pub changed: Option<LiveChange>,
    /// The version of the last visual change (FEAT-2): auto-advance re-arms only when this moves.
    pub visual_version: u64,
    /// The version of the last visual change someone made by hand (not an automatic slide turn).
    pub manual_version: u64,
    origin: Origin,
    /// The last quiet adoption (FEAT-4): the item and its old → new slide map, for the App to pass on to
    /// auto-advance. Taken by the App.
    pub adopted: Option<(Uuid, Vec<Option<usize>>)>,
    /// Inside `apply_batch`: the strongest change so far, committed once at the end.
    batch: Option<Change>,
    /// Lines drawn on the live slide from phones. Not part of the frame or the version (`drawing` module).
    pub drawing: std::sync::Arc<drawing::Drawing>,
    /// Settings → "Keep drawings when the slide changes" (the App keeps it in step with the settings).
    pub keep_drawing: bool,
    /// Counts every slide taken live (`go`), so the App runs a slide's actions once each time (N15).
    pub went: u64,
    /// N15 "Use theme": the live item's theme until another item goes live.
    theme_for: Option<(Uuid, Theme)>,
    next_inst: u64,
    history: VecDeque<Snapshot>,
    seen: VecDeque<(String, Outcome)>,
    /// A song's order changed from the Slides panel (`song_edit`): the item and where each part as it played went
    /// (old played place → new), so `reconcile` keeps the live part live. Used once.
    pub order_hint: Option<(Uuid, Vec<Option<usize>>)>,
}

const HISTORY: usize = 32;
const SEEN: usize = 1024;

impl Default for Live {
    fn default() -> Self { Self::new() }
}

impl Live {
    pub fn new() -> Self {
        let mut l = Self { version: 0, frame: Frame::default(), cursor: None, item: None, committed_version: 0, media_status: None,
            audio_status: None, notice: None, restored_version: None, clear_epoch: 0, audio_clears: 0, changed: None, visual_version: 0, manual_version: 0,
            origin: Origin::User, adopted: None, batch: None, drawing: Default::default(), keep_drawing: false, went: 0, theme_for: None, next_inst: 1, history: VecDeque::new(), seen: VecDeque::new(), order_hint: None };
        l.remember();
        l
    }

    fn remember(&mut self) {
        self.history.push_back(Snapshot { version: self.version, frame: self.frame.clone(), cursor: self.cursor.clone(), item: self.item.clone(), changed: self.changed.clone() });
        while self.history.len() > HISTORY {
            // Never drop the committed snapshot: it is what a failed request reverts to.
            let idx = self.history.iter().position(|s| s.version != self.committed_version).unwrap_or(0);
            self.history.remove(idx);
        }
    }

    fn bump(&mut self) { self.commit_change(Change::Visual) }

    /// The only place the version moves (spec §5.2 table). Inside a batch it only notes the change.
    pub(crate) fn commit_change(&mut self, change: Change) {
        if let Some(b) = self.batch.as_mut() { *b = (*b).max(change); return }
        if change == Change::None { return }
        self.version += 1;
        // Silent changes are remembered too, so the room can commit them.
        self.remember();
        if change == Change::Visual {
            self.visual_version = self.version;
            if self.origin == Origin::User { self.manual_version = self.version }
        }
    }

    /// Takes every drawn line away. No version bump. True when there were any.
    pub fn clear_drawing(&mut self) -> bool { drawing::clear_shared(&mut self.drawing) }

    fn inst(&mut self) -> u64 { let i = self.next_inst; self.next_inst += 1; i }

    /// The next playback or overlay instance number (saved with the live state, so a restart never reuses one).
    pub fn next_inst(&self) -> u64 { self.next_inst }

    /// Puts an overlay slot up (replacing one with its key). No version bump: the caller commits. False when an
    /// equal copy was already up.
    pub(crate) fn show_overlay(&mut self, slot: OverlaySlot) -> bool { slots::show(&mut self.frame.overlays, slot) }

    /// Takes the overlay slot with `key` down. No version bump. False when it was not up.
    pub(crate) fn hide_overlay(&mut self, key: &str) -> bool { slots::hide(&mut self.frame.overlays, key) }

    /// Keeps the overlay slots `f` accepts. No version bump. True when any was removed.
    pub(crate) fn retain_overlays(&mut self, f: impl Fn(&OverlaySlot) -> bool) -> bool { slots::retain(&mut self.frame.overlays, f) }

    fn committed_frame(&self) -> Option<&Frame> {
        self.history.iter().find(|s| s.version == self.committed_version).map(|s| &s.frame)
    }

    /// Is a report for this playback instance still authoritative?
    pub fn instance_valid(&self, inst: u64) -> bool {
        self.frame.instances().any(|i| i == inst) || self.committed_frame().is_some_and(|f| f.instances().any(|i| i == inst))
    }

    pub fn pending(&self) -> bool { self.committed_version < self.version }

    /// What is on air, as the room last confirmed it. `None` until something has been committed.
    pub fn committed(&self) -> Option<(Frame, Option<Cursor>, Option<LiveItem>)> {
        if self.committed_version == 0 { return None; }
        let s = self.history.iter().find(|s| s.version == self.committed_version)?;
        Some((s.frame.clone(), s.cursor.clone(), s.item.clone()))
    }

    /// Puts a saved frame back after a crash, as a new version for the room to commit.
    /// `with_media` false leaves out everything that plays: this state may be what crashed.
    pub fn restore(&mut self, s: &LiveState, with_media: bool) {
        if with_media { crate::recovery::panic::test_crash_point("restore-media"); }
        self.frame = s.frame.clone();
        self.cursor = s.cursor.clone();
        self.item = s.item.clone();
        self.next_inst = s.frame.instances().chain(s.frame.overlays.iter().map(|o| o.inst)).max().map_or(self.next_inst, |i| self.next_inst.max(i + 1));
        self.next_inst = self.next_inst.max(s.next_inst);
        self.clear_epoch = self.clear_epoch.max(s.clear_epoch);
        if !with_media {
            if self.frame.media.as_ref().is_some_and(|m| matches!(m.v, MediaLayer::Video { .. })) { self.frame.media = None; }
            self.frame.audio = None;
            if self.frame.background.as_ref().is_some_and(|b| b.v.moving()) { self.frame.background = None; }
        }
        self.bump();
        self.restored_version = Some(self.version);
    }

    pub fn apply(&mut self, env: Envelope, plan: &dyn Plan) -> Outcome {
        if let Some((_, o)) = self.seen.iter().find(|(id, _)| *id == env.id) {
            return Outcome { duplicate: true, ..o.clone() };
        }
        let result = self.run(&env, plan);
        let outcome = match result {
            Ok(()) => Outcome { ok: true, version: self.version, error: None, duplicate: false },
            Err(e) => Outcome { ok: false, version: self.version, error: Some(e), duplicate: false },
        };
        self.seen.push_back((env.id, outcome.clone()));
        if self.seen.len() > SEEN { self.seen.pop_front(); }
        outcome
    }

    /// The outcome of command `id` if it was already applied (`pplive` answers Next itself and keeps this list too).
    pub(crate) fn seen_outcome(&self, id: &str) -> Option<Outcome> {
        self.seen.iter().find(|(seen, _)| seen == id).map(|(_, o)| Outcome { duplicate: true, ..o.clone() })
    }

    /// Remembers command `id` as applied elsewhere, so a second delivery is a duplicate.
    pub(crate) fn note_seen(&mut self, id: String, outcome: Outcome) {
        self.seen.push_back((id, outcome));
        if self.seen.len() > SEEN { self.seen.pop_front(); }
    }

    /// Runs several steps as one change (FEAT-2 macros): one version bump, one history snapshot. Each step runs
    /// against its own checkpoint, so a failed step leaves nothing behind and the later steps still run. Show
    /// steps are skipped when `seen_clear` is older than the clears at the start (the batch's own clears don't
    /// count). `base`: checked once when given. `runtime` applies the caller's runtime steps.
    pub fn apply_batch(&mut self, id: &str, base: Option<u64>, seen_clear: Option<u64>, steps: &[BatchStep], plan: &dyn Plan,
        runtime: &mut dyn FnMut(usize) -> Result<(), String>) -> BatchOutcome {
        if let Some((_, o)) = self.seen.iter().find(|(seen, _)| seen == id) {
            return BatchOutcome { outcome: Outcome { duplicate: true, ..o.clone() }, failed: vec![], skipped_shows: vec![] };
        }
        let done = |l: &mut Live, outcome: Outcome, failed, skipped_shows| {
            l.seen.push_back((id.to_string(), outcome.clone()));
            if l.seen.len() > SEEN { l.seen.pop_front(); }
            BatchOutcome { outcome, failed, skipped_shows }
        };
        if base.is_some_and(|b| b != self.version) {
            let o = Outcome { ok: false, version: self.version, error: Some("stale: the screen changed before this arrived".into()), duplicate: false };
            return done(self, o, vec![], vec![]);
        }
        let epoch = self.clear_epoch;
        let cleared = seen_clear.is_none_or(|s| s < epoch);
        let (mut failed, mut skipped) = (vec![], vec![]);
        self.batch = Some(Change::None);
        for (i, step) in steps.iter().enumerate() {
            let r = match step {
                BatchStep::Cmd { show: true, .. } if cleared => { skipped.push(i); continue }
                BatchStep::Cmd { cmd, .. } => {
                    let saved = (self.frame.clone(), self.cursor.clone(), self.item.clone(), self.next_inst, self.notice.clone(), self.changed.clone(), self.batch);
                    let r = self.exec(&with_seen_clear(cmd, self.clear_epoch), plan);
                    if r.is_err() { (self.frame, self.cursor, self.item, self.next_inst, self.notice, self.changed, self.batch) = saved }
                    r
                }
                BatchStep::Runtime(n) => runtime(*n),
            };
            if let Err(e) = r { failed.push((i, e)) }
        }
        let change = self.batch.take().unwrap_or(Change::None);
        self.commit_change(change);
        let o = Outcome { ok: true, version: self.version, error: None, duplicate: false };
        done(self, o, failed, skipped)
    }

    /// An automatic slide turn (FEAT-2 auto-advance) to `slide` of the live item. It is a visual change that
    /// nobody made by hand, so it leaves `manual_version` alone.
    pub fn auto_goto(&mut self, slide: usize) -> bool {
        let (Some(item), Some(cursor)) = (self.item.clone(), self.cursor.clone()) else { return false };
        if slide >= item.slides.len() { return false }
        self.origin = Origin::Auto;
        self.go(item, cursor.item_index, slide);
        self.origin = Origin::User;
        true
    }

    fn run(&mut self, env: &Envelope, plan: &dyn Plan) -> Result<(), String> {
        let check_base = env.cmd.relative() || matches!(env.cmd, Command::Show { .. });
        if check_base {
            match env.base {
                Some(b) if b != self.version => return Err("stale: the screen changed before this arrived".into()),
                None if env.cmd.relative() => return Err("stale: missing base version".into()),
                _ => {}
            }
        }
        self.exec(&env.cmd, plan)
    }

    /// One command, its base already checked.
    fn exec(&mut self, cmd: &Command, plan: &dyn Plan) -> Result<(), String> {
        if cmd.clears() { self.clear_epoch += 1 }
        match cmd {
            Command::Next => self.step(plan, true),
            Command::Prev => self.step(plan, false),
            Command::Show { item_id, slide } => {
                let order = plan.order();
                let index = order.iter().position(|i| i == item_id).ok_or_else(|| crate::tr!("That item is not in the service."))?;
                let item = plan.build(*item_id).ok_or_else(|| crate::tr!("That item has nothing to show."))?;
                if item.slides.is_empty() { return Err(if item.kind == "blank" { crate::planning::strings::blank_choose_first() } else { crate::tr!("That item has no slides.") }); }
                let slide = (*slide).min(item.slides.len() - 1);
                self.go(item, index, slide);
                Ok(())
            }
            Command::ClearAll => { self.frame = Frame::default(); self.clear_drawing(); self.audio_clears += 1; self.bump(); Ok(()) }
            Command::ClearSlide => { self.frame.slide = None; self.clear_drawing(); self.bump(); Ok(()) }
            Command::ClearMedia => { self.frame.background = None; self.frame.media = None; self.bump(); Ok(()) }
            Command::ClearAudio => { self.frame.audio = None; self.audio_clears += 1; self.bump(); Ok(()) }
            Command::Logo => {
                self.frame.slide = None; self.frame.background = None; self.frame.media = None; self.frame.logo = true;
                self.clear_drawing();
                // Pictures, clocks and signs stay over the logo (FEAT-1 spec §5.2).
                self.retain_overlays(|s| s.source.survives_logo());
                self.bump();
                Ok(())
            }
            Command::Overlay { id, on: true, seen_clear } => {
                slots::check_seen_clear(*seen_clear, self.clear_epoch)?;
                let (z, o) = plan.saved_overlay(*id).ok_or(theme_strings::overlay_gone())?;
                let inst = self.inst();
                let mut slot = slots::saved_slot(*id, std::slice::from_ref(&o), plan.logo().as_ref(), inst)?;
                slot.z = z;
                if slots::turn_on(&mut self.frame.overlays, slot)? { self.commit_change(Change::Overlay) }
                Ok(())
            }
            Command::Overlay { id, on: false, .. } => {
                if self.hide_overlay(&overlay::saved_key(*id)) { self.commit_change(Change::Overlay) }
                Ok(())
            }
            Command::ClearOverlays => {
                if self.retain_overlays(|_| false) { self.commit_change(Change::Overlay) }
                Ok(())
            }
            Command::MediaPause { inst, paused } => {
                match &mut self.frame.media {
                    Some(Inst { inst: i, v: MediaLayer::Video { paused: p, .. } }) if *i == *inst => { *p = *paused; }
                    _ => return Err(crate::tr!("That video is no longer playing.")),
                }
                self.bump();
                Ok(())
            }
            Command::AudioPause { inst, paused } => {
                match &mut self.frame.audio {
                    Some(a) if a.inst == *inst => a.v.paused = *paused,
                    _ => return Err(crate::tr!("That audio is no longer playing.")),
                }
                self.bump();
                Ok(())
            }
            Command::Seek { inst, to } => {
                let video = self.frame.media.as_ref().is_some_and(|m| m.inst == *inst && matches!(m.v, MediaLayer::Video { .. }));
                let audio = self.frame.audio.as_ref().is_some_and(|a| a.inst == *inst);
                if !video && !audio { return Err(crate::tr!("That video or audio is no longer playing.")) }
                let n = self.frame.seek.map_or(0, |s| s.n) + 1;
                self.frame.seek = Some(Seek { inst: *inst, n, to: *to });
                self.bump();
                Ok(())
            }
            Command::Jump { item_id, kind, number, occurrence } => self.jump(*item_id, JumpTarget { kind: *kind, number: *number, occurrence: *occurrence }),
            Command::Refresh { item_id } => self.refresh(plan, *item_id),
            Command::ShowTimer { timer, seen_clear } => {
                slots::check_seen_clear(*seen_clear, self.clear_epoch)?;
                let timers = plan.automation().map(|a| a.timers.as_slice()).unwrap_or_default();
                let (index, def) = timers.iter().enumerate().find(|(_, t)| t.id == *timer).ok_or_else(crate::automation::strings::timer_gone)?;
                if def.kind.known().is_none() { return Err(crate::automation::strings::timer_needs_newer()) }
                let key = crate::automation::slots::timer_key(*timer);
                crate::automation::slots::room_for_timer(&self.frame.overlays, &key)?;
                let inst = self.inst();
                if self.show_overlay(crate::automation::slots::timer_slot(def, index, inst)) { self.commit_change(Change::Overlay) }
                Ok(())
            }
            Command::HideTimer { timer } => {
                if self.hide_overlay(&crate::automation::slots::timer_key(*timer)) { self.commit_change(Change::Overlay) }
                Ok(())
            }
            Command::ShowMessage { template, values, seen_clear } => {
                slots::check_seen_clear(*seen_clear, self.clear_epoch)?;
                let t = plan.automation().and_then(|a| a.messages.iter().find(|t| t.id == *template)).ok_or_else(crate::automation::strings::message_gone)?;
                let text = crate::automation::messages::fill(&t.text, values)?;
                let inst = self.inst();
                self.show_overlay(crate::automation::slots::message_slot(t, Uuid::new_v4(), text, inst));
                self.commit_change(Change::Overlay);
                Ok(())
            }
            Command::HideMessage => {
                if self.hide_overlay(crate::automation::slots::MESSAGE_KEY) { self.commit_change(Change::Overlay) }
                Ok(())
            }
            // The App runs macros before the engine sees them (`App::command`).
            Command::RunMacro { .. } => Err(crate::automation::strings::macro_needs_app()),
            Command::ClearLayers { layers } => { self.clear_layers(layers); Ok(()) }
            // The App keeps the stage-only slide (`App::command`); the engine never shows it.
            Command::StageOnly { .. } => Ok(()),
            Command::PlayMedia { item_id } => self.play_media(plan, *item_id),
            Command::UseTheme { theme } => self.use_theme(plan, *theme),
            // The App keeps auto-follow's mode and Hold (`App::command`); the engine never sees them.
            Command::FollowMode { .. } | Command::FollowHold { .. } => Ok(()),
        }
    }

    /// N15: an item's video or picture on the media layer, or its sound on the audio layer, over or under what is up.
    fn play_media(&mut self, plan: &dyn Plan, item_id: Uuid) -> Result<(), String> {
        if !plan.order().contains(&item_id) { return Err(crate::tr!("That item is not in the service.")) }
        let item = plan.build(item_id).ok_or_else(|| crate::tr!("That item has nothing to show."))?;
        let inst = self.inst();
        match item.slides.first().map(|s| s.content.clone()) {
            Some(SlideContent::Video { asset, end, .. }) => self.frame.media = Some(Inst { inst, v: MediaLayer::Video { asset, end, paused: false } }),
            Some(SlideContent::Still { asset }) => self.frame.media = Some(Inst { inst, v: MediaLayer::Still { asset } }),
            Some(SlideContent::Audio { asset, looped }) => {
                self.frame.audio = Some(Inst { inst, v: AudioLayer { asset, title: item.title.clone(), looped, paused: false } })
            }
            _ => return Err(crate::tr!("That item isn't a video, picture or sound.")),
        }
        self.bump();
        Ok(())
    }

    /// N15: the live item's words in theme `id` from now on (the next item has its own theme again).
    fn use_theme(&mut self, plan: &dyn Plan, id: Uuid) -> Result<(), String> {
        let style = plan.theme_style(id).ok_or_else(|| crate::tr!("That theme was deleted."))?;
        let item = self.item.as_mut().ok_or_else(|| crate::tr!("Nothing is live yet."))?;
        item.theme = style.clone();
        self.theme_for = Some((item.item_id, style.clone()));
        if let Some(SlideLayer::Text { theme, .. }) = &mut self.frame.slide { *theme = Some(style) }
        self.bump();
        Ok(())
    }

    /// N18: takes `layers` down. A screen layer always makes a new version, like F2; overlays only when one went.
    fn clear_layers(&mut self, layers: &[ClearLayer]) {
        use crate::automation::slots::{MESSAGE_KEY, TIMER_PREFIX};
        let (mut visual, mut overlay) = (false, false);
        for layer in layers {
            match layer {
                ClearLayer::Slide => { self.frame.slide = None; self.clear_drawing(); visual = true }
                ClearLayer::Media => { self.frame.background = None; self.frame.media = None; visual = true }
                ClearLayer::Audio => { self.frame.audio = None; self.audio_clears += 1; visual = true }
                ClearLayer::Logo => { self.frame.logo = false; visual = true }
                ClearLayer::Overlays => overlay |= self.retain_overlays(|s| s.key == MESSAGE_KEY || s.key.starts_with(TIMER_PREFIX)),
                ClearLayer::Message => overlay |= self.hide_overlay(MESSAGE_KEY),
                ClearLayer::Timers => overlay |= self.retain_overlays(|s| !s.key.starts_with(TIMER_PREFIX)),
                ClearLayer::Drawing => { self.clear_drawing(); visual = true }
            }
        }
        if visual { self.bump() } else if overlay { self.commit_change(Change::Overlay) }
    }

    fn step(&mut self, plan: &dyn Plan, forward: bool) -> Result<(), String> {
        let order = plan.order();
        let Some(cursor) = self.cursor.clone() else {
            if !forward { return Err(crate::tr!("Nothing is live yet.")); }
            return self.take_index(plan, &order, 0, true);
        };
        let len = self.item.as_ref().map_or(0, |i| i.slides.len());
        if forward && cursor.slide + 1 < len {
            let item = self.item.clone().unwrap();
            self.go(item, cursor.item_index, cursor.slide + 1);
            return Ok(());
        }
        if !forward && cursor.slide > 0 && len > 0 {
            let item = self.item.clone().unwrap();
            self.go(item, cursor.item_index, (cursor.slide - 1).min(len - 1));
            return Ok(());
        }
        // Cross into the neighbouring service item.
        let target = match order.iter().position(|i| *i == cursor.item_id) {
            Some(p) => if forward { p + 1 } else { p.checked_sub(1).ok_or_else(|| crate::tr!("Start of service"))? },
            // Live item was removed: the item now at its former position comes next.
            None => if forward { cursor.item_index } else { cursor.item_index.checked_sub(1).ok_or_else(|| crate::tr!("Start of service"))? },
        };
        if target >= order.len() { return Err(crate::tr!("End of service")); }
        self.take_index(plan, &order, target, forward)
    }

    /// Take the item at `index` live: first slide going forward, last slide going back.
    /// Items with no slides are skipped.
    fn take_index(&mut self, plan: &dyn Plan, order: &[Uuid], mut index: usize, forward: bool) -> Result<(), String> {
        loop {
            let id = *order.get(index).ok_or_else(|| if forward { crate::tr!("End of service") } else { crate::tr!("Start of service") })?;
            if let Some(item) = plan.build(id).filter(|i| !i.slides.is_empty()) {
                let slide = if forward { 0 } else { item.slides.len() - 1 };
                self.go(item, index, slide);
                return Ok(());
            }
            if forward { index += 1 } else { index = index.checked_sub(1).ok_or_else(|| crate::tr!("Start of service"))? }
        }
    }

    /// Apply one slide's layer set. This is the item-transition contract (review R2-03):
    ///
    /// | Incoming      | Slide layer | Background         | Media (video+its audio / still) | Logo | Audio item |
    /// |---------------|-------------|--------------------|---------------------------------|------|------------|
    /// | Text          | text        | item's (kept if same) | **stopped/removed**          | off  | continues  |
    /// | Deck image    | image       | removed            | **stopped/removed**             | off  | continues  |
    /// | Video         | removed     | unchanged (hidden) | new instance, plays from start  | off  | continues  |
    /// | Still image   | removed     | unchanged (hidden) | still image (replaces video)    | off  | continues  |
    /// | Audio item    | unchanged   | unchanged          | unchanged                       | unchanged | new instance from start |
    fn go(&mut self, mut item: LiveItem, index: usize, slide: usize) {
        self.went += 1;
        // "Use theme" lasts while the item stays live, even when one of its slides is clicked (a fresh build).
        match &self.theme_for {
            Some((id, style)) if *id == item.item_id => item.theme = style.clone(),
            Some(_) => self.theme_for = None,
            None => {}
        }
        // A drawing belongs to the slide it was drawn on (unless the church keeps them).
        let moved = self.cursor.as_ref().is_none_or(|c| (c.item_id, c.slide) != (item.item_id, slide));
        if moved && !self.keep_drawing { self.clear_drawing(); }
        let content = item.slides[slide].content.clone();
        match content {
            SlideContent::Text { text, footer, reference, size, parallel } => {
                let scripture = item.kind == "scripture";
                let passage = passage(&item.kind, &item.slides);
                let theme = Some(item.theme.clone());
                self.frame.slide = Some(SlideLayer::Text { text, footer, reference, scripture, size, passage, parallel, theme });
                self.set_background(Some(item.background.clone()));
                self.frame.media = None;
                self.frame.logo = false;
            }
            SlideContent::Design { mut design, step, play, .. } => {
                let from = self.cursor.as_ref().filter(|c| c.item_id == item.item_id).map(|c| c.slide);
                // Next from the slide before it: only the new click plays.
                let build = step > 0 && slide > 0 && from == Some(slide - 1) && matches!(self.frame.slide, Some(SlideLayer::Design { .. }));
                // Back one click on the same slide: straight to how it was, no transition.
                // `from` is the cursor in the copy that is live now; the copy just built may be shorter.
                let back = from == Some(slide + 1)
                    && item.slides.get(slide + 1).is_some_and(|s| matches!(&s.content, SlideContent::Design { step: s, .. } if *s == step + 1));
                if back { design.transition.kind = crate::design::TransitionKind::Cut }
                let play = play.filter(|_| build || step == 0 && !back);
                self.frame.slide = Some(SlideLayer::Design { design, build, play });
                self.set_background(Some(item.background.clone()));
                self.frame.media = None;
                self.frame.logo = false;
            }
            SlideContent::Image { asset, transition } => {
                self.frame.slide = Some(SlideLayer::Image { asset, transition });
                self.set_background(None);
                self.frame.media = None;
                self.frame.logo = false;
            }
            SlideContent::Video { asset, autoplay, end } => {
                let inst = self.inst();
                self.frame.media = Some(Inst { inst, v: MediaLayer::Video { asset, end, paused: !autoplay } });
                self.frame.slide = None;
                self.frame.logo = false;
            }
            SlideContent::Still { asset } => {
                let inst = self.inst();
                self.frame.media = Some(Inst { inst, v: MediaLayer::Still { asset } });
                self.frame.slide = None;
                self.frame.logo = false;
            }
            SlideContent::Audio { asset, looped } => {
                let inst = self.inst();
                self.frame.audio = Some(Inst { inst, v: AudioLayer { asset, title: item.title.clone(), looped, paused: false } });
            }
        }
        self.cursor = Some(Cursor { item_id: item.item_id, item_index: index, slide });
        // A new version of the item (a click on a slide) shows the library as it is now.
        if self.item.as_ref() != Some(&item) { self.changed = None }
        self.item = Some(item);
        self.notice = None;
        self.bump();
    }

    fn set_background(&mut self, bg: Option<Background>) {
        match bg {
            None => self.frame.background = None,
            Some(bg) => {
                if self.frame.background.as_ref().is_some_and(|b| b.v == bg) { return; } // keep playing
                let inst = self.inst();
                self.frame.background = Some(Inst { inst, v: bg });
            }
        }
    }

    /// Handle a report from the room. Returns true when the live state changed.
    /// The report only says how far a video or audio has played: the same player, in the same state as its last
    /// report (a player says so twice a second). Ask before `report` takes it.
    pub fn only_position(&self, ev: &RoomEvent) -> bool {
        let RoomEvent::Media { inst, state, error: None, .. } = ev else { return false };
        if matches!(state, PlayState::Ended | PlayState::Error) { return false }
        [&self.media_status, &self.audio_status].into_iter().flatten().any(|s| s.inst == *inst && s.state == *state)
    }

    pub fn report(&mut self, ev: RoomEvent) -> bool {
        match ev {
            RoomEvent::Committed { version } => {
                if version > self.committed_version && version <= self.version && self.history.iter().any(|s| s.version == version) {
                    self.committed_version = version;
                    return true;
                }
                false
            }
            RoomEvent::Failed { version, reason } => {
                // Only the newest request can fail meaningfully; older ones were already superseded.
                if version != self.version || !self.pending() { return false; }
                let Some(snap) = self.history.iter().find(|s| s.version == self.committed_version).cloned() else { return false };
                // Back to what is actually on air. Its instances are unchanged, so the room keeps playing them.
                // Overlays are not part of that: they stay as they were asked for.
                let overlays = std::mem::take(&mut self.frame.overlays);
                self.frame = snap.frame;
                self.frame.overlays = overlays;
                self.cursor = snap.cursor;
                self.item = snap.item;
                self.changed = snap.changed;
                self.notice = Some(crate::tr!("Could not show that: {reason}", reason = reason));
                self.bump();
                true
            }
            RoomEvent::OverlayFailed { key, inst, reason } => {
                // Only a slot that is still up with that instance; a later copy or one taken down is left alone.
                let Some(name) = slots::failed(&mut self.frame.overlays, &key, inst) else { return false };
                self.notice = Some(theme_strings::overlay_failed(&name, &reason));
                self.commit_change(Change::Overlay);
                true
            }
            RoomEvent::Media { inst, state, position, duration, error } => {
                if !self.instance_valid(inst) { return false; }
                let status = PlayStatus { inst, state: state.clone(), position, duration };
                let is_media = self.media_inst_any(inst, |f| f.media.as_ref().map(|m| m.inst));
                let is_audio = self.media_inst_any(inst, |f| f.audio.as_ref().map(|a| a.inst));
                if is_media { self.media_status = Some(status) } else if is_audio { self.audio_status = Some(status) }
                match state {
                    PlayState::Ended => {
                        let stop = self.media_end_action(inst).is_some_and(|e| e == EndAction::Stop)
                            || self.audio_layer(inst).is_some_and(|a| !a.looped);
                        if stop { return self.retire(inst) || true; }
                        true
                    }
                    PlayState::Error => {
                        self.notice = Some(crate::tr!("Playback stopped: {reason}", reason = error.unwrap_or_else(|| crate::tr!("the file could not be played"))));
                        self.retire(inst);
                        true
                    }
                    _ => true,
                }
            }
        }
    }

    fn media_inst_any(&self, inst: u64, f: impl Fn(&Frame) -> Option<u64>) -> bool {
        f(&self.frame) == Some(inst) || self.committed_frame().and_then(&f) == Some(inst)
    }

    fn media_end_action(&self, inst: u64) -> Option<EndAction> {
        [Some(&self.frame), self.committed_frame()].into_iter().flatten().find_map(|f| match &f.media {
            Some(Inst { inst: i, v: MediaLayer::Video { end, .. } }) if *i == inst => Some(*end),
            _ => None,
        })
    }

    /// The live video `inst` goes to the next slide when it ends (N24).
    pub fn ends_with_next(&self, inst: u64) -> bool {
        matches!(&self.frame.media, Some(Inst { inst: i, v: MediaLayer::Video { end: EndAction::Next, .. } }) if *i == inst)
    }

    /// Takes down a video that ended with nothing to go on to.
    pub(crate) fn retire_ended(&mut self, inst: u64) -> bool { self.retire(inst) }

    /// Every frame this engine may still show or go back to: the live one and those it remembers.
    pub fn frames(&self) -> impl Iterator<Item = &Frame> { std::iter::once(&self.frame).chain(self.history.iter().map(|s| &s.frame)) }

    /// The items those frames came from, for the pictures their other slides need.
    pub fn items(&self) -> impl Iterator<Item = &LiveItem> { self.item.iter().chain(self.history.iter().filter_map(|s| s.item.as_ref())) }

    fn audio_layer(&self, inst: u64) -> Option<AudioLayer> {
        [Some(&self.frame), self.committed_frame()].into_iter().flatten().find_map(|f| f.audio.as_ref().filter(|a| a.inst == inst).map(|a| a.v.clone()))
    }

    /// Remove a finished/failed playback everywhere, including the snapshots a
    /// failed request could revert to, so it can never come back.
    fn retire(&mut self, inst: u64) -> bool {
        for s in &mut self.history { s.frame.retire(inst); }
        if self.media_status.as_ref().is_some_and(|s| s.inst == inst) { self.media_status = None; }
        if self.audio_status.as_ref().is_some_and(|s| s.inst == inst) { self.audio_status = None; }
        if self.frame.retire(inst) { self.bump(); true } else { false }
    }

    /// The slide that Next would show (for the stage display and phone).
    pub fn next_preview(&self, plan: &dyn Plan) -> Option<(String, Slide)> {
        self.next_slot(plan).map(|(item, n)| (item.title.clone(), item.slides[n].clone()))
    }

    /// The item and the index of the slide `next_preview` names (N10: the stage draws it as a picture).
    pub fn next_slot(&self, plan: &dyn Plan) -> Option<(std::borrow::Cow<'_, LiveItem>, usize)> {
        use std::borrow::Cow;
        let Some(c) = &self.cursor else {
            let id = *plan.order().first()?;
            let item = plan.build(id)?;
            return (!item.slides.is_empty()).then_some((Cow::Owned(item), 0));
        };
        let item = self.item.as_ref()?;
        if c.slide + 1 < item.slides.len() { return Some((Cow::Borrowed(item), c.slide + 1)); }
        let order = plan.order();
        let mut idx = match order.iter().position(|i| *i == c.item_id) { Some(p) => p + 1, None => c.item_index };
        while let Some(id) = order.get(idx) {
            if let Some(it) = plan.build(*id) { if !it.slides.is_empty() { return Some((Cow::Owned(it), 0)); } }
            idx += 1;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recovery::state;
    use std::collections::HashMap;

    struct TestPlan { order: Vec<Uuid>, items: HashMap<Uuid, LiveItem> }

    impl Plan for TestPlan {
        fn order(&self) -> Vec<Uuid> { self.order.clone() }
        fn build(&self, id: Uuid) -> Option<LiveItem> { self.items.get(&id).cloned() }
    }

    fn text(t: &str) -> Slide { Slide::new(t, SlideContent::Text { text: t.into(), footer: None, reference: None, size: 0, parallel: None }) }
    fn black() -> Background { Background::Color { color: "#000".into() } }

    fn plan(items: Vec<(&str, Vec<Slide>)>) -> TestPlan {
        let mut p = TestPlan { order: vec![], items: HashMap::new() };
        for (title, slides) in items {
            let id = Uuid::new_v4();
            p.order.push(id);
            p.items.insert(id, LiveItem { item_id: id, title: title.into(), kind: "song".into(), slides, background: black(), theme: Theme::default() });
        }
        p
    }

    fn video(asset: &str, end: EndAction) -> Slide { Slide::new(asset, SlideContent::Video { asset: asset.into(), autoplay: true, end }) }
    fn audio(asset: &str) -> Slide { Slide::new(asset, SlideContent::Audio { asset: asset.into(), looped: false }) }
    fn deck(asset: &str) -> Slide { Slide::new(asset, SlideContent::Image { asset: asset.into(), transition: None }) }

    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    fn env(cmd: Command, base: Option<u64>) -> Envelope {
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Envelope { id: format!("c{n}"), base, cmd }
    }
    fn next(l: &mut Live, p: &TestPlan) -> Outcome { let b = l.version; l.apply(env(Command::Next, Some(b)), p) }
    fn shown(l: &Live) -> Option<String> { match &l.frame.slide { Some(SlideLayer::Text { text, .. }) => Some(text.clone()), _ => None } }
    fn commit(l: &mut Live) { let v = l.version; l.report(RoomEvent::Committed { version: v }); }

    fn saved(l: &Live) -> LiveState { state::snapshot(l, vec![], 1).expect("something was committed") }

    #[test]
    fn next_through_builds_marks_each_as_a_build_but_a_jump_is_not() {
        use crate::design::{Anim, AnimKind, Design, Effect, Obj, ObjKind, PctRect, Start, TextStyle, TransitionKind};
        let t = || Obj::new(ObjKind::text("x", TextStyle::default()), PctRect::new(0.0, 0.0, 10.0, 10.0));
        let objects = vec![t(), t(), t()];
        let anims = objects[1..].iter().map(|o| Anim::new(o.id, AnimKind::Entrance, Effect::Fade, Start::OnClick)).collect();
        let slides = crate::slides::design_slides(&[Design { objects, anims, ..Default::default() }]);
        let p = plan(vec![("Sermon", slides)]);
        let mut l = Live::new();
        let build = |l: &Live| match &l.frame.slide { Some(SlideLayer::Design { build, design, play }) => Some((*build, design.objects.len(), play.is_some())), _ => None };
        next(&mut l, &p);
        assert_eq!(build(&l), Some((false, 1, false)));
        next(&mut l, &p);
        assert_eq!(build(&l), Some((true, 2, true)), "the click's animation goes to the room");
        next(&mut l, &p);
        assert_eq!(build(&l), Some((true, 3, true)));
        let kind = |l: &Live| match &l.frame.slide { Some(SlideLayer::Design { design, .. }) => Some(design.transition.kind), _ => None };
        l.apply(env(Command::Show { item_id: p.order[0], slide: 0 }, None), &p);
        assert_eq!((build(&l), kind(&l)), (Some((false, 1, false)), Some(TransitionKind::Fade)), "a jump is a whole new slide");
        next(&mut l, &p);
        l.apply(env(Command::Show { item_id: p.order[0], slide: 0 }, None), &p);
        assert_eq!((build(&l), kind(&l)), (Some((false, 1, false)), Some(TransitionKind::Cut)), "back one click cuts to how it was");
        assert!(l.frame.background.is_some());
    }

    /// B1: the library copy of a designed item got shorter while it was live.
    #[test]
    fn a_shortened_designed_item_does_not_crash_update_screen_or_a_click() {
        use crate::design::{Anim, AnimKind, Design, Effect, Obj, ObjKind, PctRect, Start, TextStyle};
        let t = || Obj::new(ObjKind::text("x", TextStyle::default()), PctRect::new(0.0, 0.0, 10.0, 10.0));
        // One slide with one "on click" animation is two slides to the engine; without it, one.
        let (a, b) = (t(), t());
        let anims = vec![Anim::new(b.id, AnimKind::Entrance, Effect::Fade, Start::OnClick)];
        let two = crate::slides::design_slides(&[Design { objects: vec![a.clone(), b.clone()], anims, ..Default::default() }]);
        let one = crate::slides::design_slides(&[Design { objects: vec![a, b], ..Default::default() }]);
        assert_eq!((two.len(), one.len()), (2, 1));
        for refresh in [true, false] {
            let mut p = plan(vec![("Sermon", two.clone())]);
            let id = p.order[0];
            let mut l = Live::new();
            assert!(l.apply(env(Command::Show { item_id: id, slide: 1 }, None), &p).ok);
            p.items.get_mut(&id).unwrap().slides = one.clone();
            l.reconcile(&p);
            let cmd = if refresh { Command::Refresh { item_id: id } } else { Command::Show { item_id: id, slide: 0 } };
            assert!(l.apply(env(cmd, None), &p).ok, "refresh: {refresh}");
            assert_eq!(l.cursor.as_ref().map(|c| c.slide), Some(0));
        }
        // Three slides live on the third, cut to two, then the new last slide is clicked.
        let d = || Design { objects: vec![t()], ..Default::default() };
        let three = crate::slides::design_slides(&[d(), d(), d()]);
        let mut p = plan(vec![("Sermon", three.clone())]);
        let id = p.order[0];
        let mut l = Live::new();
        assert!(l.apply(env(Command::Show { item_id: id, slide: 2 }, None), &p).ok);
        p.items.get_mut(&id).unwrap().slides = three[..2].to_vec();
        l.reconcile(&p);
        assert!(l.apply(env(Command::Show { item_id: id, slide: 1 }, None), &p).ok);
        assert_eq!(l.cursor.as_ref().map(|c| c.slide), Some(1));
    }

    #[test]
    fn committed_is_none_until_something_commits() {
        let p = plan(vec![("A", vec![text("a1")])]);
        let mut l = Live::new();
        next(&mut l, &p);
        assert_eq!(l.committed(), None);
        commit(&mut l);
        assert!(l.committed().is_some());
    }

    #[test]
    fn snapshot_uses_committed_frame_not_requested() {
        let p = plan(vec![("A", vec![text("a1"), text("a2")])]);
        let mut l = Live::new();
        next(&mut l, &p);
        commit(&mut l);
        next(&mut l, &p); // requested, not yet on air
        let s = saved(&l);
        assert_eq!(s.frame.slide, Some(SlideLayer::Text { text: "a1".into(), footer: None, reference: None, scripture: false, size: 0, passage: vec![], parallel: None, theme: Some(Theme::default()) }));
        assert_eq!(s.cursor.unwrap().slide, 0);
        assert_eq!(s.v, 1);
    }

    #[test]
    fn snapshot_positions_are_only_for_instances_in_the_committed_frame() {
        let p = plan(vec![("V", vec![video("v.mp4", EndAction::Stop)]), ("A", vec![audio("a.mp3")])]);
        let mut l = Live::new();
        next(&mut l, &p);
        commit(&mut l);
        let inst = l.frame.media.as_ref().unwrap().inst;
        l.report(RoomEvent::Media { inst, state: PlayState::Playing, position: 12.5, duration: 60.0, error: None });
        l.audio_status = Some(PlayStatus { inst: 999, state: PlayState::Playing, position: 3.0, duration: 9.0 });
        assert_eq!(saved(&l).positions, [(inst, 12.5)]);
    }

    #[test]
    fn saved_state_survives_json() {
        let p = plan(vec![("V", vec![text("a1"), video("v.mp4", EndAction::Hold)])]);
        let mut l = Live::new();
        next(&mut l, &p);
        next(&mut l, &p);
        commit(&mut l);
        let s = saved(&l);
        assert_eq!(serde_json::from_slice::<LiveState>(&serde_json::to_vec(&s).unwrap()).unwrap(), s);
    }

    #[test]
    fn restore_reissues_instances_above_saved() {
        let p = plan(vec![("V", vec![video("a.mp4", EndAction::Stop), video("b.mp4", EndAction::Stop)])]);
        let mut old = Live::new();
        next(&mut old, &p);
        next(&mut old, &p);
        commit(&mut old);
        let s = saved(&old);
        let high = s.frame.instances().max().unwrap();

        let mut l = Live::new();
        l.restore(&s, true);
        assert_eq!(l.frame, s.frame);
        assert_eq!(l.cursor, s.cursor);
        assert_eq!(l.item, s.item);
        assert_eq!((l.version, l.committed_version), (1, 0), "the room has to commit it");
        l.apply(env(Command::Show { item_id: p.order[0], slide: 0 }, None), &p);
        assert!(l.frame.media.as_ref().unwrap().inst > high);
    }

    #[test]
    fn restore_commits_like_any_other_version() {
        let p = plan(vec![("A", vec![text("a1"), text("a2")])]);
        let mut old = Live::new();
        next(&mut old, &p);
        commit(&mut old);
        let mut l = Live::new();
        l.restore(&saved(&old), true);
        assert!(l.pending());
        commit(&mut l);
        assert_eq!(l.committed().unwrap().0.slide, old.frame.slide);
        next(&mut l, &p); // the service goes on from the restored slide
        assert_eq!(shown(&l).as_deref(), Some("a2"));
    }

    fn with_layers(l: &mut Live) -> LiveState {
        let p = plan(vec![("A", vec![text("a1")])]);
        next(l, &p);
        commit(l);
        let mut s = saved(l);
        s.frame.background = Some(Inst { inst: 1, v: black() });
        s.frame.media = Some(Inst { inst: 2, v: MediaLayer::Video { asset: "v.mp4".into(), end: EndAction::Stop, paused: false } });
        s.frame.audio = Some(Inst { inst: 3, v: AudioLayer { asset: "a.mp3".into(), title: "A".into(), looped: false, paused: false } });
        s
    }

    #[test]
    fn restore_without_media_keeps_slide_and_colour_background() {
        let s = with_layers(&mut Live::new());
        let mut l = Live::new();
        l.restore(&s, false);
        assert_eq!(l.frame.media, None);
        assert_eq!(l.frame.audio, None);
        assert_eq!(l.frame.background, s.frame.background);
        assert_eq!(l.frame.slide, s.frame.slide);
    }

    #[test]
    fn restore_without_media_keeps_a_still_and_an_image_background() {
        let mut s = with_layers(&mut Live::new());
        s.frame.media = Some(Inst { inst: 2, v: MediaLayer::Still { asset: "p.png".into() } });
        s.frame.background = Some(Inst { inst: 1, v: Background::Image { asset: "b.png".into(), dim: 0 } });
        let mut l = Live::new();
        l.restore(&s, false);
        assert_eq!(l.frame.media, s.frame.media);
        assert_eq!(l.frame.background, s.frame.background);
    }

    #[test]
    fn restore_without_media_drops_a_video_background() {
        let mut s = with_layers(&mut Live::new());
        s.frame.background = Some(Inst { inst: 1, v: Background::Video { asset: "b.mp4".into(), dim: 0 } });
        let mut l = Live::new();
        l.restore(&s, false);
        assert_eq!(l.frame.background, None);
    }

    #[test]
    fn only_video_and_audio_count_as_playing() {
        assert!(!Frame::default().plays());
        let still = Frame { media: Some(Inst { inst: 1, v: MediaLayer::Still { asset: "s.png".into() } }), background: Some(Inst { inst: 2, v: Background::Image { asset: "b.png".into(), dim: 0 } }), ..Frame::default() };
        assert!(!still.plays());
        assert!(Frame { media: Some(Inst { inst: 1, v: MediaLayer::Video { asset: "v.mp4".into(), end: EndAction::Stop, paused: false } }), ..Frame::default() }.plays());
        assert!(Frame { background: Some(Inst { inst: 1, v: Background::Video { asset: "b.mp4".into(), dim: 0 } }), ..Frame::default() }.plays());
        assert!(Frame { audio: Some(Inst { inst: 1, v: AudioLayer { asset: "a.mp3".into(), title: "a".into(), looped: false, paused: false } }), ..Frame::default() }.plays());
    }

    #[test]
    fn restore_marks_its_version() {
        let mut l = Live::new();
        assert_eq!(l.restored_version, None);
        l.restore(&with_layers(&mut Live::new()), true);
        assert_eq!(l.restored_version, Some(l.version));
    }

    #[test]
    fn restore_with_media_keeps_every_layer() {
        let s = with_layers(&mut Live::new());
        let mut l = Live::new();
        l.restore(&s, true);
        assert_eq!(l.frame, s.frame);
    }

    #[test]
    fn next_walks_slides_and_crosses_items() {
        let p = plan(vec![("A", vec![text("a1"), text("a2")]), ("B", vec![text("b1")])]);
        let mut l = Live::new();
        next(&mut l, &p); assert_eq!(shown(&l).as_deref(), Some("a1"));
        next(&mut l, &p); assert_eq!(shown(&l).as_deref(), Some("a2"));
        next(&mut l, &p); assert_eq!(shown(&l).as_deref(), Some("b1"));
        let o = next(&mut l, &p); assert!(!o.ok); assert_eq!(o.error.as_deref(), Some("End of service"));
        l.apply(env(Command::Prev, Some(l.version)), &p); assert_eq!(shown(&l).as_deref(), Some("a2"));
    }

    #[test]
    fn scripture_slides_carry_their_passage_and_size_songs_do_not() {
        let mut p = plan(vec![("Song", vec![text("s1"), text("s2")]), ("Psalm", vec![text("p1"), text("p2")])]);
        let psalm = p.order[1];
        let item = p.items.get_mut(&psalm).unwrap();
        item.kind = "scripture".into();
        for s in &mut item.slides { if let SlideContent::Text { size, .. } = &mut s.content { *size = 90 } }
        let mut l = Live::new();
        next(&mut l, &p);
        assert!(matches!(&l.frame.slide, Some(SlideLayer::Text { passage, size: 0, .. }) if passage.is_empty()));
        l.apply(env(Command::Show { item_id: psalm, slide: 1 }, None), &p);
        let Some(SlideLayer::Text { text, passage, size, scripture, .. }) = &l.frame.slide else { panic!() };
        assert_eq!((text.as_str(), *size, *scripture), ("p2", 90, true));
        assert_eq!(passage.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["p1", "p2"]);
    }

    #[test]
    fn delayed_next_cannot_undo_clear_all() {
        let p = plan(vec![("A", vec![text("a1"), text("a2")])]);
        let mut l = Live::new();
        next(&mut l, &p);
        let seen = l.version; // phone saw a1
        l.apply(env(Command::ClearAll, None), &p);
        let late = l.apply(env(Command::Next, Some(seen)), &p);
        assert!(!late.ok);
        assert_eq!(l.frame, Frame::default());
        // A fresh Next after the clear shows the next cue (position kept).
        next(&mut l, &p); assert_eq!(shown(&l).as_deref(), Some("a2"));
    }

    #[test]
    fn two_nexts_from_same_state_advance_once() {
        let p = plan(vec![("A", vec![text("1"), text("2"), text("3")])]);
        let mut l = Live::new();
        next(&mut l, &p);
        let b = l.version;
        assert!(l.apply(env(Command::Next, Some(b)), &p).ok);
        assert!(!l.apply(env(Command::Next, Some(b)), &p).ok);
        assert_eq!(shown(&l).as_deref(), Some("2"));
    }

    #[test]
    fn seek_moves_a_playing_video_or_audio_and_counts_up() {
        let p = plan(vec![("V", vec![video("v.mp4", EndAction::Stop)]), ("A", vec![audio("a.mp3")])]);
        let (v, a) = (p.order[0], p.order[1]);
        let mut l = Live::new();
        l.apply(env(Command::Show { item_id: v, slide: 0 }, None), &p);
        let vi = l.frame.media.as_ref().unwrap().inst;
        let before = l.version;
        assert!(l.apply(env(Command::Seek { inst: vi, to: SeekTo::By { secs: 15.0 } }, None), &p).ok);
        assert_eq!(l.frame.seek, Some(Seek { inst: vi, n: 1, to: SeekTo::By { secs: 15.0 } }));
        assert!(l.version > before, "a seek is a change the room must see");
        assert!(l.apply(env(Command::Seek { inst: vi, to: SeekTo::BeforeEnd { secs: 10.0 } }, None), &p).ok);
        assert_eq!(l.frame.seek.as_ref().unwrap().n, 2, "the same jump twice must still count");
        l.apply(env(Command::Show { item_id: a, slide: 0 }, None), &p);
        let ai = l.frame.audio.as_ref().unwrap().inst;
        assert!(l.apply(env(Command::Seek { inst: ai, to: SeekTo::By { secs: -15.0 } }, None), &p).ok);
        assert_eq!(l.frame.seek, Some(Seek { inst: ai, n: 3, to: SeekTo::By { secs: -15.0 } }));
    }

    #[test]
    fn seek_on_something_not_playing_is_refused_and_not_saved() {
        let p = plan(vec![("T", vec![text("1")])]);
        let mut l = Live::new();
        next(&mut l, &p);
        let out = l.apply(env(Command::Seek { inst: 99, to: SeekTo::By { secs: 15.0 } }, None), &p);
        assert!(!out.ok);
        assert_eq!(l.frame.seek, None);
        let json = serde_json::to_string(&Frame { seek: Some(Seek { inst: 1, n: 4, to: SeekTo::By { secs: 1.0 } }), ..Frame::default() }).unwrap();
        assert!(!json.contains("seek"), "a seek is an instant; it is never saved or replayed after a restart");
    }

    #[test]
    fn duplicate_delivery_vs_intentional_retrigger() {
        let p = plan(vec![("V", vec![video("v.mp4", EndAction::Stop)])]);
        let id = p.order[0];
        let mut l = Live::new();
        let e = Envelope { id: "same".into(), base: None, cmd: Command::Show { item_id: id, slide: 0 } };
        l.apply(e.clone(), &p);
        let first = l.frame.media.as_ref().unwrap().inst;
        let again = l.apply(e, &p);
        assert!(again.duplicate);
        assert_eq!(l.frame.media.as_ref().unwrap().inst, first, "duplicate must not restart");
        l.apply(env(Command::Show { item_id: id, slide: 0 }, None), &p);
        assert_ne!(l.frame.media.as_ref().unwrap().inst, first, "new request restarts");
    }

    #[test]
    fn on_air_video_reports_survive_a_failed_request() {
        let p = plan(vec![("V", vec![video("a.mp4", EndAction::Stop)]), ("B", vec![video("b.mp4", EndAction::Stop)])]);
        let mut l = Live::new();
        l.apply(env(Command::Show { item_id: p.order[0], slide: 0 }, None), &p);
        commit(&mut l);
        let a = l.frame.media.as_ref().unwrap().inst;
        l.apply(env(Command::Show { item_id: p.order[1], slide: 0 }, None), &p); // B preparing
        let b = l.frame.media.as_ref().unwrap().inst;
        // A still reports while B prepares.
        assert!(l.report(RoomEvent::Media { inst: a, state: PlayState::Playing, position: 3.0, duration: 10.0, error: None }));
        assert_eq!(l.media_status.as_ref().unwrap().position, 3.0);
        // B fails: back to A, same instance.
        let v = l.version;
        assert!(l.report(RoomEvent::Failed { version: v, reason: "decode".into() }));
        assert_eq!(l.frame.media.as_ref().unwrap().inst, a);
        commit(&mut l);
        assert!(l.report(RoomEvent::Media { inst: a, state: PlayState::Playing, position: 4.0, duration: 10.0, error: None }));
        // B's late completion is ignored.
        assert!(!l.report(RoomEvent::Media { inst: b, state: PlayState::Playing, position: 0.0, duration: 5.0, error: None }));
        // A ends -> cleared (End = Stop).
        l.report(RoomEvent::Media { inst: a, state: PlayState::Ended, position: 10.0, duration: 10.0, error: None });
        assert!(l.frame.media.is_none());
        // Old report for A after it was cleared does nothing.
        assert!(!l.report(RoomEvent::Media { inst: a, state: PlayState::Playing, position: 1.0, duration: 10.0, error: None }));
    }

    #[test]
    fn video_that_ends_while_replacement_prepares_is_not_revived() {
        let p = plan(vec![("V", vec![video("a.mp4", EndAction::Stop)]), ("B", vec![video("b.mp4", EndAction::Stop)])]);
        let mut l = Live::new();
        l.apply(env(Command::Show { item_id: p.order[0], slide: 0 }, None), &p);
        commit(&mut l);
        let a = l.frame.media.as_ref().unwrap().inst;
        l.apply(env(Command::Show { item_id: p.order[1], slide: 0 }, None), &p);
        l.report(RoomEvent::Media { inst: a, state: PlayState::Ended, position: 10.0, duration: 10.0, error: None });
        let v = l.version;
        l.report(RoomEvent::Failed { version: v, reason: "x".into() });
        assert!(l.frame.media.is_none(), "ended video must not come back after revert");
    }

    #[test]
    fn deck_after_video_stops_the_video_and_its_audio() {
        let p = plan(vec![("V", vec![video("a.mp4", EndAction::Stop)]), ("D", vec![deck("s1.png")])]);
        let mut l = Live::new();
        next(&mut l, &p);
        assert!(l.frame.media.is_some());
        next(&mut l, &p);
        assert!(l.frame.media.is_none());
        assert!(matches!(l.frame.slide, Some(SlideLayer::Image { .. })));
    }

    #[test]
    fn audio_item_keeps_visuals_and_survives_item_changes() {
        let p = plan(vec![("Walk-in", vec![audio("m.mp3")]), ("A", vec![text("a1")]), ("D", vec![deck("d.png")])]);
        let mut l = Live::new();
        l.apply(env(Command::Logo, None), &p);
        next(&mut l, &p); // audio item
        assert!(l.frame.logo, "audio does not change visuals");
        let ai = l.frame.audio.as_ref().unwrap().inst;
        next(&mut l, &p); next(&mut l, &p);
        assert_eq!(l.frame.audio.as_ref().unwrap().inst, ai);
        l.apply(env(Command::ClearMedia, None), &p);
        assert!(l.frame.audio.is_some());
        l.apply(env(Command::ClearAudio, None), &p);
        assert!(l.frame.audio.is_none());
    }

    #[test]
    fn clear_matrix() {
        let p = plan(vec![("A", vec![text("a1")])]);
        let mut l = Live::new();
        next(&mut l, &p);
        l.apply(env(Command::ClearSlide, None), &p);
        assert!(l.frame.slide.is_none() && l.frame.background.is_some());
        next(&mut l, &p); // end of service; stays cleared
        l.apply(env(Command::Show { item_id: p.order[0], slide: 0 }, None), &p);
        l.apply(env(Command::ClearMedia, None), &p);
        assert!(l.frame.slide.is_some() && l.frame.background.is_none());
        l.apply(env(Command::Logo, None), &p);
        assert!(l.frame.logo && l.frame.slide.is_none());
        l.apply(env(Command::Show { item_id: p.order[0], slide: 0 }, None), &p);
        assert!(!l.frame.logo, "any trigger replaces the logo");
    }

    #[test]
    fn same_background_keeps_playing_across_slides() {
        let p = plan(vec![("A", vec![text("1"), text("2")])]);
        let mut l = Live::new();
        next(&mut l, &p);
        let bg = l.frame.background.as_ref().unwrap().inst;
        next(&mut l, &p);
        assert_eq!(l.frame.background.as_ref().unwrap().inst, bg);
    }

    #[test]
    fn removed_live_item_continues_at_former_position() {
        let mut p = plan(vec![("A", vec![text("a")]), ("B", vec![text("b")]), ("C", vec![text("c")])]);
        let mut l = Live::new();
        l.apply(env(Command::Show { item_id: p.order[1], slide: 0 }, None), &p);
        p.order.remove(1);
        next(&mut l, &p);
        assert_eq!(shown(&l).as_deref(), Some("c"));
    }

    fn scribble(l: &mut Live, phone: Uuid) {
        let c = drawing::DrawCmd::Start { stroke: 1, color: drawing::PALETTE[0].into(), width: 12.0, pen: drawing::Pen::Pen, points: vec![[0.2, 0.3], [0.6, 0.3]], ink: Default::default() };
        std::sync::Arc::make_mut(&mut l.drawing).apply(phone, &c).unwrap();
    }

    #[test]
    fn a_drawing_clears_when_the_slide_changes() {
        let p = plan(vec![("A", vec![text("a1"), text("a2")]), ("B", vec![text("b1")])]);
        let mut l = Live::new();
        next(&mut l, &p);
        scribble(&mut l, Uuid::new_v4());
        let v = l.version;
        assert_eq!(l.version, v, "drawing never moves the version");
        // Showing the same slide again keeps it.
        let (item_id, slide) = { let c = l.cursor.clone().unwrap(); (c.item_id, c.slide) };
        let b = l.version;
        assert!(l.apply(env(Command::Show { item_id, slide }, Some(b)), &p).ok);
        assert!(!l.drawing.is_empty(), "same slide again: kept");
        next(&mut l, &p);
        assert!(l.drawing.is_empty(), "next slide: cleared");
        scribble(&mut l, Uuid::new_v4());
        next(&mut l, &p);
        assert!(l.drawing.is_empty(), "into the next item: cleared");
    }

    #[test]
    fn keep_drawings_when_the_slide_changes() {
        let p = plan(vec![("A", vec![text("a1"), text("a2")])]);
        let mut l = Live::new();
        l.keep_drawing = true;
        next(&mut l, &p);
        scribble(&mut l, Uuid::new_v4());
        next(&mut l, &p);
        assert!(!l.drawing.is_empty(), "kept across the slide change");
        // Clear Text, Clear All and Logo clear it even then.
        for cmd in [Command::ClearSlide, Command::ClearAll, Command::Logo] {
            scribble(&mut l, Uuid::new_v4());
            assert!(l.apply(env(cmd.clone(), None), &p).ok);
            assert!(l.drawing.is_empty(), "{cmd:?}");
        }
        // Clear Media and Clear Audio leave it.
        scribble(&mut l, Uuid::new_v4());
        assert!(l.apply(env(Command::ClearMedia, None), &p).ok);
        assert!(l.apply(env(Command::ClearAudio, None), &p).ok);
        assert!(!l.drawing.is_empty());
        assert!(l.clear_drawing());
        assert!(!l.clear_drawing(), "nothing left");
    }

    /// A plan whose themes are all one big style (N15 "Use theme").
    struct ThemedPlan(TestPlan);
    impl Plan for ThemedPlan {
        fn order(&self) -> Vec<Uuid> { self.0.order() }
        fn build(&self, id: Uuid) -> Option<LiveItem> { self.0.build(id) }
        fn theme_style(&self, _id: Uuid) -> Option<Theme> { Some(Theme { size: 99, ..Theme::default() }) }
    }

    #[test]
    fn play_media_keeps_the_slide_and_the_cursor() {
        let p = plan(vec![("Song", vec![text("a1"), text("a2")]), ("Walk-in", vec![audio("walk.mp3")]), ("Clip", vec![video("v.mp4", EndAction::Stop)])]);
        let mut l = Live::new();
        next(&mut l, &p);
        let cursor = l.cursor.clone();
        assert!(l.apply(env(Command::PlayMedia { item_id: p.order[1] }, None), &p).ok);
        assert_eq!(l.frame.audio.as_ref().map(|a| a.v.asset.as_str()), Some("walk.mp3"));
        assert!(l.apply(env(Command::PlayMedia { item_id: p.order[2] }, None), &p).ok);
        assert!(matches!(l.frame.media.as_ref().map(|m| &m.v), Some(MediaLayer::Video { .. })));
        assert_eq!(shown(&l).as_deref(), Some("a1"), "the words stay");
        assert_eq!(l.cursor, cursor, "Next still walks the song");
        let words = l.apply(env(Command::PlayMedia { item_id: p.order[0] }, None), &p);
        assert_eq!(words.error, Some(crate::tr!("That item isn't a video, picture or sound.")));
    }

    #[test]
    fn use_theme_lasts_while_the_item_is_live() {
        let p = ThemedPlan(plan(vec![("Song", vec![text("a1"), text("a2")]), ("Next", vec![text("b1")])]));
        let mut l = Live::new();
        assert!(!l.apply(env(Command::UseTheme { theme: Uuid::nil() }, None), &p).ok, "nothing live");
        let b = l.version;
        l.apply(env(Command::Next, Some(b)), &p);
        let went = l.went;
        assert!(l.apply(env(Command::UseTheme { theme: Uuid::nil() }, None), &p).ok);
        let size = |l: &Live| match &l.frame.slide { Some(SlideLayer::Text { theme, .. }) => theme.as_ref().map(|t| t.size), _ => None };
        assert_eq!(size(&l), Some(99));
        assert_eq!(l.went, went, "a theme change takes no slide live");
        let (b, item) = (l.version, p.0.order[0]);
        l.apply(env(Command::Show { item_id: item, slide: 1 }, Some(b)), &p);
        assert_eq!(size(&l), Some(99), "a click on another of its slides keeps it");
        assert_eq!(l.went, went + 1);
        let b = l.version;
        l.apply(env(Command::Next, Some(b)), &p);
        assert_eq!(size(&l), Some(Theme::default().size), "the next item has its own");
    }

}

#[cfg(test)]
#[path = "live_clear_tests.rs"]
mod clear_tests;
