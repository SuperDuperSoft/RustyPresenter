//! The operator fits its window from 1280 × 720 up: the service and live columns and the whole top bar stay inside
//! it (the slides column gives way), and every dialog starts at the same height, is centred across and fits inside it.
use crate::{
    app::{LiveUpdate, test_app},
    model::{ItemKind, ServiceItem},
    ui::{
        dialogs::{AddTab, add, ask, automation, find_verse, item_edit, overlays, phone, print, screens, services, show_message, slide_breaks, song_editor, stage_layouts, usage},
        operator::window::{Operator, testing::{find, open, refresh}},
    },
};
use i_slint_backend_testing::ElementHandle;
use slint::ComponentHandle;
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
    let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Good morning".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None };
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
    // The busiest live column: a phone drawing (its chip) and the performance line under the preview.
    op.ui.set_drawing_on(true);
    op.ui.global::<crate::ui::OutputsState>().set_perf("Room 60 frames a second · Stage 60 frames a second · 1.2 GB video memory free".into());
    for size in SIZES {
        resize(&op, size);
        for id in [
            "OperatorWindow::service-col", "OperatorWindow::slides-col", "OperatorWindow::live-col",
            "TopBar::service-pick", "TopBar::form", "TopBar::status-room", "TopBar::screens-btn", "TopBar::phone-btn",
            "TopBar::settings-btn", "LiveColumn::preview-view", "LiveColumn::live-what", "LiveColumn::drawing-chip",
            "LiveColumn::next-btn", "LiveColumn::clear-all", "LiveColumn::clear-logo",
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

/// Every dialog's card starts this far below the top of the dialog layer, whatever is in it.
const DIALOG_TOP: f32 = 40.0;

/// Asserts the open dialog's card starts at `DIALOG_TOP`, is centred across, and that its footer buttons and its X
/// are inside both the card and the window.
fn assert_dialog_fits(op: &Operator, name: &str, size: (f32, f32)) {
    assert_inside(op, "DialogShell::card", size);
    let (l, t, r, b) = bounds(op, "DialogShell::card");
    let (_, top, _, _) = bounds(op, "OperatorWindow::keys");
    assert!((t - top - DIALOG_TOP).abs() <= 0.5, "{name} starts {} below the top at {size:?}, not {DIALOG_TOP}", t - top);
    assert!((l - (size.0 - r)).abs() <= 1.0, "{name} is centred across at {size:?}");
    assert!(b <= size.1 - 8.0, "{name} ends at {b}, past the bottom of the {size:?} window");
    let (_, _, close_r, close_b) = bounds(op, "DialogShell::close-btn");
    assert!(close_r <= r + 0.5 && close_b <= b + 0.5, "{name}'s close button is inside its card at {size:?}");
    // The footer: every button under its line is inside the card.
    if let Some(line) = find(op, "DialogFoot::foot-line") {
        let line_y = line.absolute_position().y;
        assert!(line_y >= t, "{name}'s footer line is inside its card at {size:?}");
        let card = find(op, "DialogShell::card").expect("the card");
        let buttons: Vec<ElementHandle> = card.query_descendants().match_descendants().match_type_name("Btn").find_all()
            .into_iter().filter(|e| e.absolute_position().y > line_y).collect();
        // A footer may have no buttons yet (Automation before a row is picked); its own padding is still inside.
        assert!(line_y + 31.0 <= b + 0.5, "{name}'s footer at {line_y} is cut off by its card's bottom {b} at {size:?}");
        for btn in buttons {
            let (p, s) = (btn.absolute_position(), btn.size());
            assert!(p.y + s.height <= b + 0.5, "{name}'s footer button {:?} ends at {} past its card's bottom {b} at {size:?}",
                btn.accessible_label(), p.y + s.height);
        }
    }
}

/// The open dialog's card: left, top, width, height.
fn card(op: &Operator) -> (f32, f32, f32, f32) {
    let (l, t, r, b) = bounds(op, "DialogShell::card");
    (l, t, r - l, b - t)
}

#[test]
fn every_dialog_starts_at_the_same_top_and_fits_the_window() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    let item = with_text_item(&op);
    let service = op.app().data.lock().unwrap().services[0].id;
    let dialogs: [(&str, Box<dyn Fn(&Rc<Operator>)>); 21] = [
        ("Add song", Box::new(|op| add::open_add(op, AddTab::Song))),
        ("Add scripture", Box::new(|op| add::open_add(op, AddTab::Scripture))),
        ("Add media", Box::new(|op| add::open_add(op, AddTab::Media))),
        ("Add deck", Box::new(|op| add::open_add(op, AddTab::Deck))),
        ("Add text", Box::new(|op| add::open_add(op, AddTab::Text))),
        ("Song editor", Box::new(|op| song_editor::open_song_editor(op, None, false))),
        ("Item edit", Box::new(move |op| item_edit::open(op, item))),
        ("Services", Box::new(services::open)),
        ("Print", Box::new(move |op| print::open(op, service))),
        ("Screens", Box::new(screens::open)),
        ("Phone", Box::new(phone::open)),
        ("Usage", Box::new(usage::open)),
        ("Stage layouts", Box::new(stage_layouts::open)),
        ("Overlays", Box::new(overlays::open)),
        ("Automation", Box::new(automation::open)),
        ("Walk-in music", Box::new(crate::ui::dialogs::music::open)),
        ("Show message", Box::new(show_message::open)),
        ("Find verse", Box::new(|op| find_verse::open(op, ""))),
        ("Slide breaks", Box::new(move |op| slide_breaks::open(op, item))),
        ("Quick search", Box::new(crate::ui::dialogs::quick_search::open)),
        ("Translations", Box::new(|op| crate::ui::dialogs::translation_editor::open(op, None))),
    ];
    for (name, show) in &dialogs {
        for size in SIZES {
            show(&op);
            resize(&op, size);
            assert!(op.dialog().is_some(), "{name} opened");
            assert_dialog_fits(&op, name, size);
            op.close_dialog();
        }
    }
}

#[test]
fn questions_start_at_the_dialogs_top_over_a_dialog_or_the_bare_window() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    for size in [(1366.0, 768.0), (1809.0, 1000.0)] {
        resize(&op, size);
        for over_dialog in [false, true] {
            if over_dialog { services::open(&op) }
            ask::confirm(&op, "Delete this?", "Delete", "Cancel", true, |_| {});
            resize(&op, size);
            let (_, top, _, _) = bounds(&op, "OperatorWindow::keys");
            let (l, t, r, b) = bounds(&op, "AskOverlay::card");
            assert!((t - top - DIALOG_TOP).abs() <= 0.5, "the question starts {} below the top at {size:?}", t - top);
            assert!((l - (size.0 - r)).abs() <= 1.0 && b <= size.1, "the question is centred across and inside at {size:?}");
            ask::answer(&op, false, String::new());
            op.close_dialog();
        }
    }
}

