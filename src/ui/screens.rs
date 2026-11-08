//! The native windowing layer (`Screens`): room and stage windows live on the Slint thread and are
//! opened and closed from any thread through it. They reopen from settings at start, close when
//! their monitor is unplugged and come back on it when it returns. The operator window is opened by
//! the binary; this layer only knows it to bring it forward and to place it away from the room.
use super::{IdentifyWindow, operator::window::{MpvInfo, Operator, OperatorScreens, VideoInfo}, room::Room, stage::Stage};
use crate::app::{App, Monitor, ScreenGuard, Screens};
use crate::model::ScreenTarget;
use crate::platform::{awake::KeepAwake, monitors::{self, Change, MonitorId, Watcher, pick, to_monitors}};
use crate::render::Renderer;
use crate::video::{FrameGrabber, ffi::Mpv};
use slint::{CloseRequestResponse, ComponentHandle, Timer, TimerMode, winit_030::{invoke_from_active_event_loop, winit::event_loop::ActiveEventLoop}};
use std::{cell::RefCell, rc::Rc, sync::{Arc, Mutex, Weak, atomic::{AtomicBool, Ordering}, mpsc}, thread::ThreadId, time::Duration};

fn not_connected() -> String { crate::tr!("That screen is not connected right now.") }
/// Shown where a Linux desktop lists who is keeping it awake.
fn staying_awake() -> String { crate::tr!("A presentation is on screen") }

/// How long Identify's badges stay up.
const IDENTIFY_FOR: Duration = Duration::from_secs(4);
/// Share of a monitor's height an Identify badge takes.
const BADGE: f64 = 0.36;

/// "room" or "stage", for a window name from `ScreenSetup::keyed`.
fn kind(role: &str) -> &str { role.split(':').next().unwrap_or(role) }

/// A role's window, for the log.
fn named(role: &str) -> &'static str {
    match (kind(role), role.contains(':')) {
        ("room", false) => "room screen",
        ("room", true) => "extra audience screen",
        (_, false) => "stage display",
        (_, true) => "extra stage screen",
    }
}

/// The notice while `role`'s monitor is unplugged.
pub fn gone_notice(role: &str) -> String {
    match (kind(role), role.contains(':')) {
        ("room", false) => crate::tr!("The room screen was unplugged. It comes back by itself when that screen is plugged in again."),
        ("room", true) => crate::tr!("The extra audience screen was unplugged. It comes back by itself when that screen is plugged in again."),
        (_, false) => crate::tr!("The stage display was unplugged. It comes back by itself when that screen is plugged in again."),
        (_, true) => crate::tr!("The extra stage screen was unplugged. It comes back by itself when that screen is plugged in again."),
    }
}

/// The notice when `role`'s monitor is back but its window could not open.
fn back_failed(role: &str, error: &str) -> String {
    match (kind(role), role.contains(':')) {
        ("room", false) => crate::tr!("The room screen is plugged in again, but it could not open: {error}", error = error),
        ("room", true) => crate::tr!("The extra audience screen is plugged in again, but it could not open: {error}", error = error),
        (_, false) => crate::tr!("The stage display is plugged in again, but it could not open: {error}", error = error),
        (_, true) => crate::tr!("The extra stage screen is plugged in again, but it could not open: {error}", error = error),
    }
}

/// A badge's rectangle, centred on `m`: position and size in physical pixels.
fn badge(m: &Monitor) -> ((i32, i32), (u32, u32)) {
    let h = (f64::from(m.height) * BADGE) as i32;
    let w = h * 3 / 2;
    ((m.x + (m.width - w) / 2, m.y + (m.height - h) / 2), (w.max(1) as u32, h.max(1) as u32))
}

/// Where a role's window reopens at start: `None` to leave it closed, `Some(None)` for a plain window. A normal
/// start never opens one on the operator's (main) monitor by itself; after a crash a role that was open comes back
/// where it was, the main monitor included, since it was already there.
fn reopen_target(saved: Option<&str>, list: &[MonitorId], main: Option<&MonitorId>, restored: bool) -> Option<Option<MonitorId>> {
    if restored { return match saved { Some(id) => pick(list, Some(id), main).map(Some), None => Some(None) } }
    pick(list, Some(saved?), main).filter(|m| Some(m) != main).map(Some)
}

#[derive(Clone)]
enum Win { Room(Rc<Room>), Stage(Rc<Stage>) }

impl Win {
    fn window(&self) -> &slint::Window { match self { Win::Room(r) => r.window(), Win::Stage(s) => s.window() } }
}

/// The Slint thread's side: open windows (role, opening number, window, the guard that marks the
/// role connected while it is open), the unplug watch and its timer, the operator window (weak:
/// the operator holds these screens), and the hold that keeps the computer awake while a window is open.
#[derive(Default)]
struct Desk { wins: Vec<(String, u64, Win, ScreenGuard)>, watcher: Watcher, poll: Option<Timer>, seq: u64, operator: Option<std::rc::Weak<Operator>>, awake: Option<KeepAwake>, badges: Vec<IdentifyWindow>, badge_timer: Timer }

