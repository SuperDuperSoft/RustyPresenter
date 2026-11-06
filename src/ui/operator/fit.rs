//! The operator fits its window from 1280 × 720 up: the service and live columns and the whole top bar stay inside
//! it (the slides column gives way), every dialog, question, toast and job card is inside it, and what a dialog
//! holds is inside its card.
use crate::{
    app::{App, Job, LiveUpdate, test_app},
    model::{ItemKind, ServiceItem},
    store::Store,
    ui::{
        TranslationChip,
        dialogs::{AddTab, SettingsTab, add, ask, item_edit, phone, screens, services, settings, song_editor},
        operator::window::{Operator, testing::{find, find_all, open, refresh}},
    },
};
use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use slint::{Model, ModelRc, VecModel};
use std::rc::Rc;
use uuid::Uuid;

/// The window sizes checked: the smallest supported, the narrowest with every translation button, the window the
/// installer smoke test found cut off, 1080p and 4K.
const SIZES: [(f32, f32); 5] = [(1280.0, 720.0), (1400.0, 900.0), (1454.0, 938.0), (1920.0, 1080.0), (3840.0, 2160.0)];
const SMALLEST: (f32, f32) = SIZES[0];

/// Resizes the window and runs what a frame would (the `narrow` change handler, the overlays' `init`).
fn resize(op: &Operator, (w, h): (f32, f32)) {
    op.window().set_size(slint::LogicalSize::new(w, h));
    slint::platform::update_timers_and_animations();
    let _ = find(op, "DialogShell::close-btn");
    let _ = find(op, "AskOverlay::ok");
    slint::platform::update_timers_and_animations();
}

/// An element's box: left, top, right, bottom.
type Box4 = (f32, f32, f32, f32);

fn box_of(e: &ElementHandle) -> Box4 {
    let (p, s) = (e.absolute_position(), e.size());
    (p.x, p.y, p.x + s.width, p.y + s.height)
}

/// `id`'s box.
fn bounds(op: &Operator, id: &str) -> Box4 {
    box_of(&find(op, id).unwrap_or_else(|| panic!("{id} is shown")))
}

/// Whether `inner` lies wholly inside `outer` (half a pixel of rounding allowed).
fn within(inner: Box4, outer: Box4) -> bool {
    inner.0 >= outer.0 - 0.5 && inner.1 >= outer.1 - 0.5 && inner.2 <= outer.2 + 0.5 && inner.3 <= outer.3 + 0.5
}

/// Asserts `id` lies wholly inside the window.
fn assert_inside(op: &Operator, id: &str, (w, h): (f32, f32)) {
    let b = bounds(op, id);
    assert!(within(b, (0.0, 0.0, w, h)), "{id} at {b:?} is outside the {w}×{h} window");
}

/// Asserts every element with each of `ids`, in this order, starts after the one before it ends: left to right, or
/// top to bottom. An id with no element (a hidden pill) is skipped.
fn assert_in_order(op: &Operator, ids: &[&str], down: bool, at: &str) {
    let all: Vec<(&str, Box4)> = ids.iter().flat_map(|&id| find_all(op, id).into_iter().map(move |e| (id, box_of(&e)))).collect();
    for pair in all.windows(2) {
        let ((a, ab), (b, bb)) = (pair[0], pair[1]);
        let (end, start) = if down { (ab.3, bb.1) } else { (ab.2, bb.0) };
        assert!(end <= start + 0.5, "{a} ({ab:?}) overlaps {b} ({bb:?}) {at}");
    }
}

/// The parts of `e` that can be read or used (those with an accessible role), leaving out any of no width.
fn parts(e: &ElementHandle) -> Vec<ElementHandle> {
    e.query_descendants()
        .match_predicate(|e| e.accessible_role().is_some_and(|r| r != AccessibleRole::None) && e.size().width > 0.0)
        .find_all()
}

