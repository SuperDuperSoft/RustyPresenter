//! Monitors for the native room and stage windows: stable ids, choosing the saved
//! monitor, borderless fullscreen through Slint's winit backend, and an unplug watch.
use crate::app::Monitor;
use serde::{Deserialize, Serialize};
use slint::ComponentHandle;
use slint::winit_030::{WinitWindowAccessor, winit::{self, monitor::MonitorHandle, window::Fullscreen}};
use std::{cell::RefCell, rc::Rc, time::Duration};

/// A monitor as winit reports it, in physical pixels. Saved as `name|w×h|x,y`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MonitorId { pub name: String, pub size: (u32, u32), pub position: (i32, i32) }

impl MonitorId {
    /// The saved form, `name|w×h|x,y`; also the `id` the web Screens dialog sends back.
    pub fn key(&self) -> String {
        format!("{}|{}×{}|{},{}", self.name, self.size.0, self.size.1, self.position.0, self.position.1)
    }

    /// Read a saved `name|w×h|x,y`; `None` for anything else (such as a bare Windows device name).
    pub fn parse(saved: &str) -> Option<MonitorId> {
        let mut parts = saved.rsplitn(3, '|');
        let (pos, size, name) = (parts.next()?, parts.next()?, parts.next()?);
        let (w, h) = size.split_once('×')?;
        let (x, y) = pos.split_once(',')?;
        Some(MonitorId { name: name.into(), size: (w.parse().ok()?, h.parse().ok()?), position: (x.parse().ok()?, y.parse().ok()?) })
    }

    /// A saved device name (`\\.\DISPLAY2`, today's `DisplayRoles`) matches by name; a saved key matches all three.
    pub fn matches_saved(&self, saved: &str) -> bool {
        match MonitorId::parse(saved) { Some(id) => id == *self, None => self.name == saved }
    }
}

/// The monitor for a room or stage window. The saved choice wins, exactly first, then by name
/// (the same monitor at a new resolution or place). With nothing saved, the first monitor that is
/// not the operator's. Never the operator's monitor unless it was the saved one exactly.
pub fn pick(monitors: &[MonitorId], saved: Option<&str>, operator: Option<&MonitorId>) -> Option<MonitorId> {
    let Some(saved) = saved else { return monitors.iter().find(|m| Some(*m) != operator).cloned() };
    if let Some(m) = monitors.iter().find(|m| m.matches_saved(saved)) { return Some(m.clone()) }
    let id = MonitorId::parse(saved)?;
    let named: Vec<_> = monitors.iter().filter(|m| m.name == id.name).collect();
    // Twins share a name: by name alone, the operator's monitor is never taken for the saved one.
    named.into_iter().find(|m| Some(*m) != operator).cloned()
}

/// The web operator's Screens dialog shape, numbered left to right.
pub fn to_monitors(ids: &[MonitorId], primary: Option<&MonitorId>) -> Vec<Monitor> {
    let mut ids: Vec<_> = ids.iter().collect();
    ids.sort_by_key(|m| m.position);
    ids.into_iter().enumerate().map(|(i, m)| {
        let primary = Some(m) == primary;
        Monitor {
            id: m.key(),
            label: format!("Screen {}{} — {}×{}", i + 1, if primary { " (main)" } else { "" }, m.size.0, m.size.1),
            x: m.position.0, y: m.position.1, width: m.size.0 as i32, height: m.size.1 as i32, primary,
        }
    }).collect()
}

fn handles(w: &winit::window::Window) -> Vec<(MonitorHandle, MonitorId)> { ids(w.available_monitors()) }

fn ids(all: impl Iterator<Item = MonitorHandle>) -> Vec<(MonitorHandle, MonitorId)> {
    all.enumerate().map(|(i, m)| {
        let (s, p) = (m.size(), m.position());
        let id = MonitorId { name: m.name().unwrap_or_else(|| format!("Screen {}", i + 1)), size: (s.width, s.height), position: (p.x, p.y) };
        (m, id)
    }).collect()
}

