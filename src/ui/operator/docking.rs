//! Dock panels (design `docs/superpowers/specs/2026-11-17-dock-panels-design.md`): the operator window's panels
//! placed by the dock model (`ui::dock`), dividers, arrange mode, tabs, View → Panels and View → Layouts, windows of
//! their own on other monitors, and saving it all in Settings. Placement runs only when a window's size, a panel's
//! content size or the layout changes; nothing here runs every frame.
//!
//! What the panels show lives in globals that `rp_ui` writes to every window (`crates/rp-ui/build.rs`, `panes`), so a
//! panel shows the same thing whichever window it is in, and its buttons run the operator window's handlers.
use super::{strings as tr, window::Operator};
use crate::ui::{
    DockBar, DockHead, DockMenuRow, DockPlace, DockRect, DockTab, DockWin, OperatorWindow, PanelWindow,
    dock::{self, Builtin, Chosen, Dir, Divider, DockPrefs, Layout, Node, PanelId, Placement, WinRect, drag::{self, Drag, OnScreen, Under}},
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::{Cell, RefCell}, rc::Rc};

/// What a panel's content measured: its smallest size and the size it would like (width, height). `full` is its
/// smallest size when last laid out in full, `tight` when last laid out tight (see `TIGHT`).
#[derive(Clone, Copy, Default, PartialEq)]
struct Content { min: (f32, f32), pref: (f32, f32), full: Option<(f32, f32)>, tight: Option<(f32, f32)> }

/// Panels that lay themselves out tighter when there is too little room across: the library (its tabs become a
/// drop-down) and the Bible box (only the chosen Bible's chip). Before it is measured in full, a panel is taken to
/// need the first width across for that; laid out tight it needs at most the second (a measure taken while it was
/// still changing over can say more, and would keep it wide).
const TIGHT: [(PanelId, f32, f32); 2] = [(PanelId::Library, 760.0, 440.0), (PanelId::Bible, 600.0, 360.0)];

/// One open window: the main one first, then the windows of their own, in the layout's order.
struct Win {
    /// The layout window's `id` (0 for the main window).
    id: u32,
    size: (f32, f32),
    placed: Placement,
    places: Rc<VecModel<DockPlace>>,
    heads: Rc<VecModel<DockHead>>,
    bars: Rc<VecModel<DockBar>>,
    window: Option<PanelWindow>,
}

impl Win {
    fn new(id: u32, window: Option<PanelWindow>) -> Win {
        Win {
            id, size: (0.0, 0.0), placed: Placement::default(),
            places: Rc::new(VecModel::from(vec![DockPlace::default(); PanelId::ALL.len()])),
            heads: Rc::default(), bars: Rc::default(), window,
        }
    }
}

/// The operator's docks (`Operator::dock`).
pub(crate) struct Dock {
    pub(crate) prefs: RefCell<DockPrefs>,
    arrange: Cell<bool>,
    content: RefCell<[Content; 11]>,
    /// The panels of `TIGHT` laid out tight now.
    tight: RefCell<[bool; 11]>,
    wins: RefCell<Vec<Win>>,
    next_id: Cell<u32>,
    /// A divider being dragged: its window, the divider as the drag began and the tree then.
    bar_drag: RefCell<Option<(u32, Divider, Node)>>,
    save_timer: slint::Timer,
    /// While windows of their own are open: notices when one was moved, to remember where it is.
    watch_timer: slint::Timer,
    /// A panel's title bar being dragged (between windows too).
    drag: Cell<Option<Drag>>,
    /// The windows' ids, the one brought to the front last first (which is in front where they overlap).
    recent: RefCell<Vec<u32>>,
    /// Tests: where each window is on the screen (window id, its inside), as the testing backend has no screen.
    #[cfg(test)]
    pub(crate) fake_screen: RefCell<Vec<(u32, WinRect)>>,
}

impl Default for Dock {
    fn default() -> Self {
        Dock {
            prefs: RefCell::default(), arrange: Cell::new(false), content: RefCell::default(), tight: RefCell::default(),
            wins: RefCell::new(vec![Win::new(0, None)]), next_id: Cell::new(1), bar_drag: RefCell::default(),
            save_timer: slint::Timer::default(), watch_timer: slint::Timer::default(), drag: Cell::new(None),
            recent: RefCell::default(),
            #[cfg(test)]
            fake_screen: RefCell::default(),
        }
    }
}

/// The operator window or a window of its own: both have the same `dock-*` properties.
#[derive(Clone, Copy)]
enum Host<'a> { Main(&'a OperatorWindow), Pop(&'a PanelWindow) }

macro_rules! on_host {
    ($host:expr, |$w:ident| $body:expr) => {
        match $host { Host::Main($w) => { $body }, Host::Pop($w) => { $body } }
    };
}

/// A panel's name, as View → Panels and the windows' titles say it (the same words as `DockText.name`).
pub(crate) fn panel_name(p: PanelId) -> String {
    match p {
        PanelId::Service => tr::dock_service(),
        PanelId::Slides => tr::dock_slides(),
        PanelId::Library => tr::dock_library(),
        PanelId::Bible => tr::dock_bible(),
        PanelId::Live => tr::dock_live(),
        PanelId::UpNext => tr::dock_up_next(),
        PanelId::Cameras => tr::dock_cameras(),
        PanelId::Automation => tr::dock_automation(),
        PanelId::Livestream => tr::dock_livestream(),
        PanelId::Stage => tr::dock_stage(),
        PanelId::Follow => tr::dock_follow(),
    }
}

fn builtin_name(b: Builtin) -> String {
    match b {
        Builtin::Standard => tr::layout_standard(),
        Builtin::BigSlides => tr::layout_big_slides(),
        Builtin::Livestream => tr::layout_livestream(),
        Builtin::TwoMonitors => tr::layout_two_monitors(),
    }
}

fn builtin_key(b: Builtin) -> &'static str {
    match b { Builtin::Standard => "standard", Builtin::BigSlides => "big-slides", Builtin::Livestream => "livestream", Builtin::TwoMonitors => "two-monitors" }
}

fn rect(r: &dock::Rect) -> DockRect { DockRect { x: r.x, y: r.y, w: r.w, h: r.h } }

impl Operator {
    pub(crate) fn dock_layout(&self) -> Layout { self.dock.prefs.borrow().current.clone() }

    /// The open windows of their own, in the layout's order.
    pub(crate) fn dock_popouts(&self) -> Vec<PanelWindow> {
        self.dock.wins.borrow().iter().filter_map(|w| w.window.as_ref().map(|p| p.clone_strong())).collect()
    }

