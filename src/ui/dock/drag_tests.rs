use super::*;

/// The main window on the left monitor at 100 %, under a 60 px menu and top bar; a window of its own on the right
/// monitor at 150 %.
fn two() -> Vec<OnScreen> {
    vec![
        OnScreen { win: 1, inner: WinRect { x: 2000, y: 100, w: 900, h: 700 }, scale: 1.5, origin: (0.0, 0.0) },
        OnScreen { win: 0, inner: WinRect { x: 0, y: 0, w: 1440, h: 900 }, scale: 1.0, origin: (0.0, 60.0) },
    ]
}

#[test]
fn a_point_in_a_window_is_in_its_dock_area() {
    assert_eq!(under(&two(), 100.0, 160.0, None), Under::Window { win: 0, x: 100.0, y: 100.0 });
    // 150 %: 300 physical pixels in are 200 logical ones.
    assert_eq!(under(&two(), 2300.0, 400.0, None), Under::Window { win: 1, x: 200.0, y: 200.0 });
    // Above the main window's dock area (its top bar): still that window, above its panels.
    assert_eq!(under(&two(), 100.0, 20.0, None), Under::Window { win: 0, x: 100.0, y: -40.0 });
}

#[test]
fn the_desktop_is_outside() {
    assert_eq!(under(&two(), 1700.0, 300.0, None), Under::Outside, "between the windows");
    assert_eq!(under(&two(), -50.0, 300.0, None), Under::Outside, "left of everything");
    assert_eq!(under(&two(), 2000.0 + 900.0, 100.0, None), Under::Outside, "the right edge is outside");
}

#[test]
fn the_front_window_wins_where_windows_overlap() {
    let mut wins = two();
    // The window of its own dragged over the main window.
    wins[0].inner = WinRect { x: 1000, y: 200, w: 900, h: 700 };
    wins[0].scale = 1.0;
    assert_eq!(under(&wins, 1100.0, 300.0, None), Under::Window { win: 1, x: 100.0, y: 100.0 }, "the first is in front");
    wins.reverse();
    assert_eq!(under(&wins, 1100.0, 300.0, None), Under::Window { win: 0, x: 1100.0, y: 240.0 });
}

#[test]
fn what_the_system_says_is_in_front_decides() {
    // Windows says the main window is in front there (it was clicked last), whatever the order.
    let mut wins = two();
    wins[0].inner = WinRect { x: 1000, y: 200, w: 900, h: 700 };
    wins[0].scale = 1.0;
    assert_eq!(under(&wins, 1100.0, 300.0, Some(Some(0))), Under::Window { win: 0, x: 1100.0, y: 240.0 });
    // Another program covers our window there: let go, and the panel opens in a window of its own.
    assert_eq!(under(&wins, 1100.0, 300.0, Some(None)), Under::Outside);
}

#[test]
fn screen_and_area_points_go_both_ways() {
    for w in two() {
        let (sx, sy) = w.to_screen(123.0, 45.0);
        let (ax, ay) = w.to_area(sx, sy);
        assert!((ax - 123.0).abs() < 0.01 && (ay - 45.0).abs() < 0.01, "{w:?}");
    }
}

#[test]
fn windows_front_to_back() {
    assert_eq!(front_to_back(&[0, 1, 2], &[]), [2, 1, 0], "windows of their own open over the main window");
    assert_eq!(front_to_back(&[0, 1, 2], &[0, 1]), [0, 1, 2], "the one clicked last first");
    assert_eq!(front_to_back(&[0, 2], &[1, 0]), [0, 2], "a closed window is forgotten");
}

fn monitors() -> Vec<Monitor> {
    vec![
        Monitor { name: r"\\.\DISPLAY1".into(), rect: WinRect { x: 0, y: 0, w: 1920, h: 1080 } },
        Monitor { name: r"\\.\DISPLAY2".into(), rect: WinRect { x: 1920, y: 0, w: 1280, h: 720 } },
    ]
}

#[test]
fn a_torn_off_panel_opens_at_the_pointer_as_large_as_it_was() {
    let (name, r) = tear_off_place(600, 400, (440.0, 500.0), 1.0, &monitors());
    assert_eq!(name, r"\\.\DISPLAY1");
    // The panel, its title bar and its edges.
    assert_eq!((r.w, r.h), (456, 546));
    // The pointer on its grip.
    assert_eq!((r.x, r.y), (600 - 28, 400 - 23));
}

#[test]
fn a_torn_off_panel_opens_on_the_monitor_under_the_pointer() {
    let (name, r) = tear_off_place(3100, 700, (440.0, 500.0), 1.0, &monitors());
    assert_eq!(name, r"\\.\DISPLAY2");
    // Kept wholly on that monitor: pushed left and up from the bottom right corner.
    assert_eq!((r.x + r.w as i32, r.y + r.h as i32), (3200, 720));
    assert!(r.x >= 1920);
}

#[test]
fn a_torn_off_panel_is_never_larger_than_its_monitor() {
    let (_, r) = tear_off_place(2000, 10, (3000.0, 2000.0), 1.0, &monitors());
    assert_eq!(r, WinRect { x: 1920, y: 0, w: 1280, h: 720 });
}

#[test]
fn a_small_panel_gets_a_usable_window() {
    let (_, r) = tear_off_place(600, 400, (200.0, 44.0), 1.5, &monitors());
    assert_eq!((r.w, r.h), (480, 360), "the smallest window of its own, at 150 %");
}

#[test]
fn off_every_monitor_the_nearest_takes_it() {
    // Between monitors of different heights (below the shorter one).
    let (name, r) = tear_off_place(2500, 900, (440.0, 300.0), 1.0, &monitors());
    assert_eq!(name, r"\\.\DISPLAY2");
    assert!(r.y + r.h as i32 <= 720);
    // No monitors known (the testing backend): at the pointer.
    let (name, r) = tear_off_place(50, 60, (440.0, 300.0), 1.0, &[]);
    assert_eq!((name.as_str(), r.x, r.y), ("", 22, 37));
}