fn with_text_item(op: &Rc<Operator>) -> Uuid {
    let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Good morning".into() }, background: None, notes: String::new() };
    op.app().data.lock().unwrap().services[0].items.push(item.clone());
    refresh(op);
    item.id
}

/// The columns and the top bar at every size: inside the window, the columns at their widths, and the top bar's
/// parts one after another (in the Bible box: the input, the translation buttons, the Ctrl B hint).
fn assert_columns_fit(op: &Operator, state: &str) {
    for size in SIZES {
        resize(op, size);
        let at = format!("at {size:?} {state}");
        for id in [
            "OperatorWindow::service-col", "OperatorWindow::slides-col", "OperatorWindow::live-col",
            "TopBar::service-pick", "TopBar::form", "TopBar::status-room", "TopBar::screens-btn", "TopBar::phone-btn",
            "TopBar::settings-btn", "LiveColumn::next-btn", "LiveColumn::clear-logo",
        ] {
            assert_inside(op, id, size);
        }
        let width = |id: &str| { let (l, _, r, _) = bounds(op, id); r - l };
        assert!(width("OperatorWindow::service-col") >= 250.0, "the service column keeps its width {at}");
        assert!(width("OperatorWindow::live-col") >= 360.0, "the live column keeps its width {at}");
        assert!(width("TopBar::bible-input") >= 120.0, "the Bible box keeps room to type {at}");
        assert_in_order(op, &[
            "TopBar::service-pick", "TopBar::bible-input", "TopBar::translation-pick", "TopBar::translation-chip",
            "TopBar::ctrl-b", "TopBar::status-room", "TopBar::status-stage", "TopBar::status-phones",
            "TopBar::screens-btn", "TopBar::phone-btn", "TopBar::settings-btn",
        ], false, &at);
        let form = bounds(op, "TopBar::form");
        assert!(within(bounds(op, "TopBar::bible-input"), form) && within(bounds(op, "TopBar::ctrl-b"), form), "the Bible box holds its parts {at}");
    }
}

#[test]
fn columns_and_top_bar_fit_the_window() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    op.apply_live(LiveUpdate::new(op.app().live_view()));
    assert_columns_fit(&op, "with the usual top bar");
    // The busiest top bar: the Stage and phones pills, and three imported translations beside the eight built in.
    op.ui.set_stage_shown(true);
    op.ui.set_phones_label("2 phones".into());
    let mut chips: Vec<TranslationChip> = op.ui.get_translations().iter().collect();
    assert_eq!(chips.len(), 8);
    chips.extend(["NKJV", "CSB", "NLT"].map(|name| TranslationChip { name: name.into(), on: false, off: false }));
    op.ui.set_translations(ModelRc::new(VecModel::from(chips)));
    assert_columns_fit(&op, "with every pill and 11 translations");
    assert!(find(&op, "TopBar::status-stage").is_some() && find(&op, "TopBar::status-phones").is_some());
}

/// Asserts that what the open dialog holds fits its card. The card is sized from the window, not from what it holds,
/// so a part wider than the card, or a foot pushed below it, would be cut off by the card's clip. Every part that
/// can be read or used is inside the card from side to side (the body scrolls only up and down); the scrolling body
/// and the foot with its buttons are inside it top to bottom too.
fn assert_dialog_content_fits(op: &Operator, at: &str) {
    let card = find(op, "DialogShell::card").expect("a dialog is open");
    let c = box_of(&card);
    let everything = parts(&card);
    assert!(everything.len() > 2, "{at}: the dialog's parts are found");
    for part in &everything {
        let b = box_of(part);
        assert!(b.0 >= c.0 - 0.5 && b.2 <= c.2 + 0.5, "{at}: {:?} ({b:?}) is wider than its card {c:?}", part.accessible_label().unwrap_or_default());
    }
    let bodies = card.query_descendants().match_inherits("DialogBody").find_all();
    assert!(!bodies.is_empty(), "{at}: the dialog has a body");
    for body in &bodies {
        assert!(within(box_of(body), c), "{at}: the body {:?} is outside its card {c:?}", box_of(body));
    }
    for foot in card.query_descendants().match_inherits("DialogFoot").find_all() {
        for part in std::iter::once(foot.clone()).chain(parts(&foot)) {
            let b = box_of(&part);
            assert!(within(b, c), "{at}: the foot's {:?} ({b:?}) is outside its card {c:?}", part.accessible_label().unwrap_or_default());
        }
    }
    assert!(within(bounds(op, "DialogShell::close-btn"), c), "{at}: the close button is inside its card");
}