    /// Where the panels are in window `win` now (the main window is 0).
    pub(crate) fn dock_placement(&self, win: usize) -> Placement {
        self.dock.wins.borrow().get(win).map(|w| w.placed.clone()).unwrap_or_default()
    }


    fn win_index(&self, id: u32) -> Option<usize> { self.dock.wins.borrow().iter().position(|w| w.id == id) }

    /// What `f` gives for window `i`, if it is open.
    fn host_get<R>(&self, i: usize, f: impl Fn(Host) -> R) -> Option<R> {
        let pop = self.dock.wins.borrow().get(i).and_then(|w| w.window.as_ref().map(|p| p.clone_strong()));
        match (i, pop) {
            (0, _) => Some(f(Host::Main(&self.ui))),
            (_, Some(p)) => Some(f(Host::Pop(&p))),
            _ => None,
        }
    }

    fn host_do(&self, i: usize, f: impl Fn(Host)) {
        let pop = self.dock.wins.borrow().get(i).and_then(|w| w.window.as_ref().map(|p| p.clone_strong()));
        match (i, pop) {
            (0, _) => f(Host::Main(&self.ui)),
            (_, Some(p)) => f(Host::Pop(&p)),
            _ => {}
        }
    }

    /// Places window `i`'s panels for its size, and draws them there.
    fn dock_relayout(&self, i: usize) {
        if i == 0 { self.fit_top_bar() }
        let Some(root) = self.dock.prefs.borrow().current.windows.get(i).map(|w| w.root.clone()) else { return };
        let Some((w, h)) = self.dock.wins.borrow().get(i).map(|w| w.size) else { return };
        if w <= 0.0 || h <= 0.0 { return }
        let content = *self.dock.content.borrow();
        // First with the tight panels tight; each that then gets the room it needs in full is laid out in full.
        let tight_min = |p: PanelId, tight: &[bool; 11]| {
            let c = content[p.index()];
            let Some((_, _, cap)) = TIGHT.iter().find(|(t, _, _)| *t == p) else { return c.min };
            // Before it has been measured tight, its own smallest size (what it measured was in full).
            if tight[p.index()] { c.tight.map_or(p.min_size(), |m| (m.0.min(*cap), m.1)) } else { c.full.unwrap_or(c.min) }
        };
        let arrange = self.dock.arrange.get();
        let mut tight = [false; 11];
        for (p, _, _) in TIGHT { tight[p.index()] = true }
        let first = dock::place_with(&root, w, h, arrange, &|p| content[p.index()].pref, &|p| tight_min(p, &tight));
        for (p, need, _) in TIGHT {
            let full_w = content[p.index()].full.map_or(need, |f| f.0);
            if first.panel(p).is_some_and(|pp| pp.rect.w + 0.5 >= full_w) { tight[p.index()] = false }
        }
        let placed = if TIGHT.iter().all(|(p, _, _)| tight[p.index()]) {
            first
        } else {
            dock::place_with(&root, w, h, arrange, &|p| content[p.index()].pref, &|p| tight_min(p, &tight))
        };
        *self.dock.tight.borrow_mut() = tight;
        let (places, heads, bars) = {
            let wins = self.dock.wins.borrow();
            (wins[i].places.clone(), wins[i].heads.clone(), wins[i].bars.clone())
        };
        for p in PanelId::ALL {
            let row = placed.panel(p).map(|pp| DockPlace { here: true, shown: pp.shown, tight: tight[p.index()], x: pp.rect.x, y: pp.rect.y, w: pp.rect.w, h: pp.rect.h }).unwrap_or_default();
            if places.row_data(p.index()).as_ref() != Some(&row) { places.set_row_data(p.index(), row) }
        }
        let head_rows: Vec<DockHead> = placed.heads.iter().map(|hd| DockHead {
            x: hd.rect.x, y: hd.rect.y, w: hd.rect.w, active: hd.panels[hd.active].index() as i32,
            tabs: ModelRc::new(VecModel::from(hd.panels.iter().enumerate().map(|(k, p)| DockTab { panel: p.index() as i32, active: k == hd.active }).collect::<Vec<_>>())),
        }).collect();
        update_rows(&heads, head_rows, |a, b| a.x == b.x && a.y == b.y && a.w == b.w && a.active == b.active && a.tabs.iter().eq(b.tabs.iter()));
        let bar_rows: Vec<DockBar> = placed.dividers.iter().map(|d| DockBar { x: d.rect.x, y: d.rect.y, w: d.rect.w, h: d.rect.h, across: d.dir == Dir::Row }).collect();
        update_rows(&bars, bar_rows, |a, b| a == b);
        let (aw, ah) = (placed.width, placed.height);
        self.dock.wins.borrow_mut()[i].placed = placed;
        self.host_do(i, |h| on_host!(h, |w| {
            w.set_dock_area_width(aw);
            w.set_dock_area_height(ah);
        }));
    }

    fn dock_relayout_all(&self) {
        let n = self.dock.wins.borrow().len();
        for i in 0..n { self.dock_relayout(i) }
    }

    /// After the layout changed: windows of their own opened or closed to match it, every window placed again,
    /// the panels that are on their own say so, the menus follow, and it is saved.
    pub(crate) fn dock_changed(self: &Rc<Self>) {
        self.dock_sync_windows();
        self.dock_relayout_all();
        self.dock_shared();
        self.follow_stage();
        self.dock_save();
    }

    /// The Live panel and the top bar leave out what is a panel of its own; View → Panels and Layouts follow.
    fn dock_shared(&self) {
        let l = self.dock.prefs.borrow().current.clone();
        let ui = &self.ui;
        ui.set_up_next_apart(l.is_shown(PanelId::UpNext));
        ui.set_follow_apart(l.is_shown(PanelId::Follow));
        ui.set_cameras_apart(l.is_shown(PanelId::Cameras));
        ui.set_automation_apart(l.is_shown(PanelId::Automation));
        ui.set_livestream_apart(l.is_shown(PanelId::Livestream));
        let panels: Vec<DockMenuRow> = PanelId::ALL.iter().map(|p| DockMenuRow {
            key: format!("panel:{}", p.index()).into(), name: panel_name(*p).into(), checked: l.is_shown(*p),
        }).collect();
        ui.set_dock_panel_rows(ModelRc::new(VecModel::from(panels)));
        self.dock_layout_menu();
        // "Move to …" in each window's ⋯ menu: the other windows.
        let names: Vec<(u32, String)> = l.windows.iter().map(|w| (w.id, window_title_of(&w.root))).collect();
        let n = self.dock.wins.borrow().len();
        for i in 0..n {
            let others: Vec<DockWin> = names.iter().enumerate().filter(|(k, _)| *k != i && *k != 0)
                .map(|(k, (_, name))| DockWin { index: k as i32, name: name.into() }).collect();
            let title = names.get(i).map(|n| n.1.clone()).unwrap_or_default();
            self.host_do(i, |h| on_host!(h, |w| w.set_dock_windows(ModelRc::new(VecModel::from(others.clone())))));
            if i > 0 { self.host_do(i, |h| if let Host::Pop(w) = h { w.set_panels_title(tr::popout_title(&title).into()) }); }
        }
    }

