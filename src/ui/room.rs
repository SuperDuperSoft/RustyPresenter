//! The room output window: each new live view is prepared off the UI thread (`prepare`), swapped in
//! with a crossfade once every new layer is ready, then reported as committed (as `web/output.js`).
/// Shared with the operator's live monitor (`ui::preview`), which draws the same layers small.
pub mod prepare;
pub mod anim;
mod record;

use super::{Geo, RoomWindow, Side, Sprite, fade::{Chan, Fader}};
use crate::app::{App, LiveUpdate, LiveView, Update};
use crate::live::{Frame, MediaLayer, PlayState, RoomEvent, drawing::Drawing};
use crate::media_prefs::{Scale, SoundOut};
use crate::render::drawing::DrawingCanvas;
use crate::render::mask::{Mask, mask_pixels};
use crate::platform::monitors::{self, MonitorId};
use crate::render::{Generation, Renderer};
use crate::room::{self, Kind, LayerKey, Plan, Ready, RoomState};
use crate::screen_fix::{ScreenFix, TestPattern};
use crate::design::{Dir, Transition, TransitionKind};
use crate::video::{self, FrameGrabber, Options, Player, PlayerReport, VideoSurface, ffi::Mpv};
use prepare::{Fresh, Job, Made, Px, Work};
use slint::{ComponentHandle, Timer, TimerMode, VecModel};
use std::{cell::{Cell, RefCell}, collections::{HashMap, VecDeque}, rc::{Rc, Weak}, time::Duration};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc::Sender};
use tokio::sync::broadcast::error::RecvError;

/// Crossfade (`.layer` in `web/output.html`) and each half of a fade through black.
const FADE: Duration = Duration::from_millis(400);
const HALF: Duration = Duration::from_millis(200);

/// Slots of the players on air.
const BG: usize = 0;
const MEDIA: usize = 1;
const AUDIO: usize = 2;

fn slot(kind: Kind) -> Option<usize> {
    match kind { Kind::Background => Some(BG), Kind::Media => Some(MEDIA), Kind::Audio => Some(AUDIO), _ => None }
}

/// How long a plan's layers crossfade: Settings' look fade when only the look changed (N9; 0 cuts), else `FADE`.
fn fade_for(plan: &Plan, view: &LiveView) -> Duration {
    // N26: a designed slide's own transition. Through black, the layers change while the screen is black.
    if let Some(t) = plan.transition {
        return match t.kind {
            // A push or wipe moves the slide; the other layers crossfade for the same time.
            TransitionKind::Cut | TransitionKind::Black => Duration::ZERO,
            _ => Duration::from_millis(u64::from(t.ms.min(crate::design::TRANSITION_MAX_MS))),
        };
    }
    if plan.look_change { Duration::from_millis(u64::from(view.look_fade_ms.min(crate::model::LOOK_FADE_MAX_MS))) } else { FADE }
}

/// Each half of a fade through black: a designed slide's own time, else `HALF`.
fn half_for(plan: &Plan) -> Duration {
    match plan.transition {
        Some(t) if t.kind == TransitionKind::Black => Duration::from_millis(u64::from(t.ms.min(crate::design::TRANSITION_MAX_MS) / 2)),
        _ => HALF,
    }
}

/// A push or wipe on the slide layer: which way, and whether the new picture is on face b.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Move { kind: TransitionKind, dir: Dir, new_b: bool }

/// The push or wipe a plan's slide moves in with, if any.
fn moving(t: Option<Transition>) -> Option<Transition> { t.filter(|t| t.kind.moves()) }

/// Where a face is `p` (0–1) of the way through a push or wipe going `dir`; `new`: the face coming in.
pub(crate) fn geo(kind: TransitionKind, dir: Dir, p: f32, new: bool) -> Geo {
    let (vx, vy) = dir.step();
    let mut g = Geo::default();
    match kind {
        // The new slide comes in from the far side as the old one goes out the near one.
        TransitionKind::Push => {
            let k = if new { p - 1.0 } else { p };
            (g.x, g.y) = (vx * k, vy * k);
        }
        // The edge moves `dir`: the new slide shows behind it, the old one ahead of it.
        TransitionKind::Wipe => wipe(&mut g, dir, p, new),
        // The new slide moves in over the old one, which is cut where the new one is (so either may be on top).
        TransitionKind::Cover => if new { (g.x, g.y) = (vx * (p - 1.0), vy * (p - 1.0)) } else { wipe(&mut g, dir, p, false) },
        // The old slide moves off; the new one shows where it has gone.
        TransitionKind::Uncover => if new { wipe(&mut g, dir, p, true) } else { (g.x, g.y) = (vx * p, vy * p) },
        // The new slide opens from the middle, over the old one.
        TransitionKind::Split => if new {
            let cut = (1.0 - p) / 2.0;
            if vx != 0.0 { (g.l, g.r) = (cut, cut) } else { (g.t, g.b) = (cut, cut) }
            g.over = true;
        },
        TransitionKind::Zoom => if new { g.z = 1.0 - p; g.over = true },
        _ => {}
    }
    g
}

/// A wipe's cut on one face: the edge moves `dir`, the new slide behind it, the old one ahead of it.
fn wipe(g: &mut Geo, dir: Dir, p: f32, new: bool) {
    let cut = if new { 1.0 - p } else { p };
    match (dir, new) {
        (Dir::Left, true) | (Dir::Right, false) => g.l = cut,
        (Dir::Left, false) | (Dir::Right, true) => g.r = cut,
        (Dir::Up, true) | (Dir::Down, false) => g.t = cut,
        (Dir::Up, false) | (Dir::Down, true) => g.b = cut,
    }
}

/// Writes both slide faces' places for `mv` at `p` (none: both in place).
fn set_geo(ui: &RoomWindow, mv: Option<Move>, p: f32) {
    let (a, b) = match mv {
        Some(m) => (geo(m.kind, m.dir, p, !m.new_b), geo(m.kind, m.dir, p, m.new_b)),
        None => (Geo::default(), Geo::default()),
    };
    ui.set_slide_a_geo(a);
    ui.set_slide_b_geo(b);
}

/// The playback instance a layer key names (only players report).
fn inst(key: &LayerKey) -> Option<u64> {
    match key { LayerKey::Background(i) | LayerKey::Media(i) | LayerKey::Audio(i) => Some(*i), _ => None }
}

/// Why a layer failed when it did not load in time.
fn too_slow(kind: Kind, f: &Frame) -> String {
    match kind {
        Kind::Background => crate::tr!("background took too long to load"),
        Kind::Media if matches!(f.media.as_ref().map(|m| &m.v), Some(MediaLayer::Still { .. })) => crate::tr!("picture took too long to load"),
        Kind::Media => crate::tr!("video took too long to load"),
        Kind::Slide => crate::tr!("slide took too long to load"),
        Kind::Logo => crate::tr!("logo took too long to load"),
        Kind::Overlays => crate::tr!("overlay took too long to load"),
        Kind::Audio => crate::tr!("audio took too long to load"),
    }
}

/// The Darken fill over a background video, when it has one.
fn video_dim(kind: Kind, f: &Frame) -> Option<slint::Color> {
    if kind != Kind::Background { return None }
    let d = f.background.as_ref().map(|b| b.v.dim()).filter(|d| *d > 0)?;
    Some(slint::Color::from_argb_u8((u16::from(d) * 255 / 100) as u8, 0, 0, 0))
}

/// A layer's two faces as Rust keeps them. `b_top`: face b is the current one; `shown`: the current face is
/// visible. The window gets each face's picture and opacity as separate properties, so a change writes only the
/// face it touches (writing one struct for the whole layer dirtied both pictures).
#[derive(Clone, Default)]
struct Faces { a: Side, b: Side, b_top: bool, shown: bool, serial: u64 }

impl Faces {
    /// The face that is current (shown, or fading out once the layer is hidden).
    #[cfg(test)]
    fn front(&self) -> &Side { if self.b_top { &self.b } else { &self.a } }

    /// Puts `side` on the face that is not showing and makes it current, so it fades in over the old one.
    /// True when that is face b.
    fn put(&mut self, side: Side) -> bool {
        self.serial += 1;
        if self.shown { self.b_top = !self.b_top }
        if self.b_top { self.b = side } else { self.a = side }
        self.shown = true;
        self.b_top
    }

    /// Where the opacities of faces a and b are heading.
    fn ops(&self) -> (f32, f32) {
        let on = |b: bool| if self.shown && self.b_top == b { 1.0 } else { 0.0 };
        (on(false), on(true))
    }
}

/// The room's picture layers, bottom to top, as indices into `Inner::faces`.
const LAYERS: [Kind; 5] = [Kind::Background, Kind::Media, Kind::Slide, Kind::Logo, Kind::Overlays];

fn index(kind: Kind) -> Option<usize> { LAYERS.iter().position(|k| *k == kind) }

/// How a picture on the media layer fills the room (Settings → Media, N24): `media-fit` in `room.slint`.
fn media_fit(scale: Scale) -> i32 {
    match scale { Scale::Fit => 0, Scale::Fill => 1, Scale::Stretch => 2 }
}

type SetSide = fn(&RoomWindow, Side);
type SetOp = fn(&RoomWindow, f32);

/// The window's setters for a layer's faces: picture a, picture b, opacity a, opacity b.
fn setters(i: usize) -> (SetSide, SetSide, SetOp, SetOp) {
    match LAYERS[i] {
        Kind::Background => (RoomWindow::set_bg_a, RoomWindow::set_bg_b, RoomWindow::set_bg_a_op, RoomWindow::set_bg_b_op),
        Kind::Media => (RoomWindow::set_media_a, RoomWindow::set_media_b, RoomWindow::set_media_a_op, RoomWindow::set_media_b_op),
        Kind::Slide => (RoomWindow::set_slide_a, RoomWindow::set_slide_b, RoomWindow::set_slide_a_op, RoomWindow::set_slide_b_op),
        Kind::Overlays => (RoomWindow::set_overlays_a, RoomWindow::set_overlays_b, RoomWindow::set_overlays_a_op, RoomWindow::set_overlays_b_op),
        _ => (RoomWindow::set_logo_a, RoomWindow::set_logo_b, RoomWindow::set_logo_a_op, RoomWindow::set_logo_b_op),
    }
}

/// Work for the UI thread, from the prepare, report and update threads.
enum Msg { Made { token: u64, kind: Kind, made: Result<Made, String> }, Report(PlayerReport), Device(SoundOut), Event(RoomEvent), Drawing(Option<Px>), Fix(ScreenFix, TestPattern), Mask(Option<Px>) }

/// This window's screen fix and test pattern (N20), from the setup: the screen its window name stands for.
fn fix_of(app: &App, role: &str) -> (ScreenFix, TestPattern) {
    let d = app.data.lock().unwrap();
    d.settings.screens.screen_for(role).map_or_else(Default::default, |s| (s.fix, s.test))
}

/// What the room's threads share.
struct Shared {
    inbox: Mutex<VecDeque<Msg>>,
    /// Only the newest live view is applied.
    latest: Mutex<Option<LiveUpdate>>,
    /// Instances on air, with their layer: their reports go straight to the live engine.
    started: Mutex<HashMap<u64, Kind>>,
    /// To the live engine, in order, off the UI thread (`App::room_event` waits for saves).
    events: Sender<RoomEvent>,
    queued: AtomicBool,
    closed: AtomicBool,
    /// Asks the UI thread to `pump` (coalesced by `queued`).
    wake: Box<dyn Fn() + Send + Sync>,
    /// The size the phone drawing is drawn at (the room's canvas).
    canvas: Mutex<(u32, u32)>,
    /// The window name, for its screen's fix ("room", "room:<id>").
    role: String,
    /// N12: the mask last drawn, and the canvas size it was drawn at.
    mask: Mutex<Option<(Mask, (u32, u32))>>,
}

