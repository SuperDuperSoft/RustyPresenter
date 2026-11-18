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
    assert_eq!(names(&op), ["Standard", "Big slides", "Livestream", "Two monitors", "Easter"]);
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
    assert_eq!(names(&op).len(), 4);
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