fn id_of(w: &winit::window::Window, m: Option<MonitorHandle>) -> Option<MonitorId> {
    let m = m?;
    handles(w).into_iter().find(|(h, _)| *h == m).map(|(_, id)| id)
}

/// Connected monitors, through the winit window behind `window` (empty without one).
pub fn list(window: &slint::Window) -> Vec<MonitorId> {
    window.with_winit_window(|w| handles(w).into_iter().map(|(_, id)| id).collect()).unwrap_or_default()
}

/// Connected monitors and the main one, without a window (from `slint::winit_030::invoke_from_active_event_loop`).
pub fn on_event_loop(el: &winit::event_loop::ActiveEventLoop) -> (Vec<MonitorId>, Option<MonitorId>) {
    let all = ids(el.available_monitors());
    let main = el.primary_monitor().and_then(|p| all.iter().find(|(h, _)| *h == p).map(|(_, id)| id.clone()));
    (all.into_iter().map(|(_, id)| id).collect(), main)
}

/// The system's main monitor.
pub fn primary(window: &slint::Window) -> Option<MonitorId> { window.with_winit_window(|w| id_of(w, w.primary_monitor())).flatten() }

/// The monitor `window` is on (the operator's, for `pick`).
pub fn current(window: &slint::Window) -> Option<MonitorId> { window.with_winit_window(|w| id_of(w, w.current_monitor())).flatten() }

/// Fullscreen again on `id` when the window already exists (it does nothing before then).
pub fn refullscreen(window: &slint::Window, id: &MonitorId) -> Result<(), String> { borderless(window, id).unwrap_or(Ok(())) }

/// Borderless fullscreen on `id` (no video mode change), found exactly or by name.
pub fn fullscreen_on<C: ComponentHandle + 'static>(ui: &C, id: &MonitorId) -> Result<(), String> {
    if let Some(r) = borderless(ui.window(), id) { return r }
    // Slint creates the winit window on the event loop's next turn after `show`. Until then, cover
    // exactly that monitor, so whatever winit takes for "the window's monitor" is it, and ask for
    // fullscreen; then name the monitor itself as soon as the winit window exists.
    let ((x, y), (w, h)) = placement(id);
    let window = ui.window();
    window.set_position(slint::PhysicalPosition::new(x, y));
    window.set_size(slint::PhysicalSize::new(w, h));
    window.set_fullscreen(true);
    retry(ui.as_weak(), id.clone(), 100);
    Ok(())
}

/// Maximises `ui`'s window through winit as soon as the winit window exists. Slint's own `set_maximized` before then
/// is lost on Windows: the new window takes the component's preferred size instead (1440 × 900, taller than a
/// 1920 × 1080 screen at 125 %).
pub fn maximise<C: ComponentHandle + 'static>(ui: &C) {
    ui.window().set_maximized(true);
    if ui.window().with_winit_window(|w| w.set_maximized(true)).is_none() { retry_maximise(ui.as_weak(), 100) }
}

fn retry_maximise<C: ComponentHandle + 'static>(ui: slint::Weak<C>, tries: u32) {
    slint::Timer::single_shot(Duration::from_millis(10), move || {
        let Some(c) = ui.upgrade() else { return };
        if c.window().with_winit_window(|w| w.set_maximized(true)).is_none() && tries > 0 { retry_maximise(ui, tries - 1) }
    });
}

/// Where a window waiting for fullscreen on `id` goes: the monitor's own rectangle, wholly on it.
fn placement(id: &MonitorId) -> ((i32, i32), (u32, u32)) { (id.position, id.size) }

/// `Borderless(Some(monitor))` through the winit window; `None` while there is none yet.
fn borderless(window: &slint::Window, id: &MonitorId) -> Option<Result<(), String>> {
    window.with_winit_window(|w| {
        let all = handles(w);
        let (m, _) = all.iter().find(|(_, m)| m == id).or_else(|| all.iter().find(|(_, m)| m.name == id.name))
            .ok_or("That screen is not connected right now.")?;
        w.set_fullscreen(Some(Fullscreen::Borderless(Some(m.clone()))));
        Ok(())
    })
}

