//! The Settings window (November 17, 2026: Jake, "I should be able to have my settings open as a separate window that
//! can be large or shrink or whatever"). A real window of its own beside the operator, any size, which keeps its size,
//! place and monitor between openings (back on the main monitor when that one is gone), with the page and Show
//! advanced settings. Slint keeps each window's globals apart, so every `SettingsDlg` (and `ThemesDlg`, `KeysDlg`,
//! `SectionsDlg`, `NetworkDlg`, `StreamMoreDlg`) the Settings code reads is this window's: `win(op)` gives it.
use crate::{platform::monitors::MonitorId, ui::{SettingsWindow, operator::window::Operator}};
use serde::{Deserialize, Serialize};
use slint::ComponentHandle;
use std::{cell::{Cell, RefCell}, path::{Path, PathBuf}};

thread_local! {
    /// The Settings window of this thread's operator (made the first time it is needed, then kept: closing hides it),
    /// and which operator it belongs to (tests make several operators on one thread).
    static WIN: RefCell<Option<(usize, &'static SettingsWindow)>> = const { RefCell::new(None) };
    /// Whether it is open now.
    static OPEN: Cell<bool> = const { Cell::new(false) };
}

fn owner(op: &Operator) -> usize { op as *const Operator as usize }

/// `op`'s Settings window. It lives as long as the app (one a thread; a test's operator gets a new one).
pub fn win(op: &Operator) -> &'static SettingsWindow {
    WIN.with(|w| {
        let mut w = w.borrow_mut();
        match *w {
            Some((who, win)) if who == owner(op) => win,
            _ => {
                let made: &'static SettingsWindow = Box::leak(Box::new(SettingsWindow::new().expect("the Settings window")));
                // This window's own copy of the filter boxes' matching (Themes and backgrounds).
                made.global::<crate::ui::ListFilter>().on_matches(|text, query| crate::ui::operator::list_filter::matches(&text, &query));
                *w = Some((owner(op), made));
                OPEN.with(|o| o.set(false));
                made
            }
        }
    })
}

/// Whether `op`'s Settings window is open.
pub fn is_open(op: &Operator) -> bool {
    WIN.with(|w| matches!(*w.borrow(), Some((who, _)) if who == owner(op))) && OPEN.with(Cell::get)
}

pub(crate) fn set_open(open: bool) { OPEN.with(|o| o.set(open)) }

/// What is remembered on this computer: the window's place (physical pixels), the monitor it was on, the last page
/// and Show advanced settings. Kept in the data folder's `settings-window.json` (not in the settings or a backup:
/// another computer has other monitors).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Remembered {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// `MonitorId::key` of the monitor it was on; empty: never placed.
    pub monitor: String,
    /// `SettingsTab::key` of the last page shown.
    pub page: String,
    pub advanced: bool,
}

pub fn file(root: &Path) -> PathBuf { root.join("settings-window.json") }