    /// View → Layouts: the layouts, the one in use ticked, "(changed)" after it when the arrangement is no longer
    /// the church's saved layout it came from, and then "Update “…”" to save the changes into it.
    fn dock_layout_menu(&self) {
        let (chosen, saved, changed) = {
            let p = self.dock.prefs.borrow();
            let changed = match &p.chosen {
                Chosen::Saved(n) => p.saved.iter().find(|s| &s.name == n).is_some_and(|s| !p.current.same_arrangement(s)),
                Chosen::Builtin(_) => false,
            };
            (p.chosen.clone(), p.saved.iter().map(|s| s.name.clone()).collect::<Vec<_>>(), changed)
        };
        let mut layouts: Vec<DockMenuRow> = Builtin::ALL.iter().map(|b| DockMenuRow {
            key: format!("layout:builtin:{}", builtin_key(*b)).into(), name: builtin_name(*b).into(), checked: chosen == Chosen::Builtin(*b),
        }).collect();
        layouts.extend(saved.iter().map(|n| {
            let this = chosen == Chosen::Saved(n.clone());
            let name = if this && changed { tr::layout_changed(n) } else { n.clone() };
            DockMenuRow { key: format!("layout:saved:{n}").into(), name: name.into(), checked: this }
        }));
        let ui = &self.ui;
        ui.set_dock_layout_rows(ModelRc::new(VecModel::from(layouts)));
        ui.set_dock_saved_chosen(matches!(chosen, Chosen::Saved(_)));
        let update = match &chosen { Chosen::Saved(n) if changed => tr::layout_update(n), _ => String::new() };
        if ui.get_dock_update_title() != update.as_str() { ui.set_dock_update_title(update.into()) }
    }

