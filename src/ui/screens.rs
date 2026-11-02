//! The native windowing layer (`Screens`): room and stage windows live on the Slint thread and are
//! opened and closed from any thread through it. They reopen from settings at start, close when
//! their monitor is unplugged and come back on it when it returns.
use super::{room::Room, stage::Stage};
use crate::app::{App, Monitor, ScreenGuard, Screens};
use crate::platform::monitors::{self, Change, MonitorId, Watcher, pick, to_monitors};
use crate::render::Renderer;
use crate::video::ffi::Mpv;
use slint::{CloseRequestResponse, Timer, TimerMode, winit_030::{invoke_from_active_event_loop, winit::event_loop::ActiveEventLoop}};
use std::{cell::RefCell, rc::Rc, sync::{Arc, Mutex, Weak, mpsc}, thread::ThreadId, time::Duration};

const NOT_CONNECTED: &str = "That screen is not connected right now.";

/// How the operator hears about a role's window.
fn named(role: &str) -> &'static str { if role == "room" { "room screen" } else { "stage display" } }

/// The notice while `role`'s monitor is unplugged.
pub fn gone_notice(role: &str) -> String {
    format!("The {} was unplugged. It comes back by itself when that screen is plugged in again.", named(role))
}

enum Win { Room(Rc<Room>), Stage(Rc<Stage>) }

impl Win {
    fn window(&self) -> &slint::Window { match self { Win::Room(r) => r.window(), Win::Stage(s) => s.window() } }
}

/// The Slint thread's side: open windows (role, opening number, window, the guard that marks the
/// role connected while it is open), the unplug watch and its timer.
#[derive(Default)]
struct Desk { wins: Vec<(String, u64, Win, ScreenGuard)>, watcher: Watcher, poll: Option<Timer>, seq: u64 }

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
}

impl NativeScreens {
    /// Call on the Slint thread. `mpv` is `None` when libmpv is missing.
    pub fn new(app: &Arc<App>, renderer: Arc<Renderer>, mpv: Option<Arc<Mpv>>) -> Arc<Self> {
        Arc::new_cyclic(|me| NativeScreens {
            app: Arc::downgrade(app), renderer, mpv, ui: std::thread::current().id(), me: me.clone(),
            seen: Mutex::default(), roles: Mutex::default(),
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

    /// Closes every window (frees players and GL contexts) and stops the watch. Call on the Slint thread.
    pub fn shutdown(&self) {
        let desk = DESK.with(|d| std::mem::take(&mut *d.borrow_mut()));
        drop(desk);
        self.roles.lock().unwrap().clear();
    }

    /// Runs `f` on the Slint thread and waits for it (never waits when already there).
    fn on_ui<T: Send + 'static>(&self, f: impl FnOnce(&NativeScreens) -> T + Send + 'static) -> Result<T, String> {
        if std::thread::current().id() == self.ui { return Ok(f(self)) }
        let (tx, rx) = mpsc::sync_channel(1);
        let me = self.me.clone();
        invoke_from_active_event_loop(move |el| if let Some(s) = me.upgrade() { s.look(el); let _ = tx.send(f(&s)); })
            .map_err(|_| "The screens are not running.".to_string())?;
        rx.recv_timeout(Duration::from_secs(10)).map_err(|_| "The screens did not answer in time.".to_string())
    }

    fn look(&self, el: &ActiveEventLoop) {
        let (list, main) = monitors::on_event_loop(el);
        self.look_at(list, main);
    }

    /// Takes in the monitors connected now and acts on unplugs and returns.
    fn look_at(&self, list: Vec<MonitorId>, main: Option<MonitorId>) {
        let changes = DESK.with(|d| d.borrow_mut().watcher.step(&list));
        *self.seen.lock().unwrap() = (list, main);
        for c in changes { self.changed(c) }
    }

    /// Reopen room and stage from settings, never on the operator's (main) monitor by itself.
    fn reopen(&self) {
        let Some(app) = self.app.upgrade() else { return };
        let saved = app.data.lock().unwrap().settings.displays.clone();
        let (list, main) = self.seen.lock().unwrap().clone();
        for (role, id) in [("room", saved.room), ("stage", saved.stage)] {
            let Some(m) = id.and_then(|id| pick(&list, Some(&id), main.as_ref())).filter(|m| Some(m) != main.as_ref()) else { continue };
            if let Err(e) = self.show(role, Some(m)) { eprintln!("Could not reopen the {}: {e}", named(role)) }
        }
        app.broadcast_data();
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
                    Err(e) => live.notice = Some(format!("The {} is plugged in again, but it could not open: {e}", named(&role))),
                    Ok(()) => if live.notice.as_deref() == Some(gone_notice(&role).as_str()) { live.notice = None },
                }
            }
        }
        app.broadcast_live();
        app.broadcast_data();
    }

    fn open_here(&self, role: &str, monitor: Option<&str>) -> Result<(), String> {
        let mon = match monitor {
            Some(saved) => Some(pick(&self.seen.lock().unwrap().0, Some(saved), None).ok_or(NOT_CONNECTED)?),
            None => None,
        };
        self.show(role, mon)
    }

    /// Opens `role`'s window, full screen on `mon` (a plain window without one), replacing an open one.
    fn show(&self, role: &str, mon: Option<MonitorId>) -> Result<(), String> {
        let app = self.app.upgrade().ok_or("RustyPresenter is closing.")?;
        drop(self.take(role, None)); // the old window's players and GL context go first
        let guard = app.screen_guard(role);
        let win = match role {
            "room" => Win::Room(Room::open(app, self.renderer.clone(), self.mpv.clone(), mon.clone())?),
            "stage" => Win::Stage(Stage::open(app, mon.clone())?),
            _ => return Err("unknown window".into()),
        };
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
            match mon { Some(m) => d.watcher.watch(role, m), None => d.watcher.unwatch(role) }
        });
        self.sync_roles();
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

    fn sync_roles(&self) {
        *self.roles.lock().unwrap() = DESK.with(|d| d.borrow().wins.iter().map(|(r, _, _, _)| r.clone()).collect());
    }
}

