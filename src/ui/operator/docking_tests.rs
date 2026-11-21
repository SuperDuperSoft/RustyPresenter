//! The dock panels in the window (design 2026-11-17): Standard as today's window, arrange mode, a divider dragged,
//! a panel dropped onto another to make tabs, layouts saved, chosen and deleted, windows of their own opened and
//! closed, and nothing in any panel cut off or covered.
use super::*;
use crate::{
    app::{App, test_app},
    model::{ItemKind, ServiceItem},
    ui::{DockGuard, LivePane, ServicePane, operator::window::testing::{find, find_all, open, refresh}},
};
use i_slint_backend_testing::ElementHandle;
use slint::{LogicalPosition, platform::{PointerEventButton, WindowEvent}};
use std::sync::Arc;
use uuid::Uuid;

/// Resizes the window and runs what a frame would (change handlers, the panels' size reports).
fn resize(op: &Operator, (w, h): (f32, f32)) {
    op.window().set_size(slint::LogicalSize::new(w, h));
    settle();
}

fn settle() {
    for _ in 0..3 { slint::platform::update_timers_and_animations(); }
}

/// An element's box: left, top, right, bottom.
fn bounds(e: &ElementHandle) -> (f32, f32, f32, f32) {
    let (p, s) = (e.absolute_position(), e.size());
    (p.x, p.y, p.x + s.width, p.y + s.height)
}

fn slot(op: &Operator, id: &str) -> (f32, f32, f32, f32) { bounds(&find(op, id).unwrap_or_else(|| panic!("{id} shown"))) }

fn texts(app: &Arc<App>, names: &[&str]) -> Vec<Uuid> {
    let mut d = app.data.lock().unwrap();
    names.iter().map(|n| {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: (*n).into(), body: "Hello".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None };
        d.services[0].items.push(item.clone());
        item.id
    }).collect()
}

fn saved(app: &App) -> dock::DockPrefs { app.data.lock().unwrap().settings.dock.clone() }

fn click(w: &slint::Window, e: &ElementHandle) {
    let (l, t, r, b) = bounds(e);
    let at = LogicalPosition::new((l + r) / 2.0, (t + b) / 2.0);
    w.dispatch_event(WindowEvent::PointerMoved { position: at });
    w.dispatch_event(WindowEvent::PointerPressed { position: at, button: PointerEventButton::Left });
    w.dispatch_event(WindowEvent::PointerReleased { position: at, button: PointerEventButton::Left });
}

#[test]
fn standard_is_todays_window() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    for (w, h) in [(1440.0, 900.0), (1024.0, 768.0)] {
        resize(&op, (w, h));
        let live = slot(&op, "DockArea::live-col");
        let service = slot(&op, "DockArea::service-col");
        let slides = slot(&op, "DockArea::slides-col");
        let bible = slot(&op, "DockArea::bible-col");
        let bin = slot(&op, "DockArea::bin-col");
        // Today: the live column 440 wide on the right, 8 px from the edge, from under the top bar to the bottom.
        assert_eq!((live.2 - live.0, w - live.2), (440.0, 8.0), "live column at {w}×{h}");
        assert!((h - live.3 - 8.0).abs() < 0.6, "the live column runs to the bottom at {w}×{h}: {live:?}");
        // The service list 264 wide on the left; the slides between it and Live; the library under both.
        assert_eq!((service.0, service.2 - service.0), (8.0, 264.0), "service list at {w}×{h}");
        assert_eq!((slides.0, slides.2), (service.2 + 8.0, live.0 - 8.0), "slides at {w}×{h}");
        assert_eq!((bin.0, bin.2), (8.0, live.0 - 8.0));
        assert_eq!(bin.1, service.3 + 8.0);
        assert!((bin.3 - live.3).abs() < 0.6);
        // The Bible box over the work area, never over Live.
        assert_eq!((bible.0, bible.2, bible.1), (8.0, live.0 - 8.0, live.1));
        assert_eq!(service.1, bible.3 + 8.0);
        assert!(find(&op, "DockArea::grip").is_none(), "locked: no title bars");
    }
}