thread_local! { static DESK: RefCell<Desk> = RefCell::default(); }

pub struct NativeScreens {
    app: Weak<App>,
    renderer: Arc<Renderer>,
    mpv: Option<Arc<Mpv>>,
    /// The Slint thread: calls from it run at once, others wait for it.
    ui: ThreadId,
    me: Weak<NativeScreens>,
    /// Monitors as last seen (every 2 s), and the main one, where the operator works.
    seen: Mutex<(Vec<MonitorId>, Option<MonitorId>)>,
    roles: Mutex<Vec<String>>,
    /// After a crash: the roles that were open. They reopen instead of what settings say.
    restored: Mutex<Option<Vec<String>>>,
    /// `Settings::room_on_top` as last read (the data lock is only tried, never waited for, on this thread).
    room_on_top: AtomicBool,
}

impl NativeScreens {
    /// Call on the Slint thread. `mpv` is `None` when libmpv is missing.
    pub fn new(app: &Arc<App>, renderer: Arc<Renderer>, mpv: Option<Arc<Mpv>>) -> Arc<Self> {
        // About shows the mpv and FFmpeg versions; read them once now, off the Slint thread.
        if let Some(m) = mpv.clone() { let _ = std::thread::Builder::new().name("mpv-versions".into()).spawn(move || { m.versions(); }); }
        Arc::new_cyclic(|me| NativeScreens {
            app: Arc::downgrade(app), renderer, mpv, ui: std::thread::current().id(), me: me.clone(),
            seen: Mutex::default(), roles: Mutex::default(), restored: Mutex::default(), room_on_top: AtomicBool::new(true),
        })
    }

    /// Starts the 2 s monitor watch and reopens room and stage where settings say, once the event loop runs.
    pub fn start(&self) {
        let timer = Timer::default();
        let me = self.me.clone();
        timer.start(TimerMode::Repeated, Duration::from_secs(2), move || {
            let me = me.clone();
            let _ = invoke_from_active_event_loop(move |el| if let Some(s) = me.upgrade() { s.look(el) });
        });
        DESK.with(|d| d.borrow_mut().poll = Some(timer));
        let me = self.me.clone();
        let _ = invoke_from_active_event_loop(move |el| if let Some(s) = me.upgrade() { s.look(el); s.reopen() });
    }

    #[cfg(test)]
    fn keeps_awake(&self) -> bool { DESK.with(|d| d.borrow().awake.is_some()) }

    /// The first reopen brings back these roles, instead of what settings say. Call before `start`.
    pub fn set_restored(&self, roles: Vec<String>) { *self.restored.lock().unwrap() = Some(roles); }

    /// Closes every window (frees players and GL contexts) and stops the watch. Call on the Slint thread.
    pub fn shutdown(&self) {
        let desk = DESK.with(|d| std::mem::take(&mut *d.borrow_mut()));
        drop(desk);
        self.roles.lock().unwrap().clear();
    }

    /// The operator window, for a second launch to bring forward. Call on the Slint thread.
    pub fn set_operator(&self, operator: std::rc::Weak<Operator>) {
        DESK.with(|d| d.borrow_mut().operator = Some(operator));
    }

    /// Puts the operator window on its monitor (see `operator_monitor`), maximised. Call on the Slint
    /// thread, after the room and stage reopened.
    pub fn place_operator(&self, operator: &Operator) {
        let window = operator.window();
        let (list, main) = self.seen.lock().unwrap().clone();
        let room = DESK.with(|d| d.borrow().watcher.monitor_of("room").cloned());
        if let Some(m) = operator_monitor(&list, main.as_ref(), room.as_ref()) {
            // Anywhere on that monitor: maximising fills the monitor the window is on.
            window.set_position(slint::PhysicalPosition::new(m.position.0 + 40, m.position.1 + 40));
        }
        monitors::maximise(&operator.ui);
    }

