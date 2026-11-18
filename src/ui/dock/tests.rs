use super::*;
use PanelId::*;

/// No content wants more than its minimum.
fn no_fit(_: PanelId) -> (f32, f32) { (0.0, 0.0) }

fn std_layout() -> Layout { Layout::builtin(Builtin::Standard) }

fn main_panels(l: &Layout) -> Vec<PanelId> { l.windows[0].root.panels() }

/// Every panel is shown once or closed once, never both, never twice.
fn assert_valid(l: &Layout) {
    let mut all: Vec<PanelId> = l.windows.iter().flat_map(|w| w.root.panels()).collect();
    all.extend(&l.closed);
    let mut sorted = all.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), all.len(), "a panel twice: {all:?}");
    assert_eq!(sorted, PanelId::ALL.to_vec(), "every panel once: {all:?}");
    for w in &l.windows { assert_eq!(w.root.clone().normalized().as_ref(), Some(&w.root), "tidy tree") }
}

/// No two shown panels overlap, and each is inside the area.
fn assert_no_overlap(p: &Placement) {
    let shown: Vec<&PanelPlace> = p.panels.iter().filter(|x| x.shown).collect();
    for (i, a) in shown.iter().enumerate() {
        assert!(a.rect.x >= 0.0 && a.rect.y >= 0.0 && a.rect.right() <= p.width + 0.01 && a.rect.bottom() <= p.height + 0.01, "{a:?} inside");
        let (mw, mh) = a.panel.min_size();
        assert!(a.rect.w + 0.6 >= mw && a.rect.h + 0.6 >= mh, "{:?} keeps its minimum: {:?}", a.panel, a.rect);
        for b in &shown[i + 1..] { assert!(!a.rect.overlaps(&b.rect), "{a:?} overlaps {b:?}") }
        for h in &p.heads { if !h.panels.contains(&a.panel) { assert!(!a.rect.overlaps(&h.rect), "{a:?} under a head") } }
    }
}

#[test]
fn builtins_are_valid_and_standard_is_todays_window() {
    for b in Builtin::ALL { assert_valid(&Layout::builtin(b)); }
    let p = place(&std_layout().windows[0].root, 1440.0, 840.0, false, &|p| if p == Bible { (0.0, 58.0) } else if p == Library { (0.0, 260.0) } else { (0.0, 0.0) });
    assert_no_overlap(&p);
    let r = |x: PanelId| p.panel(x).unwrap().rect;
    // Today: 8 px around and between; the live column 440 wide on the right; the service list 264 wide.
    assert_eq!(r(Live), Rect { x: 1440.0 - 8.0 - 440.0, y: 8.0, w: 440.0, h: 840.0 - 16.0 });
    assert_eq!(r(Service).w, 264.0);
    assert_eq!(r(Service).x, 8.0);
    assert_eq!(r(Slides).x, 8.0 + 264.0 + 8.0);
    assert_eq!(r(Slides).right(), r(Live).x - 8.0);
    assert_eq!(r(Bible), Rect { x: 8.0, y: 8.0, w: r(Live).x - 16.0, h: 58.0 });
    assert_eq!(r(Library).h, 260.0);
    assert_eq!(r(Library).bottom(), 840.0 - 8.0);
    assert_eq!(r(Service).y, 8.0 + 58.0 + 8.0);
    assert!(p.heads.is_empty(), "no title bars outside arrange mode");
    assert_eq!(p.dividers.len(), 4, "work area | live, bible / middle, middle / library, service | slides");
}

#[test]
fn small_window_scrolls_instead_of_squashing() {
    let l = std_layout();
    let p = place(&l.windows[0].root, 600.0, 250.0, false, &no_fit);
    assert!(p.width > 600.0 && p.height > 250.0, "the area is larger than the window: it scrolls");
    assert_no_overlap(&p);
    // 1024 × 768 fits as today.
    let p = place(&l.windows[0].root, 1024.0, 700.0, false, &no_fit);
    assert_eq!((p.width, p.height), (1024.0, 700.0));
    assert_no_overlap(&p);
}