    /// Saves the panels in Settings: at once in tests, else a moment after the last change (a divider drag or
    /// several tab clicks make one save).
    fn dock_save(&self) {
        self.dock_remember_places();
        self.dock_layout_menu();
        let me = self.me.clone();
        let save = move || {
            let Some(op) = me.upgrade() else { return };
            let body = serde_json::to_value(&*op.dock.prefs.borrow()).unwrap_or_default();
            let me = op.me.clone();
            op.ops.call::<serde_json::Value>("settings/dock", body, move |r| if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(e, true) });
        };
        if self.ops.is_inline() { save() } else { self.dock.save_timer.start(slint::TimerMode::SingleShot, std::time::Duration::from_millis(600), save) }
    }

    /// Where each window of its own is now (its monitor and place), kept in the layout so it opens there again.
    fn dock_remember_places(&self) -> bool {
        let found: Vec<(usize, Option<(String, WinRect)>)> = self.dock.wins.borrow().iter().enumerate().skip(1)
            .map(|(i, w)| (i, w.window.as_ref().and_then(window_place))).collect();
        let mut prefs = self.dock.prefs.borrow_mut();
        let mut changed = false;
        for (i, place) in found {
            let (Some((monitor, r)), Some(dw)) = (place, prefs.current.windows.get_mut(i)) else { continue };
            if dw.place != Some(r) || dw.monitor != monitor { dw.place = Some(r); dw.monitor = monitor; changed = true; }
        }
        changed
    }

    /// Opens a window of its own for each such window of the layout, and closes those it no longer has.
    fn dock_sync_windows(self: &Rc<Self>) {
        // Every window of its own gets an id, for its callbacks to find it however the list changes.
        let ids: Vec<u32> = {
            let mut prefs = self.dock.prefs.borrow_mut();
            prefs.current.windows.iter_mut().enumerate().map(|(i, w)| {
                if i == 0 { w.id = 0 } else if w.id == 0 { w.id = self.dock.next_id.get(); self.dock.next_id.set(w.id + 1); }
                w.id
            }).collect()
        };
        let old: Vec<Win> = std::mem::take(&mut *self.dock.wins.borrow_mut());
        let mut old: Vec<Option<Win>> = old.into_iter().map(Some).collect();
        let mut wins = Vec::with_capacity(ids.len());
        for id in &ids {
            match old.iter_mut().find(|w| w.as_ref().is_some_and(|w| w.id == *id)).and_then(Option::take) {
                Some(w) => wins.push(w),
                None => wins.push(Win::new(*id, None)),
            }
        }
        // Windows the layout no longer has: their panels went elsewhere already.
        for w in old.into_iter().flatten() {
            if let Some(p) = w.window { crate::ui::unregister_popout(&p); let _ = p.hide(); }
        }
        *self.dock.wins.borrow_mut() = wins;
        let missing: Vec<(usize, u32)> = self.dock.wins.borrow().iter().enumerate().skip(1).filter(|(_, w)| w.window.is_none()).map(|(i, w)| (i, w.id)).collect();
        for (i, id) in missing {
            let Some(pw) = self.dock_open_window(id) else { continue };
            self.dock.wins.borrow_mut()[i].window = Some(pw.clone_strong());
            let (place, monitor) = self.dock.prefs.borrow().current.windows.get(i).map(|w| (w.place, w.monitor.clone())).unwrap_or_default();
            self.dock_position(&pw, place, &monitor);
            // Its size: the testing backend never resizes, so it starts at what it asked for.
            let size = pw.window().size().to_logical(pw.window().scale_factor());
            self.dock.wins.borrow_mut()[i].size = (size.width, size.height);
        }
        // While windows of their own are open, now and then: remember where they were moved to.
        if self.dock.wins.borrow().len() > 1 && !self.ops.is_inline() {
            let me = self.me.clone();
            self.dock.watch_timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(3), move || {
                // `dock_save` also says "(changed)" in View → Layouts when a saved layout's window was moved.
                if let Some(op) = me.upgrade() && op.dock_remember_places() { op.dock_save() }
            });
        } else {
            self.dock.watch_timer.stop();
        }
    }

    /// A new window of its own: the globals as the operator window has them, its clicks run the operator window's
    /// handlers, and closing it brings its panels home.
    fn dock_open_window(self: &Rc<Self>, id: u32) -> Option<PanelWindow> {
        let pw = PanelWindow::new().ok()?;
        crate::ui::pacing::attach(pw.window(), "panels", false);
        crate::ui::register_popout(&pw);
        crate::ui::copy_panes(&self.ui, &pw);
        crate::ui::forward_panes(&self.ui, &pw);
        pw.set_dock_arrange(self.dock.arrange.get());
        let places = self.dock.wins.borrow().iter().find(|w| w.id == id).map(|w| (w.places.clone(), w.heads.clone(), w.bars.clone()));
        if let Some((places, heads, bars)) = places {
            pw.set_dock_places(ModelRc::from(places));
            pw.set_dock_heads(ModelRc::from(heads));
            pw.set_dock_bars(ModelRc::from(bars));
        }
        self.dock_wire(Host::Pop(&pw), id);
        let me = self.me.clone();
        pw.on_key(move |text, ctrl, alt, shift, in_text, in_bible| me.upgrade().is_some_and(|op| op.key(&text, ctrl, alt, shift, in_text, in_bible)));
        // Which window is in front (where windows overlap), and a drag stops when the window loses the keyboard.
        let events = crate::platform::winit_events::Dispatcher::default();
        crate::platform::winit_events::attach(pw.window(), events.clone());
        let me = self.me.clone();
        events.add(move |e| if let (Some(op), crate::platform::winit_events::WindowEvent::Focused(on)) = (me.upgrade(), e) { op.dock_window_focus(id, *on) });
        let me = self.me.clone();
        pw.window().on_close_requested(move || {
            if let Some(op) = me.upgrade() { op.dock_close_window(id) }
            slint::CloseRequestResponse::HideWindow
        });
        pw.show().ok()?;
        Some(pw)
    }

    /// A window of its own was closed: its panels go back into the main window.
    pub(crate) fn dock_close_window(self: &Rc<Self>, id: u32) {
        let Some(i) = self.win_index(id) else { return };
        if i == 0 { return }
        self.dock.prefs.borrow_mut().current.pop_in(i);
        self.dock_changed();
    }

    /// Puts a window of its own on its monitor: where it was if that monitor is plugged in, else in the middle of
    /// the second monitor (built-in layouts) or of the operator window's monitor.
    fn dock_position(&self, pw: &PanelWindow, place: Option<WinRect>, monitor: &str) {
        use slint::winit_030::WinitWindowAccessor;
        let target = self.ui.window().with_winit_window(|mw| {
            let monitors: Vec<_> = mw.available_monitors().collect();
            let main = mw.current_monitor();
            let named = monitors.iter().find(|m| !monitor.is_empty() && m.name().as_deref() == Some(monitor)).cloned();
            let second = (monitor == dock::SECOND_MONITOR).then(|| monitors.iter().find(|m| Some(*m) != main.as_ref()).cloned()).flatten();
            let m = named.or(second).or(main)?;
            let (mp, ms) = (m.position(), m.size());
            let inside = |r: &WinRect| r.x >= mp.x - 8 && r.y >= mp.y - 8 && r.x + r.w as i32 <= mp.x + ms.width as i32 + 8 && r.y + r.h as i32 <= mp.y + ms.height as i32 + 8;
            Some(match place {
                Some(r) if inside(&r) && r.w >= 200 && r.h >= 150 => r,
                _ => {
                    let (w, h) = (ms.width * 3 / 5, ms.height * 3 / 4);
                    WinRect { x: mp.x + (ms.width - w) as i32 / 2, y: mp.y + (ms.height - h) as i32 / 2, w, h }
                }
            })
        }).flatten();
        if let Some(r) = target {
            pw.window().set_size(slint::PhysicalSize::new(r.w, r.h));
            pw.window().set_position(slint::PhysicalPosition::new(r.x, r.y));
        }
    }

    /// Wires one window's dock callbacks (`id`: its layout window, 0 for the main window).
    fn dock_wire(self: &Rc<Self>, host: Host, id: u32) {
        let me = self.me.clone();
        let resized = move |w: f32, h: f32| if let Some(op) = me.upgrade() && let Some(i) = op.win_index(id) {
            let size = (w, h);
            if op.dock.wins.borrow()[i].size == size { return }
            op.dock.wins.borrow_mut()[i].size = size;
            op.dock_relayout(i);
        };
        let me = self.me.clone();
        let fit = move |p: i32, tight: bool, minw: f32, minh: f32, prefw: f32, prefh: f32| if let Some(op) = me.upgrade() && let Some(p) = usize::try_from(p).ok().and_then(PanelId::from_index) {
            let old = op.dock.content.borrow()[p.index()];
            let mut c = Content { min: (minw, minh), pref: (prefw, prefh), ..old };
            // Which way it measured itself: in full or tight.
            if tight { c.tight = Some(c.min) } else { c.full = Some(c.min) }
            let near = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 0.5 && (a.1 - b.1).abs() < 0.5;
            let same = |a: Option<(f32, f32)>, b: Option<(f32, f32)>| match (a, b) { (Some(a), Some(b)) => near(a, b), (None, None) => true, _ => false };
            if near(old.min, c.min) && near(old.pref, c.pref) && same(old.full, c.full) && same(old.tight, c.tight) { return }
            op.dock.content.borrow_mut()[p.index()] = c;
            if let Some(i) = op.dock.prefs.borrow().current.window_of(p).map(|i| i) { op.dock_relayout_later(i) }
        };
        let me = self.me.clone();
        let bar_moved = move |b: i32, delta: f32| if let Some(op) = me.upgrade() && let Some(i) = op.win_index(id) { op.dock_bar_moved(i, id, b as usize, delta) };
        let me = self.me.clone();
        let bar_done = move |_b: i32| if let Some(op) = me.upgrade() && op.dock.bar_drag.borrow_mut().take().is_some() { op.dock_save() };
        let me = self.me.clone();
        let tab = move |p: i32| if let Some(op) = me.upgrade() && let Some(p) = usize::try_from(p).ok().and_then(PanelId::from_index) {
            op.dock.prefs.borrow_mut().current.activate(p);
            op.dock_changed();
        };
        let me = self.me.clone();
        let drag_moved = move |p: i32, x: f32, y: f32| if let Some(op) = me.upgrade() && let Some(i) = op.win_index(id) { op.dock_drag_moved(i, p, x, y) };
        let me = self.me.clone();
        let drag_done = move |p: i32, x: f32, y: f32| if let Some(op) = me.upgrade() && let Some(i) = op.win_index(id) { op.dock_drag_done(i, p, x, y) };
        let me = self.me.clone();
        let drag_cancel = move |_p: i32| if let Some(op) = me.upgrade() { op.dock_drag_stop() };
        let me = self.me.clone();
        let action = move |p: i32, a: slint::SharedString| if let Some(op) = me.upgrade() && let Some(p) = usize::try_from(p).ok().and_then(PanelId::from_index) { op.dock_panel_action(p, &a) };
        on_host!(host, |w| {
            w.on_dock_area_resized(resized.clone());
            w.on_dock_fit(fit.clone());
            w.on_dock_bar_moved(bar_moved.clone());
            w.on_dock_bar_done(bar_done.clone());
            w.on_dock_tab_picked(tab.clone());
            w.on_dock_drag_moved(drag_moved.clone());
            w.on_dock_drag_done(drag_done.clone());
            w.on_dock_drag_cancel(drag_cancel.clone());
            w.on_dock_panel_action(action.clone());
        });
    }

    /// A content size changed: place that window again once this turn of the event loop is over (several panels
    /// report at once when a window opens). Inline in tests.
    fn dock_relayout_later(&self, i: usize) {
        if self.ops.is_inline() { return self.dock_relayout(i) }
        let me = self.me.clone();
        slint::Timer::single_shot(std::time::Duration::ZERO, move || if let Some(op) = me.upgrade() { op.dock_relayout(i) });
    }

    fn dock_bar_moved(&self, i: usize, id: u32, b: usize, delta: f32) {
        let started = self.dock.bar_drag.borrow().as_ref().is_some_and(|(w, _, _)| *w == id);
        if !started {
            let Some(d) = self.dock.wins.borrow()[i].placed.dividers.get(b).cloned() else { return };
            let root = self.dock.prefs.borrow().current.windows[i].root.clone();
            *self.dock.bar_drag.borrow_mut() = Some((id, d, root));
        }
        let Some((_, d, root)) = self.dock.bar_drag.borrow().clone() else { return };
        let mut root = root;
        dock::drag_divider(&mut root, &d, delta);
        self.dock.prefs.borrow_mut().current.windows[i].root = root;
        self.dock_relayout(i);
    }

    /// A title bar dragged to (`x`, `y`) of window `i`'s dock area, which may be outside that window: the window
    /// keeps the mouse while its button is held (Windows, macOS and X11 all send it the moves), so the drag follows
    /// the pointer over the other windows of the app. The window under the pointer lights up where the panel would
    /// go and shows its name at the pointer; nothing changes until the button is let go.
    fn dock_drag_moved(&self, i: usize, p: i32, x: f32, y: f32) {
        let Some(panel) = usize::try_from(p).ok().and_then(PanelId::from_index) else { return };
        let Some(from) = self.dock.wins.borrow().get(i).map(|w| w.id) else { return };
        let d = match self.dock.drag.get() {
            Some(d) if d.panel == panel && d.from == from => d,
            _ => { let d = Drag { panel, from, cancelled: false }; self.dock.drag.set(Some(d)); d }
        };
        if d.cancelled { return }
        let (under, _) = self.dock_drag_under(i, x, y);
        self.dock_drag_show(Some(panel), under);
    }

    /// Let go: on a panel of any window of the app, it goes there (beside it or as a tab with it); over no window
    /// of the app, it opens in a window of its own there. A window left with no panels closes.
    fn dock_drag_done(self: &Rc<Self>, i: usize, p: i32, x: f32, y: f32) {
        let d = self.dock.drag.take();
        self.dock_drag_show(None, Under::Outside);
        let Some(panel) = usize::try_from(p).ok().and_then(PanelId::from_index) else { return };
        // Stopped with Esc or by the window losing the keyboard, or never begun: nothing moves.
        if !d.is_some_and(|d| d.panel == panel && !d.cancelled) { return }
        match self.dock_drag_under(i, x, y) {
            (Under::Window { win, x, y }, _) => {
                let target = self.dock.wins.borrow().get(win).and_then(|w| w.placed.drop_target(x, y));
                let Some((target, zone, _)) = target else { return };
                if self.dock.prefs.borrow_mut().current.drop_on(panel, target, zone) { self.dock_changed_soon() }
            }
            (Under::Outside, Some(at)) => self.dock_tear_off(panel, i, at),
            (Under::Outside, None) => {}
        }
    }

    /// Esc, or the window losing the keyboard, while a panel is dragged: nothing moves, and the rest of that drag
    /// is ignored until the button is let go.
    pub(crate) fn dock_drag_cancel(&self) {
        let Some(mut d) = self.dock.drag.get() else { return };
        if d.cancelled { return }
        d.cancelled = true;
        self.dock.drag.set(Some(d));
        self.dock_drag_show(None, Under::Outside);
    }

    /// The title bar let the drag go (a right click, or the window lost the mouse): it is over.
    fn dock_drag_stop(&self) {
        self.dock.drag.set(None);
        self.dock_drag_show(None, Under::Outside);
    }

    /// A panel is being dragged (and was not stopped).
    pub(crate) fn dock_dragging(&self) -> bool { self.dock.drag.get().is_some_and(|d| !d.cancelled) }

    /// A window of the app came to the front (`on`) or lost the keyboard.
    pub(crate) fn dock_window_focus(&self, id: u32, on: bool) {
        if on {
            {
                let mut recent = self.dock.recent.borrow_mut();
                recent.retain(|r| *r != id);
                recent.insert(0, id);
            }
            // A drag stopped by another program taking the keyboard never saw its button let go.
            if self.dock.drag.get().is_some_and(|d| d.cancelled) { self.dock.drag.set(None) }
        } else {
            self.dock_drag_cancel();
        }
    }

    /// What is under the pointer at (`x`, `y`) of window `i`'s dock area, and that point on the screen. When the
    /// system cannot say where the windows are (Wayland), the drag stays in its window, as before.
    fn dock_drag_under(&self, i: usize, x: f32, y: f32) -> (Under, Option<(f32, f32)>) {
        let wins = self.dock_on_screen();
        let Some(src) = wins.iter().find(|w| w.win == i) else { return (Under::Window { win: i, x, y }, None) };
        let (sx, sy) = src.to_screen(x, y);
        (drag::under(&wins, sx, sy, self.dock_front_at(sx, sy, &wins)), Some((sx, sy)))
    }

    /// Lights up where the panel would go in the window under the pointer, with its name at the pointer; every
    /// other window shows nothing (`None`: no window does).
    fn dock_drag_show(&self, panel: Option<PanelId>, under: Under) {
        let n = self.dock.wins.borrow().len();
        for k in 0..n {
            let at = match (panel, under) { (Some(p), Under::Window { win, x, y }) if win == k => Some((p, x, y)), _ => None };
            let lit = at.and_then(|(_, x, y)| self.dock.wins.borrow()[k].placed.drop_target(x, y)).map(|t| rect(&t.2));
            self.host_do(k, |h| on_host!(h, |w| {
                w.set_dock_lit_shown(lit.is_some());
                if let Some(r) = &lit { w.set_dock_lit(r.clone()) }
                match at {
                    Some((p, x, y)) => { w.set_dock_ghost_x(x); w.set_dock_ghost_y(y); w.set_dock_ghost(p.index() as i32); }
                    None => w.set_dock_ghost(-1),
                }
            }));
        }
    }

    /// The open windows on the screen, front to back. Empty when the system cannot say where they are.
    fn dock_on_screen(&self) -> Vec<OnScreen> {
        let ids: Vec<u32> = self.dock.wins.borrow().iter().map(|w| w.id).collect();
        let order = drag::front_to_back(&ids, &self.dock.recent.borrow());
        order.into_iter().filter_map(|id| {
            let i = self.win_index(id)?;
            let (inner, scale) = self.dock_inner(i, id)?;
            let origin = self.host_get(i, |h| on_host!(h, |w| (w.get_dock_origin_x(), w.get_dock_origin_y())))?;
            Some(OnScreen { win: i, inner, scale, origin })
        }).collect()
    }

    /// Window `i`'s inside on the screen (physical pixels) and its scale.
    fn dock_inner(&self, i: usize, id: u32) -> Option<(WinRect, f32)> {
        #[cfg(test)]
        if let Some((_, r)) = self.dock.fake_screen.borrow().iter().find(|(f, _)| *f == id).copied() {
            return self.host_get(i, |h| on_host!(h, |w| (r, w.window().scale_factor())));
        }
        let _ = id;
        use slint::winit_030::WinitWindowAccessor;
        self.host_get(i, |h| on_host!(h, |w| w.window().with_winit_window(|ww| {
            let p = ww.inner_position().ok()?;
            let s = ww.inner_size();
            Some((WinRect { x: p.x, y: p.y, w: s.width, h: s.height }, ww.scale_factor() as f32))
        }).flatten())).flatten()
    }

    /// Which window is in front at a screen point, as Windows says: one of ours, or something else (another
    /// program, the desktop). Elsewhere the order the windows were last brought to the front decides.
    #[cfg(windows)]
    fn dock_front_at(&self, x: f32, y: f32, wins: &[OnScreen]) -> Option<Option<usize>> {
        use windows_sys::Win32::{Foundation::POINT, UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, WindowFromPoint}};
        if self.ops.is_inline() { return None }
        let handles: Vec<(usize, isize)> = wins.iter().filter_map(|w| Some((w.win, self.dock_hwnd(w.win)?))).collect();
        if handles.len() != wins.len() { return None }
        // SAFETY: plain Win32 questions about a point and the window handle the first one gives.
        let top = unsafe { GetAncestor(WindowFromPoint(POINT { x: x.floor() as i32, y: y.floor() as i32 }), GA_ROOT) };
        Some(handles.iter().find(|(_, h)| *h == top as isize).map(|(w, _)| *w))
    }

    #[cfg(not(windows))]
    fn dock_front_at(&self, _x: f32, _y: f32, _wins: &[OnScreen]) -> Option<Option<usize>> { None }

    /// Window `i`'s Win32 handle.
    #[cfg(windows)]
    fn dock_hwnd(&self, i: usize) -> Option<isize> {
        use slint::winit_030::{WinitWindowAccessor, winit::raw_window_handle::{HasWindowHandle, RawWindowHandle}};
        self.host_get(i, |h| on_host!(h, |w| w.window().with_winit_window(|ww| match ww.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
            _ => None,
        }).flatten())).flatten()
    }

    /// The monitors, for a window torn off at the pointer.
    fn dock_monitors(&self) -> Vec<drag::Monitor> {
        use slint::winit_030::WinitWindowAccessor;
        self.ui.window().with_winit_window(|w| w.available_monitors().map(|m| {
            let (p, s) = (m.position(), m.size());
            drag::Monitor { name: m.name().unwrap_or_default(), rect: WinRect { x: p.x, y: p.y, w: s.width, h: s.height } }
        }).collect()).unwrap_or_default()
    }

    /// A panel let go over no window of the app (`at`, on the screen): a window of its own opens there, as large as
    /// the panel was. A panel alone in a window of its own moves that window there instead.
    fn dock_tear_off(self: &Rc<Self>, p: PanelId, from: usize, at: (f32, f32)) {
        let size = self.dock.wins.borrow().get(from).and_then(|w| w.placed.panel(p).map(|pp| (pp.rect.w, pp.rect.h))).unwrap_or(p.min_size());
        let scale = self.host_get(from, |h| on_host!(h, |w| w.window().scale_factor())).unwrap_or(1.0);
        let (monitor, place) = drag::tear_off_place(at.0.round() as i32, at.1.round() as i32, size, scale, &self.dock_monitors());
        let alone = {
            let mut prefs = self.dock.prefs.borrow_mut();
            let l = &mut prefs.current;
            match l.window_of(p) {
                Some(w) if w > 0 && l.windows[w].root.panels() == [p] => {
                    l.windows[w].place = Some(place);
                    l.windows[w].monitor = monitor.clone();
                    Some(w)
                }
                _ => None,
            }
        };
        if let Some(w) = alone {
            let pw = self.dock.wins.borrow().get(w).and_then(|w| w.window.as_ref().map(|p| p.clone_strong()));
            if let Some(pw) = pw { self.dock_position(&pw, Some(place), &monitor) }
            return self.dock_save();
        }
        let done = self.dock.prefs.borrow_mut().current.pop_out(&[p], Some(place), &monitor).is_some();
        if done { self.dock_changed_soon() } else { self.toast(tr::main_keeps_a_panel(), false) }
    }

    /// `dock_changed` once the event that asked for it is over: a drag let go in a window of its own may close
    /// that window, which must not happen while it is still handling the release. At once in tests.
    fn dock_changed_soon(self: &Rc<Self>) {
        if self.ops.is_inline() { return self.dock_changed() }
        let me = self.me.clone();
        slint::Timer::single_shot(std::time::Duration::ZERO, move || if let Some(op) = me.upgrade() { op.dock_changed() });
    }

    /// The ⋯ menu of a panel's title bar.
    pub(crate) fn dock_panel_action(self: &Rc<Self>, p: PanelId, action: &str) {
        let done = {
            let mut prefs = self.dock.prefs.borrow_mut();
            let l = &mut prefs.current;
            match action {
                "close" => l.close(p),
                "new-window" | "group-window" => {
                    let panels = if action == "group-window" { l.windows.iter().find_map(|w| w.root.group_of(p)).unwrap_or(vec![p]) } else { vec![p] };
                    l.pop_out(&panels, None, dock::SECOND_MONITOR).is_some()
                }
                "main" => l.send_home(p),
                a => match a.strip_prefix("window:").and_then(|n| n.parse::<usize>().ok()) {
                    Some(win) => l.move_to(p, win),
                    None => false,
                },
            }
        };
        if done { self.dock_changed() } else if action != "main" && !action.starts_with("window:") { self.toast(tr::main_keeps_a_panel(), false) }
    }

    /// Shows a panel if it is closed (Ctrl+B and the Bible box, File → Video inputs and the library).
    pub(crate) fn dock_show(self: &Rc<Self>, p: PanelId) {
        if self.dock.prefs.borrow().current.is_shown(p) { return }
        self.dock.prefs.borrow_mut().current.restore(p);
        self.dock_changed();
    }

    /// The window a panel is in, raised: Ctrl+B in the operator window reaches a Bible box in another window.
    fn dock_raise(&self, p: PanelId) {
        let Some(i) = self.dock.prefs.borrow().current.window_of(p) else { return };
        if i == 0 { return }
        if let Some(w) = self.dock.wins.borrow().get(i).and_then(|w| w.window.as_ref().map(|p| p.clone_strong())) {
            use slint::winit_030::WinitWindowAccessor;
            w.window().with_winit_window(|w| w.focus_window());
        }
    }

    /// View → Panels, View → Layouts, Edit → Bible box and Recent verses (`OperatorWindow.dock-menu`).
    pub(crate) fn dock_menu(self: &Rc<Self>, key: &str) {
        if key == "arrange" { return self.dock_set_arrange(!self.dock.arrange.get()) }
        if key == "bible-focus" { return self.focus_bible() }
        if key == "bible-history" {
            self.dock_show(PanelId::Bible);
            self.dock_raise(PanelId::Bible);
            let ui = &self.ui;
            return ui.set_bible_history_serial(ui.get_bible_history_serial() + 1);
        }
        if let Some(p) = key.strip_prefix("panel-show:").and_then(|n| n.parse::<usize>().ok()).and_then(PanelId::from_index) { return self.dock_show(p) }
        if let Some(p) = key.strip_prefix("panel:").and_then(|n| n.parse::<usize>().ok()).and_then(PanelId::from_index) {
            if self.dock.prefs.borrow().current.is_shown(p) {
                if !self.dock.prefs.borrow_mut().current.close(p) { return self.toast(tr::main_keeps_a_panel(), false) }
                return self.dock_changed();
            }
            return self.dock_show(p);
        }
        if let Some(b) = key.strip_prefix("layout:builtin:").and_then(|k| Builtin::ALL.into_iter().find(|b| builtin_key(*b) == k)) {
            return self.dock_choose(Chosen::Builtin(b), Layout::builtin(b));
        }
        if let Some(name) = key.strip_prefix("layout:saved:") {
            let Some(l) = self.dock.prefs.borrow().saved.iter().find(|l| l.name == name).cloned() else { return };
            return self.dock_choose(Chosen::Saved(name.to_string()), l);
        }
        match key {
            "update" => {
                let chosen = self.dock.prefs.borrow().chosen.clone();
                if let Chosen::Saved(name) = chosen { self.dock_store_as(&name) }
            }
            "reset" => self.dock_choose(Chosen::Builtin(Builtin::Standard), Layout::builtin(Builtin::Standard)),
            "save-as" => self.dock_save_as(),
            "rename" => self.dock_rename(),
            "delete" => self.dock_delete(),
            _ => {}
        }
    }

    pub(crate) fn dock_set_arrange(&self, on: bool) {
        self.dock.arrange.set(on);
        let n = self.dock.wins.borrow().len();
        for i in 0..n { self.host_do(i, |h| on_host!(h, |w| w.set_dock_arrange(on))) }
        self.dock_relayout_all();
    }

    /// Uses a layout (View → Layouts, Reset to Standard).
    pub(crate) fn dock_choose(self: &Rc<Self>, chosen: Chosen, mut l: Layout) {
        l.repair();
        // The arrangement in use is not one of the church's saved ones, so it keeps no name.
        l.name.clear();
        {
            let mut prefs = self.dock.prefs.borrow_mut();
            prefs.chosen = chosen;
            prefs.current = l;
        }
        self.dock_changed();
    }

    /// Layouts → Save this layout as…: the arrangement now, under a name (a name already used is replaced).
    fn dock_save_as(self: &Rc<Self>) {
        let initial = match &self.dock.prefs.borrow().chosen { Chosen::Saved(n) => n.clone(), Chosen::Builtin(_) => String::new() };
        let me = self.me.clone();
        crate::ui::dialogs::ask::prompt(self, &tr::layout_name_prompt(), &initial, move |name| {
            let (Some(op), Some(name)) = (me.upgrade(), name) else { return };
            op.dock_store_as(&name);
        });
    }

    /// Saves the arrangement in use as the church's layout `name`.
    pub(crate) fn dock_store_as(self: &Rc<Self>, name: &str) {
        let name = name.trim();
        if name.is_empty() { return }
        self.dock_remember_places();
        {
            let mut prefs = self.dock.prefs.borrow_mut();
            let mut l = prefs.current.clone();
            l.name = name.to_string();
            for w in &mut l.windows { w.id = 0 }
            prefs.saved.retain(|s| !s.name.eq_ignore_ascii_case(name));
            prefs.saved.push(l);
            prefs.chosen = Chosen::Saved(name.to_string());
        }
        self.dock_shared();
        self.dock_save();
        self.toast(tr::layout_saved(name), false);
    }

    fn dock_rename(self: &Rc<Self>) {
        let Chosen::Saved(old) = self.dock.prefs.borrow().chosen.clone() else { return };
        let me = self.me.clone();
        crate::ui::dialogs::ask::prompt(self, &tr::layout_rename_prompt(), &old.clone(), move |name| {
            let (Some(op), Some(name)) = (me.upgrade(), name) else { return };
            op.dock_rename_to(&old, &name);
        });
    }

    pub(crate) fn dock_rename_to(self: &Rc<Self>, old: &str, new: &str) {
        let new = new.trim();
        if new.is_empty() || new == old { return }
        {
            let mut prefs = self.dock.prefs.borrow_mut();
            prefs.saved.retain(|s| s.name == old || !s.name.eq_ignore_ascii_case(new));
            if let Some(l) = prefs.saved.iter_mut().find(|l| l.name == old) { l.name = new.to_string() }
            if prefs.chosen == Chosen::Saved(old.to_string()) { prefs.chosen = Chosen::Saved(new.to_string()) }
        }
        self.dock_shared();
        self.dock_save();
    }

    fn dock_delete(self: &Rc<Self>) {
        let Chosen::Saved(name) = self.dock.prefs.borrow().chosen.clone() else { return };
        let me = self.me.clone();
        crate::ui::dialogs::ask::confirm(self, &tr::layout_delete_question(&name), &tr::delete_layout_button(), &tr::cancel_action(), true, move |ok| {
            if let (Some(op), true) = (me.upgrade(), ok) { op.dock_delete_saved(&name) }
        });
    }

    /// Deletes the church's layout `name`; the window goes back to Standard.
    pub(crate) fn dock_delete_saved(self: &Rc<Self>, name: &str) {
        self.dock.prefs.borrow_mut().saved.retain(|l| l.name != name);
        self.dock_choose(Chosen::Builtin(Builtin::Standard), Layout::builtin(Builtin::Standard));
    }

    /// The top bar: what does not fit the window goes under More (`TopBar.tight`). Its need is known whether those
    /// buttons show or not, so one comparison settles it.
    pub(crate) fn fit_top_bar(&self) {
        let w = self.ui.window().size().to_logical(self.ui.window().scale_factor()).width;
        let tight = self.ui.get_top_need() > w + 0.5;
        if self.ui.get_top_tight() != tight { self.ui.set_top_tight(tight) }
    }

    /// Ctrl+B and Edit → Bible box: the Bible box (shown again if closed, its window raised) with its text selected.
    pub(crate) fn focus_bible(self: &Rc<Self>) {
        self.dock_show(PanelId::Bible);
        self.dock_raise(PanelId::Bible);
        let ui = &self.ui;
        ui.set_bible_focus_serial(ui.get_bible_focus_serial() + 1);
    }

    /// Opens the Bible picker (the chosen Bible's chip; tests).
    pub(crate) fn open_bible_picker(self: &Rc<Self>) {
        self.dock_show(PanelId::Bible);
        let ui = &self.ui;
        ui.set_bible_picker_serial(ui.get_bible_picker_serial() + 1);
    }
}