impl Shared {
    /// Queues `m` for the UI thread; once closed it is dropped (a player in it is freed here).
    fn send(&self, m: Msg) {
        let m = { let mut q = self.inbox.lock().unwrap(); if self.closed.load(Ordering::Acquire) { Some(m) } else { q.push_back(m); None } };
        if m.is_none() { self.wake() }
    }
    fn wake(&self) { if !self.queued.swap(true, Ordering::AcqRel) { (self.wake)() } }
    fn emit(&self, ev: RoomEvent) { let _ = self.events.send(ev); }
    /// For a report from an instance on air: whether the live engine wants it. Like
    /// `web/output.js`, the background only speaks up when it fails.
    fn on_air(&self, r: &PlayerReport) -> Option<bool> {
        self.started.lock().unwrap().get(&r.inst).map(|k| *k != Kind::Background || r.state == PlayState::Error)
    }
}


/// What a prepared layer shows. `Anim`: a designed slide's end picture and its click group's pictures.
enum Got { Pic(Side), Play(Rc<Player>), Anim(Side, Vec<anim::SpriteImg>) }

/// The newest plan while its layers are prepared. `heard`: what players not made yet reported, by
/// serial (`Some` = failed); readiness is per player, as an older plan's player may share the instance.
/// `fade`: how long its layers crossfade (Settings' look fade for a look change, N9).
struct Prep { plan: Plan, frame: Frame, pause: Vec<(u64, bool)>, waiting: Vec<Kind>, got: Vec<(Kind, Got)>, heard: HashMap<u64, Option<String>>, fade: Duration }

impl Prep {
    fn done(&mut self, kind: Kind) { self.waiting.retain(|k| *k != kind) }
}

struct Inner {
    ui: RoomWindow,
    surface: Rc<VideoSurface>,
    monitor: Option<MonitorId>,
    work: Arc<Work>,
    /// To the room's prepare thread (`prepare::worker`), which ends when this is dropped.
    jobs: Sender<Job>,
    shared: Arc<Shared>,
    state: RefCell<RoomState>,
    prep: RefCell<Option<Prep>>,
    /// Players on air, by slot.
    players: RefCell<[Option<Rc<Player>>; 3]>,
    /// A swap waiting for the screen to turn black, with its commit unless already sent.
    deferred: RefCell<Option<(Prep, Option<RoomEvent>)>>,
    /// Ends the half for `deferred` (restarted by each swap, so it is always the newest's).
    fade: Timer,
    /// The picture layers' faces, by `index`.
    faces: RefCell<[Faces; 5]>,
    /// Drives the faces' opacities and the black overlay, redrawing only this window.
    fader: Fader,
    /// Each layer's (face a, face b) opacity, by `index`.
    face_ops: [(Chan, Chan); 5],
    /// The newest live view, for the clock's tick.
    last: RefCell<Option<LiveView>>,
    /// The newest `Seek::n` already done (N4), so each jump happens once.
    seeked: Cell<u64>,
    /// Runs once a second while a bound overlay (a clock) is on: a new text prepares only the overlay layer.
    clock: Timer,
    black: Chan,
    /// How far a push or wipe on the slide layer is (0–1), and which one runs (`None`: the faces sit still).
    motion: Chan,
    moving: Rc<Cell<Option<Move>>>,
    /// A designed slide's click group while it plays (on its own frame timer) and the window's sprites.
    anim: anim::Animator,
    sprites: Rc<VecModel<Sprite>>,
    /// The window level last set (`Room::set_on_top`); `None` until the window exists.
    on_top: Cell<Option<bool>>,
    /// Diagnostics only (`Room::set_swap_probe`): called each time a new view is put on screen.
    swap_probe: RefCell<Option<Box<dyn Fn()>>>,
    /// The screen's fix (its delay is used here; colour and corners go to `surface`) and test pattern (N20).
    fix: Cell<(ScreenFix, TestPattern)>,
    /// Live views held back by the screen's delay, oldest first, with when each is due.
    delayed: RefCell<VecDeque<(std::time::Instant, LiveUpdate)>>,
    delay_timer: Timer,
    /// N22: the recording this window feeds (`record`).
    rec: RefCell<Option<record::RoomRec>>,
    me: Weak<Inner>,
}

impl Inner {
    /// Handles everything that arrived from other threads.
    fn pump(&self) {
        self.shared.queued.store(false, Ordering::Release);
        let latest = self.shared.latest.lock().unwrap().take();
        if let Some(u) = latest { self.arrived(u) }
        loop {
            let m = self.shared.inbox.lock().unwrap().pop_front();
            match m {
                Some(Msg::Made { token, kind, made }) => self.made(token, kind, made),
                Some(Msg::Report(r)) => self.report(r),
                Some(Msg::Device(d)) => self.sync_device(&d),
                Some(Msg::Event(ev)) => self.shared.emit(ev),
                Some(Msg::Drawing(px)) => self.ui.set_drawing(px.map(Px::image).unwrap_or_default()),
                Some(Msg::Fix(fix, test)) => self.set_fix(fix, test),
                Some(Msg::Mask(px)) => self.ui.set_mask(px.map(Px::image).unwrap_or_default()),
                None => break,
            }
        }
    }

    /// A new live view: shown now, or once the screen's delay has passed (N20).
    fn arrived(&self, u: LiveUpdate) {
        let wait = self.fix.get().0.delay_ms();
        if wait == 0 && self.delayed.borrow().is_empty() { return self.view(&u) }
        self.delayed.borrow_mut().push_back((std::time::Instant::now() + Duration::from_millis(wait), u));
        self.next_delayed();
    }

    /// Shows the views now due (the newest of them; older ones would only flash) and waits for the next.
    fn next_delayed(&self) {
        let now = std::time::Instant::now();
        let mut due = None;
        while self.delayed.borrow().front().is_some_and(|(at, _)| *at <= now) { due = self.delayed.borrow_mut().pop_front().map(|(_, u)| u) }
        if let Some(u) = due { self.view(&u) }
        let next = self.delayed.borrow().front().map(|(at, _)| at.saturating_duration_since(now));
        if let Some(wait) = next {
            let me = self.me.clone();
            self.delay_timer.start(TimerMode::SingleShot, wait, move || if let Some(i) = me.upgrade() { i.next_delayed() });
        }
    }