#[test]
fn arrange_mode_adds_title_bars_and_takes_them_away() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    resize(&op, (1440.0, 900.0));
    op.dock_menu("arrange");
    settle();
    assert!(op.ui.get_dock_arrange());
    assert_eq!(find_all(&op, "DockArea::grip").len(), 5, "a grip on each panel");
    let (_, bt, _, _) = slot(&op, "DockArea::bible-col");
    assert_eq!(bt, find_all(&op, "DockArea::head-bar")[0].absolute_position().y + 30.0, "the title bar sits on its panel");
    op.dock_menu("arrange");
    settle();
    assert!(find(&op, "DockArea::grip").is_none());
}

#[test]
fn dragging_a_divider_resizes_and_is_kept() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    resize(&op, (1440.0, 900.0));
    // The strip left of Live, dragged 100 px left with the mouse.
    let live = slot(&op, "DockArea::live-col");
    let bar = find_all(&op, "DockArea::divider").into_iter().find(|d| (bounds(d).2 - live.0).abs() < 0.6).expect("the strip left of Live");
    let (l, t, r, b) = bounds(&bar);
    let w = op.window();
    let from = LogicalPosition::new((l + r) / 2.0, (t + b) / 2.0);
    w.dispatch_event(WindowEvent::PointerMoved { position: from });
    w.dispatch_event(WindowEvent::PointerPressed { position: from, button: PointerEventButton::Left });
    for dx in [20.0, 60.0, 100.0] {
        w.dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(from.x - dx, from.y) });
        settle();
    }
    w.dispatch_event(WindowEvent::PointerReleased { position: LogicalPosition::new(from.x - 100.0, from.y), button: PointerEventButton::Left });
    settle();
    let live = slot(&op, "DockArea::live-col");
    assert_eq!(live.2 - live.0, 540.0, "Live 100 px wider");
    let kept = saved(&app);
    let p = dock::place(&kept.current.windows[0].root, 1440.0, 800.0, false, &|_| (0.0, 0.0));
    assert_eq!(p.panel(PanelId::Live).unwrap().rect.w, 540.0, "saved in Settings");
}

#[test]
fn dropping_a_panel_in_the_middle_of_another_makes_tabs() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    resize(&op, (1440.0, 900.0));
    op.dock_set_arrange(true);
    settle();
    // The library's title bar dragged to the middle of the slides (area coordinates).
    let s = op.dock_placement(0).panel(PanelId::Slides).unwrap().rect;
    let (x, y) = (s.x + s.w / 2.0, s.y + s.h / 2.0);
    op.ui.invoke_dock_drag_moved(PanelId::Library.index() as i32, x, y);
    assert!(op.ui.get_dock_lit_shown(), "the place it would go lights up");
    op.ui.invoke_dock_drag_done(PanelId::Library.index() as i32, x, y);
    settle();
    assert!(!op.ui.get_dock_lit_shown());
    assert_eq!(op.dock_layout().windows[0].root.group_of(PanelId::Slides), Some(vec![PanelId::Slides, PanelId::Library]));
    op.dock_set_arrange(false);
    settle();
    let tabs = find_all(&op, "DockArea::tab-chip");
    assert_eq!(tabs.len(), 2, "a tab bar with both, even when locked");
    // A click on the Slides tab shows the slides.
    let slides_tab = tabs.into_iter().find(|t| t.accessible_label().unwrap_or_default() == "Slides").unwrap();
    click(op.window(), &slides_tab);
    settle();
    assert!(op.dock_placement(0).panel(PanelId::Slides).unwrap().shown);
    assert!(!op.dock_placement(0).panel(PanelId::Library).unwrap().shown);
}