    /// Runs `f` on the Slint thread and waits for it (never waits when already there).
    fn on_ui<T: Send + 'static>(&self, f: impl FnOnce(&NativeScreens) -> T + Send + 'static) -> Result<T, String> {
        if std::thread::current().id() == self.ui { return Ok(f(self)) }
        let (tx, rx) = mpsc::sync_channel(1);
        let me = self.me.clone();
        invoke_from_active_event_loop(move |el| if let Some(s) = me.upgrade() { s.look(el); let _ = tx.send(f(&s)); })
            .map_err(|_| crate::tr!("The screens are not running."))?;
        rx.recv_timeout(Duration::from_secs(10)).map_err(|_| crate::tr!("The screens did not answer in time."))
    }

    fn look(&self, el: &ActiveEventLoop) {
        let (list, main) = monitors::on_event_loop(el);
        self.look_at(list, main);
    }

    /// Takes in the monitors connected now and acts on unplugs and returns, and puts back a window
    /// that Windows moved off its monitor (unplugged and back between two looks).
    pub(crate) fn look_at(&self, list: Vec<MonitorId>, main: Option<MonitorId>) {
        let changes = DESK.with(|d| d.borrow_mut().watcher.step(&list));
        *self.seen.lock().unwrap() = (list, main);
        for c in changes { self.changed(c) }
        let moved: Vec<_> = DESK.with(|d| {
            let d = d.borrow();
            d.wins.iter().filter_map(|(role, _, win, _)| {
                let m = d.watcher.misplaced(role, monitors::current(win.window()).as_ref())?;
                Some((role.clone(), win.clone(), m))
            }).collect()
        });
        for (role, win, m) in moved {
            if let Err(e) = monitors::refullscreen(win.window(), &m) { eprintln!("Could not put the {} back: {e}", named(&role)) }
        }
        self.level_room();
    }

    /// Whether the room window should be kept on top now.
    #[cfg(test)]
    fn room_should_be_on_top(&self) -> bool { self.should_be_on_top("room") }

    /// Whether an audience window should be kept on top now: the setting is on and it is full screen on a monitor
    /// other than the operator's (where the operator window is, else where it would go).
    fn should_be_on_top(&self, role: &str) -> bool {
        if let Some(app) = self.app.upgrade() {
            if let Ok(d) = app.data.try_lock() { self.room_on_top.store(d.settings.room_on_top, Ordering::Relaxed) }
        }
        let (list, main) = self.seen.lock().unwrap().clone();
        DESK.with(|d| {
            let d = d.borrow();
            let room = d.watcher.monitor_of("room").cloned();
            let operator = d.operator.as_ref().and_then(std::rc::Weak::upgrade).and_then(|op| monitors::current(op.window()))
                .or_else(|| operator_monitor(&list, main.as_ref(), room.as_ref()));
            room_on_top(self.room_on_top.load(Ordering::Relaxed), d.watcher.monitor_of(role), operator.as_ref())
        })
    }

    /// Puts the audience windows on top, or back to normal, as `should_be_on_top` says (at open and every look).
    fn level_room(&self) {
        let rooms: Vec<(String, Rc<Room>)> = DESK.with(|d| d.borrow().wins.iter().filter_map(|(role, _, w, _)| match w { Win::Room(r) => Some((role.clone(), r.clone())), _ => None }).collect());
        for (role, r) in rooms { r.set_on_top(self.should_be_on_top(&role)) }
    }

    /// Reopen the screens: what was open before a crash, otherwise what the setup's switches say.
    fn reopen(&self) {
        let Some(app) = self.app.upgrade() else { return };
        let restored = self.restored.lock().unwrap().take();
        let windows = start_windows(&app, restored.is_some());
        let (list, main) = self.seen.lock().unwrap().clone();
        for w in &windows {
            if restored.as_ref().is_some_and(|roles| !roles.iter().any(|r| *r == w.key)) { continue }
            let id = match &w.target { ScreenTarget::Monitor(m) => Some(m.as_str()), ScreenTarget::Window => None, ScreenTarget::None => continue };
            let Some(m) = reopen_target(id, &list, main.as_ref(), restored.is_some()) else { continue };
            let plain = m.is_none();
            let opened = self.show(&w.key, m);
            if plain && opened.is_ok() { self.size_window(&w.key, w.size) }
            if let Err(e) = opened { eprintln!("Could not reopen the {}: {e}", named(&w.key)) }
        }
        app.screens_applied(windows);
        app.broadcast_data();
    }

    /// Sets the size of `role`'s plain window.
    fn size_window(&self, role: &str, (w, h): (u32, u32)) {
        DESK.with(|d| if let Some((_, _, win, _)) = d.borrow().wins.iter().find(|(r, ..)| r == role) {
            win.window().set_size(slint::PhysicalSize::new(w, h));
        });
    }

    /// Identify (Slint thread): a badge on each monitor, which goes away by itself. A second click starts again.
    fn identify_here(&self, marks: Vec<(String, String, String)>) -> Result<(), String> {
        let monitors = to_monitors(&self.seen.lock().unwrap().0, None);
        let operator = DESK.with(|d| d.borrow().operator.as_ref().and_then(std::rc::Weak::upgrade)).filter(|o| o.focused());
        let mut badges = vec![];
        for (id, number, what) in marks {
            let Some(m) = monitors.iter().find(|m| m.id == id) else { continue };
            let ui = IdentifyWindow::new().map_err(|e| crate::tr!("Could not show the screen numbers: {error}", error = e))?;
            crate::i18n::sync_slint();
            ui.set_number(number.into());
            ui.set_what(what.into());
            let ((x, y), (w, h)) = badge(m);
            ui.window().set_position(slint::PhysicalPosition::new(x, y));
            ui.window().set_size(slint::PhysicalSize::new(w, h));
            ui.show().map_err(|e| crate::tr!("Could not show the screen numbers: {error}", error = e))?;
            badges.push(ui);
        }
        if let Some(op) = operator {
            op.refocus();
            let op = Rc::downgrade(&op);
            Timer::single_shot(Duration::from_millis(150), move || if let Some(op) = op.upgrade() { op.refocus() });
        }
        DESK.with(|d| {
            let mut d = d.borrow_mut();
            for old in std::mem::replace(&mut d.badges, badges) { let _ = old.hide(); }
            d.badge_timer.start(TimerMode::SingleShot, IDENTIFY_FOR, || DESK.with(|d| {
                let badges = std::mem::take(&mut d.borrow_mut().badges);
                for b in badges { let _ = b.hide(); }
            }));
        });
        Ok(())
    }

    /// An unplug or return from the watch (Slint thread).
    pub fn changed(&self, c: Change) {
        let Some(app) = self.app.upgrade() else { return };
        match c {
            Change::Gone(role) => {
                drop(self.take(&role, None));
                app.live.lock().unwrap().notice = Some(gone_notice(&role));
            }
            Change::Back(role, m) => {
                // Video and audio do not resume by themselves after the room was gone (as after a restart).
                if role == "room" { app.room_recovered() }
                let opened = self.show(&role, Some(m));
                let mut live = app.live.lock().unwrap();
                match opened {
                    Err(e) => live.notice = Some(back_failed(&role, &e)),
                    Ok(()) => if live.notice.as_deref() == Some(gone_notice(&role).as_str()) { live.notice = None },
                }
            }
        }
        app.broadcast_live();
        app.broadcast_data();
    }

    fn open_here(&self, role: &str, monitor: Option<&str>) -> Result<(), String> {
        let mon = match monitor {
            Some(saved) => Some(pick(&self.seen.lock().unwrap().0, Some(saved), None).ok_or_else(not_connected)?),
            None => None,
        };
        self.show(role, mon)
    }

    /// Opens `role`'s window, full screen on `mon` (a plain window without one), replacing an open one.
    fn show(&self, role: &str, mon: Option<MonitorId>) -> Result<(), String> {
        let app = self.app.upgrade().ok_or_else(|| crate::tr!("RustyPresenter is closing."))?;
        drop(self.take(role, None)); // the old window's players and GL context go first
        let guard = app.screen_guard(role);
        // winit activates every window it creates, which would leave the operator's keys dead in the new
        // full-screen window: when the operator had focus, it gets it back (now and once the window settled).
        let operator = DESK.with(|d| d.borrow().operator.as_ref().and_then(std::rc::Weak::upgrade)).filter(|o| o.focused());
        let win = match kind(role) {
            "room" if role == "room" => Win::Room(Room::open(app, self.renderer.clone(), self.mpv.clone(), mon.clone())?),
            "room" => Win::Room(Room::open_mirror(app, self.renderer.clone(), self.mpv.clone(), mon.clone())?),
            "stage" => Win::Stage(Stage::open(app, mon.clone())?),
            _ => return Err("unknown window".into()),
        };
        if let Win::Room(r) = &win { r.set_screen(role) }
        if let Some(op) = operator {
            op.refocus();
            let op = Rc::downgrade(&op);
            Timer::single_shot(Duration::from_millis(150), move || if let Some(op) = op.upgrade() { op.refocus() });
        }
        let id = DESK.with(|d| { let mut d = d.borrow_mut(); d.seq += 1; d.seq });
        // Closing the window (Alt+F4) closes it for good, as the operator's Close would.
        let (me, r) = (self.me.clone(), role.to_string());
        win.window().on_close_requested(move || {
            let (me, r) = (me.clone(), r.clone());
            Timer::single_shot(Duration::ZERO, move || if let Some(s) = me.upgrade() {
                if !s.take(&r, Some(id)).is_empty() {
                    DESK.with(|d| d.borrow_mut().watcher.unwatch(&r));
                    if let Some(app) = s.app.upgrade() { app.broadcast_data() }
                }
            });
            CloseRequestResponse::HideWindow
        });
        DESK.with(|d| {
            let mut d = d.borrow_mut();
            d.wins.push((role.into(), id, win, guard));
            match mon { Some(m) => d.watcher.watch(role, m, &self.seen.lock().unwrap().0), None => d.watcher.unwatch(role) }
        });
        self.sync_roles();
        if kind(role) == "room" { self.level_room() }
        Ok(())
    }

    /// Takes `role`'s window (only opening `id`, when given) out of the desk; dropping it closes it.
    #[must_use]
    fn take(&self, role: &str, id: Option<u64>) -> Vec<Win> {
        let out = DESK.with(|d| {
            let wins = &mut d.borrow_mut().wins;
            let (out, keep) = std::mem::take(wins).into_iter().partition(|(r, i, _, _)| r == role && id.is_none_or(|id| id == *i));
            *wins = keep;
            out
        });
        self.sync_roles();
        // The guards drop here: the role is no longer connected once its window is out.
        out.into_iter().map(|(_, _, w, _)| w).collect()
    }

    /// Publishes the open roles. An open room or stage window also keeps the screens on: when Windows turns the
    /// display off for being idle, every monitor goes dark, the projector included.
    fn sync_roles(&self) {
        *self.roles.lock().unwrap() = DESK.with(|d| {
            let mut d = d.borrow_mut();
            if d.wins.is_empty() { d.awake = None } else if d.awake.is_none() { d.awake = Some(KeepAwake::acquire(&staying_awake())) }
            d.wins.iter().map(|(r, _, _, _)| r.clone()).collect()
        });
    }
}