/// Updates a list model row by row, so the elements drawing it stay (a strip being dragged keeps its drag).
fn update_rows<T: Clone + 'static>(model: &VecModel<T>, rows: Vec<T>, same: impl Fn(&T, &T) -> bool) {
    if model.row_count() != rows.len() { return model.set_vec(rows) }
    for (i, r) in rows.into_iter().enumerate() {
        if !model.row_data(i).is_some_and(|old| same(&old, &r)) { model.set_row_data(i, r) }
    }
}

/// A window's title: its panels' names.
fn window_title_of(root: &Node) -> String {
    root.panels().into_iter().map(panel_name).collect::<Vec<_>>().join(", ")
}

/// Where a window of its own is: its monitor's name and its place (none on the testing backend).
fn window_place(w: &PanelWindow) -> Option<(String, WinRect)> {
    use slint::winit_030::WinitWindowAccessor;
    let monitor = w.window().with_winit_window(|w| w.current_monitor().and_then(|m| m.name()))??;
    let (p, s) = (w.window().position(), w.window().size());
    Some((monitor, WinRect { x: p.x, y: p.y, w: s.width, h: s.height }))
}

/// Wires the operator window's dock and shows the layout Settings keeps.
pub fn wire(op: &Rc<Operator>) {
    let mut prefs = op.app().data.lock().unwrap().settings.dock.clone();
    prefs.current.repair();
    *op.dock.prefs.borrow_mut() = prefs;
    {
        let wins = op.dock.wins.borrow();
        op.ui.set_dock_places(ModelRc::from(wins[0].places.clone()));
        op.ui.set_dock_heads(ModelRc::from(wins[0].heads.clone()));
        op.ui.set_dock_bars(ModelRc::from(wins[0].bars.clone()));
    }
    op.dock_wire(Host::Main(&op.ui), 0);
    let me = op.me.clone();
    op.window_events().add(move |e| if let (Some(op), crate::platform::winit_events::WindowEvent::Focused(on)) = (me.upgrade(), e) { op.dock_window_focus(0, *on) });
    let me = op.me.clone();
    op.ui.on_top_need_changed(move || if let Some(op) = me.upgrade() { op.fit_top_bar() });
    let me = op.me.clone();
    op.ui.on_dock_menu(move |key| if let Some(op) = me.upgrade() { op.dock_menu(&key) });
    // Panel buttons that run the operator window's own handlers (a panel may be in a window of its own).
    let me = op.me.clone();
    op.ui.on_service_check(move || if let Some(op) = me.upgrade() { op.ui.invoke_check_service() });
    let me = op.me.clone();
    op.ui.on_slides_welcome_screens(move || if let Some(op) = me.upgrade() { op.ui.invoke_screens() });
    // The library's fold: every window follows (View → Show media library says the same).
    let me = op.me.clone();
    op.ui.on_bin_fold(move |open| if let Some(op) = me.upgrade() { op.ui.set_bin_open(open) });
    // Its first size: the testing backend never resizes, so the window starts at what it has.
    let size = op.ui.window().size().to_logical(op.ui.window().scale_factor());
    op.dock.wins.borrow_mut()[0].size = (size.width, (size.height - 60.0).max(0.0));
    op.dock_sync_windows();
    op.dock_relayout_all();
    op.dock_shared();
}

/// Windows of their own close with the operator window.
pub(crate) fn close_all(op: &Operator) {
    for w in op.dock.wins.borrow_mut().iter_mut() {
        if let Some(p) = w.window.take() { crate::ui::unregister_popout(&p); let _ = p.hide(); }
    }
}

#[cfg(test)]
#[path = "docking_tests.rs"]
mod tests;