#[test]
fn layouts_saved_chosen_deleted_and_reset() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    resize(&op, (1440.0, 900.0));
    op.dock_menu(&format!("panel:{}", PanelId::Cameras.index()));
    assert!(op.dock_layout().is_shown(PanelId::Cameras), "View → Panels → Cameras shows it");
    op.dock_store_as("Easter");
    let names = |op: &Operator| -> Vec<String> { op.ui.get_dock_layout_rows().iter().map(|r| r.name.to_string()).collect() };
    assert_eq!(names(&op), ["Standard", "Big slides", "ProPresenter style", "EasyWorship style", "Livestream", "Two monitors", "Easter"]);
    assert!(op.ui.get_dock_saved_chosen());
    assert_eq!(saved(&app).saved.len(), 1, "saved in Settings");
    op.dock_menu("layout:builtin:big-slides");
    assert_eq!(op.dock_layout().windows[0].root.group_of(PanelId::Service), Some(vec![PanelId::Service, PanelId::Library]));
    assert!(!op.ui.get_dock_saved_chosen());
    op.dock_menu("layout:saved:Easter");
    assert!(op.dock_layout().is_shown(PanelId::Cameras), "Easter has Cameras");
    assert_eq!(saved(&app).chosen, Chosen::Saved("Easter".into()));
    op.dock_rename_to("Easter", "Holy week");
    assert_eq!(names(&op).last().unwrap(), "Holy week");
    op.dock_delete_saved("Holy week");
    assert_eq!(names(&op).len(), Builtin::ALL.len());
    assert_eq!(op.dock_layout(), Layout::builtin(Builtin::Standard));
    // Reset to Standard after changes.
    op.dock_menu(&format!("panel:{}", PanelId::Library.index()));
    assert!(!op.dock_layout().is_shown(PanelId::Library));
    op.dock_menu("reset");
    assert_eq!(op.dock_layout(), Layout::builtin(Builtin::Standard));
    assert_eq!(saved(&app).current, Layout::builtin(Builtin::Standard));
}

#[test]
fn the_layout_in_settings_opens_next_time() {
    let app = test_app();
    {
        let op = open(&app);
        refresh(&op);
        op.dock_menu("layout:builtin:livestream");
    }
    let op = open(&app);
    assert!(op.dock_layout().is_shown(PanelId::Livestream), "the window opens as it was left");
    assert!(op.ui.get_livestream_apart(), "the top bar leaves the stream's buttons to the panel");
}

#[test]
fn a_panel_in_a_window_of_its_own_shows_the_same_and_its_clicks_work() {
    let app = test_app();
    let ids = texts(&app, &["Welcome", "Notices"]);
    let op = open(&app);
    refresh(&op);
    op.ui.set_live_what("Welcome".into());
    op.dock_panel_action(PanelId::Service, "new-window");
    op.dock_panel_action(PanelId::Live, "window:1");
    settle();
    let pops = op.dock_popouts();
    assert_eq!(pops.len(), 1);
    let pw = &pops[0];
    assert_eq!(op.dock_layout().window_of(PanelId::Live), Some(1));
    assert!(find(&op, "DockArea::service-col").is_none() && find(&op, "DockArea::live-col").is_none(), "gone from the main window");
    assert_eq!(pw.global::<LivePane>().get_live_what(), "Welcome", "the state it had");
    // What changes later shows there too.
    op.ui.set_live_what("Notices".into());
    assert_eq!(pw.global::<LivePane>().get_live_what(), "Notices");
    pw.window().set_size(slint::LogicalSize::new(900.0, 700.0));
    settle();
    let rows: Vec<ElementHandle> = ElementHandle::find_by_element_id(pw, "ServiceColumn::row").collect();
    assert_eq!(rows.len(), 2, "the service list in the window of its own");
    assert!(ElementHandle::find_by_element_id(pw, "LiveColumn::next-btn").next().is_some());
    // A click on a row there selects it, as in the main window.
    click(pw.window(), &rows[1]);
    settle();
    assert_eq!(op.state.borrow().selected, Some(ids[1]));
    assert_eq!(pw.global::<ServicePane>().get_service_rows().row_data(1).unwrap().selected, true);
    // The title names its panels.
    assert_eq!(pw.get_panels_title(), "RustyPresenter — Service, Live");
    // Closed: its panels come back into the main window, none lost.
    let id = op.dock_layout().windows[1].id;
    op.dock_close_window(id);
    settle();
    assert!(op.dock_popouts().is_empty());
    for p in [PanelId::Service, PanelId::Live] { assert_eq!(op.dock_layout().window_of(p), Some(0), "{p:?}") }
    assert!(find(&op, "DockArea::service-col").is_some() && find(&op, "DockArea::live-col").is_some());
}

#[test]
fn two_monitors_opens_the_live_panels_in_their_own_window() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    op.dock_menu("layout:builtin:two-monitors");
    settle();
    assert_eq!(op.dock_popouts().len(), 1);
    assert_eq!(op.dock_popouts()[0].get_panels_title(), "RustyPresenter — Live, Cameras, Livestream, Message and macros");
    // Remembered: saved with its window, which opens again next time.
    assert_eq!(saved(&app).current.windows.len(), 2);
    op.dock_menu("reset");
    assert!(op.dock_popouts().is_empty(), "Standard has no window of its own");
}