impl OperatorScreens for NativeScreens {
    fn video_info(&self) -> VideoInfo {
        let mpv = self.mpv.as_ref().map(|m| {
            let v = m.versions();
            MpvInfo { api: m.api_version(), path: m.path().display().to_string(), mpv_version: v.mpv, ffmpeg_version: v.ffmpeg, configuration: v.configuration }
        });
        VideoInfo { draw_mode: crate::video::drawing(), mpv }
    }

    /// The room's drawn player, when the room window is open (the operator calls this on the Slint thread, where
    /// the windows live; from any other thread there is none).
    fn preview_grabber(&self) -> Option<FrameGrabber> {
        DESK.with(|d| d.borrow().wins.iter().find_map(|(role, _, w, _)| match w { Win::Room(r) if role == "room" => r.drawn_grabber(), _ => None }))
    }

    /// N5: what the stage window shows now, at most `max_w` wide; `None` while it is closed or cannot be read.
    fn stage_snapshot(&self, max_w: u32) -> Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>> {
        let stage = DESK.with(|d| d.borrow().wins.iter().find_map(|(role, _, w, _)| match w { Win::Stage(s) if role == "stage" => Some(s.clone()), _ => None }))?;
        let px = stage.window().take_snapshot().ok()?;
        Some(super::perf::shrink(&px, max_w))
    }
}

