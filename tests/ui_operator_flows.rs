//! The operator's main flows (native spec §13, v1 spec §14.2 "Navigation") on Slint's testing backend. Each test
//! drives the window only through element ids and key presses, the way an operator would, then checks the core
//! (`app.live_view()`, `app.data`) and what the window shows. Actions run inline (`Ops::inline`); with no event loop
//! there is no follower either, so `sync` hands the window the core's state after each step, as the follower would.
use i_slint_backend_testing::{ElementHandle, mock_elapsed_time};
use rustypresenter::{
    app::{App, LiveUpdate},
    model::{ItemKind, ServiceItem, Song},
    store::Store,
    ui::{operator::{view::OperatorSnapshot, window::{DialogKind, Operator}}, test_support::{operator_for_tests, window_of}},
};
use slint::{
    LogicalPosition, LogicalSize, SharedString,
    platform::{Key, PointerEventButton, WindowEvent},
};
use std::{path::PathBuf, rc::Rc, sync::Arc, time::Duration};
use uuid::Uuid;

/// Installs the testing backend once per thread: Slint's platform slot is thread-local and cargo runs each test on
/// its own thread (the same guard as `ui::ui_test_init`).
fn ui_init() {
    thread_local!(static INIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    INIT.with(|done| {
        if !done.replace(true) {
            i_slint_backend_testing::init_no_event_loop();
        }
    });
}

/// A data folder of its own, removed when the test ends.
struct TempRoot(PathBuf);

impl Drop for TempRoot {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

/// One test's app and operator window. Fields drop in order: the window, the app, then the folder.
struct Flow { op: Rc<Operator>, app: Arc<App>, _root: TempRoot }

/// A fresh app over an empty data folder, with `setup` run on its data before the window opens.
fn flow(setup: impl FnOnce(&App)) -> Flow {
    ui_init();
    let root = std::env::temp_dir().join(format!("rp-ui-flow-{}", Uuid::new_v4()));
    let store = Store::open(root.clone()).unwrap();
    let data = store.load().unwrap();
    let app = App::new(store, data, 0, None);
    setup(&app);
    let op = operator_for_tests(app.clone());
    op.window().set_size(LogicalSize::new(1400.0, 900.0));
    let f = Flow { op, app, _root: TempRoot(root) };
    f.sync();
    f
}

impl Flow {
    /// What the follower would deliver now: the live view first, then a full read (`ui::operator::window::listen`).
    fn sync(&self) {
        self.op.apply_live(LiveUpdate::new(self.app.live_view()));
        self.op.apply_snapshot(Arc::new(OperatorSnapshot::read(&self.app)));
        slint::platform::update_timers_and_animations();
    }

    fn find(&self, id: &str) -> Option<ElementHandle> { ElementHandle::find_by_element_id(window_of(&self.op), id).next() }

    fn get(&self, id: &str) -> ElementHandle { self.find(id).unwrap_or_else(|| panic!("{id} is on screen")) }

    fn find_all(&self, id: &str) -> Vec<ElementHandle> { ElementHandle::find_by_element_id(window_of(&self.op), id).collect() }

    fn row(&self, i: usize) -> ElementHandle { self.find_all("ServiceColumn::row").swap_remove(i) }

    /// A click, then the updates it caused.
    fn click(&self, id: &str) {
        self.get(id).mock_single_click(PointerEventButton::Left);
        self.sync();
    }

    fn press(&self, text: impl Into<SharedString>) {
        let text = text.into();
        self.op.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        self.op.window().dispatch_event(WindowEvent::KeyReleased { text });
    }

    /// A key press, then the updates it caused.
    fn key(&self, k: Key) {
        self.press(char::from(k));
        self.sync();
    }

    fn cursor(&self) -> Option<(Uuid, usize)> { self.app.live_view().cursor.map(|c| (c.item_id, c.slide)) }

    fn items(&self) -> Vec<ServiceItem> { self.app.data.lock().unwrap().services[0].items.clone() }

    fn order(&self) -> Vec<Uuid> { self.items().iter().map(|i| i.id).collect() }

    fn label(&self, id: &str) -> String { self.get(id).accessible_label().unwrap_or_default().to_string() }
}

/// An announcement in the current service; an empty line in `body` starts a new slide.
fn add_text(app: &App, title: &str, body: &str) -> Uuid {
    let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: title.into(), body: body.into() }, background: None, notes: String::new(), theme: None };
    app.data.lock().unwrap().services[0].items.push(item.clone());
    item.id
}

