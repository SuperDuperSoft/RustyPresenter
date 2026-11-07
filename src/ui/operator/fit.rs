//! The operator fits its window from 1280 × 720 up: the service and live columns and the whole top bar stay inside
//! it (the slides column gives way), and every dialog is centred in it and inside it.
use crate::{
    app::{LiveUpdate, test_app},
    model::{ItemKind, ServiceItem},
    ui::{
        dialogs::{AddTab, SettingsTab, add, item_edit, phone, screens, services, settings, song_editor},
        operator::window::{Operator, testing::{find, open, refresh}},
    },
};
use i_slint_backend_testing::ElementHandle;
use std::rc::Rc;
use uuid::Uuid;

/// The window sizes checked: the smallest supported, the laptop budget (FEAT-1 §7.2), the window the installer smoke
/// test found cut off, 1080p and 4K. The testing backend draws the menu bar inside these heights.
const SIZES: [(f32, f32); 5] = [(1280.0, 720.0), (1366.0, 768.0), (1454.0, 938.0), (1920.0, 1080.0), (3840.0, 2160.0)];

/// Resizes the window and runs what a frame would (the `narrow` change handler, the overlays' `init`).
fn resize(op: &Operator, (w, h): (f32, f32)) {
    op.window().set_size(slint::LogicalSize::new(w, h));
    slint::platform::update_timers_and_animations();
    let _ = find(op, "DialogShell::close-btn");
    slint::platform::update_timers_and_animations();
}

/// `id`'s box: left, top, right, bottom.
fn bounds(op: &Operator, id: &str) -> (f32, f32, f32, f32) {
    let e: ElementHandle = find(op, id).unwrap_or_else(|| panic!("{id} is shown"));
    let (p, s) = (e.absolute_position(), e.size());
    (p.x, p.y, p.x + s.width, p.y + s.height)
}

/// Asserts `id` lies wholly inside the window (half a pixel of rounding allowed).
fn assert_inside(op: &Operator, id: &str, (w, h): (f32, f32)) {
    let (l, t, r, b) = bounds(op, id);
    assert!(l >= -0.5 && t >= -0.5 && r <= w + 0.5 && b <= h + 0.5, "{id} at {l},{t}–{r},{b} is outside the {w}×{h} window");
}

fn with_text_item(op: &Rc<Operator>) -> Uuid {
    let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Good morning".into() }, background: None, notes: String::new(), theme: None };
    op.app().data.lock().unwrap().services[0].items.push(item.clone());
    refresh(op);
    item.id
}

#[test]
fn columns_and_top_bar_fit_the_window() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    op.apply_live(LiveUpdate::new(op.app().live_view()));
    for size in SIZES {
        resize(&op, size);
        for id in [
            "OperatorWindow::service-col", "OperatorWindow::slides-col", "OperatorWindow::live-col",
            "TopBar::service-pick", "TopBar::form", "TopBar::status-room", "TopBar::screens-btn", "TopBar::phone-btn",
            "TopBar::settings-btn", "LiveColumn::next-btn", "LiveColumn::clear-logo",
        ] {
            assert_inside(&op, id, size);
        }
        let width = |id: &str| { let (l, _, r, _) = bounds(&op, id); r - l };
        assert!(width("OperatorWindow::service-col") >= 250.0, "the service column keeps its width at {size:?}");
        assert!(width("OperatorWindow::live-col") >= 360.0, "the live column keeps its width at {size:?}");
        // The top bar's parts do not overlap: each starts after the one before it ends.
        let order = ["TopBar::service-pick", "TopBar::form", "TopBar::status-room", "TopBar::screens-btn", "TopBar::phone-btn", "TopBar::settings-btn"];
        for pair in order.windows(2) {
            assert!(bounds(&op, pair[0]).2 <= bounds(&op, pair[1]).0 + 0.5, "{} overlaps {} at {size:?}", pair[0], pair[1]);
        }
    }
}