impl Screens for NativeScreens {
    fn open(&self, role: &str, monitor: Option<&str>) -> Result<(), String> {
        match role {
            // A second launch asks for this (spec §8.4): bring the operator window forward.
            "operator" => self.on_ui(|_| {
                if let Some(op) = DESK.with(|d| d.borrow().operator.as_ref().and_then(std::rc::Weak::upgrade)) { op.raise() }
            }),
            _ if matches!(kind(role), "room" | "stage") => {
                let (role, monitor) = (role.to_string(), monitor.map(str::to_string));
                self.on_ui(move |s| s.open_here(&role, monitor.as_deref()))?
            }
            _ => Err("unknown window".into()),
        }
    }

    fn open_window(&self, role: &str, size: (u32, u32)) -> Result<(), String> {
        self.open(role, None)?;
        let role = role.to_string();
        self.on_ui(move |s| s.size_window(&role, size))
    }

    fn identify(&self, marks: Vec<(String, String, String)>) {
        match self.on_ui(move |s| s.identify_here(marks)) {
            Ok(Err(e)) | Err(e) => eprintln!("{e}"),
            Ok(Ok(())) => {}
        }
    }

    fn close(&self, role: &str) {
        let role = role.to_string();
        let closed = self.on_ui(move |s| { drop(s.take(&role, None)); DESK.with(|d| d.borrow_mut().watcher.unwatch(&role)) });
        if let Err(e) = closed { eprintln!("Could not close the window: {e}") }
    }

    fn open_roles(&self) -> Vec<String> { self.roles.lock().unwrap().clone() }

    fn audio_devices(&self) -> Vec<crate::video::ffi::AudioDevice> { self.mpv.as_ref().map(|m| m.audio_devices()).unwrap_or_default() }

    /// As last seen (at most 2 s old): never waits for the Slint thread, so it is safe under `App`'s locks.
    fn monitors(&self) -> Vec<Monitor> {
        let (list, main) = self.seen.lock().unwrap().clone();
        to_monitors(&list, main.as_ref())
    }
}

/// The windows to open at start. With Settings → Video → "Turn the screens on at start" off, the room and stage
/// switches are turned off instead (saved, so the top bar shows it), and nothing opens until the operator switches
/// them on. After a crash (`restored`) what was open always comes back.
pub fn start_windows(app: &App, restored: bool) -> Vec<crate::model::ScreenWindow> {
    let (on, windows) = { let d = app.data.lock().unwrap(); (d.settings.screens_at_start, d.settings.screens.windows()) };
    if on || restored || windows.is_empty() { return windows }
    let off = app.edit_untracked(|d| {
        for g in crate::model::ScreenSetup::GROUPS { if let Some(g) = d.settings.screens.group_mut(g) { g.on = false } }
        Ok(())
    });
    if let Err(e) = off { eprintln!("Could not leave the screens off at start: {e}") }
    vec![]
}