    /// The screen's fix and test pattern (N20): colour and corners for the GL pass, the pattern over everything.
    fn set_fix(&self, fix: ScreenFix, test: TestPattern) {
        let (old_fix, old_test) = self.fix.replace((fix, test));
        self.surface.set_fix(fix);
        if fix.delay < old_fix.delay || fix.delay == 0 {
            // A shorter wait: what is held back is due sooner.
            let wait = Duration::from_millis(fix.delay_ms());
            for (at, _) in self.delayed.borrow_mut().iter_mut() { *at = (*at).min(std::time::Instant::now() + wait) }
            self.next_delayed();
        }
        if test != old_test {
            let px = crate::screen_fix::pattern_pixels(test, self.canvas());
            let (w, h) = self.canvas();
            self.ui.set_test_pattern(px.map(|p| slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(&p, w.max(1), h.max(1)))).unwrap_or_default());
            self.ui.set_testing(!test.is_off());
        }
    }

    fn view(&self, view: &LiveView) {
        *self.last.borrow_mut() = Some(view.clone());
        self.sync_clock(view);
        let plan = self.state.borrow_mut().plan(view);
        match plan {
            Some(plan) => self.start(plan, view),
            None => { self.sync_pause(&room::pause_sync(view)); self.sync_seek(view.frame.seek) }
        }
    }

    /// The size layers are prepared at: the room monitor's, else the window's.
    fn canvas(&self) -> (u32, u32) {
        if let Some(m) = &self.monitor { return m.size }
        let s = self.ui.window().size();
        if s.width > 0 && s.height > 0 { (s.width, s.height) } else { (1920, 1080) }
    }

    fn start(&self, plan: Plan, view: &LiveView) {
        let old = self.prep.borrow_mut().take();
        if let Some(old) = old { self.dispose(old) } // the `token` rule: only the newest plan is prepared
        // Whatever an older plan still had to prepare (or render) is dropped from here on.
        let wanted = self.work.generation.next();
        let (token, wants) = (plan.token, plan.prepare.clone());
        for w in &wants {
            let (me, kind, why) = (self.me.clone(), w.kind, too_slow(w.kind, &view.frame));
            Timer::single_shot(w.timeout, move || if let Some(i) = me.upgrade() { i.timed_out(token, kind, why) });
        }
        *self.shared.canvas.lock().unwrap() = self.canvas();
        if self.shared.role == "room" { self.work.app.set_room_canvas(self.canvas()) }
        let waiting = wants.iter().map(|w| w.kind).collect();
        let fade = fade_for(&plan, view);
        *self.prep.borrow_mut() = Some(Prep { plan, frame: view.frame.clone(), pause: room::pause_sync(view), waiting, got: vec![], heard: HashMap::new(), fade });
        if !wants.is_empty() {
            let job = Job { token, wants, frame: view.frame.clone(), theme: view.theme.clone(), logo: view.logo.clone(), canvas: self.canvas(), wanted };
            if self.jobs.send(job).is_err() { return self.fail(token, crate::tr!("the room could not prepare it")) }
        }
        self.check();
    }

    /// Ticks four times a second while the frame has a bound overlay (a clock or a FEAT-2 timer), so a timer's
    /// second shows close to when it changes; `RoomState::plan` prepares again only when the text changed.
    fn sync_clock(&self, view: &LiveView) {
        let bound = view.frame.overlays.iter().any(|o| o.source.is_bound());
        if bound == self.clock.running() { return }
        if !bound { return self.clock.stop() }
        let me = self.me.clone();
        self.clock.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            let Some(i) = me.upgrade() else { return };
            let view = i.last.borrow().clone();
            if let Some(v) = view { i.view(&v) }
        });
    }

    fn timed_out(&self, token: u64, kind: Kind, why: String) {
        let late = self.prep.borrow().as_ref().is_some_and(|p| p.plan.token == token && p.waiting.contains(&kind));
        if late { self.fail(token, why) }
    }

    fn made(&self, token: u64, kind: Kind, made: Result<Made, String>) {
        if !self.prep.borrow().as_ref().is_some_and(|p| p.plan.token == token) { return } // stale: dropping disposes it
        match made {
            Err(reason) => return self.fail(token, reason),
            Ok(Made::Pic(px, color)) => {
                if kind == Kind::Media { self.ui.set_media_fit(media_fit(self.work.app.data.lock().unwrap().settings.media.scale)) }
                let side = Side { image: px.map(Px::image).unwrap_or_default(), color };
                if let Some(p) = self.prep.borrow_mut().as_mut() { p.got.push((kind, Got::Pic(side))); p.done(kind) }
            }
            Ok(Made::Anim(px, layers)) => {
                let side = Side { image: px.image(), color: Default::default() };
                if let Some(p) = self.prep.borrow_mut().as_mut() { p.got.push((kind, Got::Anim(side, layers.images()))); p.done(kind) }
            }
            Ok(Made::Player(Fresh(player))) => {
                let player = Rc::new(player);
                if kind != Kind::Audio { self.surface.add(player.clone()) } // a render context, so it loads undrawn
                let heard = self.prep.borrow_mut().as_mut().and_then(|p| {
                    let h = p.heard.remove(&player.serial());
                    if h == Some(None) { p.done(kind) }
                    p.got.push((kind, Got::Play(player)));
                    h.flatten()
                });
                if let Some(reason) = heard { return self.fail(token, reason) }
            }
        }
        self.check();
    }

    /// A player report: forwarded once its instance is on air, else it tells whether a prepared player is ready.
    fn report(&self, r: PlayerReport) {
        match self.shared.on_air(&r) { Some(true) => return self.shared.emit(r.into()), Some(false) => return, None => {} }
        let failed = {
            let mut prep = self.prep.borrow_mut();
            let Some(p) = prep.as_mut() else { return };
            let Some(kind) = p.plan.prepare.iter().find(|w| inst(&w.key) == Some(r.inst)).map(|w| w.kind) else { return };
            if !p.waiting.contains(&kind) { return }
            let error = (r.state == PlayState::Error).then(|| r.error.unwrap_or_else(|| crate::tr!("the file could not be played")));
            // Only this plan's player counts (or the report that none could be made); any other
            // waits for its `Made`, which a disposed player never gets.
            let ours = r.player == 0 || p.got.iter().any(|(k, g)| *k == kind && matches!(g, Got::Play(pl) if pl.serial() == r.player));
            match (ours, error) {
                (true, Some(e)) => Some((p.plan.token, e)),
                (true, None) => { p.done(kind); None }
                (false, e) => { let h = p.heard.entry(r.player).or_default(); if e.is_some() { *h = e } None }
            }
        };
        match failed { Some((token, reason)) => self.fail(token, reason), None => self.check() }
    }

    /// Preparing `token` failed: drop what was prepared and keep what is on air.
    fn fail(&self, token: u64, reason: String) {
        let prep = self.prep.borrow_mut().take_if(|p| p.plan.token == token);
        let Some(prep) = prep else { return };
        self.dispose(prep);
        // A swap waiting for black is already shown as far as `state` goes: commit it first,
        // or the engine would go back past it. The screen still swaps only once black.
        let commit = self.deferred.borrow_mut().as_mut().and_then(|d| d.1.take());
        if let Some(c) = commit { self.shared.emit(c) }
        let ev = self.state.borrow_mut().failed(token, reason);
        if let Some(ev) = ev { self.shared.emit(ev) }
    }

    fn check(&self) {
        if !self.prep.borrow().as_ref().is_some_and(|p| p.waiting.is_empty()) { return }
        let Some(prep) = self.prep.borrow_mut().take() else { return };
        let ready = self.state.borrow_mut().ready(prep.plan.token);
        match ready { Ready::Swap { commit } => self.swap(prep, commit), Ready::Stale => self.dispose(prep) }
    }

    fn swap(&self, prep: Prep, commit: RoomEvent) {
        self.flush();
        if !prep.plan.fade_through_black { return self.apply(prep, Some(commit)) }
        let half = half_for(&prep.plan);
        self.fader.fade(self.black, 1.0, half);
        *self.deferred.borrow_mut() = Some((prep, Some(commit)));
        let me = self.me.clone();
        self.fade.start(TimerMode::SingleShot, half, move || if let Some(i) = me.upgrade() { i.flush() });
    }

    /// Applies a swap that waited for black, and fades back in.
    fn flush(&self) {
        let d = self.deferred.borrow_mut().take();
        if let Some((prep, commit)) = d { let half = half_for(&prep.plan); self.apply(prep, commit); self.fader.fade(self.black, 0.0, half) }
    }

    fn apply(&self, prep: Prep, commit: Option<RoomEvent>) {
        let mut players = self.players.borrow_mut();
        let fade = prep.fade;
        for kind in &prep.plan.remove {
            self.hide(*kind, fade);
            if let Some(s) = slot(*kind) { self.retire(players[s].take()) }
        }
        for (kind, got) in prep.got {
            let old = slot(kind).and_then(|s| players[s].take());
            match got {
                Got::Pic(side) if kind == Kind::Slide && prep.plan.build => self.put_over(kind, side, fade),
                Got::Pic(side) if kind == Kind::Slide && let Some(t) = moving(prep.plan.transition) => self.put_moving(side, t),
                Got::Pic(side) => self.put(kind, side, fade),
                Got::Anim(side, imgs) => match &prep.frame.slide {
                    Some(crate::live::SlideLayer::Design { play: Some(play), .. }) => self.put_animated(side, imgs, (**play).clone(), prep.plan.build, fade),
                    _ => self.put(kind, side, fade),
                },
                Got::Play(p) => {
                    // Video is drawn under the scene (audio has no layer); a background video's Darken is a black
                    // fill on its layer.
                    match video_dim(kind, &prep.frame) { Some(c) => self.put(kind, Side { image: Default::default(), color: c }, fade), None => self.hide(kind, fade) }
                    self.shared.started.lock().unwrap().insert(p.inst(), kind);
                    if let Some(s) = slot(kind) { players[s] = Some(p) }
                }
            }
            self.retire(old);
        }
        if let Some(bg) = &players[BG] { bg.set_paused(false) }
        let drawn = players[MEDIA].clone().or_else(|| players[BG].clone());
        drop(players);
        self.surface.set_player(drawn);
        self.ui.set_video(self.surface.active());
        self.ui.set_bg_visible(room::draw_background(&prep.frame));
        self.sync_pause(&prep.pause);
        self.sync_seek(prep.frame.seek);
        if let Some(c) = commit { self.shared.emit(c) }
        if let Some(f) = self.swap_probe.borrow().as_ref() { f() }
    }

    /// Takes a player off air: silent at once, freed after the fade.
    fn retire(&self, p: Option<Rc<Player>>) {
        let Some(p) = p else { return };
        self.shared.started.lock().unwrap().remove(&p.inst());
        p.set_paused(true);
        let surface = self.surface.clone();
        Timer::single_shot(FADE, move || surface.remove(&p));
    }

    /// Drops what a plan prepared without showing it.
    fn dispose(&self, prep: Prep) {
        for (_, got) in prep.got {
            if let Got::Play(p) = got { self.surface.remove(&p) }
        }
    }

    /// Moves what is on air to the sound output just chosen (a device back after it went away, or Listen turned on
    /// or off), with its delay and routing. New players read it from the settings.
    fn sync_device(&self, sound: &SoundOut) {
        let players = self.players.borrow();
        for p in [&players[MEDIA], &players[AUDIO]].into_iter().flatten() { p.set_sound(sound) }
    }

    /// Does the newest jump once, on the player it names (one taken off air since is left alone).
    fn sync_seek(&self, seek: Option<crate::live::Seek>) {
        let Some(s) = seek.filter(|s| s.n > self.seeked.get()) else { return };
        self.seeked.set(s.n);
        let players = self.players.borrow();
        for p in [&players[MEDIA], &players[AUDIO]].into_iter().flatten().filter(|p| p.inst() == s.inst) {
            match s.to {
                crate::live::SeekTo::By { secs } => p.seek_by(secs),
                crate::live::SeekTo::BeforeEnd { secs } => p.seek_before_end(secs),
            }
        }
    }

    fn sync_pause(&self, pairs: &[(u64, bool)]) {
        let players = self.players.borrow();
        for (inst, paused) in pairs {
            for p in [&players[MEDIA], &players[AUDIO]].into_iter().flatten().filter(|p| p.inst() == *inst) { p.set_paused(*paused) }
        }
    }

    /// Puts `side` on `kind`'s hidden face (only that face's picture is written) and crossfades to it.
    fn put(&self, kind: Kind, side: Side, fade: Duration) {
        let Some(i) = index(kind) else { return };
        if kind == Kind::Slide { self.end_motion() }
        let (on_b, ops) = { let mut f = self.faces.borrow_mut(); let on_b = f[i].put(side.clone()); (on_b, f[i].ops()) };
        let (set_a, set_b, _, _) = setters(i);
        if on_b { set_b(&self.ui, side) } else { set_a(&self.ui, side) }
        self.fade_to(i, ops, fade);
    }

    /// N26 builds: `side` holds everything on screen plus what is new, so it fades in over the old picture (or the
    /// old one fades out over it) with the old one kept at full: what stays never dims, only the new part appears.
    fn put_over(&self, kind: Kind, side: Side, fade: Duration) {
        let Some(i) = index(kind) else { return };
        if kind == Kind::Slide { self.end_motion() }
        let (was_shown, on_b, serial) = { let mut f = self.faces.borrow_mut(); let was = f[i].shown; let on_b = f[i].put(side.clone()); (was, on_b, f[i].serial) };
        let (set_a, set_b, _, _) = setters(i);
        if on_b { set_b(&self.ui, side) } else { set_a(&self.ui, side) }
        let ops = self.faces.borrow()[i].ops();
        if !was_shown { return self.fade_to(i, ops, fade) }
        let (ca, cb) = self.face_ops[i];
        if on_b {
            self.fader.fade(ca, 1.0, Duration::ZERO);
            self.fader.fade(cb, 1.0, fade);
            // Once the new face is up, the old one under it goes (it would darken see-through edges twice).
            let me = self.me.clone();
            Timer::single_shot(fade, move || if let Some(me) = me.upgrade() {
                if me.faces.borrow()[i].serial == serial { me.fader.fade(ca, 0.0, Duration::ZERO) }
            });
        } else {
            self.fader.fade(ca, 1.0, Duration::ZERO);
            self.fader.fade(cb, 0.0, fade);
        }
    }

    /// Push and wipe: `side` goes on the slide's hidden face, fully up, and moves in over `t.ms` as the old face
    /// moves out (or is uncovered); then the old face goes.
    fn put_moving(&self, side: Side, t: Transition) {
        let Some(i) = index(Kind::Slide) else { return };
        self.end_motion();
        let (on_b, serial) = { let mut f = self.faces.borrow_mut(); let on_b = f[i].put(side.clone()); (on_b, f[i].serial) };
        let (set_a, set_b, _, _) = setters(i);
        if on_b { set_b(&self.ui, side) } else { set_a(&self.ui, side) }
        let (ca, cb) = self.face_ops[i];
        let (new, old) = if on_b { (cb, ca) } else { (ca, cb) };
        self.moving.set(Some(Move { kind: t.kind, dir: t.dir, new_b: on_b }));
        set_geo(&self.ui, self.moving.get(), 0.0);
        self.fader.jump(new, 1.0);
        let dur = Duration::from_millis(u64::from(t.ms.min(crate::design::TRANSITION_MAX_MS)));
        self.fader.fade(self.motion, 1.0, dur);
        let me = self.me.clone();
        Timer::single_shot(dur, move || if let Some(me) = me.upgrade() && me.faces.borrow()[i].serial == serial {
            me.fader.jump(old, 0.0);
            me.end_motion();
        });
    }

    /// A designed slide's click group: `side` (the slide once it has played) goes on the hidden face, unseen, and
    /// the group's pictures play over the slide layer; then the face shows and they go. A build's old face goes at
    /// once (the pictures hold all of it); a new slide's fades out under them as they come in with its
    /// transition, and then the group plays (group 0).
    fn put_animated(&self, side: Side, imgs: Vec<anim::SpriteImg>, play: crate::design::Play, build: bool, fade: Duration) {
        let Some(i) = index(Kind::Slide) else { return };
        self.end_motion();
        let on_b = self.faces.borrow_mut()[i].put(side.clone());
        let (set_a, set_b, _, _) = setters(i);
        if on_b { set_b(&self.ui, side) } else { set_a(&self.ui, side) }
        let (ca, cb) = self.face_ops[i];
        let (new, old) = if on_b { (cb, ca) } else { (ca, cb) };
        self.fader.jump(new, 0.0);
        if build { self.fader.jump(old, 0.0) } else { self.fader.fade(old, 0.0, fade) }
        let lead_ms = if build { 0.0 } else { fade.as_secs_f32() * 1000.0 };
        let run = anim::Running { imgs, play, area: anim::area(self.canvas()), lead_ms };
        let (me, sprites) = (self.me.clone(), self.sprites.clone());
        self.anim.start(run, move |rows| match rows {
            Some((rows, _)) => anim::set_rows(&sprites, rows),
            None => if let Some(i) = me.upgrade() { i.after_anim() },
        });
    }

    /// Ends a playing group where it is: its pictures go and the slide's faces show as they should.
    fn end_anim(&self) { if self.anim.stop() { self.after_anim() } }

    /// A group is over: its pictures go and the slide's faces show as they should.
    fn after_anim(&self) {
        anim::set_rows(&self.sprites, vec![]);
        let Some(i) = index(Kind::Slide) else { return };
        let (a, b) = self.faces.borrow()[i].ops();
        let (ca, cb) = self.face_ops[i];
        self.fader.jump(ca, a);
        self.fader.jump(cb, b);
    }

    /// Stops a push or wipe where it is: both slide faces back in place (a new slide cuts in over it).
    fn end_motion(&self) {
        self.end_anim();
        if self.moving.take().is_none() { return }
        self.fader.jump(self.motion, 0.0);
        set_geo(&self.ui, None, 0.0);
        // The face that was going out goes at once.
        let Some(i) = index(Kind::Slide) else { return };
        let (a, b) = self.faces.borrow()[i].ops();
        let (ca, cb) = self.face_ops[i];
        self.fader.jump(ca, a);
        self.fader.jump(cb, b);
    }

    /// Fades `kind` out (its pictures stay, for a later crossfade).
    fn hide(&self, kind: Kind, fade: Duration) {
        let Some(i) = index(kind) else { return };
        if kind == Kind::Slide { self.end_motion() }
        let ops = { let mut f = self.faces.borrow_mut(); f[i].shown = false; f[i].ops() };
        self.fade_to(i, ops, fade);
    }

    fn fade_to(&self, i: usize, (a, b): (f32, f32), fade: Duration) {
        let (ca, cb) = self.face_ops[i];
        self.fader.fade(ca, a, fade);
        self.fader.fade(cb, b, fade);
    }

    fn close(&self) {
        // Under the inbox lock, so nothing is queued after the drain: a queued player holds the
        // reports sender, which would keep `forward` (and so `Shared`) alive for good.
        let inbox = { let mut q = self.shared.inbox.lock().unwrap(); self.shared.closed.store(true, Ordering::Release); std::mem::take(&mut *q) };
        drop(inbox);
        let prep = self.prep.borrow_mut().take();
        if let Some(p) = prep { self.dispose(p) }
        let deferred = self.deferred.borrow_mut().take();
        if let Some((p, _)) = deferred { self.dispose(p) }
        let players = std::mem::take(&mut *self.players.borrow_mut());
        for p in players.into_iter().flatten() {
            self.shared.started.lock().unwrap().remove(&p.inst());
            self.surface.remove(&p);
        }
        self.record(None);
        let _ = self.ui.hide();
    }
}