impl Screens for NativeScreens {
    fn open(&self, role: &str, monitor: Option<&str>) -> Result<(), String> {
        match role {
            // The operator is the web page until the native operator window exists (Plan 3).
            "operator" => { if let Some(app) = self.app.upgrade() { open_browser(&format!("http://127.0.0.1:{}/", app.port)) } Ok(()) }
            "room" | "stage" => {
                let (role, monitor) = (role.to_string(), monitor.map(str::to_string));
                self.on_ui(move |s| s.open_here(&role, monitor.as_deref()))?
            }
            _ => Err("unknown window".into()),
        }
    }

    fn close(&self, role: &str) {
        let role = role.to_string();
        let closed = self.on_ui(move |s| { drop(s.take(&role, None)); DESK.with(|d| d.borrow_mut().watcher.unwatch(&role)) });
        if let Err(e) = closed { eprintln!("Could not close the window: {e}") }
    }

    fn open_roles(&self) -> Vec<String> { self.roles.lock().unwrap().clone() }

    /// As last seen (at most 2 s old): never waits for the Slint thread, so it is safe under `App`'s locks.
    fn monitors(&self) -> Vec<Monitor> {
        let (list, main) = self.seen.lock().unwrap().clone();
        to_monitors(&list, main.as_ref())
    }
}

/// Opens `url` in the system browser.
pub fn open_browser(url: &str) {
    use std::process::Command;
    #[cfg(windows)]
    let child = { use std::os::windows::process::CommandExt; Command::new("cmd").args(["/C", "start", "", url]).creation_flags(0x0800_0000).spawn() };
    #[cfg(target_os = "macos")]
    let child = Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let child = Command::new("xdg-open").arg(url).spawn();
    match child {
        Ok(mut c) => { let _ = std::thread::Builder::new().name("browser".into()).spawn(move || c.wait()); }
        Err(e) => eprintln!("Could not open {url} in the browser: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_app;

    fn mon(name: &str, w: u32, x: i32) -> MonitorId { MonitorId { name: name.into(), size: (w, w * 9 / 16), position: (x, 0) } }

    fn screens(app: &Arc<App>) -> Arc<NativeScreens> {
        i_slint_backend_testing::init_no_event_loop();
        NativeScreens::new(app, Renderer::new(), None)
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
    fn unknown_monitor_is_a_readable_error() {
        let app = test_app();
        let s = screens(&app);
        s.look_at(vec![mon(r"\\.\DISPLAY1", 1920, 0)], None);
        assert_eq!(s.open("room", Some(r"\\.\DISPLAY9")), Err(NOT_CONNECTED.into()));
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
}