/// Whether the room window is kept on top: only with the setting on, and only while it is full screen on a monitor
/// that is not the operator's (on the operator's own monitor it would cover the operator; a plain test window is
/// never on top).
pub fn room_on_top(setting: bool, room: Option<&MonitorId>, operator: Option<&MonitorId>) -> bool {
    setting && matches!((room, operator), (Some(r), Some(o)) if r != o)
}

/// The operator's monitor: the main one, unless the room is on it (saved there on purpose); then the
/// first one the room is not on (`pick`'s rule turned round). With one monitor, that one.
pub fn operator_monitor(monitors: &[MonitorId], main: Option<&MonitorId>, room: Option<&MonitorId>) -> Option<MonitorId> {
    let main = main.filter(|m| monitors.contains(*m));
    if let Some(m) = main.filter(|m| Some(*m) != room) { return Some(m.clone()) }
    monitors.iter().find(|m| Some(*m) != room).or(main).or(monitors.first()).cloned()
}

#[cfg(test)]
mod tests {

    mod reopening {
        use super::super::reopen_target;
        use crate::platform::monitors::MonitorId;

        fn mon(name: &str, x: i32) -> MonitorId { MonitorId { name: name.into(), size: (1920, 1080), position: (x, 0) } }

        #[test]
        fn normal_start_skips_the_main_monitor() {
            let (a, b) = (mon("A", 0), mon("B", 1920));
            assert_eq!(reopen_target(Some(&a.key()), &[a.clone(), b.clone()], Some(&a), false), None);
            assert_eq!(reopen_target(Some(&b.key()), &[a.clone(), b.clone()], Some(&a), false), Some(Some(b)));
        }

        #[test]
        fn normal_start_with_nothing_saved_stays_closed() {
            let a = mon("A", 0);
            assert_eq!(reopen_target(None, &[a.clone()], Some(&a), false), None);
        }

        #[test]
        fn restored_role_returns_to_the_main_monitor() {
            let a = mon("A", 0);
            assert_eq!(reopen_target(Some(&a.key()), &[a.clone()], Some(&a), true), Some(Some(a)));
        }

        #[test]
        fn restored_role_with_no_monitor_was_a_plain_window() {
            let a = mon("A", 0);
            assert_eq!(reopen_target(None, &[a.clone()], Some(&a), true), Some(None));
        }

        #[test]
        fn restored_role_on_an_unplugged_monitor_stays_closed() {
            let (a, b) = (mon("A", 0), mon("B", 1920));
            assert_eq!(reopen_target(Some(&b.key()), &[a.clone()], Some(&a), true), None);
        }
    }
    use super::*;
    use crate::app::test_app;

    fn mon(name: &str, w: u32, x: i32) -> MonitorId { MonitorId { name: name.into(), size: (w, w * 9 / 16), position: (x, 0) } }

    fn screens(app: &Arc<App>) -> Arc<NativeScreens> {
        i_slint_backend_testing::init_no_event_loop();
        NativeScreens::new(app, Renderer::new(), None)
    }

    #[test]
    fn display_open_operator_raises_window() {
        let app = test_app();
        let s = screens(&app);
        s.open("operator", None).unwrap(); // no operator yet: nothing to raise, and no error
        let op = Operator::open(app.clone(), crate::ui::ops::Ops::inline(app.clone()), Rc::new(s.clone())).unwrap();
        s.set_operator(Rc::downgrade(&op));
        slint::ComponentHandle::hide(&op.ui).unwrap();
        assert!(!op.window().is_visible());
        s.open("operator", None).unwrap();
        assert_eq!(op.raises.get(), 1);
        assert!(op.window().is_visible(), "shown again");
        assert!(!op.window().is_minimized(), "and not minimised");
        assert!(s.open_roles().is_empty(), "the operator is not a room or stage window");
        s.shutdown();
    }

    #[test]
    fn place_operator_maximises_the_window() {
        let app = test_app();
        let s = screens(&app);
        let op = Operator::open(app.clone(), crate::ui::ops::Ops::inline(app.clone()), Rc::new(s.clone())).unwrap();
        assert!(!op.window().is_maximized());
        s.place_operator(&op);
        assert!(op.window().is_maximized());
        s.shutdown();
    }

    #[test]
    fn operator_avoids_room_monitor() {
        let (laptop, projector) = (mon(r"\\.\DISPLAY1", 1920, 0), mon(r"\\.\DISPLAY2", 1280, 1920));
        let both = [laptop.clone(), projector.clone()];
        assert_eq!(operator_monitor(&both, Some(&laptop), Some(&projector)), Some(laptop.clone()), "the main monitor");
        assert_eq!(operator_monitor(&both, Some(&laptop), None), Some(laptop.clone()));
        assert_eq!(operator_monitor(&both, Some(&laptop), Some(&laptop)), Some(projector.clone()), "the room was saved on the main one");
        assert_eq!(operator_monitor(&both, None, Some(&laptop)), Some(projector.clone()), "no main known");
        assert_eq!(operator_monitor(&[laptop.clone()], Some(&laptop), Some(&laptop)), Some(laptop.clone()), "one monitor");
        assert_eq!(operator_monitor(&[], None, None), None);
        assert_eq!(operator_monitor(&[projector.clone()], Some(&laptop), None), Some(projector.clone()), "main unplugged");
    }