fn retry<C: ComponentHandle + 'static>(ui: slint::Weak<C>, id: MonitorId, tries: u32) {
    slint::Timer::single_shot(Duration::from_millis(10), move || {
        let Some(c) = ui.upgrade() else { return };
        if borderless(c.window(), &id).is_none() && tries > 0 { retry(ui, id, tries - 1) }
    });
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// The role's monitor was unplugged: close its window and tell the operator.
    Gone(String),
    /// The same monitor is back, where it is now: reopen the window on it.
    Back(String, MonitorId),
}

/// Watches the monitors that room and stage windows are on. Pure: feed it `list()` with `step`.
/// Each watch is (role, its monitor, connected, the monitors seen at the last look).
#[derive(Default)]
pub struct Watcher { watched: Vec<(String, MonitorId, bool, Vec<MonitorId>)> }

impl Watcher {
    /// `role`'s window is on `id`, with `now` connected; replaces an earlier watch for that role.
    pub fn watch(&mut self, role: &str, id: MonitorId, now: &[MonitorId]) {
        self.unwatch(role);
        self.watched.push((role.into(), id, true, now.to_vec()));
    }

    /// The monitor `role`'s window is on while that monitor is connected.
    pub fn monitor_of(&self, role: &str) -> Option<&MonitorId> {
        self.watched.iter().find(|(r, _, present, _)| r == role && *present).map(|(_, id, ..)| id)
    }

    /// The operator closed `role`'s window: stop watching for its monitor.
    pub fn unwatch(&mut self, role: &str) { self.watched.retain(|(r, ..)| r != role); }

    /// The monitor `role`'s window belongs on, when it is connected but the window is `on` another
    /// (Windows moves it to the operator's when its monitor drops out and back between two looks).
    pub fn misplaced(&self, role: &str, on: Option<&MonitorId>) -> Option<MonitorId> {
        let (_, id, present, _) = self.watched.iter().find(|(r, ..)| r == role)?;
        (*present && on.is_some_and(|on| on != id)).then(|| id.clone())
    }

    /// Compare with the monitors connected now; each change is reported once. A monitor found by
    /// name alone counts only when it is the one new monitor with that name: a twin that was
    /// already connected (often the operator's) is never taken for the watched one.
    pub fn step(&mut self, monitors: &[MonitorId]) -> Vec<Change> {
        let mut out = vec![];
        for (role, id, present, seen) in &mut self.watched {
            let seen = std::mem::replace(seen, monitors.to_vec());
            let fresh: Vec<_> = monitors.iter().filter(|m| m.name == id.name && !seen.contains(m)).collect();
            let found = monitors.iter().find(|m| *m == id).or(match fresh[..] { [only] => Some(only), _ => None });
            match (found, *present) {
                (None, true) => { *present = false; out.push(Change::Gone(role.clone())); }
                (Some(m), false) => { *present = true; *id = m.clone(); out.push(Change::Back(role.clone(), m.clone())); }
                (Some(m), true) => *id = m.clone(),
                (None, false) => {}
            }
        }
        out
    }
}