/// What was remembered; nothing (the defaults) when the file is missing or unreadable.
pub fn load(root: &Path) -> Remembered {
    std::fs::read_to_string(file(root)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save(root: &Path, r: &Remembered) {
    if let Ok(t) = serde_json::to_string_pretty(r) && let Err(e) = std::fs::write(file(root), t) { eprintln!("settings window: {e}") }
}

/// Where the window goes: its remembered place when that monitor is still connected (kept wholly on it), else the
/// main monitor (or the first), a little in from its corner, no bigger than it. `None` with nothing to go on.
pub fn place(r: &Remembered, monitors: &[MonitorId], main: Option<&MonitorId>) -> Option<((i32, i32), (u32, u32))> {
    let fit = |m: &MonitorId, (x, y): (i32, i32), (w, h): (u32, u32)| {
        let (w, h) = (w.min(m.size.0).max(400), h.min(m.size.1).max(300));
        let x = x.clamp(m.position.0, m.position.0 + m.size.0 as i32 - w as i32);
        let y = y.clamp(m.position.1, m.position.1 + m.size.1 as i32 - h as i32);
        ((x, y), (w, h))
    };
    if r.width > 0 && let Some(m) = monitors.iter().find(|m| m.matches_saved(&r.monitor)) {
        return Some(fit(m, (r.x, r.y), (r.width, r.height)));
    }
    let m = main.or(monitors.first())?;
    let (w, h) = if r.width > 0 { (r.width, r.height) } else { (m.size.0 * 7 / 10, m.size.1 * 8 / 10) };
    Some(fit(m, (m.position.0 + 60, m.position.1 + 60), (w, h)))
}

/// Puts the window where it was (or on the main monitor), before it is shown.
pub fn restore(op: &Operator, r: &Remembered) {
    let w = win(op);
    let monitors = crate::platform::monitors::list(op.window());
    let main = crate::platform::monitors::primary(op.window());
    match place(r, &monitors, main.as_ref()) {
        Some(((x, y), (wd, ht))) => {
            w.window().set_position(slint::PhysicalPosition::new(x, y));
            w.window().set_size(slint::PhysicalSize::new(wd, ht));
        }
        // No monitor known (tests, or a system that does not say): a roomy size of its own.
        None => w.window().set_size(slint::LogicalSize::new(1280.0, 960.0)),
    }
}

/// Keeps the window's place, monitor, page and Show advanced settings, as it closes.
pub fn remember(op: &Operator, page: &str, advanced: bool) {
    let root = op.app().store.root.clone();
    let w = win(op);
    let (pos, size) = (w.window().position(), w.window().size());
    let monitor = crate::platform::monitors::current(w.window()).map(|m| m.key()).unwrap_or_default();
    let mut r = load(&root);
    if size.width > 0 && size.height > 0 {
        r.x = pos.x;
        r.y = pos.y;
        r.width = size.width;
        r.height = size.height;
        if !monitor.is_empty() { r.monitor = monitor }
    }
    r.page = page.to_string();
    r.advanced = advanced;
    save(&root, &r);
}

/// Whether the Settings window has the keyboard (false without a real window, as in tests).
pub fn has_focus(op: &Operator) -> bool {
    use slint::winit_030::WinitWindowAccessor;
    is_open(op) && win(op).window().with_winit_window(|w| w.has_focus()).unwrap_or(false)
}

/// Brings the operator window to the front, for a window Settings opened there (Screens, Your scenes…).
pub fn raise_operator(op: &Operator) {
    use slint::winit_030::WinitWindowAccessor;
    let _ = op.window().with_winit_window(|w| w.focus_window());
}

/// Brings the window to the front (it may be behind the operator).
pub fn raise(op: &Operator) {
    use slint::winit_030::WinitWindowAccessor;
    let _ = win(op).window().with_winit_window(|w| w.focus_window());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(name: &str, x: i32, w: u32) -> MonitorId { MonitorId { name: name.into(), size: (w, 1080), position: (x, 0) } }

    #[test]
    fn the_window_comes_back_where_it_was_or_on_the_main_monitor() {
        let (left, right) = (m("A", 0, 1920), m("B", 1920, 1920));
        let was = Remembered { x: 2000, y: 100, width: 1000, height: 700, monitor: right.key(), ..Default::default() };
        assert_eq!(place(&was, &[left.clone(), right.clone()], Some(&left)), Some(((2000, 100), (1000, 700))), "where it was");
        // The second monitor is unplugged: on the main one, its size kept.
        assert_eq!(place(&was, std::slice::from_ref(&left), Some(&left)), Some(((60, 60), (1000, 700))));
        // Bigger than the monitor, or partly off it: kept wholly on it.
        let big = Remembered { x: -500, y: -40, width: 5000, height: 3000, monitor: left.key(), ..Default::default() };
        assert_eq!(place(&big, std::slice::from_ref(&left), Some(&left)), Some(((0, 0), (1920, 1080))));
        // Never placed: most of the main monitor.
        assert_eq!(place(&Remembered::default(), &[left.clone(), right.clone()], Some(&right)), Some(((1980, 60), (1344, 864))));
        assert_eq!(place(&Remembered::default(), &[], None), None);
    }

    #[test]
    fn what_is_remembered_survives_a_broken_file() {
        let dir = std::env::temp_dir().join(format!("rp-settings-window-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = Dir(dir);
        assert_eq!(load(dir.path()), Remembered::default());
        let r = Remembered { x: 5, y: 6, width: 700, height: 500, monitor: "A|1920×1080|0,0".into(), page: "StreamSound".into(), advanced: true };
        save(dir.path(), &r);
        assert_eq!(load(dir.path()), r);
        std::fs::write(file(dir.path()), "{ not json").unwrap();
        assert_eq!(load(dir.path()), Remembered::default());
        std::fs::write(file(dir.path()), r#"{"page":"Bible","later":1}"#).unwrap();
        assert_eq!(load(dir.path()).page, "Bible");
    }

    /// A folder removed when the test ends.
    struct Dir(PathBuf);
    impl Dir { fn path(&self) -> &Path { &self.0 } }
    impl Drop for Dir { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
}
