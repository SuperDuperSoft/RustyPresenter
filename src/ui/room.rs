//! The room output window: each new live view is prepared off the UI thread (`prepare`), swapped in
//! with a crossfade once every new layer is ready, then reported as committed (as `web/output.js`).
/// Shared with the operator's live monitor (`ui::preview`), which draws the same layers small.
pub mod prepare;

use super::{Layer, RoomWindow, Side};
use crate::app::{App, LiveUpdate, LiveView, Update};
use crate::live::{Frame, MediaLayer, PlayState, RoomEvent};
use crate::platform::monitors::{self, MonitorId};
use crate::render::Renderer;
use crate::room::{self, Kind, LayerKey, Plan, Ready, RoomState};
use crate::video::{self, FrameGrabber, Options, Player, PlayerReport, VideoSurface, ffi::Mpv};
use prepare::{Fresh, Job, Made, Px, Work};
use slint::{ComponentHandle, Timer, TimerMode};
use std::{cell::RefCell, collections::{HashMap, VecDeque}, rc::{Rc, Weak}, time::Duration};
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

/// The playback instance a layer key names (only players report).
fn inst(key: &LayerKey) -> Option<u64> {
    match key { LayerKey::Background(i) | LayerKey::Media(i) | LayerKey::Audio(i) => Some(*i), _ => None }
}

/// How a layer is named in "… took too long to load".
fn what(kind: Kind, f: &Frame) -> &'static str {
    match kind {
        Kind::Background => "background",
        Kind::Media if matches!(f.media.as_ref().map(|m| &m.v), Some(MediaLayer::Still { .. })) => "picture",
        Kind::Media => "video",
        Kind::Slide => "slide",
        Kind::Logo => "logo",
        Kind::Audio => "audio",
    }
}

/// The face that is current (shown, or fading out once the layer is hidden).
#[cfg(test)]
fn front(l: &Layer) -> &Side { if l.b_top { &l.b } else { &l.a } }

/// Puts `side` on the face that is not showing and makes it current, so it fades in over the old one.
pub(super) fn put(l: &mut Layer, side: Side) {
    if l.shown { l.b_top = !l.b_top }
    if l.b_top { l.b = side } else { l.a = side }
    l.shown = true;
}

/// Work for the UI thread, from the prepare, report and update threads.
enum Msg { Made { token: u64, kind: Kind, made: Result<Made, String> }, Report(PlayerReport) }

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

/// What a prepared layer shows.
enum Got { Pic(Side), Play(Rc<Player>) }

/// The newest plan while its layers are prepared. `heard`: what players not made yet reported, by
/// serial (`Some` = failed); readiness is per player, as an older plan's player may share the instance.
struct Prep { plan: Plan, frame: Frame, pause: Vec<(u64, bool)>, waiting: Vec<Kind>, got: Vec<(Kind, Got)>, heard: HashMap<u64, Option<String>> }

impl Prep {
    fn done(&mut self, kind: Kind) { self.waiting.retain(|k| *k != kind) }
}

struct Inner {
    ui: RoomWindow,
    surface: Rc<VideoSurface>,
    monitor: Option<MonitorId>,
    work: Arc<Work>,
    shared: Arc<Shared>,
    state: RefCell<RoomState>,
    prep: RefCell<Option<Prep>>,
    /// Players on air, by slot.
    players: RefCell<[Option<Rc<Player>>; 3]>,
    /// A swap waiting for the screen to turn black, with its commit unless already sent.
    deferred: RefCell<Option<(Prep, Option<RoomEvent>)>>,
    /// Ends the half for `deferred` (restarted by each swap, so it is always the newest's).
    fade: Timer,
    me: Weak<Inner>,
}

impl Inner {
    /// Handles everything that arrived from other threads.
    fn pump(&self) {
        self.shared.queued.store(false, Ordering::Release);
        let latest = self.shared.latest.lock().unwrap().take();
        if let Some(u) = latest { self.view(&u) }
        loop {
            let m = self.shared.inbox.lock().unwrap().pop_front();
            match m {
                Some(Msg::Made { token, kind, made }) => self.made(token, kind, made),
                Some(Msg::Report(r)) => self.report(r),
                None => break,
            }
        }
    }