#[test]
fn ctrl_b_brings_a_closed_bible_box_back() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    op.dock_menu(&format!("panel:{}", PanelId::Bible.index()));
    settle();
    assert!(find(&op, "BibleBox::bible-input").is_none());
    op.focus_bible();
    settle();
    assert!(find(&op, "BibleBox::bible-input").is_some());
}

#[test]
fn the_last_panel_of_the_main_window_stays() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    for p in PanelId::ALL { op.dock_menu(&format!("panel:{}", p.index())) }
    // Every shown panel was toggled: the closed ones opened, the open ones closed, but not the last.
    let l = op.dock_layout();
    assert!(!l.windows[0].root.panels().is_empty());
    assert!(op.toast_log().iter().any(|(t, _)| t == "The main window keeps at least one panel.") || l.windows[0].root.panels().len() > 1);
}

/// Jake's "the Bible thing can go over the room thing": the Bible box was in the top bar, where at some widths its
/// chips ran past its own box over the Room light, and its lists dropped down over the live preview. It is a panel
/// now, and its lists keep clear of the previews.
#[test]
fn nothing_covers_anything() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    // The busiest top bar: recording, live with every button, the stage and remotes lights.
    op.ui.set_recording(true);
    op.ui.set_replay_minutes(2);
    op.ui.set_record_time("12:34".into());
    op.ui.set_can_stream(true);
    op.ui.set_live(true);
    op.ui.set_live_level(3);
    op.ui.set_chat_on(true);
    op.ui.set_scene_labels(ModelRc::new(VecModel::from(vec!["Slides".into(), "Camera".into()])));
    op.ui.set_stage_shown(true);
    op.ui.set_phones_label("2 remotes".into());
    op.ui.set_check_label("Check".into());
    for b in Builtin::ALL {
        op.dock_menu(&format!("layout:builtin:{}", builtin_key(b)));
        for size in [(1024.0, 768.0), (1280.0, 800.0), (1440.0, 900.0), (1920.0, 1080.0)] {
            resize(&op, size);
            // The top bar's parts stay inside the window and in order.
            let bar = ["TopBar::service-pick", "TopBar::status-room", "TopBar::record-btn", "TopBar::live-btn", "TopBar::screens-btn", "TopBar::phone-btn", "TopBar::settings-btn"];
            let mut last = 0.0;
            for id in bar {
                let (l, _, r, _) = bounds(&find(&op, id).unwrap_or_else(|| panic!("{id} shown at {size:?} ({b:?})")));
                assert!(l >= last - 0.5 && r <= size.0 + 0.5, "{id} at {l}–{r} in the {size:?} top bar ({b:?})");
                last = r;
            }
            // No panel covers another.
            let placed = op.dock_placement(0);
            let shown: Vec<_> = placed.panels.iter().filter(|p| p.shown).collect();
            for (i, a) in shown.iter().enumerate() {
                for c in &shown[i + 1..] { assert!(!a.rect.overlaps(&c.rect), "{a:?} covers {c:?} at {size:?}") }
            }
            // The Bible picker under its chip: never over the live preview.
            if let Some(pick) = find(&op, "BibleBox::translation-pick") {
                let (l, _, r, bt) = bounds(&pick);
                let g = op.ui.global::<DockGuard>();
                let x = g.invoke_place_x(r - 300.0, bt + 8.0, 300.0, 460.0);
                let room = g.invoke_room_below(x, bt + 8.0, 300.0);
                if let Some(live) = find(&op, "DockArea::live-col") {
                    let (ll, lt, lr, lb) = bounds(&live);
                    let h = room.min(460.0);
                    let covers = x < lr && ll < x + 300.0 && bt + 8.0 < lb && lt < bt + 8.0 + h;
                    assert!(!covers, "the picker at {x},{} {h} high covers the preview {ll},{lt}–{lr},{lb} at {size:?} ({b:?})", bt + 8.0);
                }
                let _ = l;
            }
        }
    }
}