#[test]
fn every_builtin_places_without_overlap_at_many_sizes() {
    for b in Builtin::ALL {
        let l = Layout::builtin(b);
        for w in &l.windows {
            for (ww, hh) in [(1024.0, 700.0), (1280.0, 720.0), (1920.0, 1040.0), (3840.0, 2100.0), (500.0, 300.0)] {
                for arrange in [false, true] {
                    assert_no_overlap(&place(&w.root, ww, hh, arrange, &|_| (100.0, 120.0)));
                }
            }
        }
    }
}

#[test]
fn arrange_mode_gives_every_group_a_title_bar() {
    let p = place(&std_layout().windows[0].root, 1440.0, 900.0, true, &no_fit);
    assert_eq!(p.heads.len(), 5);
    assert_no_overlap(&p);
    for pp in &p.panels {
        let h = p.heads.iter().find(|h| h.panels.contains(&pp.panel)).unwrap();
        assert_eq!(h.rect.bottom(), pp.rect.y, "the title bar sits on its panel");
    }
}

#[test]
fn drop_beside_and_into_tabs() {
    let mut l = std_layout();
    // Library onto the slides' middle: a tab with them.
    assert!(l.drop_on(Library, Slides, Zone::Center));
    assert_valid(&l);
    assert_eq!(l.windows[0].root.group_of(Slides), Some(vec![Slides, Library]));
    let p = place(&l.windows[0].root, 1440.0, 900.0, false, &no_fit);
    assert_eq!(p.heads.len(), 1, "a tab bar over the two");
    assert!(p.panel(Library).unwrap().shown && !p.panel(Slides).unwrap().shown, "the dropped one is picked");
    l.activate(Slides);
    let p = place(&l.windows[0].root, 1440.0, 900.0, false, &no_fit);
    assert!(p.panel(Slides).unwrap().shown);
    assert_no_overlap(&p);

    // Back out to the right of the live column, then below it.
    for zone in [Zone::Left, Zone::Right, Zone::Top, Zone::Bottom] {
        assert!(l.drop_on(Library, Live, zone), "{zone:?}");
        assert_valid(&l);
        let p = place(&l.windows[0].root, 1600.0, 1000.0, false, &no_fit);
        assert_no_overlap(&p);
        let (a, b) = (p.panel(Library).unwrap().rect, p.panel(Live).unwrap().rect);
        match zone {
            Zone::Left => assert!(a.right() <= b.x),
            Zone::Right => assert!(b.right() <= a.x),
            Zone::Top => assert!(a.bottom() <= b.y),
            Zone::Bottom => assert!(b.bottom() <= a.y),
            Zone::Center => unreachable!(),
        }
    }
}

#[test]
fn drops_that_change_nothing_are_refused() {
    let mut l = std_layout();
    let before = l.clone();
    assert!(!l.drop_on(Live, Live, Zone::Center));
    assert!(!l.drop_on(Live, Live, Zone::Left), "alone in its group: nowhere to go");
    assert_eq!(l, before);
    l.drop_on(Library, Slides, Zone::Center);
    assert!(!l.drop_on(Library, Slides, Zone::Center), "already a tab there");
    // Out of its own group, beside it.
    assert!(l.drop_on(Library, Library, Zone::Bottom));
    assert_valid(&l);
    assert_eq!(l.windows[0].root.group_of(Slides), Some(vec![Slides]));
}

#[test]
fn closing_and_restoring() {
    let mut l = std_layout();
    assert!(l.close(Library));
    assert!(!l.is_shown(Library) && l.closed.contains(&Library));
    assert_valid(&l);
    l.restore(Library);
    assert!(l.is_shown(Library));
    assert_valid(&l);
    let p = place(&l.windows[0].root, 1440.0, 900.0, false, &no_fit);
    assert!(p.panel(Library).unwrap().rect.y > p.panel(Slides).unwrap().rect.y, "back under the slides");
    // A closed extra comes back under Live.
    l.restore(Cameras);
    assert_valid(&l);
    let p = place(&l.windows[0].root, 1440.0, 900.0, false, &no_fit);
    let (c, live) = (p.panel(Cameras).unwrap().rect, p.panel(Live).unwrap().rect);
    assert!(c.y >= live.bottom() && c.x == live.x, "under Live: {c:?} {live:?}");
    // Closing the last panel of a group of tabs leaves the other one.
    l.drop_on(UpNext, Live, Zone::Center);
    l.close(UpNext);
    assert_eq!(l.windows[0].root.group_of(Live), Some(vec![Live]));
    assert_valid(&l);
}