    fn view(&self, view: &LiveView) {
        let plan = self.state.borrow_mut().plan(view);
        match plan { Some(plan) => self.start(plan, view), None => self.sync_pause(&room::pause_sync(view)) }
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
        let (token, wants) = (plan.token, plan.prepare.clone());
        for w in &wants {
            let (me, kind, what) = (self.me.clone(), w.kind, what(w.kind, &view.frame));
            Timer::single_shot(w.timeout, move || if let Some(i) = me.upgrade() { i.timed_out(token, kind, what) });
        }
        let waiting = wants.iter().map(|w| w.kind).collect();
        *self.prep.borrow_mut() = Some(Prep { plan, frame: view.frame.clone(), pause: room::pause_sync(view), waiting, got: vec![], heard: HashMap::new() });
        if !wants.is_empty() {
            let job = Job { token, wants, frame: view.frame.clone(), theme: view.theme.clone(), logo: view.logo.clone(), canvas: self.canvas() };
            let (work, shared) = (self.work.clone(), self.shared.clone());
            let spawned = std::thread::Builder::new().name("room-prepare".into()).spawn(move || prepare::prepare(&work, job, |m| shared.send(m)));
            if let Err(e) = spawned { return self.fail(token, format!("the room could not prepare it ({e})")) }
        }
        self.check();
    }

    fn timed_out(&self, token: u64, kind: Kind, what: &str) {
        let late = self.prep.borrow().as_ref().is_some_and(|p| p.plan.token == token && p.waiting.contains(&kind));
        if late { self.fail(token, format!("{what} took too long to load")) }
    }