/// Jake: "sometimes things cut off and I can't see it all". Every button and light in every panel lies inside its
/// panel, from a 1024 × 768 laptop to a 1080p monitor, in every built-in layout.
#[test]
fn nothing_in_a_panel_is_cut_off() {
    let app = test_app();
    texts(&app, &["A rather long announcement title that goes on and on", "Notices"]);
    let op = open(&app);
    refresh(&op);
    op.apply_live(crate::app::LiveUpdate::new(app.live_view()));
    op.ui.set_drawing_on(true);
    let types = ["Btn", "NavBtn", "ClearBtn", "ClearAllBtn", "TopBtn", "Pill", "RecordBtn", "LiveBtn", "PanicBtn", "Select"];
    for b in Builtin::ALL {
        op.dock_menu(&format!("layout:builtin:{}", builtin_key(b)));
        for size in [(1024.0, 768.0), (1920.0, 1080.0)] {
            resize(&op, size);
            for id in ["DockArea::service-col", "DockArea::slides-col", "DockArea::bin-col", "DockArea::bible-col", "DockArea::live-col",
                "DockArea::cameras-col", "DockArea::automation-col", "DockArea::livestream-col"] {
                let Some(panel) = find(&op, id) else { continue };
                let (pl, pt, pr, pb) = bounds(&panel);
                for t in types {
                    for e in panel.query_descendants().match_type_name(t).find_all() {
                        if e.size().width <= 0.0 { continue }
                        let (l, tp, r, bt) = bounds(&e);
                        assert!(l >= pl - 0.5 && r <= pr + 0.5 && tp >= pt - 0.5 && bt <= pb + 0.5,
                            "{t} {:?} at {l},{tp}–{r},{bt} is cut off by {id} {pl},{pt}–{pr},{pb} at {size:?} ({b:?})", e.accessible_label());
                    }
                }
            }
        }
    }
}

#[test]
fn the_first_zoom_suits_the_screen() {
    use super::super::menu::auto_zoom_step;
    assert_eq!(auto_zoom_step(1093.0, 614.0), Some(-2), "1366 × 768 at 125 %: 80 %");
    assert_eq!(auto_zoom_step(1280.0, 720.0), Some(-1), "1280 × 720: 90 %");
    assert_eq!(auto_zoom_step(1366.0, 768.0), None);
    assert_eq!(auto_zoom_step(1920.0, 1080.0), None);
    assert_eq!(auto_zoom_step(2560.0, 1440.0), Some(1), "a large monitor at 100 %: 110 %");
}



// Dragging a panel between windows (dock panels design, "Dragging between windows"). The testing backend has no
// screen, so each window is given its place on one (`fake_screen`): the main window at the top left, a window of
// its own on the right with desktop between them. The pointer is pressed on a title bar's grip and moved and let go
// in that window's terms, outside it too, as Windows sends them while the window keeps the mouse.

const MAIN: WinRect = WinRect { x: 0, y: 0, w: 1440, h: 900 };
const OTHER: WinRect = WinRect { x: 2000, y: 100, w: 900, h: 700 };

/// The operator window in arrange mode with Live in a window of its own, both placed on the screen.
fn two_windows(app: &Arc<App>) -> Rc<Operator> {
    let op = open(app);
    refresh(&op);
    resize(&op, (1440.0, 900.0));
    op.dock_panel_action(PanelId::Live, "new-window");
    op.dock_set_arrange(true);
    let pw = op.dock_popouts()[0].clone_strong();
    pw.window().set_size(slint::LogicalSize::new(900.0, 700.0));
    settle();
    let id = op.dock_layout().windows[1].id;
    *op.dock.fake_screen.borrow_mut() = vec![(0, MAIN), (id, OTHER)];
    op
}

/// The grip of `p`'s title bar in `w`.
fn grip(w: &impl ComponentHandle, p: PanelId) -> ElementHandle {
    let label = format!("Move {}", panel_name(p));
    ElementHandle::find_by_element_id(w, "DockArea::grip").find(|g| g.accessible_label().unwrap_or_default() == label)
        .unwrap_or_else(|| panic!("{label}"))
}