/// Jake's rule: switching tabs never moves or resizes a dialog, and its footer stays in the window.
#[test]
fn tabbed_dialogs_keep_their_place_and_size_on_every_tab() {
    use crate::ui::AddState;
    use slint::ComponentHandle;
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    for size in [(1366.0, 768.0), (1809.0, 1000.0)] {
        resize(&op, size);
        // Automation: its five tabs.
        automation::open(&op);
        resize(&op, size);
        let first = card(&op);
        for tab in [1, 2, 3, 4, 0] {
            automation::goto(&op, tab, -1);
            resize(&op, size);
            let name = format!("Automation tab {tab}");
            assert_eq!(card(&op), first, "{name} keeps the dialog's place and size at {size:?}");
            assert_dialog_fits(&op, &name, size);
        }
        op.close_dialog();
        // Add: its five tiles.
        add::open_add(&op, AddTab::Song);
        resize(&op, size);
        let first = card(&op);
        for tab in [1, 2, 3, 4, 0] {
            op.ui.global::<AddState>().invoke_pick_tab(tab);
            resize(&op, size);
            let name = format!("Add tab {tab}");
            assert_eq!(card(&op), first, "{name} keeps the dialog's place and size at {size:?}");
            assert_dialog_fits(&op, &name, size);
        }
        op.close_dialog();
        // Overlays: the list and the editor.
        overlays::open(&op);
        resize(&op, size);
        let first = card(&op);
        op.ui.global::<crate::ui::OverlaysDlg>().set_editing(true);
        resize(&op, size);
        assert_eq!(card(&op), first, "the overlay editor keeps the dialog's place and size at {size:?}");
        assert_dialog_fits(&op, "Overlay editor", size);
        op.close_dialog();
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
    let empty = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: Uuid::new_v4(), arrangement: None }, background: None, notes: String::new(), theme: None };
    op.app().data.lock().unwrap().services[0].items.push(empty.clone());
    refresh(&op);
    op.ui.invoke_service_select(empty.id.to_string().into());
    slint::platform::update_timers_and_animations();
    assert!(find(&op, "SlidesPanel::nothing").is_some());
    assert_free_limits(&op, "an item with no slides");
}
