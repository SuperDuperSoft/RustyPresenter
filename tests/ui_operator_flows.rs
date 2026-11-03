//! The operator's main flows (native spec §13, v1 §14.2 Navigation), driven only through element ids and keys on the
//! Slint testing backend. Each test gets its own data folder.
use i_slint_backend_testing::{ElementHandle, mock_elapsed_time};
use rustypresenter::{
    app::App,
    model::{ItemKind, ServiceItem},
    store::Store,
    ui::{operator::window::Operator, test_support::{operator_for_tests, operator_window, sync}},
};
use slint::{LogicalPosition, SharedString, platform::{Key, PointerEventButton, WindowEvent}};
use std::{rc::Rc, sync::Arc, time::Duration};
use uuid::Uuid;

fn test_app() -> Arc<App> {
    let store = Store::open(std::env::temp_dir().join(format!("rp-flow-{}", Uuid::new_v4()))).unwrap();
    let data = store.load().unwrap();
    App::new(store, data, 0, None)
}

/// Slint's platform slot is thread-local, as is this guard (the same as `ui::ui_test_init`).
fn init() {
    thread_local!(static INIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    INIT.with(|done| if !done.replace(true) { i_slint_backend_testing::init_no_event_loop() });
}

fn opened(app: &Arc<App>) -> Rc<Operator> {
    init();
    let op = operator_for_tests(app.clone());
    op.window().set_size(slint::LogicalSize::new(1400.0, 900.0));
    sync(&op);
    op
}

fn all(op: &Operator, id: &str) -> Vec<ElementHandle> { ElementHandle::find_by_element_id(operator_window(op), id).collect() }

fn one(op: &Operator, id: &str) -> ElementHandle { all(op, id).into_iter().next().unwrap_or_else(|| panic!("{id} on screen")) }

fn settle(op: &Operator) {
    slint::platform::update_timers_and_animations();
    let _ = all(op, "DialogShell::close-btn");
    slint::platform::update_timers_and_animations();
}

fn click(op: &Operator, id: &str) {
    one(op, id).mock_single_click(PointerEventButton::Left);
    settle(op);
    sync(op);
}

fn press(op: &Operator, text: impl Into<SharedString>) {
    let text = text.into();
    op.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    op.window().dispatch_event(WindowEvent::KeyReleased { text });
}

fn key(op: &Operator, k: Key) {
    press(op, char::from(k));
    sync(op);
}

fn type_text(op: &Operator, text: &str) { for c in text.chars() { press(op, c) } }

fn label(e: &ElementHandle) -> String { e.accessible_label().unwrap_or_default().to_string() }

fn add_text(app: &App, title: &str, body: &str) -> Uuid {
    let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: title.into(), body: body.into() }, background: None, notes: String::new() };
    app.data.lock().unwrap().services[0].items.push(item.clone());
    item.id
}

fn order(app: &App) -> Vec<Uuid> { app.data.lock().unwrap().services[0].items.iter().map(|i| i.id).collect() }

fn cursor(app: &App) -> Option<(Uuid, usize)> { app.live_view().cursor.map(|c| (c.item_id, c.slide)) }

/// Takes `item` live from its first slide, as a double-click on the row would.
fn take_live(op: &Operator, item_id: Uuid) {
    op.ops().cmd(rustypresenter::live::Command::Show { item_id, slide: 0 }, None, |o| assert!(o.ok, "{o:?}"));
    sync(op);
}

#[test]
fn select_item_shows_its_slides_without_going_live() {
    let app = test_app();
    add_text(&app, "A", "a");
    add_text(&app, "B", "b");
    let op = opened(&app);
    let version = app.live_view().version;
    all(&op, "ServiceColumn::row")[1].mock_single_click(PointerEventButton::Left);
    settle(&op);
    assert_eq!(label(&one(&op, "SlidesPanel::slides-title")), "B");
    assert_eq!(app.live_view().version, version);
    assert_eq!(cursor(&app), None);
}

#[test]
fn next_and_back_move_the_live_slide() {
    let app = test_app();
    let a = add_text(&app, "A", "a1\n\na2\n\na3");
    let op = opened(&app);
    take_live(&op, a);
    click(&op, "LiveColumn::next-btn");
    click(&op, "LiveColumn::next-btn");
    click(&op, "LiveColumn::prev-btn");
    assert_eq!(cursor(&app), Some((a, 1)));
    assert_eq!(label(&one(&op, "LiveColumn::up-next")), "a3");
}