/// A point of window `i`'s dock area on the screen.
fn on_screen(op: &Operator, i: usize, at: (f32, f32)) -> (f32, f32) {
    let r = if i == 0 { MAIN } else { OTHER };
    let o = if i == 0 { (op.ui.get_dock_origin_x(), op.ui.get_dock_origin_y()) } else {
        let pw = &op.dock_popouts()[i - 1];
        (pw.get_dock_origin_x(), pw.get_dock_origin_y())
    };
    (r.x as f32 + o.0 + at.0, r.y as f32 + o.1 + at.1)
}

/// Presses the grip `g` in `w` (placed at `r` on the screen), moves a little, then to each screen point in turn.
fn drag_to(w: &slint::Window, g: &ElementHandle, r: WinRect, points: &[(f32, f32)]) {
    let (l, t, rr, b) = bounds(g);
    let from = LogicalPosition::new((l + rr) / 2.0, (t + b) / 2.0);
    w.dispatch_event(WindowEvent::PointerMoved { position: from });
    w.dispatch_event(WindowEvent::PointerPressed { position: from, button: PointerEventButton::Left });
    w.dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(from.x + 10.0, from.y + 10.0) });
    for (x, y) in points {
        w.dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(x - r.x as f32, y - r.y as f32) });
        settle();
    }
}

fn let_go(w: &slint::Window, r: WinRect, (x, y): (f32, f32)) {
    w.dispatch_event(WindowEvent::PointerReleased { position: LogicalPosition::new(x - r.x as f32, y - r.y as f32), button: PointerEventButton::Left });
    settle();
}

#[test]
fn a_panel_dragged_onto_another_window_goes_there() {
    let app = test_app();
    let op = two_windows(&app);
    let pw = op.dock_popouts()[0].clone_strong();
    // The library's title bar dragged out of the main window, over the desktop, onto the right of Live.
    let live = op.dock_placement(1).panel(PanelId::Live).unwrap().rect;
    let over = on_screen(&op, 1, (live.x + live.w * 0.9, live.y + live.h / 2.0));
    drag_to(op.window(), &grip(&op.ui, PanelId::Library), MAIN, &[(1700.0, 400.0), over]);
    assert!(pw.get_dock_lit_shown(), "the window under the pointer lights up where it would go");
    assert!(!op.ui.get_dock_lit_shown(), "the main window does not");
    assert_eq!(pw.get_dock_ghost(), PanelId::Library.index() as i32, "its name at the pointer there");
    assert_eq!(op.ui.get_dock_ghost(), -1);
    let lit = pw.get_dock_lit();
    assert!(lit.x > live.x + 1.0 && (lit.w - live.w / 2.0).abs() < 1.0, "the right half of Live: {lit:?}");
    assert_eq!(op.dock_layout().window_of(PanelId::Library), Some(0), "nothing moves before the button is let go");
    let_go(op.window(), MAIN, over);
    let l = op.dock_layout();
    assert_eq!(l.window_of(PanelId::Library), Some(1), "dropped in the window of its own");
    assert!(!pw.get_dock_lit_shown() && pw.get_dock_ghost() == -1, "nothing left lit");
    assert!(ElementHandle::find_by_element_id(&pw, "DockArea::bin-col").next().is_some(), "the library shows there");
    assert!(find(&op, "DockArea::bin-col").is_none());
    assert!(saved(&app).current.same_arrangement(&l), "remembered for next time");
}

#[test]
fn the_last_panel_dragged_out_of_a_window_closes_it() {
    let app = test_app();
    let op = two_windows(&app);
    let pw = op.dock_popouts()[0].clone_strong();
    // Live dragged from its window of its own back onto the middle of the slides in the main window.
    let s = op.dock_placement(0).panel(PanelId::Slides).unwrap().rect;
    let over = on_screen(&op, 0, (s.x + s.w / 2.0, s.y + s.h / 2.0));
    drag_to(pw.window(), &grip(&pw, PanelId::Live), OTHER, &[over]);
    assert!(op.ui.get_dock_lit_shown());
    let_go(pw.window(), OTHER, over);
    assert_eq!(op.dock_layout().windows[0].root.group_of(PanelId::Slides), Some(vec![PanelId::Slides, PanelId::Live]));
    assert!(op.dock_popouts().is_empty(), "the window with nothing left closed");
    assert_eq!(saved(&app).current.windows.len(), 1);
}

