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
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use uuid::Uuid;

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
    Text { text: String, footer: Option<String>, reference: Option<String>, scripture: bool },
    Image { asset: AssetId },
}

/// The full layer set the room shows. Bottom → top: background, media, slide, logo. Audio is separate.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Frame {
    pub background: Option<Inst<Background>>,
    pub media: Option<Inst<MediaLayer>>,
    pub slide: Option<SlideLayer>,
    pub logo: bool,
    pub audio: Option<Inst<AudioLayer>>,
}

impl Frame {
    fn instances(&self) -> impl Iterator<Item = u64> + '_ {
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
}

impl Command {
    fn relative(&self) -> bool { matches!(self, Command::Next | Command::Prev) }
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
}

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
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PlayStatus {
    pub inst: u64,
    pub state: PlayState,
    pub position: f64,
    pub duration: f64,
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
    next_inst: u64,
    history: VecDeque<Snapshot>,
    seen: VecDeque<(String, Outcome)>,
}

const HISTORY: usize = 32;
const SEEN: usize = 1024;

impl Default for Live {
    fn default() -> Self { Self::new() }
}

impl Live {
    pub fn new() -> Self {
        let mut l = Self { version: 0, frame: Frame::default(), cursor: None, item: None, committed_version: 0, media_status: None,
            audio_status: None, notice: None, next_inst: 1, history: VecDeque::new(), seen: VecDeque::new() };
        l.remember();
        l
    }

    fn remember(&mut self) {
        self.history.push_back(Snapshot { version: self.version, frame: self.frame.clone(), cursor: self.cursor.clone(), item: self.item.clone() });
        while self.history.len() > HISTORY {
            // Never drop the committed snapshot: it is what a failed request reverts to.
            let idx = self.history.iter().position(|s| s.version != self.committed_version).unwrap_or(0);
            self.history.remove(idx);
        }
    }

    fn bump(&mut self) {
        self.version += 1;
        self.remember();
    }

    fn inst(&mut self) -> u64 { let i = self.next_inst; self.next_inst += 1; i }

    fn committed_frame(&self) -> Option<&Frame> {
        self.history.iter().find(|s| s.version == self.committed_version).map(|s| &s.frame)
    }

    /// What is on air: the committed snapshot, never the requested `frame`/`cursor`/`item`.
    /// `None` until the room has committed something.
    pub fn committed(&self) -> Option<(Frame, Option<Cursor>, Option<LiveItem>)> {
        if self.committed_version == 0 { return None; }
        self.history.iter().find(|s| s.version == self.committed_version).map(|s| (s.frame.clone(), s.cursor.clone(), s.item.clone()))
    }

    /// Puts a saved state back after a crash, as a new version for the room to commit.
    /// Without media, video and audio stay off; slides, stills, colour and image backgrounds return.
    pub fn restore(&mut self, s: &crate::recovery::state::LiveState, with_media: bool) {
        let mut frame = s.frame.clone();
        if !with_media {
            if frame.media.as_ref().is_some_and(|m| matches!(m.v, MediaLayer::Video { .. })) { frame.media = None; }
            if frame.background.as_ref().is_some_and(|b| matches!(b.v, Background::Video { .. })) { frame.background = None; }
            frame.audio = None;
        }
        // New playbacks never reuse a saved instance, whose reports could still be in flight.
        self.next_inst = self.next_inst.max(s.frame.instances().max().unwrap_or(0) + 1);
        self.frame = frame;
        self.cursor = s.cursor.clone();
        self.item = s.item.clone();
        self.bump();
    }

    /// Is a report for this playback instance still authoritative?
    pub fn instance_valid(&self, inst: u64) -> bool {
        self.frame.instances().any(|i| i == inst) || self.committed_frame().is_some_and(|f| f.instances().any(|i| i == inst))
    }

    pub fn pending(&self) -> bool { self.committed_version < self.version }

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

    fn run(&mut self, env: &Envelope, plan: &dyn Plan) -> Result<(), String> {
        let check_base = env.cmd.relative() || matches!(env.cmd, Command::Show { .. });
        if check_base {
            match env.base {
                Some(b) if b != self.version => return Err("stale: the screen changed before this arrived".into()),
                None if env.cmd.relative() => return Err("stale: missing base version".into()),
                _ => {}
            }
        }
        match &env.cmd {
            Command::Next => self.step(plan, true),
            Command::Prev => self.step(plan, false),
            Command::Show { item_id, slide } => {
                let order = plan.order();
                let index = order.iter().position(|i| i == item_id).ok_or("That item is not in the service.")?;
                let item = plan.build(*item_id).ok_or("That item has nothing to show.")?;
                if item.slides.is_empty() { return Err("That item has no slides.".into()); }
                let slide = (*slide).min(item.slides.len() - 1);
                self.go(item, index, slide);
                Ok(())
            }
            Command::ClearAll => { self.frame = Frame::default(); self.bump(); Ok(()) }
            Command::ClearSlide => { self.frame.slide = None; self.bump(); Ok(()) }
            Command::ClearMedia => { self.frame.background = None; self.frame.media = None; self.bump(); Ok(()) }
            Command::ClearAudio => { self.frame.audio = None; self.bump(); Ok(()) }
            Command::Logo => {
                self.frame.slide = None; self.frame.background = None; self.frame.media = None; self.frame.logo = true;
                self.bump();
                Ok(())
            }
            Command::MediaPause { inst, paused } => {
                match &mut self.frame.media {
                    Some(Inst { inst: i, v: MediaLayer::Video { paused: p, .. } }) if *i == *inst => { *p = *paused; }
                    _ => return Err("That video is no longer playing.".into()),
                }
                self.bump();
                Ok(())
            }
            Command::AudioPause { inst, paused } => {
                match &mut self.frame.audio {
                    Some(a) if a.inst == *inst => a.v.paused = *paused,
                    _ => return Err("That audio is no longer playing.".into()),
                }
                self.bump();
                Ok(())
            }
        }
    }