#[test]
fn every_dialog_is_centred_and_inside_the_window() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    let item = with_text_item(&op);
    let dialogs: [(&str, Box<dyn Fn(&Rc<Operator>)>); 18] = [
        ("Add song", Box::new(|op| add::open_add(op, AddTab::Song))),
        ("Add scripture", Box::new(|op| add::open_add(op, AddTab::Scripture))),
        ("Add media", Box::new(|op| add::open_add(op, AddTab::Media))),
        ("Add deck", Box::new(|op| add::open_add(op, AddTab::Deck))),
        ("Add text", Box::new(|op| add::open_add(op, AddTab::Text))),
        ("Song editor", Box::new(|op| song_editor::open_song_editor(op, None, false))),
        ("Item edit", Box::new(move |op| item_edit::open(op, item))),
        ("Services", Box::new(services::open)),
        ("Screens", Box::new(screens::open)),
        ("Phone", Box::new(phone::open)),
        ("Settings look", Box::new(|op| settings::open(op, SettingsTab::Look))),
        ("Settings themes", Box::new(|op| settings::open(op, SettingsTab::Themes))),
        ("Settings language", Box::new(|op| settings::open(op, SettingsTab::Language))),
        ("Settings songs", Box::new(|op| settings::open(op, SettingsTab::Songs))),
        ("Settings Bible", Box::new(|op| settings::open(op, SettingsTab::Bible))),
        ("Settings video", Box::new(|op| settings::open(op, SettingsTab::Video))),
        ("Settings backups", Box::new(|op| settings::open(op, SettingsTab::Backups))),
        ("Settings help", Box::new(|op| settings::open(op, SettingsTab::Help))),
        ("Settings about", Box::new(|op| settings::open(op, SettingsTab::About))),
    ];
    for (name, show) in &dialogs {
        for size in SIZES {
            show(&op);
            resize(&op, size);
            assert!(op.dialog().is_some(), "{name} opened");
            assert_inside(&op, "DialogShell::card", size);
            let (l, t, r, b) = bounds(&op, "DialogShell::card");
            // Centred in the area under the menu bar (the testing backend draws the bar inside the window).
            let (_, top, _, _) = bounds(&op, "OperatorWindow::keys");
            assert!((l - (size.0 - r)).abs() <= 1.0 && (t - top - (size.1 - b)).abs() <= 1.0, "{name} is centred at {size:?}");
            // The head's X is inside the card, so nothing in the card is wider than the card.
            let (_, _, close_r, close_b) = bounds(&op, "DialogShell::close-btn");
            assert!(close_r <= r + 0.5 && close_b <= b + 0.5, "{name}'s close button is inside its card at {size:?}");
            op.close_dialog();
        }
    }
}

#[test]
fn bible_chips_collapse_to_the_chosen_one_in_a_narrow_window() {
    use crate::ui::operator::window::testing::find_all;
    use slint::platform::PointerEventButton;
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    resize(&op, (1454.0, 938.0));
    assert!(find(&op, "TopBar::translation-pick").is_some(), "the chosen Bible's button is always there");
    assert_eq!(find_all(&op, "TopBar::translation-chip").len(), 2, "then the favourite and recent chips (here WEB, BSB)");
    resize(&op, (1280.0, 720.0));
    assert!(find(&op, "TopBar::translation-chip").is_none());
    let pick = find(&op, "TopBar::translation-pick").expect("the chosen translation's button");
    assert_eq!(pick.accessible_label().unwrap_or_default(), "KJV");
    assert!(pick.size().width <= 80.0, "the button keeps its own width, not the Bible box's: {}", pick.size().width);
    pick.mock_single_click(PointerEventButton::Left);
    slint::platform::update_timers_and_animations();
    let rows = find_all(&op, "BiblePickerList::picker-row");
    let snap = op.state.borrow().snap.clone().unwrap();
    assert_eq!(rows.len(), snap.translations.len(), "every Bible, once");
    let web = rows.iter().find(|r| r.accessible_label().unwrap_or_default() == "WEB").expect("WEB in the list");
    web.mock_single_click(PointerEventButton::Left);
    assert_eq!(app.data.lock().unwrap().settings.translation, "WEB");
}
/// The window's own limits, as the winit backend reads them for the window's minimum and maximum size
/// (`WindowProperties::layout_constraints`): horizontal, then vertical.
fn limits(op: &Operator) -> (i_slint_core::layout::LayoutInfo, i_slint_core::layout::LayoutInfo) {
    use i_slint_core::{item_tree::ItemTreeRc, layout::Orientation, window::WindowInner};
    let c = WindowInner::from_pub(op.window()).component();
    let c = ItemTreeRc::borrow_pin(&c);
    (c.as_ref().layout_info(Orientation::Horizontal), c.as_ref().layout_info(Orientation::Vertical))
}