/// Run `watcher` every 2 s on the Slint event loop. `list` returns `None` when it cannot look
/// (no window to ask). The watch runs while the returned timer lives.
pub fn poll(watcher: Rc<RefCell<Watcher>>, list: impl Fn() -> Option<Vec<MonitorId>> + 'static, on: impl Fn(Change) + 'static) -> slint::Timer {
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_secs(2), move || {
        let Some(now) = list() else { return };
        let changes = watcher.borrow_mut().step(&now);
        changes.into_iter().for_each(&on);
    });
    timer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(name: &str, w: u32, h: u32, x: i32, y: i32) -> MonitorId { MonitorId { name: name.into(), size: (w, h), position: (x, y) } }
    fn laptop() -> MonitorId { mon(r"\.\DISPLAY1", 1920, 1080, 0, 0) }
    fn projector() -> MonitorId { mon(r"\.\DISPLAY2", 1280, 720, 1920, 0) }

    #[test]
    fn windows_device_name_matches_saved_choice() {
        assert!(projector().matches_saved(r"\.\DISPLAY2"));
        assert!(!laptop().matches_saved(r"\.\DISPLAY2"));
        assert_eq!(pick(&[laptop(), projector()], Some(r"\.\DISPLAY2"), Some(&laptop())), Some(projector()));
    }

    #[test]
    fn exact_triple_beats_name_only() {
        let p = projector();
        assert_eq!(p.key(), r"\.\DISPLAY2|1280×720|1920,0");
        assert!(p.matches_saved(&p.key()));
        assert!(!mon(r"\.\DISPLAY2", 1920, 1080, 1920, 0).matches_saved(&p.key()));
        // Two monitors with the same name (identical models on macOS/Linux): the exact one wins.
        let twin = mon("DELL U2414H", 1920, 1080, 0, 0);
        let other = mon("DELL U2414H", 1920, 1080, 1920, 0);
        assert_eq!(pick(&[twin.clone(), other.clone()], Some(&other.key()), None), Some(other.clone()));
        // The saved twin moved: name-only never lands on the operator's twin.
        let moved = mon("DELL U2414H", 1920, 1080, 3840, 0);
        assert_eq!(pick(&[twin.clone(), moved.clone()], Some(&other.key()), Some(&twin)), Some(moved));
        // The saved twin is gone: the operator's twin, the only one left with that name, is not it.
        assert_eq!(pick(&[twin.clone()], Some(&other.key()), Some(&twin)), None);
    }

    #[test]
    fn name_only_used_when_resolution_changed() {
        let saved = projector().key();
        let now = mon(r"\.\DISPLAY2", 1920, 1080, 1920, 0);
        assert!(!now.matches_saved(&saved));
        assert_eq!(pick(&[laptop(), now.clone()], Some(&saved), Some(&laptop())), Some(now));
        // A saved monitor that is not connected is not replaced by another one.
        assert_eq!(pick(&[laptop(), mon(r"\.\DISPLAY3", 800, 600, -800, 0)], Some(&saved), Some(&laptop())), None);
    }

    #[test]
    fn never_picks_operator_monitor_implicitly() {
        assert_eq!(pick(&[laptop()], None, Some(&laptop())), None);
        assert_eq!(pick(&[laptop(), projector()], None, Some(&laptop())), Some(projector()));
        assert_eq!(pick(&[laptop()], Some(r"\.\DISPLAY9"), Some(&laptop())), None);
    }

    #[test]
    fn explicitly_saved_operator_monitor_is_allowed() {
        assert_eq!(pick(&[laptop(), projector()], Some(r"\.\DISPLAY1"), Some(&laptop())), Some(laptop()));
        assert_eq!(pick(&[laptop()], Some(&laptop().key()), Some(&laptop())), Some(laptop()));
    }

    #[test]
    fn watcher_reports_gone_and_back() {
        let mut w = Watcher::default();
        w.watch("room", projector(), &[laptop(), projector()]);
        w.watch("stage", laptop(), &[laptop(), projector()]);
        assert_eq!(w.step(&[laptop(), projector()]), vec![]);
        assert_eq!(w.step(&[laptop()]), vec![Change::Gone("room".into())]);
        assert_eq!(w.step(&[laptop()]), vec![], "gone is reported once");
        // Back at a new resolution: still the same monitor, reported with where it is now.
        let back = mon(r"\.\DISPLAY2", 1920, 1080, 1920, 0);
        assert_eq!(w.step(&[laptop(), back.clone()]), vec![Change::Back("room".into(), back.clone())]);
        assert_eq!(w.step(&[laptop(), back]), vec![]);
        w.unwatch("room");
        assert_eq!(w.step(&[laptop()]), vec![]);
        assert_eq!(w.step(&[]), vec![Change::Gone("stage".into())]);
    }

    #[test]
    fn watcher_never_swaps_to_a_twin() {
        // Identical models share a name (macOS/Linux); the operator is on one, the room on the other.
        let (ops, room) = (mon("DELL U2414H", 1920, 1080, 0, 0), mon("DELL U2414H", 1920, 1080, 1920, 0));
        let mut w = Watcher::default();
        w.watch("room", room.clone(), &[ops.clone(), room.clone()]);
        assert_eq!(w.step(&[ops.clone(), room.clone()]), vec![]);
        assert_eq!(w.step(&[ops.clone()]), vec![Change::Gone("room".into())], "the operator's twin is not the room's monitor");
        assert_eq!(w.step(&[ops.clone()]), vec![]);
        assert_eq!(w.step(&[ops.clone(), room.clone()]), vec![Change::Back("room".into(), room.clone())]);
        // Plugged back somewhere new: the twin that appeared is the one, not the operator's.
        assert_eq!(w.step(&[ops.clone()]), vec![Change::Gone("room".into())]);
        let moved = mon("DELL U2414H", 2560, 1440, 3840, 0);
        assert_eq!(w.step(&[ops.clone(), moved.clone()]), vec![Change::Back("room".into(), moved)]);
    }

    #[test]
    fn watcher_first_step_never_takes_a_twin_already_there() {
        // The room's monitor is gone before the first look; the operator's twin was there all along.
        let (ops, room) = (mon("DELL U2414H", 1920, 1080, 0, 0), mon("DELL U2414H", 1920, 1080, 1920, 0));
        let mut w = Watcher::default();
        w.watch("room", room.clone(), &[ops.clone()]);
        assert_eq!(w.step(&[ops.clone()]), vec![Change::Gone("room".into())]);
        assert_eq!(w.step(&[ops.clone(), room.clone()]), vec![Change::Back("room".into(), room)]);
    }

    #[test]
    fn watcher_finds_a_window_moved_off_its_monitor() {
        // Unplugged and back within one look: the list is the same, but Windows moved the window.
        let mut w = Watcher::default();
        w.watch("room", projector(), &[laptop(), projector()]);
        assert_eq!(w.step(&[laptop(), projector()]), vec![]);
        assert_eq!(w.misplaced("room", Some(&laptop())), Some(projector()));
        assert_eq!(w.misplaced("room", Some(&projector())), None);
        assert_eq!(w.misplaced("room", None), None, "no answer, no move");
        assert_eq!(w.misplaced("stage", Some(&laptop())), None);
        assert_eq!(w.step(&[laptop()]), vec![Change::Gone("room".into())]);
        assert_eq!(w.misplaced("room", Some(&laptop())), None, "a gone monitor is not moved to");
    }

    fn overlap(a: ((i32, i32), (u32, u32)), m: &MonitorId) -> i64 {
        let ((ax, ay), (aw, ah)) = a;
        let w = (ax + aw as i32).min(m.position.0 + m.size.0 as i32) - ax.max(m.position.0);
        let h = (ay + ah as i32).min(m.position.1 + m.size.1 as i32) - ay.max(m.position.1);
        w.max(0) as i64 * h.max(0) as i64
    }

    #[test]
    fn placement_lies_wholly_on_a_small_projector() {
        // winit's fallback takes the monitor with the largest overlap, so none may lie on the laptop.
        for p in [mon(r"\.\DISPLAY2", 1024, 768, -1024, 0), mon(r"\.\DISPLAY2", 1024, 768, 0, -768), projector()] {
            let at = placement(&p);
            assert_eq!(overlap(at, &p), p.size.0 as i64 * p.size.1 as i64);
            assert_eq!(overlap(at, &laptop()), 0);
        }
    }

    #[test]
    fn converts_to_the_web_screens_shape() {
        let list = to_monitors(&[projector(), laptop()], Some(&laptop()));
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, laptop().key());
        assert_eq!(list[0].label, "Screen 1 (main) — 1920×1080");
        assert!(list[0].primary);
        assert_eq!((list[1].x, list[1].y, list[1].width, list[1].height, list[1].primary), (1920, 0, 1280, 720, false));
        assert_eq!(list[1].label, "Screen 2 — 1280×720");
    }
}