/// An open room window. It follows the live view until it is dropped.
pub struct Room { inner: Rc<Inner> }

impl Room {
    /// Open the room window, full screen on `monitor` (a plain window without one). Without
    /// libmpv everything but video and audio works; those report the missing player.
    pub fn open(app: Arc<App>, renderer: Arc<Renderer>, mpv: Option<Arc<Mpv>>, monitor: Option<MonitorId>) -> Result<Rc<Room>, String> {
        Self::open_as(app, renderer, mpv, monitor, None)
    }

    /// An extra audience screen: it follows the same live view, silently, and leaves telling the live engine what is
    /// on air to the room.
    /// `role` is its window name ("room:<id>"), for its screen's fix.
    pub fn open_mirror(app: Arc<App>, renderer: Arc<Renderer>, mpv: Option<Arc<Mpv>>, monitor: Option<MonitorId>, role: &str) -> Result<Rc<Room>, String> {
        Self::open_as(app, renderer, mpv, monitor, Some(role))
    }

    fn open_as(app: Arc<App>, renderer: Arc<Renderer>, mpv: Option<Arc<Mpv>>, monitor: Option<MonitorId>, mirror_of: Option<&str>) -> Result<Rc<Room>, String> {
        let mirror = mirror_of.is_some();
        let role = mirror_of.unwrap_or("room").to_string();
        let ui = RoomWindow::new().map_err(|e| crate::tr!("Could not open the room screen: {error}", error = e))?;
        // Slint picks the computer's language for the first window; show the app's.
        crate::i18n::sync_slint();
        let weak = ui.as_weak();
        // The room is the one window that waits for its monitor's refresh: the projector never tears.
        let surface = VideoSurface::attach(&ui, video::drawing(), true, move |img| if let Some(ui) = weak.upgrade() { ui.set_video_frame(img.unwrap_or_default()) });
        let (tx, reports) = std::sync::mpsc::channel();
        let mut audio = video::players(mpv.clone(), tx.clone());
        audio.options = Options { vo: "null", ..Options::default() };
        let mut video_players = video::players(mpv, tx);
        if mirror {
            audio.options.ao = "null";
            video_players.options.ao = "null";
        }
        let work = Arc::new(Work { app: app.clone(), renderer, video: video_players, audio, generation: Generation::default(), fonts: Mutex::default() });
        let weak = ui.as_weak();
        let wake = Box::new(move || { let _ = weak.upgrade_in_event_loop(|ui| ui.invoke_pump()); });
        let (events, rx) = std::sync::mpsc::channel();
        let shared = Arc::new(Shared { inbox: Mutex::default(), latest: Mutex::default(), started: Mutex::default(), events, queued: AtomicBool::new(false), closed: AtomicBool::new(false), wake, canvas: Mutex::new((1920, 1080)), role: role.clone(), mask: Mutex::default() });
        let to_shared = shared.clone();
        let jobs = prepare::worker(work.clone(), move |m| to_shared.send(m)).map_err(|e| crate::tr!("Could not open the room screen: {error}", error = e))?;
        let to_live = app.clone();
        let _ = std::thread::Builder::new().name("room-events".into()).spawn(move || for ev in rx { if !mirror { to_live.room_event(ev) } });
        // The faces' opacities and the black overlay, each written straight to this window.
        let fader = Fader::new();
        let op = |set: SetOp| { let w = ui.as_weak(); fader.channel(0.0, move |v| if let Some(ui) = w.upgrade() { set(&ui, v) }) };
        let face_ops: [(Chan, Chan); 5] = std::array::from_fn(|i| { let (_, _, a, b) = setters(i); (op(a), op(b)) });
        let black = op(RoomWindow::set_black);
        let moving: Rc<Cell<Option<Move>>> = Rc::default();
        let motion = { let (w, m) = (ui.as_weak(), moving.clone()); fader.channel(0.0, move |p| if let Some(ui) = w.upgrade() { set_geo(&ui, m.get(), p) }) };
        let sprites: Rc<VecModel<Sprite>> = Rc::default();
        ui.set_sprites(slint::ModelRc::from(sprites.clone()));
        let inner = Rc::new_cyclic(|me: &Weak<Inner>| {
            let me2 = me.clone();
            ui.on_pump(move || if let Some(i) = me2.upgrade() { i.pump() });
            Inner {
                ui: ui.clone_strong(), surface, monitor: monitor.clone(), work, jobs, shared: shared.clone(), state: RefCell::default(),
                prep: RefCell::default(), players: RefCell::default(), deferred: RefCell::default(), fade: Timer::default(),
                faces: RefCell::default(), fader, face_ops, last: RefCell::default(), seeked: Cell::default(), clock: Timer::default(), black, motion, moving, anim: anim::Animator::default(), sprites, on_top: Cell::default(), swap_probe: RefCell::default(),
                fix: Cell::default(), delayed: RefCell::default(), delay_timer: Timer::default(), rec: RefCell::default(), me: me.clone(),
            }
        });
        let room = Rc::new(Room { inner });
        ui.show().map_err(|e| crate::tr!("Could not open the room screen: {error}", error = e))?;
        if let Some(m) = &monitor { monitors::fullscreen_on(&ui, m)? }
        // Subscribe before reading the view so no update falls in between.
        let rx = app.subscribe();
        forward(reports, shared.clone());
        follow(app.clone(), rx, shared);
        *room.inner.shared.canvas.lock().unwrap() = room.inner.canvas();
        if role == "room" { app.set_room_canvas(room.inner.canvas()) }
        let (fix, test) = fix_of(&app, &role);
        room.inner.set_fix(fix, test);
        // N12: off the Slint thread, so the data lock is never waited for there.
        let (to_app, to_shared) = (app.clone(), room.inner.shared.clone());
        let _ = std::thread::Builder::new().name("room-mask".into()).spawn(move || sync_mask(&to_app, &to_shared));
        room.inner.view(&app.live_view());
        Ok(room)
    }

    pub fn window(&self) -> &slint::Window { self.inner.ui.window() }

    /// Keeps the room window above other windows, or lets it go back to normal. Windows hands out paints top
    /// window first, so on top the room's frames are no longer queued behind the operator's (`NativeScreens`
    /// decides when: only full screen on a monitor of its own). Until the window exists nothing happens, and the
    /// next call tries again.
    pub fn set_on_top(&self, on: bool) {
        use slint::winit_030::{WinitWindowAccessor, winit::window::WindowLevel};
        if self.inner.on_top.get() == Some(on) { return }
        let level = if on { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal };
        if self.window().with_winit_window(|w| w.set_window_level(level)).is_some() { self.inner.on_top.set(Some(on)) }
    }

    /// The frame grabber of the player being drawn (the foreground video, else the background video), for the
    /// operator's live preview; `None` while no video is drawn.
    pub fn drawn_grabber(&self) -> Option<FrameGrabber> {
        let players = self.inner.players.borrow();
        players[MEDIA].as_ref().or(players[BG].as_ref()).map(|p| p.grabber())
    }

    /// N22: sends what this window shows (and, when `sink` takes it, the sound its players play) to a recording;
    /// `None` stops.
    pub fn record(&self, sink: Option<Arc<crate::record::FrameSink>>) { self.inner.record(sink) }

    /// The recording this window feeds, if any.
    pub fn recording(&self) -> Option<Arc<crate::record::FrameSink>> { self.inner.recording() }

    /// Diagnostics only (`examples/slide_bench.rs`): the video surface, whose OpenGL probe runs after each frame
    /// (in OpenGL mode it holds the window's rendering notifier).
    #[doc(hidden)]
    pub fn surface(&self) -> &Rc<VideoSurface> { &self.inner.surface }