/// Asserts the window can shrink to 1280 × 720 and grow to 3840 × 2160 at each of `SIZES` and 1920 × 1080, and
/// that at its own smallest size (1024 × 640, 1280 × 800 at 125 %) its minimum does not follow its height: Windows
/// grows a window to its minimum, so a minimum one pixel taller than the window grows it without end.
fn assert_free_limits(op: &Operator, state: &str) {
    for size in [(1920.0, 1080.0), SIZES[2], SIZES[0]] {
        resize(op, size);
        let (h, v) = limits(op);
        assert!(h.min <= 1280.0 && v.min <= 720.0, "{state}: the window can shrink to 1280×720 from {size:?}: its minimum is {}×{}", h.min, v.min);
        assert!(h.max >= 3840.0 && v.max >= 2160.0, "{state}: the window can grow from {size:?}: its maximum is {}×{}", h.max, v.max);
    }
    // Set below its minimum at 125 % (1280 × 720 pixels, 1024 × 576), the window is grown to its minimum rounded up
    // to whole pixels, as the winit backend does (`adjust_window_size_to_satisfy_constraints`), until it settles.
    let scale = 1.25;
    op.window().dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: scale });
    let mut height: f32 = 576.0;
    let mut seen = vec![];
    for _ in 0..6 {
        resize(op, (1024.0, height));
        let min = limits(op).1.min;
        seen.push((height, min));
        if (height * scale).round() < min * scale { height = (min * scale).ceil() / scale }
    }
    op.window().dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
    assert!(height <= 720.0 && seen[4] == seen[5], "{state}: from 1024×576 at 125 % the window (height, minimum) went {seen:?}");
}

#[test]
fn window_limits_never_follow_its_size() {
    // A part filling its parent by `width: parent.width` with no `x`/`y` counts as fixed at that size, so the
    // parent's minimum (and the window's) became the window's size: it could never shrink, and its height grew
    // by a pixel a turn until Windows stopped it at 65535. Every state of the slides column is checked: each
    // shows a different overlay (the Welcome, "Choose something", "Nothing to show", the grid).
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    op.apply_live(LiveUpdate::new(op.app().live_view()));
    assert!(find(&op, "SlidesPanel::welcome").is_some());
    assert_free_limits(&op, "an empty service (Welcome)");
    let item = with_text_item(&op);
    op.ui.invoke_service_select(item.to_string().into());
    assert_free_limits(&op, "a text item selected");
    op.state.borrow_mut().selected = None;
    op.render_slides();
    assert!(find(&op, "SlidesPanel::choose").is_some());
    assert_free_limits(&op, "nothing selected");
    // A song that is not in the library has no slides.
    let empty = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: Uuid::new_v4() }, background: None, notes: String::new(), theme: None };
    op.app().data.lock().unwrap().services[0].items.push(empty.clone());
    refresh(&op);
    op.ui.invoke_service_select(empty.id.to_string().into());
    slint::platform::update_timers_and_animations();
    assert!(find(&op, "SlidesPanel::nothing").is_some());
    assert_free_limits(&op, "an item with no slides");
}