#[test]
fn every_dialog_is_centred_and_inside_the_window() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    let item = with_text_item(&op);
    let dialogs: [(&str, Box<dyn Fn(&Rc<Operator>)>); 17] = [
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
            assert!((l - (size.0 - r)).abs() <= 1.0 && (t - (size.1 - b)).abs() <= 1.0, "{name} is centred at {size:?}");
            assert_dialog_content_fits(&op, &format!("{name} at {size:?}"));
            op.close_dialog();
        }
    }
}

#[test]
fn paired_phone_dialog_fits() {
    // With a network address, so the QR code is drawn.
    let root = std::env::temp_dir().join(format!("rp-test-{}", Uuid::new_v4()));
    let store = Store::open(root).unwrap();
    let data = store.load().unwrap();
    let app = App::new(store, data, 0, Some("http://192.168.1.20:7878".into()));
    let op = open(&app);
    refresh(&op);
    phone::open(&op);
    resize(&op, SMALLEST);
    find(&op, "PhoneDialog::pair-btn").expect("Pair a phone").mock_single_click(slint::platform::PointerEventButton::Left);
    resize(&op, SMALLEST);
    assert!(find(&op, "PhoneDialog::pin").is_some(), "the code shows");
    assert_dialog_content_fits(&op, "the paired Phone dialog");
    // The code and its steps sit side by side inside the card; the body scrolls them into view.
    let card = bounds(&op, "DialogShell::card");
    for id in ["PhoneDialog::qr", "PhoneDialog::pin", "PhoneDialog::countdown"] {
        let b = bounds(&op, id);
        assert!(b.0 >= card.0 - 0.5 && b.2 <= card.2 + 0.5, "{id} ({b:?}) is inside the card {card:?}");
    }
}

#[test]
fn questions_fit_the_smallest_window() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    let long = "This service has items that are not saved yet, and some of their pictures and videos are missing. ".repeat(30);
    for (third, text) in [(false, "Delete this song?".to_string()), (true, long.clone()), (false, long)] {
        if third {
            ask::choose(&op, &text, "Replace", "Keep both", |_| {});
        } else {
            ask::confirm(&op, &text, "Delete", "Cancel", true, |_| {});
        }
        resize(&op, SMALLEST);
        let at = format!("{} characters{}", text.len(), if third { ", three buttons" } else { "" });
        assert_inside(&op, "AskOverlay::card", SMALLEST);
        let card = bounds(&op, "AskOverlay::card");
        for id in ["AskOverlay::cancel", "AskOverlay::other", "AskOverlay::ok"] {
            if let Some(e) = find(&op, id) {
                assert!(within(box_of(&e), card), "{id} ({:?}) is inside the question {card:?} ({at})", box_of(&e));
            }
        }
        assert_eq!(find(&op, "AskOverlay::other").is_some(), third);
        crate::ui::dialogs::ask::answer(&op, false, String::new());
        resize(&op, SMALLEST);
        assert!(find(&op, "AskOverlay::card").is_none());
    }
}