#[test]
fn a_panel_let_go_over_the_desktop_opens_a_window_of_its_own_there() {
    let app = test_app();
    let op = two_windows(&app);
    let was = op.dock_placement(0).panel(PanelId::Library).unwrap().rect;
    drag_to(op.window(), &grip(&op.ui, PanelId::Library), MAIN, &[(1700.0, 300.0)]);
    assert!(!op.ui.get_dock_lit_shown() && op.dock_popouts().iter().all(|p| !p.get_dock_lit_shown()), "nothing to light up over the desktop");
    let_go(op.window(), MAIN, (1700.0, 300.0));
    let l = op.dock_layout();
    assert_eq!(l.windows.len(), 3);
    assert_eq!(l.windows[2].root.panels(), [PanelId::Library]);
    let (_, place) = drag::tear_off_place(1700, 300, (was.w, was.h), 1.0, &[]);
    assert_eq!(l.windows[2].place, Some(place), "at the pointer, as large as the panel was");
    assert_eq!(op.dock_popouts().len(), 2, "its window opened");
}

#[test]
fn a_panel_alone_in_its_window_let_go_over_the_desktop_moves_its_window() {
    let app = test_app();
    let op = two_windows(&app);
    let pw = op.dock_popouts()[0].clone_strong();
    drag_to(pw.window(), &grip(&pw, PanelId::Live), OTHER, &[(1700.0, 300.0)]);
    let_go(pw.window(), OTHER, (1700.0, 300.0));
    let l = op.dock_layout();
    assert_eq!(l.windows.len(), 2, "no new window");
    assert_eq!(l.windows[1].root.panels(), [PanelId::Live]);
    assert_eq!(l.windows[1].place.map(|p| (p.x, p.y)), Some((1700 - 28, 300 - 23)), "moved to the pointer");
}

#[test]
fn let_go_back_in_its_own_window_it_is_a_drop_as_before() {
    let app = test_app();
    let op = two_windows(&app);
    let s = op.dock_placement(0).panel(PanelId::Service).unwrap().rect;
    let over = on_screen(&op, 0, (s.x + s.w / 2.0, s.y + s.h / 2.0));
    // Out over the other window and back.
    let live = op.dock_placement(1).panel(PanelId::Live).unwrap().rect;
    drag_to(op.window(), &grip(&op.ui, PanelId::Library), MAIN, &[on_screen(&op, 1, (live.x + 50.0, live.y + 50.0)), over]);
    assert!(op.ui.get_dock_lit_shown() && !op.dock_popouts()[0].get_dock_lit_shown());
    assert_eq!(op.ui.get_dock_ghost(), PanelId::Library.index() as i32);
    let_go(op.window(), MAIN, over);
    assert_eq!(op.dock_layout().windows[0].root.group_of(PanelId::Service), Some(vec![PanelId::Service, PanelId::Library]));
    assert_eq!(op.dock_layout().windows.len(), 2);
}

#[test]
fn esc_a_right_click_or_losing_the_keyboard_stops_a_drag() {
    let app = test_app();
    let op = two_windows(&app);
    let before = op.dock_layout();
    let live = op.dock_placement(1).panel(PanelId::Live).unwrap().rect;
    let over = on_screen(&op, 1, (live.x + live.w / 2.0, live.y + live.h / 2.0));
    let pw = op.dock_popouts()[0].clone_strong();
    // Esc: nothing moves, and the rest of the drag is ignored.
    drag_to(op.window(), &grip(&op.ui, PanelId::Library), MAIN, &[over]);
    assert!(pw.get_dock_lit_shown());
    assert!(op.key("\u{1b}", false, false, false, false, false), "Esc is taken by the drag");
    assert!(!pw.get_dock_lit_shown() && pw.get_dock_ghost() == -1);
    let w = op.window();
    w.dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(over.0 + 5.0, over.1) });
    settle();
    assert!(!pw.get_dock_lit_shown(), "moving on after Esc shows nothing");
    let_go(w, MAIN, over);
    assert_eq!(op.dock_layout(), before);
    assert!(!op.dock_dragging());
    // A right click while dragging.
    drag_to(w, &grip(&op.ui, PanelId::Library), MAIN, &[over]);
    let at = LogicalPosition::new(over.0, over.1);
    w.dispatch_event(WindowEvent::PointerPressed { position: at, button: PointerEventButton::Right });
    w.dispatch_event(WindowEvent::PointerReleased { position: at, button: PointerEventButton::Right });
    settle();
    assert!(!pw.get_dock_lit_shown());
    let_go(w, MAIN, over);
    assert_eq!(op.dock_layout(), before, "a right click stops it");
    // Another program takes the keyboard mid-drag.
    drag_to(w, &grip(&op.ui, PanelId::Library), MAIN, &[over]);
    op.dock_window_focus(0, false);
    assert!(!pw.get_dock_lit_shown());
    let_go(w, MAIN, over);
    assert_eq!(op.dock_layout(), before, "losing the keyboard stops it");
    // And the next drag works.
    op.dock_window_focus(0, true);
    drag_to(w, &grip(&op.ui, PanelId::Library), MAIN, &[over]);
    let_go(w, MAIN, over);
    assert_eq!(op.dock_layout().window_of(PanelId::Library), Some(1));
}