/// A library song in the current service; returns the service item's id.
fn add_song(app: &App, title: &str, lyrics: &str) -> Uuid {
    let song = Song { id: Uuid::new_v4(), title: title.into(), lyrics: lyrics.into(), ..Default::default() };
    let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: None, notes: String::new(), theme: None };
    let mut data = app.data.lock().unwrap();
    data.songs.push(song);
    data.services[0].items.push(item.clone());
    item.id
}

fn center(e: &ElementHandle) -> LogicalPosition {
    let (p, s) = (e.absolute_position(), e.size());
    LogicalPosition::new(p.x + s.width / 2.0, p.y + s.height / 2.0)
}

#[test]
fn select_item_shows_its_slides_without_going_live() {
    let mut ids = vec![];
    let f = flow(|app| ids = vec![add_text(app, "Welcome", "Hello"), add_text(app, "Offering", "Give\n\nThanks")]);
    let before = f.app.live_view().version;
    f.row(1).mock_single_click(PointerEventButton::Left);
    f.sync();
    assert_eq!(f.label("SlidesPanel::slides-title"), "Offering");
    assert_eq!(f.find_all("SlidesPanel::thumb").len(), 2, "the selected item's two slides");
    assert_eq!(f.app.live_view().version, before, "nothing went live");
    assert_eq!(f.cursor(), None);
}

#[test]
fn next_and_back_move_the_live_slide() {
    let mut id = Uuid::nil();
    let f = flow(|app| id = add_text(app, "A", "a1\n\na2\n\na3"));
    // A double click on the row takes it live at slide 0.
    let row = f.row(0);
    row.mock_single_click(PointerEventButton::Left);
    row.mock_single_click(PointerEventButton::Left);
    f.sync();
    assert_eq!(f.cursor(), Some((id, 0)));
    f.click("LiveColumn::next-btn");
    f.click("LiveColumn::next-btn");
    f.click("LiveColumn::prev-btn");
    assert_eq!(f.cursor(), Some((id, 1)));
    let up = f.get("LiveColumn::up-next");
    assert_eq!(up.accessible_description().unwrap_or_default(), "A · A 3");
    assert_eq!(up.accessible_label().unwrap_or_default(), "a3");
    assert_eq!(f.label("LiveColumn::live-what"), "A · A 2");
}

#[test]
fn next_ignores_selection_and_follows_service_order() {
    let (mut a, mut b, mut c) = (Uuid::nil(), Uuid::nil(), Uuid::nil());
    let f = flow(|app| {
        a = add_song(app, "A", "Verse 1\na1\n\nChorus\na2");
        b = add_song(app, "B", "Verse 1\nb1");
        c = add_song(app, "C", "Verse 1\nc1");
    });
    // A goes live (double click), then C is selected (one click).
    let row = f.row(0);
    row.mock_single_click(PointerEventButton::Left);
    row.mock_single_click(PointerEventButton::Left);
    f.sync();
    f.row(2).mock_single_click(PointerEventButton::Left);
    f.sync();
    assert_eq!(f.label("SlidesPanel::slides-title"), "C", "C is selected");
    assert_eq!(f.cursor(), Some((a, 0)));
    f.key(Key::RightArrow);
    assert_eq!(f.cursor(), Some((a, 1)), "mid-item, Next stays in A");
    f.key(Key::RightArrow);
    assert_eq!(f.cursor(), Some((b, 0)), "at A's last slide, Next takes B live (service order), not the selected C");
    assert_ne!(f.cursor().map(|c| c.0), Some(c));
}

#[test]
fn clear_all_from_button_and_f1() {
    let mut id = Uuid::nil();
    let f = flow(|app| id = add_text(app, "A", "a1"));
    let show = || {
        let row = f.row(0);
        row.mock_single_click(PointerEventButton::Left);
        row.mock_single_click(PointerEventButton::Left);
        f.sync();
        assert!(f.app.live_view().frame.slide.is_some(), "A is showing");
    };
    let empty = || {
        let frame = f.app.live_view().frame;
        assert!(frame.slide.is_none() && frame.media.is_none() && frame.audio.is_none() && !frame.logo, "an empty frame: {frame:?}");
    };
    show();
    f.click("LiveColumn::clear-all");
    empty();
    assert_eq!(f.label("LiveColumn::live-what"), "Clear");
    // F1 clears too, even while the keyboard is in the Bible box; nothing is typed there.
    show();
    f.click("TopBar::bible-input");
    f.press("J");
    f.key(Key::F1);
    empty();
    assert_eq!(f.get("TopBar::bible-input").accessible_value().unwrap_or_default(), "J");
}