    /// Diagnostics only (`examples/slide_bench.rs`): `f` runs on the UI thread each time a new view goes on screen
    /// (the moment the crossfade starts), so the bench can time the room's first frame after it.
    #[doc(hidden)]
    pub fn set_swap_probe(&self, f: impl Fn() + 'static) { *self.inner.swap_probe.borrow_mut() = Some(Box::new(f)) }

    #[cfg(test)]
    fn pump(&self) { self.inner.pump() }
}

impl Drop for Room {
    fn drop(&mut self) { self.inner.close() }
}

/// Player reports: straight to the live engine for instances on air, else to the room (preparing).
fn forward(reports: std::sync::mpsc::Receiver<PlayerReport>, shared: Arc<Shared>) {
    let _ = std::thread::Builder::new().name("room-reports".into()).spawn(move || {
        for r in reports {
            if shared.closed.load(Ordering::Relaxed) { return }
            match shared.on_air(&r) { Some(true) => shared.emit(r.into()), Some(false) => {}, None => shared.send(Msg::Report(r)) }
        }
    });
}

/// Draws the phone lines on a thread of its own, only the newest drawing each time, and hands the picture to the
/// UI thread; while ink is fading it draws again by itself (about 30 times a second during a fade). Ends when
/// `follow` (the sender) does.
fn drawer(shared: Arc<Shared>) -> std::sync::mpsc::Sender<Arc<Drawing>> {
    use std::sync::mpsc::RecvTimeoutError;
    let (tx, rx) = std::sync::mpsc::channel::<Arc<Drawing>>();
    let _ = std::thread::Builder::new().name("room-drawing".into()).spawn(move || {
        let mut canvas = DrawingCanvas::new();
        let mut shown: Option<Arc<Drawing>> = None;
        loop {
            let wait = shown.as_ref().and_then(|d| d.next_change(std::time::Instant::now()));
            let mut d = match wait.map_or_else(|| rx.recv().map_err(|_| RecvTimeoutError::Disconnected), |w| rx.recv_timeout(w)) {
                Ok(d) => d,
                Err(RecvTimeoutError::Timeout) => match shown.clone() { Some(d) => d, None => continue },
                Err(RecvTimeoutError::Disconnected) => return,
            };
            while let Ok(newer) = rx.try_recv() { d = newer }
            shown = Some(d.clone());
            let size = *shared.canvas.lock().unwrap();
            let px = canvas.render(&d, size).map(|p| Px::Premul(p.buffer()));
            if shared.closed.load(Ordering::Relaxed) { return }
            shared.send(Msg::Drawing(px));
        }
    });
    tx
}

/// The mask of the audience screen shown as `role`; none for a screen that is not in the setup.
fn mask_of(app: &App, role: &str) -> Mask {
    app.data.lock().unwrap().settings.screens.keyed("audience").into_iter().find(|(k, _)| k == role).map(|(_, s)| s.mask).unwrap_or_default()
}

/// Draws the window's mask again when it or the canvas changed (any thread but Slint's).
fn sync_mask(app: &App, shared: &Shared) {
    let (mask, size) = (mask_of(app, &shared.role), *shared.canvas.lock().unwrap());
    {
        let mut last = shared.mask.lock().unwrap();
        if *last == Some((mask, size)) { return }
        *last = Some((mask, size));
    }
    let px = mask_pixels(&mask, size.0, size.1).map(|p| Px::Premul(slint::SharedPixelBuffer::clone_from_slice(p.data(), size.0, size.1)));
    shared.send(Msg::Mask(px));
}

/// Live updates from a plain thread; only the newest view is applied.
fn follow(app: Arc<App>, mut rx: tokio::sync::broadcast::Receiver<Update>, shared: Arc<Shared>) {
    let sound = move |app: &App| app.sound_out();
    let mut device = sound(&app);
    let mut fix = fix_of(&app, &shared.role);
    let drawings = drawer(shared.clone());
    // The drawing last handed to the drawer. `rev` only grows, so an older one (a live view read before the newest
    // strokes, sent after them) is left out.
    let mut drawn = app.live_view().drawing;
    if !drawn.is_empty() { let _ = drawings.send(drawn.clone()); }
    let _ = std::thread::Builder::new().name("room-updates".into()).spawn(move || loop {
        let mut draw = |d: &Arc<Drawing>| if d.rev > drawn.rev { drawn = d.clone(); let _ = drawings.send(d.clone()); };
        let update = match rx.blocking_recv() {
            Ok(Update::Drawing(d)) => { draw(&d); continue }
            Ok(Update::Live(u)) => u,
            Ok(Update::Data) => {
                sync_mask(&app, &shared);
                let now = sound(&app);
                if now != device {
                    device = now.clone();
                    shared.send(Msg::Device(now));
                }
                let now = fix_of(&app, &shared.role);
                if now != fix {
                    fix = now;
                    shared.send(Msg::Fix(now.0, now.1));
                }
                continue;
            }
            Ok(_) => continue,
            Err(RecvError::Lagged(_)) => LiveUpdate::new(app.live_view()),
            Err(RecvError::Closed) => return,
        };
        if shared.closed.load(Ordering::Relaxed) { return }
        draw(&update.drawing);
        *shared.latest.lock().unwrap() = Some(update);
        shared.wake();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, live::*, model::*, room::Plan};
    use std::time::{Duration, Instant};
    use uuid::Uuid;
    use slint::Model;

    fn add(app: &App, kind: ItemKind) -> Uuid {
        let id = Uuid::new_v4();
        app.data.lock().unwrap().services[0].items.push(ServiceItem { id, kind, background: None, notes: String::new(), theme: None });
        id
    }
    fn media(app: &App, kind: MediaKind, asset: &str) -> Uuid {
        let media_id = Uuid::new_v4();
        app.data.lock().unwrap().media.push(MediaFile { id: media_id, name: asset.into(), kind, asset: asset.into(), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() });
        add(app, ItemKind::Media { media_id, autoplay: true, end: EndAction::Stop, looped: false })
    }
    fn scripture(app: &App) -> Uuid {
        let verses = vec![VerseText::new("John 3:16", "For God so loved the world")];
        add(app, ItemKind::Scripture { reference: "John 3:16".into(), translation: "KJV".into(), verses, per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() })
    }
    fn run(app: &App, cmd: Command) -> u64 {
        let out = app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd });
        assert!(out.ok, "{out:?}");
        out.version
    }
    fn show(app: &App, item_id: Uuid) -> u64 { run(app, Command::Show { item_id, slide: 0 }) }

