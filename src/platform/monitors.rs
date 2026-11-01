//! Monitors for the native room and stage windows: stable ids, choosing the saved
//! monitor, borderless fullscreen through Slint's winit backend, and an unplug watch.
use crate::app::Monitor;
use serde::{Deserialize, Serialize};
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
/// not the operator's. Never the operator's monitor unless it was the saved one.
pub fn pick(monitors: &[MonitorId], saved: Option<&str>, operator: Option<&MonitorId>) -> Option<MonitorId> {
    let Some(saved) = saved else { return monitors.iter().find(|m| Some(*m) != operator).cloned() };
    if let Some(m) = monitors.iter().find(|m| m.matches_saved(saved)) { return Some(m.clone()) }
    let id = MonitorId::parse(saved)?;
    let named: Vec<_> = monitors.iter().filter(|m| m.name == id.name).collect();
    // Twins share a name: the operator's monitor counts as the saved one moved only when it is the only one.
    match named[..] {
        [only] => Some(only.clone()),
        _ => named.into_iter().find(|m| Some(*m) != operator).cloned(),
    }
}

/// The web operator's Screens dialog shape, numbered left to right like `displays::monitors`.
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

fn handles(w: &winit::window::Window) -> Vec<(MonitorHandle, MonitorId)> {
    w.available_monitors().enumerate().map(|(i, m)| {
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

/// The system's main monitor.
pub fn primary(window: &slint::Window) -> Option<MonitorId> { window.with_winit_window(|w| id_of(w, w.primary_monitor())).flatten() }

/// The monitor `window` is on (the operator's, for `pick`).
pub fn current(window: &slint::Window) -> Option<MonitorId> { window.with_winit_window(|w| id_of(w, w.current_monitor())).flatten() }

/// Borderless fullscreen on `id` (no video mode change), found exactly or by name.
pub fn fullscreen_on(window: &slint::Window, id: &MonitorId) -> Result<(), String> {
    window.with_winit_window(|w| {
        let all = handles(w);
        let (m, _) = all.iter().find(|(_, m)| m == id).or_else(|| all.iter().find(|(_, m)| m.name == id.name))
            .ok_or("That screen is not connected right now.")?;
        w.set_fullscreen(Some(Fullscreen::Borderless(Some(m.clone()))));
        Ok(())
    }).unwrap_or_else(|| Err("This window cannot go full screen.".into()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// The role's monitor was unplugged: close its window and tell the operator.
    Gone(String),
    /// The same monitor is back, where it is now: reopen the window on it.
    Back(String, MonitorId),
}

/// Watches the monitors that room and stage windows are on. Pure: feed it `list()` with `step`.
#[derive(Default)]
pub struct Watcher { watched: Vec<(String, MonitorId, bool)> }

impl Watcher {
    /// `role`'s window is on `id`; replaces an earlier watch for that role.
    pub fn watch(&mut self, role: &str, id: MonitorId) {
        self.unwatch(role);
        self.watched.push((role.into(), id, true));
    }

    /// The operator closed `role`'s window: stop watching for its monitor.
    pub fn unwatch(&mut self, role: &str) { self.watched.retain(|(r, _, _)| r != role); }

    /// Compare with the monitors connected now; each change is reported once.
    pub fn step(&mut self, monitors: &[MonitorId]) -> Vec<Change> {
        let mut out = vec![];
        for (role, id, present) in &mut self.watched {
            let found = monitors.iter().find(|m| *m == id).or_else(|| monitors.iter().find(|m| m.name == id.name));
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
        w.watch("room", projector());
        w.watch("stage", laptop());
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
