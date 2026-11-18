//! Dragging a panel by its title bar out of its window, like a browser tab (dock panels design, "Dragging between
//! windows"): which of the app's windows is under the pointer and where in its panels, and where a panel let go over
//! no window of the app opens in a window of its own. Pure: `ui::operator::docking` asks the windows where they are
//! and draws what this works out.
//!
//! Screen points are physical pixels (as Windows and the monitors count them); a window's panels are placed in
//! logical pixels from the top left of its dock area.
use super::{HEAD, PAD, WinRect};

/// One of the app's operator windows (the main window or a window of its own) as a drag across the screen sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnScreen {
    /// The layout window it shows (0 is the main window).
    pub win: usize,
    /// Its inside (without the frame and title bar Windows draws) on the screen, in physical pixels.
    pub inner: WinRect,
    /// Physical pixels to one logical pixel (1.5 at 150 % in Windows' display settings).
    pub scale: f32,
    /// Where its dock area starts inside the window, in logical pixels (under the menu and top bar in the main
    /// window; scrolled windows count the scroll).
    pub origin: (f32, f32),
}

impl OnScreen {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let r = self.inner;
        x >= r.x as f32 && y >= r.y as f32 && x < r.x as f32 + r.w as f32 && y < r.y as f32 + r.h as f32
    }

    /// A screen point in its dock area's terms.
    pub fn to_area(&self, x: f32, y: f32) -> (f32, f32) {
        let s = self.scale.max(0.1);
        ((x - self.inner.x as f32) / s - self.origin.0, (y - self.inner.y as f32) / s - self.origin.1)
    }

    /// A point of its dock area on the screen.
    pub fn to_screen(&self, ax: f32, ay: f32) -> (f32, f32) {
        let s = self.scale.max(0.1);
        (self.inner.x as f32 + (ax + self.origin.0) * s, self.inner.y as f32 + (ay + self.origin.1) * s)
    }
}

/// What the pointer is over during a drag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Under {
    /// One of the app's windows, at this point of its dock area.
    Window { win: usize, x: f32, y: f32 },
    /// No window of the app: the desktop, or another program's window in front of ours.
    Outside,
}

/// The window under screen point (`x`, `y`). `wins` are front to back. `front` is what the system says is in front
/// at that point, when it can tell (Windows): `Some(Some(win))` one of ours, `Some(None)` something else (another
/// program covers ours there, or the desktop). Without it, the first of `wins` that holds the point.
pub fn under(wins: &[OnScreen], x: f32, y: f32, front: Option<Option<usize>>) -> Under {
    let hit = match front {
        Some(Some(win)) => wins.iter().find(|w| w.win == win),
        Some(None) => None,
        None => wins.iter().find(|w| w.contains(x, y)),
    };
    match hit {
        Some(w) => { let (ax, ay) = w.to_area(x, y); Under::Window { win: w.win, x: ax, y: ay } }
        None => Under::Outside,
    }
}

/// The app's windows front to back, by their ids: the ones brought to the front most lately first (`recent`, most
/// recent first), then the others, windows of their own before the main window (they open over it).
pub fn front_to_back(ids: &[u32], recent: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = recent.iter().copied().filter(|r| ids.contains(r)).collect();
    out.dedup();
    let mut rest: Vec<u32> = ids.iter().copied().filter(|i| !out.contains(i)).collect();
    // Later windows of their own opened later, over the earlier ones; the main window last.
    rest.sort_by_key(|i| (*i == 0, std::cmp::Reverse(*i)));
    out.extend(rest);
    out
}

/// A monitor: the name the system gives it and where it is, in physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor { pub name: String, pub rect: WinRect }

/// The smallest window of its own (`PanelWindow`'s smallest size), logical pixels.
const MIN_W: f32 = 320.0;
const MIN_H: f32 = 240.0;

/// Where a panel let go over no window of the app opens: a window of its own on the monitor under the pointer
/// (`x`, `y`), as large as the panel was (`panel`, logical pixels, plus its title bar and edges) but never larger than
/// that monitor, placed so the pointer is on its title bar near the left, and kept wholly on that monitor. Returns
/// the monitor's name and the window's place (physical pixels). With no monitors known, at the pointer.
pub fn tear_off_place(x: i32, y: i32, panel: (f32, f32), scale: f32, monitors: &[Monitor]) -> (String, WinRect) {
    let s = scale.max(0.1);
    let w = ((panel.0 + 2.0 * PAD).max(MIN_W) * s).round() as i64;
    let h = ((panel.1 + HEAD + 2.0 * PAD).max(MIN_H) * s).round() as i64;
    // The pointer on the new window's grip: a little in from its left, half way down its title bar.
    let (gx, gy) = (((PAD + 20.0) * s).round() as i64, ((PAD + HEAD / 2.0) * s).round() as i64);
    let (mut left, mut top) = (x as i64 - gx, y as i64 - gy);
    let (mut w, mut h) = (w, h);
    let m = monitors.iter().find(|m| inside(&m.rect, x, y)).or_else(|| {
        monitors.iter().min_by_key(|m| distance(&m.rect, x, y))
    });
    let name = m.map(|m| m.name.clone()).unwrap_or_default();
    if let Some(m) = m {
        let r = m.rect;
        let (mx, my, mw, mh) = (r.x as i64, r.y as i64, r.w as i64, r.h as i64);
        w = w.min(mw);
        h = h.min(mh);
        left = left.clamp(mx, mx + mw - w);
        top = top.clamp(my, my + mh - h);
    }
    (name, WinRect { x: left as i32, y: top as i32, w: w.max(1) as u32, h: h.max(1) as u32 })
}

fn inside(r: &WinRect, x: i32, y: i32) -> bool {
    x >= r.x && y >= r.y && (x as i64) < r.x as i64 + r.w as i64 && (y as i64) < r.y as i64 + r.h as i64
}

/// How far a point is from a monitor (0 inside), squared.
fn distance(r: &WinRect, x: i32, y: i32) -> i64 {
    let gap = |p: i64, a: i64, len: i64| if p < a { a - p } else if p >= a + len { p - (a + len) + 1 } else { 0 };
    let (dx, dy) = (gap(x as i64, r.x as i64, r.w as i64), gap(y as i64, r.y as i64, r.h as i64));
    dx * dx + dy * dy
}

/// A drag in progress: the panel, the window it started in, and whether Esc, a right click or the window losing
/// the keyboard stopped it (then nothing happens until the button is let go).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag { pub panel: super::PanelId, pub from: u32, pub cancelled: bool }

#[cfg(test)]
#[path = "drag_tests.rs"]
mod tests;