    #[test]
    fn room_is_on_top_only_on_a_monitor_of_its_own() {
        let (laptop, projector) = (mon(r"\\.\DISPLAY1", 1920, 0), mon(r"\\.\DISPLAY2", 2560, 1920));
        assert!(room_on_top(true, Some(&projector), Some(&laptop)));
        assert!(!room_on_top(false, Some(&projector), Some(&laptop)), "the setting turns it off");
        assert!(!room_on_top(true, Some(&laptop), Some(&laptop)), "never over the operator");
        assert!(!room_on_top(true, None, Some(&laptop)), "a test window is a plain window");
        assert!(!room_on_top(true, Some(&projector), None));

        let app = test_app();
        let s = screens(&app);
        s.look_at(vec![laptop.clone(), projector.clone()], Some(laptop.clone()));
        s.open("room", Some(&projector.key())).unwrap();
        assert!(s.room_should_be_on_top(), "full screen on the projector");
        app.data.lock().unwrap().settings.room_on_top = false;
        assert!(!s.room_should_be_on_top(), "turned off in Settings");
        app.data.lock().unwrap().settings.room_on_top = true;
        s.open("room", None).unwrap();
        assert!(!s.room_should_be_on_top(), "a test window");
        s.open("room", Some(&laptop.key())).unwrap();
        assert!(s.room_should_be_on_top(), "the room saved on the main monitor: the operator goes to the other one");
        s.shutdown();
    }

    #[test]
    fn stays_awake_while_a_screen_is_open() {
        let app = test_app();
        let s = screens(&app);
        assert!(!s.keeps_awake(), "nothing is showing");
        s.open("room", None).unwrap();
        assert!(s.keeps_awake(), "the room is up");
        s.open("stage", None).unwrap();
        s.close("room");
        assert!(s.keeps_awake(), "the stage screen is still up");
        s.close("stage");
        assert!(!s.keeps_awake(), "the computer can rest again");
        s.open("room", None).unwrap();
        s.shutdown();
        assert!(!s.keeps_awake());
    }

    #[test]
    fn opens_and_closes_windows_by_role() {
        let app = test_app();
        let s = screens(&app);
        s.open("stage", None).unwrap();
        s.open("room", None).unwrap();
        assert_eq!(s.open_roles(), ["stage", "room"]);
        s.open("stage", None).unwrap(); // replaces the open one
        assert_eq!(s.open_roles(), ["room", "stage"]);
        s.close("room");
        assert_eq!(s.open_roles(), ["stage"]);
        assert!(s.open("projector", None).is_err());
        s.shutdown();
        assert!(s.open_roles().is_empty());
    }

    #[test]
    fn open_native_windows_count_as_connected_screens() {
        let app = test_app();
        let s = screens(&app);
        s.open("room", None).unwrap();
        s.open("stage", None).unwrap();
        s.open("room", None).unwrap(); // a replaced window does not count twice
        assert_eq!((app.state_json()["room_connected"].clone(), app.state_json()["stage_connected"].clone()), (true.into(), true.into()));
        s.close("room");
        assert_eq!(app.state_json()["room_connected"], false);
        assert_eq!(app.room_conns.load(std::sync::atomic::Ordering::Relaxed), 0);
        s.shutdown();
        assert_eq!(app.state_json()["stage_connected"], false);
    }

    #[test]
    fn extra_screens_open_beside_the_main_ones() {
        let app = test_app();
        let s = screens(&app);
        s.open("room", None).unwrap();
        s.open("room:lobby", None).unwrap();
        s.open("stage:wall", None).unwrap();
        assert_eq!(s.open_roles(), ["room", "room:lobby", "stage:wall"]);
        assert_eq!(app.room_conns.load(Ordering::Relaxed), 1, "only the room counts as the room");
        s.close("room:lobby");
        assert_eq!(s.open_roles(), ["room", "stage:wall"]);
        assert!(s.open("projector:1", None).is_err());
        s.shutdown();
    }

    #[test]
    fn a_sized_window_takes_its_size() {
        let app = test_app();
        let s = screens(&app);
        Screens::open_window(&*s, "room", (640, 360)).unwrap();
        DESK.with(|d| assert_eq!(d.borrow().wins[0].2.window().size(), slint::PhysicalSize::new(640, 360)));
        s.shutdown();
    }