#[test]
fn the_main_window_keeps_a_panel() {
    let mut l = std_layout();
    for p in PanelId::ALL { l.close(p); }
    assert_eq!(main_panels(&l).len(), 1, "the last one stays");
    assert_valid(&l);
    let last = main_panels(&l)[0];
    assert!(l.pop_out(&[last], None, "").is_none());
}

#[test]
fn pop_out_and_back() {
    let mut l = std_layout();
    let w = l.pop_out(&[Live], Some(WinRect { x: 1920, y: 0, w: 800, h: 600 }), r"\\.\DISPLAY2").unwrap();
    assert_eq!(w, 1);
    assert_eq!(l.window_of(Live), Some(1));
    assert_valid(&l);
    // More panels go into it.
    l.restore(Cameras);
    assert_eq!(l.window_of(Cameras), Some(0), "back in the main window (Live is not there)");
    assert!(l.move_to(Cameras, 1));
    assert_eq!(l.window_of(Cameras), Some(1));
    assert!(l.drop_on(Stage, Cameras, Zone::Bottom));
    assert_eq!(l.window_of(Stage), Some(1));
    assert_valid(&l);
    for arrange in [false, true] { assert_no_overlap(&place(&l.windows[1].root, 800.0, 600.0, arrange, &no_fit)); }
    // Closing the window: every panel back into the main window, none lost.
    l.pop_in(1);
    assert_eq!(l.windows.len(), 1);
    for p in [Live, Cameras, Stage] { assert_eq!(l.window_of(p), Some(0), "{p:?}") }
    assert_valid(&l);
    let p = place(&l.windows[0].root, 1440.0, 900.0, false, &no_fit);
    assert!(p.panel(Live).unwrap().rect.x > p.panel(Slides).unwrap().rect.x, "Live back right of the slides");
}

#[test]
fn moving_the_last_panel_out_of_a_window_removes_it() {
    let mut l = Layout::builtin(Builtin::TwoMonitors);
    l.pop_out(&[Bible], None, "").unwrap();
    assert_eq!(l.windows.len(), 3);
    // Bible (window 2) into window 1; then window 1's panels back home: window 2 is now 1.
    assert!(l.move_to(Bible, 1));
    assert_eq!(l.windows.len(), 2);
    assert_eq!(l.window_of(Bible), Some(1));
    let mut l2 = Layout::builtin(Builtin::TwoMonitors);
    l2.pop_out(&[Bible], None, "").unwrap();
    // Out of window 1 entirely into window 2, which becomes window 1.
    for p in [Live, Cameras, Livestream] { assert!(l2.move_to(p, l2.window_of(Bible).unwrap()), "{p:?}") }
    assert!(l2.move_to(Automation, l2.window_of(Bible).unwrap()));
    assert_eq!(l2.windows.len(), 2);
    assert_valid(&l2);
}

#[test]
fn a_tab_group_pops_out_together() {
    let mut l = Layout::builtin(Builtin::BigSlides);
    let g = l.windows[0].root.group_of(Service).unwrap();
    let w = l.pop_out(&g, None, "").unwrap();
    assert_eq!(l.windows[w].root.group_of(Library), Some(vec![Service, Library]));
    assert_valid(&l);
}

#[test]
fn divider_drag_resizes_within_minimums() {
    let mut l = std_layout();
    let root = &mut l.windows[0].root;
    let p = place(root, 1440.0, 900.0, false, &no_fit);
    // The divider between the work area and Live: dragging left makes Live wider.
    let d = p.dividers.iter().find(|d| d.path.is_empty()).unwrap().clone();
    drag_divider(root, &d, -100.0);
    let p2 = place(root, 1440.0, 900.0, false, &no_fit);
    assert_eq!(p2.panel(Live).unwrap().rect.w, 540.0);
    // Far past the minimum: stops there.
    drag_divider(root, &d, 5000.0);
    let p3 = place(root, 1440.0, 900.0, false, &no_fit);
    assert_eq!(p3.panel(Live).unwrap().rect.w, Live.min_size().0);
    assert_no_overlap(&p3);
    // Two shares: the service list (pixels) beside the slides (a share).
    let d = p3.dividers.iter().find(|d| d.dir == Dir::Row && !d.path.is_empty()).unwrap().clone();
    drag_divider(root, &d, 40.0);
    assert_eq!(place(root, 1440.0, 900.0, false, &no_fit).panel(Service).unwrap().rect.w, 304.0);
}