/// Jake: "It needs to let us save arrangements and remember arrangements, or layouts." What dragging changes is
/// kept for next time, and "Save this layout as…" saves it, windows and all.
#[test]
fn what_dragging_changes_is_remembered_and_saved_as_a_layout() {
    let app = test_app();
    let (layout, place) = {
        let op = two_windows(&app);
        // A cross-window drop: the slides onto Live's window; a tear-off: the library onto the desktop.
        let live = op.dock_placement(1).panel(PanelId::Live).unwrap().rect;
        let over = on_screen(&op, 1, (live.x + live.w / 2.0, live.y + live.h * 0.9));
        drag_to(op.window(), &grip(&op.ui, PanelId::Slides), MAIN, &[over]);
        let_go(op.window(), MAIN, over);
        drag_to(op.window(), &grip(&op.ui, PanelId::Library), MAIN, &[(1700.0, 300.0)]);
        let_go(op.window(), MAIN, (1700.0, 300.0));
        let l = op.dock_layout();
        assert_eq!(l.window_of(PanelId::Slides), Some(1));
        assert_eq!(l.window_of(PanelId::Library), Some(2));
        close_all(&op);
        (l.clone(), l.windows[2].place)
    };
    // Next time.
    let op = open(&app);
    refresh(&op);
    let back = op.dock_layout();
    assert!(back.same_arrangement(&layout), "the same windows and panels next time");
    assert_eq!(back.windows[2].place, place, "the torn-off window where it was");
    assert_eq!(op.dock_popouts().len(), 2, "its windows open again");
    op.dock_store_as("Sunday");
    let kept = saved(&app);
    let sunday = kept.saved.iter().find(|l| l.name == "Sunday").expect("saved");
    assert!(sunday.same_arrangement(&layout), "Save this layout as… saves every window");
    assert_eq!(sunday.windows[2].place, place);
}

/// A church keeps its named layout up to date: View → Layouts says when the layout in use was changed since it was
/// chosen, and "Update “Sunday”" saves the changes into it.
#[test]
fn a_changed_saved_layout_says_so_and_updates_in_one_click() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    resize(&op, (1440.0, 900.0));
    op.dock_store_as("Sunday");
    let names = |op: &Operator| -> Vec<String> { op.ui.get_dock_layout_rows().iter().map(|r| r.name.to_string()).collect() };
    assert_eq!(names(&op).last().unwrap(), "Sunday");
    assert_eq!(op.ui.get_dock_update_title(), "", "nothing to update");
    op.dock_menu(&format!("panel:{}", PanelId::Cameras.index()));
    assert_eq!(names(&op).last().unwrap(), "Sunday (changed)");
    assert_eq!(op.ui.get_dock_update_title(), "Update “Sunday”");
    op.dock_menu("update");
    assert_eq!(names(&op).last().unwrap(), "Sunday");
    assert_eq!(op.ui.get_dock_update_title(), "");
    assert!(saved(&app).saved[0].is_shown(PanelId::Cameras), "the saved layout has the change");
    // Choosing another layout and coming back: Sunday as updated.
    op.dock_menu("layout:builtin:standard");
    op.dock_menu("layout:saved:Sunday");
    assert!(op.dock_layout().is_shown(PanelId::Cameras));
    assert_eq!(names(&op).last().unwrap(), "Sunday");
}