    fn made(&self, token: u64, kind: Kind, made: Result<Made, String>) {
        if !self.prep.borrow().as_ref().is_some_and(|p| p.plan.token == token) { return } // stale: dropping disposes it
        match made {
            Err(reason) => return self.fail(token, reason),
            Ok(Made::Pic(px, color)) => {
                let side = Side { image: px.map(Px::image).unwrap_or_default(), color };
                if let Some(p) = self.prep.borrow_mut().as_mut() { p.got.push((kind, Got::Pic(side))); p.done(kind) }
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
            let error = (r.state == PlayState::Error).then(|| r.error.unwrap_or_else(|| "the file could not be played".into()));
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
        self.ui.set_black(1.0);
        *self.deferred.borrow_mut() = Some((prep, Some(commit)));
        let me = self.me.clone();
        self.fade.start(TimerMode::SingleShot, HALF, move || if let Some(i) = me.upgrade() { i.flush() });
    }

    /// Applies a swap that waited for black, and fades back in.
    fn flush(&self) {
        let d = self.deferred.borrow_mut().take();
        if let Some((prep, commit)) = d { self.apply(prep, commit); self.ui.set_black(0.0) }
    }

    fn apply(&self, prep: Prep, commit: Option<RoomEvent>) {
        let mut players = self.players.borrow_mut();
        for kind in &prep.plan.remove {
            self.layer(*kind, |l| l.shown = false);
            if let Some(s) = slot(*kind) { self.retire(players[s].take()) }
        }
        for (kind, got) in prep.got {
            let old = slot(kind).and_then(|s| players[s].take());
            match got {
                Got::Pic(side) => self.layer(kind, |l| put(l, side)),
                Got::Play(p) => {
                    self.layer(kind, |l| l.shown = false); // video is drawn under the scene; audio has no layer
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
        if let Some(c) = commit { self.shared.emit(c) }
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

    fn sync_pause(&self, pairs: &[(u64, bool)]) {
        let players = self.players.borrow();
        for (inst, paused) in pairs {
            for p in [&players[MEDIA], &players[AUDIO]].into_iter().flatten().filter(|p| p.inst() == *inst) { p.set_paused(*paused) }
        }
    }

    fn layer(&self, kind: Kind, f: impl FnOnce(&mut Layer)) {
        let (get, set): (fn(&RoomWindow) -> Layer, fn(&RoomWindow, Layer)) = match kind {
            Kind::Background => (RoomWindow::get_bg, RoomWindow::set_bg),
            Kind::Media => (RoomWindow::get_media, RoomWindow::set_media),
            Kind::Slide => (RoomWindow::get_slide, RoomWindow::set_slide),
            Kind::Logo => (RoomWindow::get_logo, RoomWindow::set_logo),
            Kind::Audio => return,
        };
        let mut l = get(&self.ui);
        f(&mut l);
        set(&self.ui, l);
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
        let _ = self.ui.hide();
    }
}

/// An open room window. It follows the live view until it is dropped.
pub struct Room { inner: Rc<Inner> }

impl Room {
    /// Open the room window, full screen on `monitor` (a plain window without one). Without
    /// libmpv everything but video and audio works; those report the missing player.
    pub fn open(app: Arc<App>, renderer: Arc<Renderer>, mpv: Option<Arc<Mpv>>, monitor: Option<MonitorId>) -> Result<Rc<Room>, String> {
        let ui = RoomWindow::new().map_err(|e| format!("Could not open the room screen: {e}"))?;
        let weak = ui.as_weak();
        let surface = VideoSurface::attach(&ui, video::drawing(), move |img| if let Some(ui) = weak.upgrade() { ui.set_video_frame(img.unwrap_or_default()) });
        let (tx, reports) = std::sync::mpsc::channel();
        let mut audio = video::players(mpv.clone(), tx.clone());
        audio.options = Options { vo: "null", ..Options::default() };
        let work = Arc::new(Work { app: app.clone(), renderer, video: video::players(mpv, tx), audio });
        let weak = ui.as_weak();
        let wake = Box::new(move || { let _ = weak.upgrade_in_event_loop(|ui| ui.invoke_pump()); });
        let (events, rx) = std::sync::mpsc::channel();
        let shared = Arc::new(Shared { inbox: Mutex::default(), latest: Mutex::default(), started: Mutex::default(), events, queued: AtomicBool::new(false), closed: AtomicBool::new(false), wake });
        let to_live = app.clone();
        let _ = std::thread::Builder::new().name("room-events".into()).spawn(move || for ev in rx { to_live.room_event(ev) });
        let inner = Rc::new_cyclic(|me: &Weak<Inner>| {
            let me2 = me.clone();
            ui.on_pump(move || if let Some(i) = me2.upgrade() { i.pump() });
            Inner {
                ui: ui.clone_strong(), surface, monitor: monitor.clone(), work, shared: shared.clone(), state: RefCell::default(),
                prep: RefCell::default(), players: RefCell::default(), deferred: RefCell::default(), fade: Timer::default(), me: me.clone(),
            }
        });
        let room = Rc::new(Room { inner });
        ui.show().map_err(|e| format!("Could not open the room screen: {e}"))?;
        if let Some(m) = &monitor { monitors::fullscreen_on(&ui, m)? }
        // Subscribe before reading the view so no update falls in between.
        let rx = app.subscribe();
        forward(reports, shared.clone());
        follow(app.clone(), rx, shared);
        room.inner.view(&app.live_view());
        Ok(room)
    }

    pub fn window(&self) -> &slint::Window { self.inner.ui.window() }

    /// The frame grabber of the player being drawn (the foreground video, else the background video), for the
    /// operator's live preview; `None` while no video is drawn.
    pub fn drawn_grabber(&self) -> Option<FrameGrabber> {
        let players = self.inner.players.borrow();
        players[MEDIA].as_ref().or(players[BG].as_ref()).map(|p| p.grabber())
    }

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

/// Live updates from a plain thread; only the newest view is applied.
fn follow(app: Arc<App>, mut rx: tokio::sync::broadcast::Receiver<Update>, shared: Arc<Shared>) {
    let _ = std::thread::Builder::new().name("room-updates".into()).spawn(move || loop {
        let update = match rx.blocking_recv() {
            Ok(Update::Live(u)) => u,
            Ok(_) => continue,
            Err(RecvError::Lagged(_)) => LiveUpdate::new(app.live_view()),
            Err(RecvError::Closed) => return,
        };
        if shared.closed.load(Ordering::Relaxed) { return }
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

    fn add(app: &App, kind: ItemKind) -> Uuid {
        let id = Uuid::new_v4();
        app.data.lock().unwrap().services[0].items.push(ServiceItem { id, kind, background: None, notes: String::new() });
        id
    }
    fn media(app: &App, kind: MediaKind, asset: &str) -> Uuid {
        let media_id = Uuid::new_v4();
        app.data.lock().unwrap().media.push(MediaFile { id: media_id, name: asset.into(), kind, asset: asset.into(), status: MediaStatus::Ready, original_asset: None, folder: String::new() });
        add(app, ItemKind::Media { media_id, autoplay: true, end: EndAction::Stop, looped: false })
    }
    fn scripture(app: &App) -> Uuid {
        let verses = vec![VerseText { reference: "John 3:16".into(), text: "For God so loved the world".into() }];
        add(app, ItemKind::Scripture { reference: "John 3:16".into(), translation: "KJV".into(), verses, per_slide: 0, text_size: 0, abbr: String::new(), also: vec![] })
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
    fn size(l: &Layer) -> (u32, u32) { let s = front(l).image.size(); (s.width, s.height) }
    fn open(app: &Arc<App>) -> Rc<Room> { Room::open(app.clone(), Renderer::new(), None, None).unwrap() }

    #[test]
    fn text_slide_sets_slide_image_and_commits() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let v = show(&app, scripture(&app));
        let room = open(&app);
        let ui = &room.inner.ui;
        assert!(pump_until(&room, 15.0, || size(&ui.get_slide()).0 > 0 && app.live_view().committed == v), "slide {:?}, committed {}", size(&ui.get_slide()), app.live_view().committed);
        assert!(ui.get_slide().shown);
        // Drawn at the room's size, not scaled up from a thumbnail.
        assert!(size(&ui.get_slide()).0 >= 1280, "{:?}", size(&ui.get_slide()));
        assert!(ui.get_bg().shown, "the theme background is up");
        assert!(!ui.get_media().shown && !ui.get_logo().shown && !ui.get_video());
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

    fn prep(plan: Plan, frame: Frame) -> Prep { Prep { plan, frame, pause: vec![], waiting: vec![], got: vec![], heard: Default::default() } }

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
        assert!(i.deferred.borrow().as_ref().is_some_and(|d| d.1.is_none()) && i.ui.get_black() == 1.0, "the swap is shown before black");
        assert!(pump_until(&room, 5.0, || app.live_view().notice.is_some()));
        assert_eq!(app.live_view().item.map(|s| s.item_id), Some(two));
        i.flush();
        assert!(i.deferred.borrow().is_none() && i.ui.get_black() == 0.0);
    }

    #[test]
    fn each_fade_through_black_waits_its_own_half() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let room = open(&app);
        let i = &room.inner;
        let swap = |token| i.swap(prep(Plan { token, version: 0, prepare: vec![], remove: vec![], fade_through_black: true }, Frame::default()), RoomEvent::Committed { version: 0 });
        swap(100);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(150));
        swap(101);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100)); // the first half is over
        assert!(i.deferred.borrow().is_some() && i.ui.get_black() == 1.0, "the second swap is applied before the screen is black");
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(150));
        assert!(i.deferred.borrow().is_none() && i.ui.get_black() == 0.0);
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
        let ui = &room.inner.ui;
        assert!(pump_until(&room, 5.0, || app.live_view().committed == v));
        assert!(!ui.get_slide().shown && !ui.get_bg().shown && !ui.get_media().shown && !ui.get_logo().shown);
    }

    #[test]
    fn missing_still_asset_reports_failed_and_keeps_previous() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let text = scripture(&app);
        let still = media(&app, MediaKind::Image, "missing.png");
        let v = show(&app, text);
        let room = open(&app);
        let ui = &room.inner.ui;
        assert!(pump_until(&room, 15.0, || app.live_view().committed == v));
        let before = size(&ui.get_slide());
        show(&app, still);
        assert!(pump_until(&room, 5.0, || app.live_view().notice.is_some()), "no failure reported");
        let notice = app.live_view().notice.unwrap();
        assert!(notice.contains("picture could not be loaded"), "{notice}");
        // The engine goes back to what is on air, and the room commits that without changes.
        assert!(pump_until(&room, 5.0, || { let l = app.live_view(); !l.pending && l.committed > v }));
        assert!(app.live_view().frame.slide.is_some());
        assert!(ui.get_slide().shown && size(&ui.get_slide()) == before);
        assert!(!ui.get_media().shown);
    }

    #[test]
    fn logo_layer_follows_frame_logo() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        image::RgbaImage::from_pixel(64, 32, image::Rgba([200, 0, 0, 255])).save(app.store.asset_path("logo.png")).unwrap();
        app.data.lock().unwrap().settings.logo = Some("logo.png".into());
        let room = open(&app);
        let ui = &room.inner.ui;
        let v = run(&app, Command::Logo);
        assert!(pump_until(&room, 5.0, || app.live_view().committed == v));
        assert!(ui.get_logo().shown);
        assert_eq!(size(&ui.get_logo()), (64, 32));
        let v = run(&app, Command::ClearAll);
        assert!(pump_until(&room, 5.0, || app.live_view().committed == v));
        assert!(!ui.get_logo().shown);
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
        assert!(notice.contains(crate::video::ffi::MISSING), "{notice}");
        assert!(app.live_view().frame.audio.is_none());
    }

    /// One second of silence, 16-bit mono PCM.
    fn silent_wav(path: &std::path::Path) {
        let (rate, n) = (8000u32, 8000u32);
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + n * 2).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(n * 2).to_le_bytes());
        b.resize(b.len() + (n * 2) as usize, 0);
        std::fs::write(path, b).unwrap();
    }

    #[test]
    fn audio_item_plays_after_commit_and_reports_by_instance() {
        if std::env::var("RP_VIDEO_TESTS").as_deref() != Ok("1") { return }
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        silent_wav(&app.store.asset_path("hush.wav"));
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
        assert!(pump_until(&room, 10.0, || ui.get_black() > 0.5 || app.live_view().committed == v), "notice {:?}", app.live_view().notice);
        assert!(ui.get_black() > 0.5, "starting a video fades through black");
        assert!(pump_until(&room, 5.0, || app.live_view().committed == v));
        assert_eq!(ui.get_black(), 0.0);
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