#[test]
fn toasts_and_jobs_fit_the_smallest_window() {
    let app = test_app();
    app.jobs.lock().unwrap().extend([
        Job { id: Uuid::new_v4(), label: "Importing slides".into(), percent: 30, ..Job::default() },
        Job { id: Uuid::new_v4(), label: "Importing talk.pptx".into(), error: Some("PowerPoint could not open this file. ".repeat(6)), ..Job::default() },
    ]);
    let op = open(&app);
    refresh(&op);
    let _upload = op.client_job("Uploading a very long file name from the church camera.mp4");
    op.toast("Saved", false);
    op.toast("John 3:16 added".to_string(), false);
    op.toast("That file could not be read. ".repeat(12), true);
    resize(&op, SMALLEST);
    let (toasts, jobs) = (find_all(&op, "ToastStack::toast"), find_all(&op, "JobsBox::job"));
    assert_eq!((toasts.len(), jobs.len()), (3, 3));
    for e in toasts.iter().chain(&jobs) {
        assert!(within(box_of(e), (0.0, 0.0, SMALLEST.0, SMALLEST.1)), "{:?} ({:?}) is inside the window", e.accessible_label().unwrap_or_default(), box_of(e));
    }
    assert_in_order(&op, &["ToastStack::toast"], true, "(toasts)");
    assert_in_order(&op, &["JobsBox::job"], true, "(job cards)");
}

#[test]
fn translation_buttons_collapse_in_a_narrow_window() {
    use slint::platform::PointerEventButton;
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    resize(&op, (1454.0, 938.0));
    assert!(find(&op, "TopBar::translation-pick").is_none());
    assert!(find_all(&op, "TopBar::translation-chip").len() > 1, "every translation has its button");
    resize(&op, SMALLEST);
    assert!(find(&op, "TopBar::translation-chip").is_none());
    let pick = find(&op, "TopBar::translation-pick").expect("the chosen translation's button");
    assert_eq!(pick.accessible_label().unwrap_or_default(), "KJV");
    assert!(pick.size().width <= 80.0, "the button keeps its own width, not the Bible box's: {}", pick.size().width);
    pick.mock_single_click(PointerEventButton::Left);
    slint::platform::update_timers_and_animations();
    let rows = find_all(&op, "TopBar::picker-row");
    assert_eq!(rows.len(), op.ui.get_translations().row_count(), "the list shows every translation");
    // As the full-width buttons do, a translation that is not connected says how to connect it.
    let tip = |name: &str| rows.iter().find(|r| r.accessible_label().unwrap_or_default() == name)
        .and_then(|r| r.accessible_description()).unwrap_or_default().to_string();
    assert_eq!(tip("KJV"), "KJV");
    assert_eq!(tip("NIV"), "NIV isn't connected yet (Settings → Bible). You can still paste its verses with Add → Bible.");
    let web = rows.iter().position(|r| r.accessible_label().unwrap_or_default() == "WEB").expect("WEB in the list");
    rows[web].mock_single_click(PointerEventButton::Left);
    slint::platform::update_timers_and_animations();
    // The button reads WEB at once, before the core's next update reaches the window.
    assert_eq!(find(&op, "TopBar::translation-pick").unwrap().accessible_label().unwrap_or_default(), "WEB");
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

/// Asserts the window can shrink to 1280 × 720 and grow to 3840 × 2160 at each of `SIZES`, and that at its own
/// smallest size (1024 × 640, 1280 × 800 at 125 %) its minimum does not follow its height: Windows grows a window to
/// its minimum, so a minimum one pixel taller than the window grows it without end.
fn assert_free_limits(op: &Operator, state: &str) {
    for size in SIZES.iter().rev().copied() {
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
    let empty = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: Uuid::new_v4() }, background: None, notes: String::new() };
    op.app().data.lock().unwrap().services[0].items.push(empty.clone());
    refresh(&op);
    op.ui.invoke_service_select(empty.id.to_string().into());
    slint::platform::update_timers_and_animations();
    assert!(find(&op, "SlidesPanel::nothing").is_some());
    assert_free_limits(&op, "an item with no slides");
}