    /// Pumps the room (and Slint's mock clock, at real speed) until `done` or `secs` pass.
    fn pump_until(room: &Room, secs: f64, done: impl Fn() -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs_f64(secs);
        let mut last = Instant::now();
        while Instant::now() < end {
            room.pump();
            let now = Instant::now();
            i_slint_backend_testing::mock_elapsed_time(now - last);
            last = now;
            if done() { return true }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }
    /// The current face of `kind` as the window has it.
    fn face(room: &Room, kind: Kind) -> Side {
        let i = &room.inner;
        let f = i.faces.borrow()[index(kind).unwrap()].clone();
        let get: fn(&RoomWindow) -> Side = match (kind, f.b_top) {
            (Kind::Background, false) => RoomWindow::get_bg_a, (Kind::Background, true) => RoomWindow::get_bg_b,
            (Kind::Media, false) => RoomWindow::get_media_a, (Kind::Media, true) => RoomWindow::get_media_b,
            (Kind::Slide, false) => RoomWindow::get_slide_a, (Kind::Slide, true) => RoomWindow::get_slide_b,
            (_, false) => RoomWindow::get_logo_a, (_, true) => RoomWindow::get_logo_b,
        };
        let side = get(&i.ui);
        assert_eq!(side.image.size(), f.front().image.size(), "the window has the face Rust put there");
        side
    }
    fn size(room: &Room, kind: Kind) -> (u32, u32) { let s = face(room, kind).image.size(); (s.width, s.height) }
    /// Whether `kind` is shown: its current face is fading in or fully in (the other one is heading out).
    fn shown(room: &Room, kind: Kind) -> bool {
        let i = &room.inner;
        let idx = index(kind).unwrap();
        let f = i.faces.borrow()[idx].clone();
        let (a, b) = i.face_ops[idx];
        let (front, back) = if f.b_top { (b, a) } else { (a, b) };
        assert_eq!(i.fader.target(back), 0.0, "the other face fades out");
        assert_eq!(f.shown, i.fader.target(front) == 1.0);
        f.shown
    }
    fn black(room: &Room) -> f32 { room.inner.fader.target(room.inner.black) }
    fn open(app: &Arc<App>) -> Rc<Room> { Room::open(app.clone(), Renderer::new(), None, None).unwrap() }

    #[test]
    fn a_look_change_fades_for_the_settings_time() {
        let mut v = test_app().live_view();
        let plan = |look_change| Plan { token: 1, version: 1, prepare: vec![], remove: vec![], fade_through_black: false, look_change, transition: None, build: false };
        v.look_fade_ms = 2500;
        assert_eq!(fade_for(&plan(true), &v), Duration::from_millis(2500));
        assert_eq!(fade_for(&plan(false), &v), FADE, "a new slide crossfades as always");
        v.look_fade_ms = 0;
        assert_eq!(fade_for(&plan(true), &v), Duration::ZERO, "0 cuts");
        v.look_fade_ms = 60_000;
        assert_eq!(fade_for(&plan(true), &v), Duration::from_secs(5), "never longer than Settings offers");
    }

    #[test]
    fn a_designed_slide_fades_cuts_or_goes_through_black_for_its_own_time() {
        use crate::design::Transition;
        let v = test_app().live_view();
        let plan = |kind, ms| Plan { token: 1, version: 1, prepare: vec![], remove: vec![], fade_through_black: false, look_change: false, transition: Some(Transition { kind, ms, ..Default::default() }), build: false };
        assert_eq!(fade_for(&plan(TransitionKind::Fade, 1200), &v), Duration::from_millis(1200));
        assert_eq!(fade_for(&plan(TransitionKind::Cut, 1200), &v), Duration::ZERO);
        assert_eq!(half_for(&plan(TransitionKind::Black, 1200)), Duration::from_millis(600));
        assert_eq!(half_for(&plan(TransitionKind::Fade, 1200)), HALF);
    }

    #[test]
    fn push_and_wipe_move_the_faces_the_way_they_go() {
        let g = |x: f32, y: f32, l: f32, r: f32, t: f32, b: f32| Geo { x, y, l, r, t, b, ..Default::default() };
        let z = Geo::default();
        // Push left: the new slide comes in from the right, the old one goes out to the left.
        assert_eq!(geo(TransitionKind::Push, Dir::Left, 0.0, true), g(1.0, 0.0, 0.0, 0.0, 0.0, 0.0));
        assert_eq!(geo(TransitionKind::Push, Dir::Left, 0.25, false), g(-0.25, 0.0, 0.0, 0.0, 0.0, 0.0));
        assert_eq!(geo(TransitionKind::Push, Dir::Up, 0.5, true), g(0.0, 0.5, 0.0, 0.0, 0.0, 0.0), "up: in from below");
        assert_eq!(geo(TransitionKind::Push, Dir::Down, 1.0, true), z);
        // Wipe right: the edge moves right, the new slide showing to the left of it.
        assert_eq!(geo(TransitionKind::Wipe, Dir::Right, 0.25, true), g(0.0, 0.0, 0.0, 0.75, 0.0, 0.0));
        assert_eq!(geo(TransitionKind::Wipe, Dir::Right, 0.25, false), g(0.0, 0.0, 0.25, 0.0, 0.0, 0.0));
        assert_eq!(geo(TransitionKind::Wipe, Dir::Left, 0.25, true), g(0.0, 0.0, 0.75, 0.0, 0.0, 0.0));
        assert_eq!(geo(TransitionKind::Wipe, Dir::Down, 0.5, true), g(0.0, 0.0, 0.0, 0.0, 0.0, 0.5));
        assert_eq!(geo(TransitionKind::Wipe, Dir::Up, 0.5, false), g(0.0, 0.0, 0.0, 0.0, 0.0, 0.5));
        assert_eq!(geo(TransitionKind::Fade, Dir::Up, 0.5, false), z);
        let v = test_app().live_view();
        let plan = |kind| Plan { token: 1, version: 1, prepare: vec![], remove: vec![], fade_through_black: false, look_change: false, transition: Some(Transition { kind, ms: 900, dir: Dir::Up }), build: false };
        assert_eq!(fade_for(&plan(TransitionKind::Push), &v), Duration::from_millis(900), "the other layers fade as it moves");
        assert_eq!(moving(plan(TransitionKind::Wipe).transition).map(|t| t.kind), Some(TransitionKind::Wipe));
        assert_eq!(moving(plan(TransitionKind::Fade).transition), None);
    }

    #[test]
    fn cover_uncover_split_and_zoom_move_the_faces() {
        let g = |x: f32, y: f32, l: f32, r: f32, t: f32, b: f32| Geo { x, y, l, r, t, b, ..Default::default() };
        // Cover left: the new slide comes in from the right; the old one is cut where it is.
        assert_eq!(geo(TransitionKind::Cover, Dir::Left, 0.25, true), g(0.75, 0.0, 0.0, 0.0, 0.0, 0.0));
        assert_eq!(geo(TransitionKind::Cover, Dir::Left, 0.25, false), g(0.0, 0.0, 0.0, 0.25, 0.0, 0.0));
        // Uncover left: the old slide goes out to the left; the new one shows where it was.
        assert_eq!(geo(TransitionKind::Uncover, Dir::Left, 0.25, false), g(-0.25, 0.0, 0.0, 0.0, 0.0, 0.0));
        assert_eq!(geo(TransitionKind::Uncover, Dir::Left, 0.25, true), g(0.0, 0.0, 0.75, 0.0, 0.0, 0.0));
        let split = geo(TransitionKind::Split, Dir::Up, 0.5, true);
        assert_eq!((split.t, split.b, split.l, split.over), (0.25, 0.25, 0.0, true));
        let zoom = geo(TransitionKind::Zoom, Dir::Left, 0.25, true);
        assert_eq!((zoom.z, zoom.over), (0.75, true));
        assert_eq!(geo(TransitionKind::Zoom, Dir::Left, 0.25, false), Geo::default());
        assert!(TransitionKind::Zoom.moves() && !TransitionKind::Zoom.has_dir() && !TransitionKind::Dissolve.moves());
        assert_eq!(serde_json::from_str::<TransitionKind>("\"morph\"").unwrap(), TransitionKind::Dissolve, "a newer kind plays as a fade");
    }

    #[test]
    fn a_pushed_slide_moves_in_and_the_old_one_goes_after() {
        i_slint_backend_testing::init_no_event_loop();
        let room = open(&test_app());
        let (i, ui) = (&room.inner, &room.inner.ui);
        let side = || Side { image: Default::default(), color: slint::Color::from_argb_u8(255, 1, 2, 3) };
        i.put(Kind::Slide, side(), Duration::ZERO);
        let slot = index(Kind::Slide).unwrap();
        let (ca, cb) = i.face_ops[slot];
        i.put_moving(side(), Transition { kind: TransitionKind::Push, ms: 300, dir: Dir::Left });
        // The new face (b) is up at once, starting off to the right; the old one stays up as it goes.
        assert_eq!((i.fader.target(ca), i.fader.target(cb)), (1.0, 1.0));
        assert!(ui.get_slide_b_geo().x > 0.95 && ui.get_slide_a_geo().x.abs() < 0.05, "{:?}", (ui.get_slide_b_geo(), ui.get_slide_a_geo()));
        assert!(pump_until(&room, 3.0, || i.moving.get().is_none() && !i.fader.running()));
        assert_eq!((ui.get_slide_a_geo(), ui.get_slide_b_geo()), (Geo::default(), Geo::default()));
        assert_eq!((i.fader.value(ca), i.fader.value(cb)), (0.0, 1.0));
        // A slide that crossfades in mid-wipe stops the wipe: both faces back in place.
        i.put_moving(side(), Transition { kind: TransitionKind::Wipe, ms: 2000, dir: Dir::Right });
        assert!(ui.get_slide_a_geo().r > 0.9, "the new face (a) starts cut away");
        i.put(Kind::Slide, side(), FADE);
        assert_eq!((i.moving.get(), ui.get_slide_a_geo()), (None, Geo::default()));
        assert!(shown(&room, Kind::Slide));
    }

    #[test]
    fn a_build_fades_the_new_face_in_over_the_old_held_at_full() {
        i_slint_backend_testing::init_no_event_loop();
        let room = open(&test_app());
        let i = &room.inner;
        let side = || Side { image: Default::default(), color: slint::Color::from_argb_u8(255, 1, 2, 3) };
        i.put(Kind::Slide, side(), FADE);
        let slot = index(Kind::Slide).unwrap();
        let (ca, cb) = i.face_ops[slot];
        i.put_over(Kind::Slide, side(), FADE);
        // The new face is b (on top): it fades in while a stays up.
        assert!(i.faces.borrow()[slot].b_top);
        assert_eq!((i.fader.target(ca), i.fader.target(cb)), (1.0, 1.0));
        i.put_over(Kind::Slide, side(), FADE);
        // The new face is a (under): it is up at once and b fades out over it.
        assert_eq!((i.fader.target(ca), i.fader.target(cb)), (1.0, 0.0));
    }

    #[test]
    fn text_slide_sets_slide_image_and_commits() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let v = show(&app, scripture(&app));
        let room = open(&app);
        let ui = &room.inner.ui;
        assert!(pump_until(&room, 15.0, || size(&room, Kind::Slide).0 > 0 && app.live_view().committed == v), "slide {:?}, committed {}", size(&room, Kind::Slide), app.live_view().committed);
        assert!(shown(&room, Kind::Slide));
        // Drawn at the room's size, not scaled up from a thumbnail.
        assert!(size(&room, Kind::Slide).0 >= 1280, "{:?}", size(&room, Kind::Slide));
        assert!(shown(&room, Kind::Background), "the theme background is up");
        assert!(!shown(&room, Kind::Media) && !shown(&room, Kind::Logo) && !ui.get_video());
        // The crossfade runs on the room's own timer and ends fully in.
        assert!(pump_until(&room, 2.0, || ui.get_slide_a_op() + ui.get_slide_b_op() == 1.0 && !room.inner.fader.running()));
    }

    #[test]
    fn a_click_plays_its_animation_over_the_slide_then_the_slide_takes_over() {
        use crate::design::{Anim, AnimKind, Design, Effect, Obj, ObjKind, PctRect, Shape, Start};
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let shape = |x| { let mut o = Obj::new(ObjKind::Shape { shape: Shape::Rect }, PctRect::new(x, 10.0, 20.0, 20.0)); o.look.fill = Some("#ff0000".into()); o };
        let (a, b) = (shape(10.0), shape(60.0));
        let mut fly = Anim::new(b.id, AnimKind::Entrance, Effect::Fly, Start::OnClick);
        fly.dur_ms = 600;
        let id = add(&app, ItemKind::Design { title: "Talk".into(), slides: vec![Design { objects: vec![a, b], anims: vec![fly], ..Default::default() }] });
        let v = show(&app, id);
        let room = open(&app);
        let ui = &room.inner.ui;
        assert!(pump_until(&room, 15.0, || app.live_view().committed == v && !room.inner.fader.running()));
        assert_eq!(ui.get_sprites().row_count(), 0, "nothing plays as the slide comes up");
        let step = |cmd| { let out = app.command(Envelope { id: Uuid::new_v4().to_string(), base: Some(app.live_view().version), cmd }); assert!(out.ok, "{out:?}"); out.version };
        let v = step(Command::Next);
        // Both of them: on the first frame the flying shape can still be off the screen, with no picture of its own yet.
        // The room reports the click as shown a moment after its pictures go up, so wait for both.
        assert!(pump_until(&room, 15.0, || ui.get_sprites().row_count() == 2 && app.live_view().committed == v), "the click's pictures go up");
        assert_eq!(app.live_view().committed, v);
        let rows: Vec<_> = (0..ui.get_sprites().row_count()).filter_map(|r| ui.get_sprites().row_data(r)).collect();
        assert_eq!(rows.len(), 2, "the still shape and the flying one");
        assert_eq!(ui.get_slide_a_op() + ui.get_slide_b_op(), 0.0, "the slide layer waits under them");
        assert!(pump_until(&room, 5.0, || ui.get_sprites().row_count() == 0), "they go once it has played");
        assert_eq!(ui.get_slide_a_op() + ui.get_slide_b_op(), 1.0);
        // Back: straight to the slide as it was, nothing plays.
        step(Command::Prev);
        assert!(pump_until(&room, 15.0, || room.inner.prep.borrow().is_none() && app.live_view().frame.slide.is_some() && ui.get_slide_a_op() + ui.get_slide_b_op() == 1.0));
        assert_eq!(ui.get_sprites().row_count(), 0);
    }

    #[test]
    fn group_0_comes_in_with_the_slides_transition_then_plays() {
        use crate::design::{Anim, AnimKind, Design, Effect, Obj, ObjKind, PctRect, Shape, Start, Transition, TransitionKind};
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let shape = |x| { let mut o = Obj::new(ObjKind::Shape { shape: Shape::Rect }, PctRect::new(x, 10.0, 20.0, 20.0)); o.look.fill = Some("#ff0000".into()); o };
        let (a, b) = (shape(10.0), shape(60.0));
        let mut fade = Anim::new(b.id, AnimKind::Entrance, Effect::Fade, Start::WithPrevious);
        fade.dur_ms = 600;
        let transition = Transition { kind: TransitionKind::Fade, ms: 800, ..Default::default() };
        let id = add(&app, ItemKind::Design { title: "Talk".into(), slides: vec![Design { objects: vec![a, b], anims: vec![fade], transition, ..Default::default() }] });
        let room = open(&app);
        let ui = &room.inner.ui;
        show(&app, id);
        assert!(pump_until(&room, 15.0, || ui.get_sprites().row_count() == 2), "group 0 plays as the slide comes up");
        let row = |r| ui.get_sprites().row_data(r).unwrap();
        assert_eq!(ui.get_sprites().row_count(), 2);
        assert!(row(0).op < 1.0, "the still shape comes in with the transition, not a cut: {}", row(0).op);
        assert_eq!(row(1).op, 0.0, "the fading shape waits for the transition");
        assert!(pump_until(&room, 5.0, || ui.get_sprites().row_count() == 0), "they go once it has played");
        assert_eq!(ui.get_slide_a_op() + ui.get_slide_b_op(), 1.0);
    }

    /// The room's screen in the setup, changed by `f`.
    fn fix_room(app: &App, f: impl FnOnce(&mut Screen)) {
        app.edit(|d| { d.settings.screens.assign("audience", ScreenTarget::Window); f(&mut d.settings.screens.audience.screens[0]); Ok(()) }).unwrap();
        app.broadcast_data();
    }