#[test]
fn divider_between_two_shares_keeps_them_shares() {
    let mut n = row(vec![child(tabs(&[Slides]), Size::Weight(1.0)), child(tabs(&[Stage]), Size::Weight(1.0))]);
    let p = place(&n, 1016.0, 600.0, false, &no_fit);
    assert_eq!(p.panel(Slides).unwrap().rect.w, 496.0);
    let d = p.dividers[0].clone();
    drag_divider(&mut n, &d, 100.0);
    let Node::Split { children, .. } = &n else { panic!() };
    assert!(matches!(children[0].size, Size::Weight(_)) && matches!(children[1].size, Size::Weight(_)));
    let p = place(&n, 1016.0, 600.0, false, &no_fit);
    assert_eq!(p.panel(Slides).unwrap().rect.w, 596.0);
    // The shares follow a bigger window.
    let p = place(&n, 2016.0, 600.0, false, &no_fit);
    assert!((p.panel(Slides).unwrap().rect.w - 1992.0 * 596.0 / 992.0).abs() < 1.5);
}

#[test]
fn fit_follows_content() {
    let root = std_layout().windows[0].root.clone();
    let tall = place(&root, 1440.0, 900.0, false, &|p| if p == Library { (0.0, 300.0) } else { (0.0, 0.0) });
    let short = place(&root, 1440.0, 900.0, false, &|p| if p == Library { (0.0, 44.0) } else { (0.0, 0.0) });
    assert_eq!(tall.panel(Library).unwrap().rect.h, 300.0);
    assert_eq!(short.panel(Library).unwrap().rect.h, 44.0, "a folded library is just its header");
    assert!(short.panel(Slides).unwrap().rect.h > tall.panel(Slides).unwrap().rect.h, "the slides take the room");
}

#[test]
fn drop_targets_by_position() {
    let l = std_layout();
    let p = place(&l.windows[0].root, 1440.0, 900.0, false, &no_fit);
    let live = p.panel(Live).unwrap().rect;
    let at = |fx: f32, fy: f32| p.drop_target(live.x + live.w * fx, live.y + live.h * fy).map(|(t, z, _)| (t, z));
    assert_eq!(at(0.5, 0.5), Some((Live, Zone::Center)));
    assert_eq!(at(0.1, 0.5), Some((Live, Zone::Left)));
    assert_eq!(at(0.9, 0.5), Some((Live, Zone::Right)));
    assert_eq!(at(0.5, 0.05), Some((Live, Zone::Top)));
    assert_eq!(at(0.5, 0.95), Some((Live, Zone::Bottom)));
    let (_, _, lit) = p.drop_target(live.x + 5.0, live.y + live.h / 2.0).unwrap();
    assert_eq!(lit, Rect { w: live.w / 2.0, ..live });
    assert_eq!(p.drop_target(2.0, 2.0), None, "the padding is no panel");
}

#[test]
fn keep_clear_is_the_previews() {
    let mut l = std_layout();
    l.restore(Stage);
    let p = place(&l.windows[0].root, 1440.0, 900.0, false, &no_fit);
    let clear = p.keep_clear();
    assert_eq!(clear, vec![p.panel(Live).unwrap().rect, p.panel(Stage).unwrap().rect]);
}