#[test]
fn bible_lookup_then_enter_takes_it_live() {
    let f = flow(|_| {});
    let input = f.get("TopBar::bible-input");
    input.mock_single_click(PointerEventButton::Left);
    input.set_accessible_value("John 3:16");
    // Enter in the box looks the passage up, adds it and arms it; nothing goes live yet.
    f.key(Key::Return);
    assert_eq!(f.items().len(), 1, "the passage was added");
    assert_eq!(f.cursor(), None, "the first Enter only adds and arms");
    assert_eq!(input.accessible_value().unwrap_or_default(), "", "the box is cleared");
    // The keys are back on the window: the second Enter shows the armed passage.
    f.key(Key::Return);
    let v = f.app.live_view();
    assert_eq!(v.item.as_ref().map(|i| i.title.as_str()), Some("John 3:16 (KJV)"));
    assert_eq!(f.cursor(), Some((f.items()[0].id, 0)), "the room cursor is on slide 0");
}

#[test]
fn add_song_through_dialog() {
    let f = flow(|_| {});
    f.click("ServiceColumn::add-btn");
    assert_eq!(f.op.dialog(), Some(DialogKind::Add(rustypresenter::ui::dialogs::AddTab::Song)));
    f.click("AddSongTab::new-song");
    // The editor replaced the Add dialog; its title takes the keyboard 30 ms later.
    mock_elapsed_time(Duration::from_millis(40));
    f.sync();
    assert_eq!(f.op.dialog(), Some(DialogKind::SongEditor { song_id: None, add_after: true }));
    f.get("SongEditor::title").set_accessible_value("Amazing Grace");
    f.get("SongEditor::lyrics").set_accessible_value("Verse 1\nAmazing grace how sweet the sound\nThat saved a wretch like me");
    f.click("SongEditor::save");
    let songs = f.app.data.lock().unwrap().songs.clone();
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].title, "Amazing Grace");
    let items = f.items();
    assert_eq!(items.len(), 1, "the service has one item");
    assert!(matches!(items[0].kind, ItemKind::Song { song_id, .. } if song_id == songs[0].id), "a song item: {:?}", items[0].kind);
    assert_eq!(f.op.dialog(), None, "the dialog is closed");
    assert_eq!(f.find_all("ServiceColumn::row").len(), 1);
}

#[test]
fn reorder_by_drag() {
    let mut ids = vec![];
    let f = flow(|app| ids = vec![add_text(app, "A", "a"), add_text(app, "B", "b"), add_text(app, "C", "c")]);
    // Drag row 0 onto row 2: `service/move {id: A, to: 2}`.
    f.row(0).mock_drag(center(&f.row(2)), PointerEventButton::Left);
    f.sync();
    assert_eq!(f.order(), [ids[1], ids[2], ids[0]]);
    let titles: Vec<String> = f.find_all("ServiceColumn::row").iter().map(|r| r.accessible_label().unwrap_or_default().to_string()).collect();
    assert_eq!(titles, ["B", "C", "A"], "the window shows the new order");
}

#[test]
fn dialog_blocks_presentation_keys() {
    let mut id = Uuid::nil();
    let f = flow(|app| id = add_text(app, "A", "a1\n\na2"));
    let row = f.row(0);
    row.mock_single_click(PointerEventButton::Left);
    row.mock_single_click(PointerEventButton::Left);
    f.sync();
    f.click("TopBar::settings-btn");
    assert!(matches!(f.op.dialog(), Some(DialogKind::Settings(_))), "Settings is open: {:?}", f.op.dialog());
    let version = f.app.live_view().version;
    f.key(Key::RightArrow);
    assert_eq!(f.app.live_view().version, version, "→ does nothing while a dialog is open");
    assert_eq!(f.cursor(), Some((id, 0)));
    f.key(Key::F1);
    assert!(f.app.live_view().frame.slide.is_none(), "F1 still clears");
    assert!(f.op.dialog().is_some(), "the dialog stays open");
}