    #[test]
    fn identify_shows_a_badge_on_each_monitor_that_is_there() {
        let app = test_app();
        let s = screens(&app);
        let (laptop, projector) = (mon(r"\\.\DISPLAY1", 1920, 0), mon(r"\\.\DISPLAY2", 1280, 1920));
        s.look_at(vec![laptop.clone(), projector.clone()], Some(laptop.clone()));
        s.identify(vec![(laptop.key(), "1".into(), "This computer".into()), (projector.key(), "2".into(), "Audience".into()), ("gone".into(), "3".into(), "x".into())]);
        assert_eq!(DESK.with(|d| d.borrow().badges.len()), 2);
        s.shutdown();
        assert_eq!(DESK.with(|d| d.borrow().badges.len()), 0);
    }

    #[test]
    fn a_badge_sits_in_the_middle_of_its_monitor() {
        let m = Monitor { id: "x".into(), label: String::new(), x: 1920, y: 0, width: 1280, height: 720, primary: false };
        let ((x, y), (w, h)) = badge(&m);
        assert_eq!((w, h), (388, 259));
        assert_eq!((x - 1920, y), ((1280 - 388) / 2, (720 - 259) / 2));
    }

    #[test]
    fn unknown_monitor_is_a_readable_error() {
        let app = test_app();
        let s = screens(&app);
        s.look_at(vec![mon(r"\\.\DISPLAY1", 1920, 0)], None);
        assert_eq!(s.open("room", Some(r"\\.\DISPLAY9")), Err(not_connected()));
        assert!(s.open_roles().is_empty());
    }

    #[test]
    fn monitors_are_listed_in_the_web_shape() {
        let app = test_app();
        let s = screens(&app);
        let (laptop, projector) = (mon(r"\\.\DISPLAY1", 1920, 0), mon(r"\\.\DISPLAY2", 1280, 1920));
        s.look_at(vec![projector.clone(), laptop.clone()], Some(laptop.clone()));
        let list = s.monitors();
        assert_eq!(list.iter().map(|m| (m.id.clone(), m.primary)).collect::<Vec<_>>(), [(laptop.key(), true), (projector.key(), false)]);
    }

    #[test]
    fn room_monitor_gone_closes_it_and_tells_the_operator() {
        let app = test_app();
        let s = screens(&app);
        let (laptop, projector) = (mon(r"\\.\DISPLAY1", 1920, 0), mon(r"\\.\DISPLAY2", 1280, 1920));
        s.look_at(vec![laptop.clone(), projector.clone()], Some(laptop.clone()));
        s.open("room", Some(&projector.key())).unwrap();
        assert_eq!(s.open_roles(), ["room"]);
        s.look_at(vec![laptop.clone()], Some(laptop.clone()));
        assert!(s.open_roles().is_empty(), "the room window closes");
        assert_eq!(app.live_view().notice, Some(gone_notice("room")));
        assert!(gone_notice("room").contains("room screen was unplugged"));
        assert!(gone_notice("room:1").contains("extra audience screen was unplugged"));
        // Plugged in again: the room reopens on it, and the operator hears it restarted.
        s.look_at(vec![laptop.clone(), projector], Some(laptop));
        assert_eq!(s.open_roles(), ["room"]);
        assert_eq!(app.live_view().notice.as_deref(), Some("The room screen restarted."));
        s.shutdown();
    }

    #[test]
    fn closing_stops_the_unplug_watch() {
        let app = test_app();
        let s = screens(&app);
        let (laptop, projector) = (mon(r"\\.\DISPLAY1", 1920, 0), mon(r"\\.\DISPLAY2", 1280, 1920));
        s.look_at(vec![laptop.clone(), projector.clone()], Some(laptop.clone()));
        s.open("stage", Some(&projector.key())).unwrap();
        s.close("stage");
        s.look_at(vec![laptop.clone()], Some(laptop.clone()));
        s.look_at(vec![laptop.clone(), projector], Some(laptop));
        assert!(s.open_roles().is_empty(), "a closed window does not come back");
        assert_eq!(app.live_view().notice, None);
    }

    #[test]
    fn screens_wait_for_the_switch_when_not_turned_on_at_start() {
        let app = test_app();
        app.edit(|d| { d.settings.screens.assign("audience", ScreenTarget::Window); d.settings.screens.assign("stage", ScreenTarget::Window); Ok(()) }).unwrap();
        assert_eq!(start_windows(&app, false).len(), 2, "on by default: what was on comes back");
        app.edit(|d| { d.settings.screens_at_start = false; Ok(()) }).unwrap();
        assert_eq!(start_windows(&app, true).len(), 2, "after a crash what was open always comes back");
        assert!(start_windows(&app, false).is_empty());
        let screens = app.data.lock().unwrap().settings.screens.clone();
        assert!(!screens.audience.on && !screens.stage.on, "the switches show off, so one click turns them on");
        assert_eq!(screens.audience.screens[0].output().target, ScreenTarget::Window, "where they show is kept");
    }

    #[test]
    fn stage_preview_needs_the_stage_window() {
        let app = test_app();
        let s = screens(&app);
        assert!(s.stage_snapshot(320).is_none(), "no stage window, no picture");
        s.shutdown();
    }
}