#[test]
fn saved_prefs_round_trip_and_load_leniently() {
    let mut prefs = DockPrefs::default();
    prefs.current.drop_on(Library, Slides, Zone::Center);
    let mut saved = Layout::builtin(Builtin::TwoMonitors);
    saved.name = "Easter".into();
    saved.windows[1].place = Some(WinRect { x: -1280, y: 10, w: 1280, h: 1000 });
    prefs.saved.push(saved);
    prefs.chosen = Chosen::Saved("Easter".into());
    let json = serde_json::to_string(&prefs).unwrap();
    let back: DockPrefs = serde_json::from_str(&json).unwrap();
    assert_eq!(back, prefs);

    // A newer version's panel and a layout this one cannot read: the panel is left out, a panel this layout does
    // not mention comes back at its Standard place, the unreadable layout is dropped.
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let tabs = &mut v["current"]["windows"][0]["root"]["children"][0]["node"]["children"][0]["node"]["panels"];
    tabs.as_array_mut().unwrap().push("hologram".into());
    v["saved"].as_array_mut().unwrap().push(serde_json::json!({ "windows": [{ "root": { "type": "spiral" } }] }));
    v["current"]["closed"] = serde_json::json!(["up-next", "cameras"]);
    let back: DockPrefs = serde_json::from_value(v).unwrap();
    assert_eq!(back.saved.len(), 1);
    assert_valid(&back.current);
    assert!(!back.current.is_shown(Automation) && back.current.closed.contains(&Automation), "not mentioned: closed, as in Standard");
    assert!(back.current.closed.contains(&Cameras));

    // Nonsense loads as Standard.
    let back: DockPrefs = serde_json::from_str(r#"{"current": 5, "chosen": "zz", "saved": "no"}"#).unwrap_or_default();
    assert_eq!(back.current, std_layout());
    let back: DockPrefs = serde_json::from_str(r#"{"current": {"windows": []}}"#).unwrap();
    assert_eq!(back.current, std_layout());
}

#[test]
fn repair_removes_duplicates_and_empty_parts() {
    let mut l = Layout {
        name: String::new(),
        windows: vec![
            DockWindow { monitor: String::new(), place: None, id: 0, root: row(vec![
                child(tabs(&[Live, Live, Slides]), Size::Weight(f32::NAN)),
                child(tabs(&[]), Size::Px(-3.0)),
                child(column(vec![child(tabs(&[Service]), Size::Fit)]), Size::Px(f32::INFINITY)),
            ]) },
            DockWindow { monitor: String::new(), place: None, id: 0, root: tabs(&[]) },
        ],
        closed: vec![Live, Bible],
    };
    l.repair();
    assert_valid(&l);
    assert_eq!(l.windows.len(), 1, "an empty window of its own goes");
    assert!(l.closed.contains(&Bible) && !l.closed.contains(&Live));
    assert!(l.is_shown(Library), "missing: back at its Standard place");
}

#[test]
fn the_same_arrangement_ignores_names_ids_and_rounding() {
    let mut a = Layout::builtin(Builtin::TwoMonitors);
    a.windows[1].place = Some(WinRect { x: 1920, y: 0, w: 900, h: 700 });
    let mut b = a.clone();
    b.name = "Sunday".into();
    b.windows[1].id = 7;
    b.windows[1].place = Some(WinRect { x: 1924, y: 3, w: 898, h: 700 });
    b.closed.reverse();
    assert!(a.same_arrangement(&b), "names, ids, closed order and a few pixels do not count");
    b.windows[1].place = Some(WinRect { x: 0, y: 0, w: 900, h: 700 });
    assert!(!a.same_arrangement(&b), "a window moved to another monitor counts");
    let mut c = a.clone();
    assert!(c.drop_on(Cameras, Live, Zone::Center));
    assert!(!a.same_arrangement(&c), "a panel moved counts");
    let mut d = a.clone();
    d.restore(Stage);
    assert!(!a.same_arrangement(&d), "a panel shown again counts");
}

#[test]
fn a_panel_dropped_in_another_window_leaves_an_empty_window_to_close() {
    let mut l = std_layout();
    l.pop_out(&[Live], None, "").unwrap();
    // The library dragged from the main window onto Live's right half in the window of its own.
    assert!(l.drop_on(Library, Live, Zone::Right));
    assert_eq!(l.window_of(Library), Some(1));
    // And Live dragged back onto the slides: the window of its own keeps the library.
    assert!(l.drop_on(Live, Slides, Zone::Bottom));
    assert_eq!(l.windows.len(), 2);
    // The library too: nothing is left there, so that window goes.
    assert!(l.drop_on(Library, Slides, Zone::Center));
    assert_eq!(l.windows.len(), 1, "a window of its own with no panels closes");
    assert_valid(&l);
}