    fn step(&mut self, plan: &dyn Plan, forward: bool) -> Result<(), String> {
        let order = plan.order();
        let Some(cursor) = self.cursor.clone() else {
            if !forward { return Err("Nothing is live yet.".into()); }
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
            Some(p) => if forward { p + 1 } else { p.checked_sub(1).ok_or("Start of service")? },
            // Live item was removed: the item now at its former position comes next.
            None => if forward { cursor.item_index } else { cursor.item_index.checked_sub(1).ok_or("Start of service")? },
        };
        if target >= order.len() { return Err("End of service".into()); }
        self.take_index(plan, &order, target, forward)
    }

    /// Take the item at `index` live: first slide going forward, last slide going back.
    /// Items with no slides are skipped.
    fn take_index(&mut self, plan: &dyn Plan, order: &[Uuid], mut index: usize, forward: bool) -> Result<(), String> {
        loop {
            let id = *order.get(index).ok_or(if forward { "End of service" } else { "Start of service" })?;
            if let Some(item) = plan.build(id).filter(|i| !i.slides.is_empty()) {
                let slide = if forward { 0 } else { item.slides.len() - 1 };
                self.go(item, index, slide);
                return Ok(());
            }
            if forward { index += 1 } else { index = index.checked_sub(1).ok_or("Start of service")? }
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
    fn go(&mut self, item: LiveItem, index: usize, slide: usize) {
        let content = item.slides[slide].content.clone();
        match content {
            SlideContent::Text { text, footer, reference } => {
                let scripture = item.kind == "scripture";
                self.frame.slide = Some(SlideLayer::Text { text, footer, reference, scripture });
                self.set_background(Some(item.background.clone()));
                self.frame.media = None;
                self.frame.logo = false;
            }
            SlideContent::Image { asset } => {
                self.frame.slide = Some(SlideLayer::Image { asset });
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
                self.frame = snap.frame;
                self.cursor = snap.cursor;
                self.item = snap.item;
                self.notice = Some(format!("Could not show that: {reason}"));
                self.bump();
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
                        self.notice = Some(format!("Playback stopped: {}", error.unwrap_or_else(|| "the file could not be played".into())));
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
        let Some(c) = &self.cursor else {
            let id = *plan.order().first()?;
            let item = plan.build(id)?;
            return item.slides.first().map(|s| (item.title.clone(), s.clone()));
        };
        let item = self.item.as_ref()?;
        if c.slide + 1 < item.slides.len() { return Some((item.title.clone(), item.slides[c.slide + 1].clone())); }
        let order = plan.order();
        let mut idx = match order.iter().position(|i| *i == c.item_id) { Some(p) => p + 1, None => c.item_index };
        while let Some(id) = order.get(idx) {
            if let Some(it) = plan.build(*id) { if let Some(s) = it.slides.first() { return Some((it.title.clone(), s.clone())); } }
            idx += 1;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct TestPlan { order: Vec<Uuid>, items: HashMap<Uuid, LiveItem> }

    impl Plan for TestPlan {
        fn order(&self) -> Vec<Uuid> { self.order.clone() }
        fn build(&self, id: Uuid) -> Option<LiveItem> { self.items.get(&id).cloned() }
    }

    fn text(t: &str) -> Slide { Slide { label: t.into(), content: SlideContent::Text { text: t.into(), footer: None, reference: None }, notes: String::new() } }
    fn black() -> Background { Background::Color { color: "#000".into() } }

    fn plan(items: Vec<(&str, Vec<Slide>)>) -> TestPlan {
        let mut p = TestPlan { order: vec![], items: HashMap::new() };
        for (title, slides) in items {
            let id = Uuid::new_v4();
            p.order.push(id);
            p.items.insert(id, LiveItem { item_id: id, title: title.into(), kind: "song".into(), slides, background: black() });
        }
        p
    }

    fn video(asset: &str, end: EndAction) -> Slide { Slide { label: asset.into(), content: SlideContent::Video { asset: asset.into(), autoplay: true, end }, notes: String::new() } }
    fn audio(asset: &str) -> Slide { Slide { label: asset.into(), content: SlideContent::Audio { asset: asset.into(), looped: false }, notes: String::new() } }
    fn deck(asset: &str) -> Slide { Slide { label: asset.into(), content: SlideContent::Image { asset: asset.into() }, notes: String::new() } }

    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    fn env(cmd: Command, base: Option<u64>) -> Envelope {
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Envelope { id: format!("c{n}"), base, cmd }
    }
    fn next(l: &mut Live, p: &TestPlan) -> Outcome { let b = l.version; l.apply(env(Command::Next, Some(b)), p) }
    fn shown(l: &Live) -> Option<String> { match &l.frame.slide { Some(SlideLayer::Text { text, .. }) => Some(text.clone()), _ => None } }
    fn commit(l: &mut Live) { let v = l.version; l.report(RoomEvent::Committed { version: v }); }

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
}