    #[test]
    fn a_screen_delay_holds_changes_back() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        fix_room(&app, |s| s.fix.delay = 30); // 480 ms
        let room = open(&app);
        let item = scripture(&app);
        let started = Instant::now();
        let v = show(&app, item);
        pump_until(&room, 0.25, || false);
        assert!(app.live_view().committed < v, "still waiting");
        assert!(pump_until(&room, 15.0, || app.live_view().committed == v));
        assert!(started.elapsed() >= Duration::from_millis(480));
        // No delay: the next change is not held.
        fix_room(&app, |s| s.fix.delay = 0);
        assert!(pump_until(&room, 5.0, || room.inner.fix.get().0.delay == 0));
        let v = run(&app, Command::ClearSlide);
        let started = Instant::now();
        assert!(pump_until(&room, 15.0, || app.live_view().committed == v));
        assert!(started.elapsed() < Duration::from_millis(480), "{:?}", started.elapsed());
    }

    #[test]
    fn a_test_pattern_covers_the_room_and_follows_the_setup() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let room = open(&app);
        let ui = &room.inner.ui;
        assert!(!ui.get_testing());
        fix_room(&app, |s| { s.test = crate::screen_fix::TestPattern::Bars; s.fix.red = 0.8 });
        assert!(pump_until(&room, 5.0, || ui.get_testing()), "the pattern comes up");
        assert!(ui.get_test_pattern().size().width > 0);
        assert_eq!(room.inner.surface.fix().red, 0.8, "the colour fix goes to the picture");
        app.edit_untracked(|d| { d.settings.screens.clear_tests(); Ok(()) }).unwrap();
        app.broadcast_data();
        assert!(pump_until(&room, 5.0, || !ui.get_testing()), "and goes again");
    }

    #[test]
    fn a_phone_drawing_appears_over_the_slide_and_goes_with_it() {
        use crate::live::drawing::{DrawCmd, Pen, PALETTE};
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let phone = Uuid::new_v4();
        app.phones.lock().unwrap().push(crate::app::Phone { id: phone, name: "iPad".into(), hash: String::new(), last_seen: 0, may_draw: true });
        let item = scripture(&app);
        show(&app, item);
        let room = open(&app);
        let ui = &room.inner.ui;
        let line = DrawCmd::Start { stroke: 1, color: PALETTE[1].into(), width: 20.0, pen: Pen::Pen, points: vec![[0.25, 0.5], [0.75, 0.5]], ink: Default::default() };
        app.draw(phone, &line).unwrap();
        assert!(pump_until(&room, 10.0, || ui.get_drawing().size().width > 0), "the drawing layer got a picture");
        let img = ui.get_drawing();
        let (w, h) = (img.size().width, img.size().height);
        assert!(w >= 1280, "drawn at the room's size: {w}");
        let px = img.to_rgba8_premultiplied().or_else(|| img.to_rgba8()).expect("pixels");
        let at = |x: u32, y: u32| px.as_slice()[(y * w + x) as usize];
        let mid = at(w / 2, h / 2);
        assert_eq!((mid.r, mid.g, mid.b, mid.a), (0xff, 0x3b, 0x30, 255), "a red line across the middle");
        assert_eq!(at(w / 2, h / 4).a, 0, "nothing above it");
        assert_eq!(at(w / 10, h / 2).a, 0, "nor before its start");
        // Clear Text takes it away with the words.
        run(&app, Command::ClearSlide);
        assert!(pump_until(&room, 10.0, || ui.get_drawing().size().width == 0), "gone with the text");
    }

    #[test]
    fn a_changed_sound_output_reaches_the_room_once() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let room = open(&app);
        let sent = || room.inner.shared.inbox.lock().unwrap().iter().filter(|m| matches!(m, Msg::Device(d) if d.device == "wasapi/{b}")).count();
        app.data.lock().unwrap().settings.audio_device = "wasapi/{b}".into();
        app.broadcast_data();
        let end = Instant::now() + Duration::from_secs(5);
        while sent() == 0 && Instant::now() < end { std::thread::sleep(Duration::from_millis(5)); }
        assert_eq!(sent(), 1);
        app.broadcast_data();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(sent(), 1, "the same output again is not news");
        // Listen (N21): the headphones, with no delay or routing; off again: the main output as it was.
        app.data.lock().unwrap().settings.media = crate::media_prefs::MediaPrefs { listen_device: "wasapi/{phones}".into(), delay_ms: 80, ..Default::default() };
        app.listening.store(true, Ordering::Relaxed);
        app.broadcast_data();
        let phones = || room.inner.shared.inbox.lock().unwrap().iter().any(|m| matches!(m, Msg::Device(d) if d.device == "wasapi/{phones}" && d.delay == 0.0));
        let end = Instant::now() + Duration::from_secs(5);
        while !phones() && Instant::now() < end { std::thread::sleep(Duration::from_millis(5)); }
        assert!(phones());
        app.listening.store(false, Ordering::Relaxed);
        app.broadcast_data();
        let end = Instant::now() + Duration::from_secs(5);
        while sent() < 2 && Instant::now() < end { std::thread::sleep(Duration::from_millis(5)); }
        assert_eq!(sent(), 2, "back to the main output, now with its delay");
    }

    #[test]
    fn media_fit_follows_settings() {
        assert_eq!([Scale::Fit, Scale::Fill, Scale::Stretch].map(media_fit), [0, 1, 2]);
    }

    #[test]
    fn closing_drains_the_inbox_and_stops_queueing() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let room = open(&app);
        let report = || PlayerReport { inst: 99, player: 1, state: PlayState::Playing, position: 0.0, duration: 0.0, error: None };
        room.inner.shared.inbox.lock().unwrap().push_back(Msg::Report(report()));
        let shared = room.inner.shared.clone();
        drop(room);
        assert!(shared.inbox.lock().unwrap().is_empty(), "close drains the inbox");
        shared.send(Msg::Report(report()));
        assert!(shared.inbox.lock().unwrap().is_empty(), "nothing is queued once closed");
    }

    fn prep(plan: Plan, frame: Frame) -> Prep { Prep { plan, frame, pause: vec![], waiting: vec![], got: vec![], heard: Default::default(), fade: FADE } }

    #[test]
    fn a_failure_while_fading_through_black_commits_the_swap_first() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let (one, two, three) = (scripture(&app), scripture(&app), scripture(&app));
        let v = show(&app, one);
        let room = open(&app);
        assert!(pump_until(&room, 15.0, || app.live_view().committed == v));
        let i = &room.inner;
        // `two` swaps in through black by hand, so its commit waits for the screen to turn black.
        show(&app, two);
        let view = app.live_view();
        let mut plan = i.state.borrow_mut().plan(&view).unwrap();
        plan.fade_through_black = true;
        let Ready::Swap { commit } = i.state.borrow_mut().ready(plan.token) else { panic!("stale") };
        i.swap(prep(plan, view.frame.clone()), commit);
        // `three` fails meanwhile: the engine goes back to what the room shows (`two`), not `one`.
        show(&app, three);
        let plan = i.state.borrow_mut().plan(&app.live_view()).unwrap();
        let token = plan.token;
        *i.prep.borrow_mut() = Some(prep(plan, Frame::default()));
        i.fail(token, "it broke".into());
        // Only the commit went early: the layers still wait for the screen to be black.
        assert!(i.deferred.borrow().as_ref().is_some_and(|d| d.1.is_none()) && black(&room) == 1.0, "the swap is shown before black");
        assert!(pump_until(&room, 5.0, || app.live_view().notice.is_some()));
        assert_eq!(app.live_view().item.map(|s| s.item_id), Some(two));
        i.flush();
        assert!(i.deferred.borrow().is_none() && black(&room) == 0.0);
    }

    #[test]
    fn each_fade_through_black_waits_its_own_half() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let room = open(&app);
        let i = &room.inner;
        let swap = |token| i.swap(prep(Plan { token, version: 0, prepare: vec![], remove: vec![], fade_through_black: true, look_change: false, transition: None, build: false }, Frame::default()), RoomEvent::Committed { version: 0 });
        swap(100);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(150));
        swap(101);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100)); // the first half is over
        assert!(i.deferred.borrow().is_some() && black(&room) == 1.0, "the second swap is applied before the screen is black");
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(150));
        assert!(i.deferred.borrow().is_none() && black(&room) == 0.0);
    }

    #[test]
    fn background_reports_stay_in_the_room_unless_they_fail() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let v = show(&app, scripture(&app));
        let room = open(&app);
        assert!(pump_until(&room, 15.0, || app.live_view().committed == v));
        let inst = app.live_view().frame.background.expect("the theme background").inst;
        room.inner.shared.started.lock().unwrap().insert(inst, Kind::Background);
        let mut rx = app.subscribe();
        // The commit's own update can still be on its way when `committed` already reads `v`.
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}
        let report = |state, error| PlayerReport { inst, player: 1, state, position: 1.0, duration: 9.0, error };
        room.inner.report(report(PlayState::Playing, None));
        std::thread::sleep(Duration::from_millis(300));
        assert!(rx.try_recv().is_err(), "a playing background is not news to the live engine");
        room.inner.report(report(PlayState::Error, Some("it broke".into())));
        assert!(pump_until(&room, 5.0, || app.live_view().notice.is_some()));
    }

    #[test]
    fn a_slow_save_does_not_stall_the_room() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let v = show(&app, scripture(&app));
        let room = open(&app);
        // Everything is prepared but not pumped yet.
        let wanted = room.inner.prep.borrow().as_ref().map_or(0, |p| p.waiting.len());
        let end = Instant::now() + Duration::from_secs(15);
        while room.inner.shared.inbox.lock().unwrap().len() < wanted && Instant::now() < end { std::thread::sleep(Duration::from_millis(5)) }
        let ((tx, locked), held) = (std::sync::mpsc::channel(), app.clone());
        let saver = std::thread::spawn(move || { let _data = held.data.lock().unwrap(); tx.send(()).unwrap(); std::thread::sleep(Duration::from_secs(1)) });
        locked.recv().unwrap();
        let t = Instant::now();
        room.pump(); // swaps and commits
        assert!(t.elapsed() < Duration::from_millis(500), "the room waited {:?} for the data lock", t.elapsed());
        saver.join().unwrap();
        assert!(pump_until(&room, 5.0, || app.live_view().committed == v));
    }

    #[test]
    fn clear_all_hides_layers_and_commits() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let v = show(&app, scripture(&app));
        let room = open(&app);
        assert!(pump_until(&room, 15.0, || app.live_view().committed == v));
        let v = run(&app, Command::ClearAll);
        assert!(pump_until(&room, 5.0, || app.live_view().committed == v));
        assert!(LAYERS.iter().all(|k| !shown(&room, *k)));
        let ui = &room.inner.ui;
        assert!(pump_until(&room, 2.0, || ui.get_slide_a_op() + ui.get_slide_b_op() + ui.get_bg_a_op() + ui.get_bg_b_op() == 0.0), "faded out");
    }

    #[test]
    fn missing_still_asset_reports_failed_and_keeps_previous() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let text = scripture(&app);
        let still = media(&app, MediaKind::Image, "missing.png");
        let v = show(&app, text);
        let room = open(&app);
        assert!(pump_until(&room, 15.0, || app.live_view().committed == v));
        let before = size(&room, Kind::Slide);
        show(&app, still);
        assert!(pump_until(&room, 5.0, || app.live_view().notice.is_some()), "no failure reported");
        let notice = app.live_view().notice.unwrap();
        assert!(notice.contains("picture could not be loaded"), "{notice}");
        // The engine goes back to what is on air, and the room commits that without changes.
        assert!(pump_until(&room, 5.0, || { let l = app.live_view(); !l.pending && l.committed > v }));
        assert!(app.live_view().frame.slide.is_some());
        assert!(shown(&room, Kind::Slide) && size(&room, Kind::Slide) == before);
        assert!(!shown(&room, Kind::Media));
    }

    #[test]
    fn logo_layer_follows_frame_logo() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        image::RgbaImage::from_pixel(64, 32, image::Rgba([200, 0, 0, 255])).save(app.store.asset_path("logo.png")).unwrap();
        app.data.lock().unwrap().settings.logo = Some("logo.png".into());
        let room = open(&app);
        let v = run(&app, Command::Logo);
        assert!(pump_until(&room, 5.0, || app.live_view().committed == v));
        assert!(shown(&room, Kind::Logo));
        assert_eq!(size(&room, Kind::Logo), (64, 32));
        let v = run(&app, Command::ClearAll);
        assert!(pump_until(&room, 5.0, || app.live_view().committed == v));
        assert!(!shown(&room, Kind::Logo));
    }

    #[test]
    fn each_audience_screen_follows_its_own_mask() {
        use crate::render::mask::{Mask, MaskShape};
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let second = {
            let mut d = app.data.lock().unwrap();
            let s = &mut d.settings.screens.audience;
            s.screens[0].output_mut().target = ScreenTarget::Window;
            let mut other = Screen::new(Uuid::new_v4(), "Lobby TV");
            other.output_mut().target = ScreenTarget::Window;
            other.mask = Mask { shape: MaskShape::Ellipse, inset: 0, radius: 0 };
            s.screens.push(other.clone());
            other.id
        };
        let room = open(&app);
        let lobby = Room::open_mirror(app.clone(), Renderer::new(), None, None, &format!("room:{second}")).unwrap();
        let has = |r: &Room| r.inner.ui.get_mask().size().width > 0;
        assert!(pump_until(&lobby, 5.0, || has(&lobby)), "the lobby screen is cut to an oval");
        let px = lobby.inner.ui.get_mask().to_rgba8_premultiplied().unwrap();
        assert_eq!(px.as_slice()[0].a, 255, "the corner is black");
        assert_eq!(px.as_slice()[(px.height() / 2 * px.width() + px.width() / 2) as usize].a, 0, "the middle shows");
        pump_until(&room, 0.3, || false);
        assert!(!has(&room), "the main screen has no mask");
        // A change in the setup reaches the open window.
        app.edit(|d| { d.settings.screens.audience.screens[0].mask = Mask { shape: MaskShape::Inset, inset: 5, radius: 0 }; Ok::<_, String>(()) }).unwrap();
        app.broadcast_data();
        assert!(pump_until(&room, 5.0, || has(&room)), "the new mask shows");
        app.edit(|d| { d.settings.screens.audience.screens[0].mask = Mask::default(); Ok::<_, String>(()) }).unwrap();
        app.broadcast_data();
        assert!(pump_until(&room, 5.0, || !has(&room)), "and goes again");
    }

    #[test]
    fn missing_libmpv_fails_audio_with_the_reinstall_hint() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let audio = media(&app, MediaKind::Audio, "hymn.wav");
        let room = open(&app);
        show(&app, audio);
        assert!(pump_until(&room, 5.0, || app.live_view().notice.is_some()));
        let notice = app.live_view().notice.unwrap();
        assert!(notice.contains(&crate::video::ffi::missing()), "{notice}");
        assert!(app.live_view().frame.audio.is_none());
    }

    #[test]
    fn audio_item_plays_after_commit_and_reports_by_instance() {
        if std::env::var("RP_VIDEO_TESTS").as_deref() != Ok("1") { return }
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        crate::video::silent_wav(&app.store.asset_path("hush.wav"), 1000);
        let audio = media(&app, MediaKind::Audio, "hush.wav");
        let mpv = crate::video::ffi::Mpv::load().expect("libmpv (set RP_LIBMPV)");
        let room = Room::open(app.clone(), Renderer::new(), Some(mpv), None).unwrap();
        let v = show(&app, audio);
        let inst = app.live_view().frame.audio.as_ref().unwrap().inst;
        assert!(pump_until(&room, 10.0, || app.live_view().committed == v), "notice {:?}", app.live_view().notice);
        let playing = || app.live_view().audio.is_some_and(|s| s.inst == inst && s.state == PlayState::Playing);
        assert!(pump_until(&room, 5.0, playing), "{:?}", app.live_view().audio);
        // Stopping retires the instance: the player is paused at once and gone after the fade.
        let v = run(&app, Command::ClearAudio);
        assert!(pump_until(&room, 5.0, || app.live_view().committed == v));
        assert!(room.inner.players.borrow()[AUDIO].as_ref().is_none());
    }

    #[test]
    fn video_item_fades_through_black_and_is_drawn() {
        if std::env::var("RP_VIDEO_TESTS").as_deref() != Ok("1") { return }
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        // mpv plays an animated GIF as video: 3 s of frames changing colour.
        let frames = (0..30u8).map(|i| {
            let img = image::RgbaImage::from_pixel(64, 36, image::Rgba([i * 8, 255 - i * 8, 90, 255]));
            image::Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(100, 1))
        });
        let file = std::fs::File::create(app.store.asset_path("clip.gif")).unwrap();
        image::codecs::gif::GifEncoder::new(file).encode_frames(frames).unwrap();
        let clip = media(&app, MediaKind::Video, "clip.gif");
        let text = scripture(&app);
        let mpv = crate::video::ffi::Mpv::load().expect("libmpv (set RP_LIBMPV)");
        let room = Room::open(app.clone(), Renderer::new(), Some(mpv), None).unwrap();
        let ui = &room.inner.ui;
        let v = show(&app, clip);
        let inst = app.live_view().frame.media.as_ref().unwrap().inst;
        assert!(pump_until(&room, 10.0, || black(&room) > 0.5 || app.live_view().committed == v), "notice {:?}", app.live_view().notice);
        assert!(black(&room) > 0.5, "starting a video fades through black");
        assert!(pump_until(&room, 5.0, || app.live_view().committed == v));
        assert_eq!(black(&room), 0.0);
        assert!(pump_until(&room, 2.0, || ui.get_black() == 0.0), "the black overlay fades away");
        assert!(ui.get_video() && !ui.get_bg_visible());
        assert!(pump_until(&room, 5.0, || ui.get_video_frame().size().width > 0), "software frames reach the window");
        let playing = || app.live_view().media.is_some_and(|s| s.inst == inst && s.state == PlayState::Playing);
        assert!(pump_until(&room, 5.0, playing), "{:?}", app.live_view().media);
        // The operator's live preview takes its frames from the drawn player.
        let grabber = room.drawn_grabber().expect("the drawn video's grabber");
        assert!(pump_until(&room, 3.0, || grabber.grab(64).is_some()), "the preview gets a frame");
        // Moving on retires the instance: off air at once, so its reports stop reaching the engine.
        let v = show(&app, text);
        assert!(pump_until(&room, 10.0, || app.live_view().committed == v));
        assert!(room.inner.players.borrow()[MEDIA].is_none() && !ui.get_video() && ui.get_bg_visible());
        assert!(room.drawn_grabber().is_none(), "no video drawn, nothing for the preview");
        assert!(!room.inner.shared.started.lock().unwrap().contains_key(&inst));
        assert!(pump_until(&room, 2.0, || ui.get_video_frame().size().width == 0), "the last frame is cleared");
    }

    #[test]
    fn a_disposed_players_late_report_does_not_ready_its_successor() {
        if std::env::var("RP_VIDEO_TESTS").as_deref() != Ok("1") { return }
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let file = std::fs::File::create(app.store.asset_path("still.gif")).unwrap();
        let frame = image::Frame::new(image::RgbaImage::from_pixel(64, 36, image::Rgba([0, 90, 200, 255])));
        image::codecs::gif::GifEncoder::new(file).encode_frames([frame]).unwrap();
        let clip = media(&app, MediaKind::Video, "still.gif");
        let mpv = crate::video::ffi::Mpv::load().expect("libmpv (set RP_LIBMPV)");
        let room = Room::open(app.clone(), Renderer::new(), Some(mpv), None).unwrap();
        let i = &room.inner;
        let wait = |what: &str, f: &dyn Fn() -> bool| {
            let end = Instant::now() + Duration::from_secs(10);
            while !f() && Instant::now() < end { std::thread::sleep(Duration::from_millis(5)) }
            assert!(f(), "{what}");
        };
        let queued = || i.shared.latest.lock().unwrap().is_some();
        let made = || i.shared.inbox.lock().unwrap().iter().any(|m| matches!(m, Msg::Made { made: Ok(Made::Player(_)), .. }));
        let player = || i.prep.borrow().as_ref().and_then(|p| p.got.iter().find_map(|(_, g)| match g { Got::Play(p) => Some(p.serial()), _ => None }));
        show(&app, clip);
        let inst = app.live_view().frame.media.as_ref().unwrap().inst;
        wait("the view is queued", &queued);
        room.pump();
        wait("the first player is made", &made);
        room.pump();
        let first = player().expect("the first player is preparing");
        // Pausing before the commit re-prepares the instance with a new player.
        let v = run(&app, Command::MediaPause { inst, paused: true });
        wait("the pause is queued", &queued);
        room.pump();
        i.report(PlayerReport { inst, player: first, state: PlayState::Paused, position: 0.0, duration: 1.0, error: None });
        wait("the second player is made", &made);
        room.pump();
        assert_ne!(player(), Some(first));
        assert!(i.prep.borrow().as_ref().is_some_and(|p| p.waiting.contains(&Kind::Media)), "the new player is ready before it has loaded");
        assert!(pump_until(&room, 10.0, || app.live_view().committed == v), "notice {:?}", app.live_view().notice);
    }

    #[test]
    fn dropping_with_a_video_queued_frees_everything() {
        if std::env::var("RP_VIDEO_TESTS").as_deref() != Ok("1") { return }
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let file = std::fs::File::create(app.store.asset_path("still.gif")).unwrap();
        let frame = image::Frame::new(image::RgbaImage::from_pixel(64, 36, image::Rgba([0, 90, 200, 255])));
        image::codecs::gif::GifEncoder::new(file).encode_frames([frame]).unwrap();
        let clip = media(&app, MediaKind::Video, "still.gif");
        let mpv = crate::video::ffi::Mpv::load().expect("libmpv (set RP_LIBMPV)");
        let room = Room::open(app.clone(), Renderer::new(), Some(mpv), None).unwrap();
        show(&app, clip);
        let queued = || room.inner.shared.latest.lock().unwrap().is_some();
        let end = Instant::now() + Duration::from_secs(5);
        while !queued() && Instant::now() < end { std::thread::sleep(Duration::from_millis(5)) }
        room.pump(); // preparing starts; the player is queued but never pumped
        let made = || room.inner.shared.inbox.lock().unwrap().iter().any(|m| matches!(m, Msg::Made { made: Ok(Made::Player(_)), .. }));
        let end = Instant::now() + Duration::from_secs(10);
        while !made() && Instant::now() < end { std::thread::sleep(Duration::from_millis(5)) }
        assert!(made(), "the prepared player is waiting in the inbox");
        let weak = Arc::downgrade(&room.inner.shared);
        drop(room);
        run(&app, Command::ClearAll); // lets the update thread see `closed`
        let end = Instant::now() + Duration::from_secs(5);
        while weak.strong_count() > 0 && Instant::now() < end { std::thread::sleep(Duration::from_millis(10)) }
        assert_eq!(weak.strong_count(), 0, "the room's threads and the queued player are freed");
    }
}