#[test]
fn next_ignores_selection_and_follows_service_order() {
    let app = test_app();
    let a = add_text(&app, "A", "a1\n\na2");
    let b = add_text(&app, "B", "b1");
    add_text(&app, "C", "c1");
    let op = opened(&app);
    take_live(&op, a);
    all(&op, "ServiceColumn::row")[2].mock_single_click(PointerEventButton::Left);
    settle(&op);
    click(&op, "LiveColumn::next-btn");
    assert_eq!(cursor(&app), Some((a, 1)), "still in the live item");
    click(&op, "LiveColumn::next-btn");
    assert_eq!(cursor(&app), Some((b, 0)), "the next in service order, not the selection");
}

#[test]
fn clear_all_from_button_and_f1() {
    let app = test_app();
    let a = add_text(&app, "A", "a1");
    let op = opened(&app);
    take_live(&op, a);
    assert!(app.live_view().frame.slide.is_some());
    click(&op, "LiveColumn::clear-all");
    assert!(app.live_view().frame.slide.is_none(), "the button clears");

    take_live(&op, a);
    one(&op, "TopBar::bible-input").mock_single_click(PointerEventButton::Left);
    key(&op, Key::F1);
    assert!(app.live_view().frame.slide.is_none(), "F1 works with the keyboard in the Bible box");
}

#[test]
fn bible_lookup_then_enter_takes_it_live() {
    let app = test_app();
    let op = opened(&app);
    one(&op, "TopBar::bible-input").mock_single_click(PointerEventButton::Left);
    type_text(&op, "John 3:16");
    key(&op, Key::Return);
    let added = *order(&app).last().expect("the passage is in the service");
    assert_eq!(cursor(&app), None, "the first Enter only arms it");
    key(&op, Key::Return);
    assert_eq!(cursor(&app), Some((added, 0)));
    assert_eq!(app.live_view().item.expect("live item").title, "John 3:16 (KJV)");
}

#[test]
fn add_song_through_dialog() {
    let app = test_app();
    let op = opened(&app);
    click(&op, "ServiceColumn::add-btn");
    click(&op, "AddDialog::song-tile");
    click(&op, "AddSongTab::new-song");
    mock_elapsed_time(Duration::from_millis(40)); // The title takes the keyboard 30 ms after the swap.
    settle(&op);
    type_text(&op, "Amazing Grace");
    let st = slint::ComponentHandle::global::<rustypresenter::ui::SongEditorState>(operator_window(&op));
    st.set_lyrics("Verse 1\nAmazing grace".into());
    click(&op, "SongEditor::save");
    let d = app.data.lock().unwrap();
    assert!(matches!(d.services[0].items[..], [ServiceItem { kind: ItemKind::Song { .. }, .. }]));
    assert_eq!(d.songs[0].title, "Amazing Grace");
    drop(d);
    assert!(all(&op, "DialogShell::close-btn").is_empty(), "the dialog closed");
}

#[test]
fn reorder_by_drag() {
    let app = test_app();
    let ids = [add_text(&app, "A", "a"), add_text(&app, "B", "b"), add_text(&app, "C", "c")];
    let op = opened(&app);
    let center = |e: &ElementHandle| {
        let (p, s) = (e.absolute_position(), e.size());
        LogicalPosition::new(p.x + s.width / 2.0, p.y + s.height / 2.0)
    };
    let rows = all(&op, "ServiceColumn::row");
    let (from, to) = (center(&rows[0]), center(&rows[2]));
    let w = op.window();
    w.dispatch_event(WindowEvent::PointerMoved { position: from });
    w.dispatch_event(WindowEvent::PointerPressed { position: from, button: PointerEventButton::Left });
    for dy in [4.0, 8.0, 12.0, 16.0] {
        mock_elapsed_time(Duration::from_millis(10));
        w.dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(from.x, from.y + dy) });
    }
    mock_elapsed_time(Duration::from_millis(10));
    w.dispatch_event(WindowEvent::PointerMoved { position: to });
    w.dispatch_event(WindowEvent::PointerReleased { position: to, button: PointerEventButton::Left });
    assert_eq!(order(&app), [ids[1], ids[2], ids[0]]);
}

#[test]
fn dialog_blocks_presentation_keys() {
    let app = test_app();
    let a = add_text(&app, "A", "a1\n\na2");
    let op = opened(&app);
    take_live(&op, a);
    click(&op, "TopBar::settings-btn");
    assert!(!all(&op, "DialogShell::close-btn").is_empty(), "Settings opened");
    let version = app.live_view().version;
    key(&op, Key::RightArrow);
    assert_eq!(app.live_view().version, version, "the arrow stays in the dialog");
    assert_eq!(cursor(&app), Some((a, 0)));
    key(&op, Key::F1);
    assert!(app.live_view().frame.slide.is_none(), "F1 clears from inside a dialog");
}
