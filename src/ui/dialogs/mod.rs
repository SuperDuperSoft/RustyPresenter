//! The operator's dialogs (`operator.js:353-1000`) as overlays inside the operator window (`ui/dialogs/`): Slint
//! 1.18 has no modal or owner-window API (native spec §6). One dialog at a time, swapped in place like the web's
//! single `<dialog>`; confirm and prompt questions (`ask`) stack above it, or above the bare window.
pub mod add;
pub mod add_media;
pub mod ask;
pub mod background;
pub mod item_edit;
pub(crate) mod pending;
pub mod phone;
pub mod screens;
pub mod services;
pub mod song_editor;
#[path = "../../strings/dialogs.rs"]
pub mod strings;
#[path = "../../strings/add.rs"]
pub mod add_strings;

use crate::ui::operator::window::Operator;
use std::rc::{Rc, Weak};
use uuid::Uuid;

pub use ask::{confirm, prompt};

/// The tiles of Add to service (`operator.js:385-388`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AddTab { #[default] Song, Scripture, Media, Deck, Text }

/// The tabs of Settings (`operator.js:883`), and About (native spec §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SettingsTab { #[default] Look, Songs, Bible, Video, Backups, Help, About }

/// The dialogs of the web operator; one at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    Add(AddTab),
    /// `None` is a new song; `add_after` adds it to the service once saved (`openSongEditor`, `operator.js:656`).
    SongEditor { song_id: Option<Uuid>, add_after: bool },
    ItemEdit(Uuid),
    Services,
    Screens,
    Phone,
    Settings(SettingsTab),
}

impl DialogKind {
    /// The three dialogs that set the web `dialogRefresh` (`operator.js:388, 827, 859`). The others are never
    /// redrawn by an update, so a phone or a conversion never wipes what the user typed.
    pub fn refreshes(&self) -> bool { matches!(self, DialogKind::Add(_) | DialogKind::Screens | DialogKind::Phone) }

    /// `dialog(…, { wide: true })`: the song editor and Settings.
    pub fn wide(&self) -> bool { matches!(self, DialogKind::SongEditor { .. } | DialogKind::Settings(_)) }

    /// The name the window's dialog layer switches its content on.
    pub fn id(&self) -> &'static str {
        match self {
            DialogKind::Add(_) => "add",
            DialogKind::SongEditor { .. } => "song-editor",
            DialogKind::ItemEdit(_) => "item-edit",
            DialogKind::Services => "services",
            DialogKind::Screens => "screens",
            DialogKind::Phone => "phone",
            DialogKind::Settings(_) => "settings",
        }
    }
}

/// What the open dialog asked to run later, and the questions waiting to be answered (the top one is shown).
#[derive(Default)]
pub struct DialogHooks {
    pub(crate) on_close: Option<Box<dyn FnOnce(&Rc<Operator>)>>,
    pub(crate) refresh: Option<Rc<dyn Fn(&Rc<Operator>)>>,
    pub(crate) asks: Vec<ask::Ask>,
}

impl Operator {
    /// The open dialog, if any.
    pub fn dialog(&self) -> Option<DialogKind> { self.state.borrow().dialog }

    /// Shows `kind`, replacing the open dialog in place (the web reuses one `<dialog>`; the replaced one's
    /// close hook does not run, as `d.onclose` is simply replaced there). The caller then sets its hooks.
    pub fn open_dialog(&self, kind: DialogKind) {
        let title = self.dialog_title(kind);
        let old = {
            let mut st = self.state.borrow_mut();
            st.dialog = Some(kind);
            (st.dialogs.on_close.take(), st.dialogs.refresh.take())
        };
        drop(old);
        self.ui.set_dialog_title(title.into());
        self.ui.set_dialog_wide(kind.wide());
        self.ui.set_dialog_kind(kind.id().into());
        self.ui.set_dialog_open(true);
        // A dialog already up keeps its shell, so its key scope is told to take the keyboard again; a new shell
        // takes it as it is made.
        if self.state.borrow().asks == 0 { self.ui.invoke_focus_dialog(); }
    }

    /// Closes the dialog (X, Esc or a finished action), runs its close hook and gives the keyboard back.
    pub fn close_dialog(&self) {
        let (was, on_close, refresh) = {
            let mut st = self.state.borrow_mut();
            (st.dialog.take(), st.dialogs.on_close.take(), st.dialogs.refresh.take())
        };
        drop(refresh);
        if was.is_none() { return }
        self.ui.set_dialog_open(false);
        self.ui.set_dialog_kind("".into());
        if self.state.borrow().asks == 0 { self.ui.invoke_focus_keys(); }
        if let (Some(hook), Some(op)) = (on_close, self.me.upgrade()) { hook(&op) }
    }

    /// Runs `hook` when the open dialog closes (the web `onClose`; Phone stops pairing).
    pub fn on_dialog_close(&self, hook: impl FnOnce(&Rc<Operator>) + 'static) {
        let mut st = self.state.borrow_mut();
        if st.dialog.is_some() { st.dialogs.on_close = Some(Box::new(hook)); }
    }

    /// Redraws the open dialog after each `Data` or conversion update (the web `dialogRefresh`). Only Add, Screens
    /// and Phone are ever redrawn (`DialogKind::refreshes`).
    pub fn on_dialog_refresh(&self, redraw: impl Fn(&Rc<Operator>) + 'static) {
        let mut st = self.state.borrow_mut();
        if st.dialog.is_some() { st.dialogs.refresh = Some(Rc::new(redraw)); }
    }

    /// Called by `apply_snapshot` when the core's data changed.
    pub fn dialog_refresh(&self) {
        let redraw = {
            let st = self.state.borrow();
            st.dialog.filter(DialogKind::refreshes).and_then(|_| st.dialogs.refresh.clone())
        };
        if let (Some(redraw), Some(op)) = (redraw, self.me.upgrade()) { redraw(&op) }
    }

    fn dialog_title(&self, kind: DialogKind) -> String {
        match kind {
            DialogKind::Add(_) => strings::ADD.into(),
            DialogKind::SongEditor { song_id: None, .. } => strings::NEW_SONG.into(),
            DialogKind::SongEditor { .. } => strings::EDIT_SONG.into(),
            // `dialog(it.title, …)` (`operator.js:796`).
            DialogKind::ItemEdit(id) => self.state.borrow().snap.as_ref()
                .and_then(|s| s.items.iter().find(|i| i.id == id).map(|i| i.title.clone()))
                .unwrap_or_default(),
            DialogKind::Services => strings::SERVICES.into(),
            DialogKind::Screens => strings::SCREENS.into(),
            DialogKind::Phone => strings::PHONE.into(),
            DialogKind::Settings(_) => strings::SETTINGS.into(),
        }
    }
}

/// Connects the dialog layer's callbacks. They hold a `Weak`, so the window does not keep itself alive.
pub(crate) fn wire(op: &Rc<Operator>) {
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_dialog_close(move || if let Some(op) = me.upgrade() { op.close_dialog() });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_ask_answer(move |ok, text| if let Some(op) = me.upgrade() { ask::answer(&op, ok, text.into()) });
    add::wire_add_button(op);
    services::wire(op);
    screens::wire(op);
    phone::wire(op);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, LiveUpdate, test_app},
        live::Command,
        model::{ItemKind, ServiceItem},
        ui::{operator::{view::OperatorSnapshot, window::testing::{find, open, refresh}}},
    };
    use slint::{SharedString, platform::{Key, PointerEventButton, WindowEvent}};
    use std::{cell::{Cell, RefCell}, sync::Arc};

    #[test]
    fn dialog_refresh_only_for_add_screens_phone() {
        let refreshing: Vec<DialogKind> = [
            DialogKind::Add(AddTab::Media), DialogKind::SongEditor { song_id: None, add_after: false }, DialogKind::ItemEdit(Uuid::nil()),
            DialogKind::Services, DialogKind::Screens, DialogKind::Phone, DialogKind::Settings(SettingsTab::Look),
        ].into_iter().filter(DialogKind::refreshes).collect();
        assert_eq!(refreshing, [DialogKind::Add(AddTab::Media), DialogKind::Screens, DialogKind::Phone]);

        // In the window: a new read redraws Add, never Settings; a job's progress patch redraws nothing.
        let app = test_app();
        let op = opened(&app);
        let count = |kind: DialogKind| {
            op.open_dialog(kind);
            let n = Rc::new(Cell::new(0));
            let seen = n.clone();
            op.on_dialog_refresh(move |_| seen.set(seen.get() + 1));
            refresh(&op);
            let snap = op.state.borrow().snap.clone().unwrap();
            op.apply_snapshot(Arc::new(OperatorSnapshot::clone(&snap)));
            n.get()
        };
        assert_eq!(count(DialogKind::Add(AddTab::Song)), 1, "the read, not the patch that shares its data");
        assert_eq!(count(DialogKind::Screens), 1);
        assert_eq!(count(DialogKind::Phone), 1);
        assert_eq!(count(DialogKind::Settings(SettingsTab::Look)), 0);
        assert_eq!(count(DialogKind::SongEditor { song_id: None, add_after: true }), 0);
        assert_eq!(count(DialogKind::Services), 0);
    }

    fn add_text(app: &App, title: &str, body: &str) -> Uuid {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: title.into(), body: body.into() }, background: None, notes: String::new() };
        app.data.lock().unwrap().services[0].items.push(item.clone());
        item.id
    }

    fn opened(app: &Arc<App>) -> Rc<Operator> {
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1400.0, 900.0));
        refresh(&op);
        live(&op);
        op
    }

    fn live(op: &Operator) { op.apply_live(LiveUpdate::new(op.app().live_view())); }

    fn show(op: &Operator, item_id: Uuid, slide: usize) {
        op.ops().cmd(Command::Show { item_id, slide }, None, |o| assert!(o.ok, "{o:?}"));
        live(op);
    }

    fn cursor(app: &App) -> Option<(Uuid, usize)> { app.live_view().cursor.map(|c| (c.item_id, c.slide)) }

    /// Runs what a frame would: change handlers, and making the overlays (whose `init` takes the keyboard).
    fn settle(op: &Operator) {
        slint::platform::update_timers_and_animations();
        let _ = find(op, "DialogShell::close-btn");
        let _ = find(op, "AskOverlay::ok");
        slint::platform::update_timers_and_animations();
    }

    fn press(op: &Operator, text: impl Into<SharedString>) {
        let text = text.into();
        op.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        op.window().dispatch_event(WindowEvent::KeyReleased { text });
        settle(op);
    }

    fn key(op: &Operator, k: Key) { press(op, char::from(k)) }

    fn ask_up(op: &Operator) -> bool { find(op, "AskOverlay::ok").is_some() }

    fn confirm_cell(op: &Rc<Operator>) -> Rc<Cell<Option<bool>>> {
        let answer = Rc::new(Cell::new(None));
        let got = answer.clone();
        confirm(op, "Delete “Amazing Grace” from the library?", strings::OK, strings::CANCEL, false, move |ok| got.set(Some(ok)));
        settle(op);
        answer
    }

    #[test]
    fn confirm_enter_is_ok_esc_is_cancel() {
        let app = test_app();
        let op = opened(&app);
        let answer = confirm_cell(&op);
        assert!(ask_up(&op));
        assert_eq!(op.state.borrow().asks, 1);
        key(&op, Key::Return);
        assert_eq!(answer.get(), Some(true));
        assert!(!ask_up(&op));
        assert_eq!(op.state.borrow().asks, 0);

        let answer = confirm_cell(&op);
        key(&op, Key::Escape);
        assert_eq!(answer.get(), Some(false));
        assert!(!ask_up(&op));

        let answer = confirm_cell(&op);
        find(&op, "AskOverlay::ok").unwrap().mock_single_click(PointerEventButton::Left);
        settle(&op);
        assert_eq!(answer.get(), Some(true), "the OK button");
    }

    #[test]
    fn prompt_returns_text_or_none() {
        let app = test_app();
        let op = opened(&app);
        let ask = |op: &Rc<Operator>| {
            let answer: Rc<RefCell<Option<Option<String>>>> = Rc::default();
            let got = answer.clone();
            prompt(op, "Name for the new service", "Service 1/2/2026", move |name| *got.borrow_mut() = Some(name));
            settle(op);
            answer
        };
        let answer = ask(&op);
        key(&op, Key::Return);
        assert_eq!(*answer.borrow(), Some(Some("Service 1/2/2026".to_string())), "the initial text, unchanged");

        let answer = ask(&op);
        for c in "Easter".chars() { press(&op, c) }
        key(&op, Key::Return);
        assert_eq!(*answer.borrow(), Some(Some("Easter".to_string())), "typing replaces the selected initial text");

        let answer = ask(&op);
        press(&op, "x");
        key(&op, Key::Escape);
        assert_eq!(*answer.borrow(), Some(None));
        assert_eq!(op.state.borrow().asks, 0);
    }

    #[test]
    fn esc_on_ask_cancels_only_the_ask() {
        let app = test_app();
        let op = opened(&app);
        op.open_dialog(DialogKind::Settings(SettingsTab::Backups));
        settle(&op);
        let answer = confirm_cell(&op);
        key(&op, Key::Escape);
        assert_eq!(answer.get(), Some(false));
        assert!(!ask_up(&op));
        assert_eq!(op.dialog(), Some(DialogKind::Settings(SettingsTab::Backups)), "the dialog under it stays open");
        assert!(op.ui.get_dialog_open());
        key(&op, Key::Escape);
        assert_eq!(op.dialog(), None, "the dialog has the keyboard again");
        assert!(!op.ui.get_dialog_open());
    }

    #[test]
    fn right_arrow_with_only_a_confirm_open_does_nothing() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2\n\na3");
        let op = opened(&app);
        show(&op, id, 0);
        let answer = confirm_cell(&op);
        key(&op, Key::RightArrow);
        key(&op, Key::RightArrow);
        assert_eq!(cursor(&app), Some((id, 0)));
        assert_eq!(answer.get(), None, "still waiting");
        key(&op, Key::Escape);
        key(&op, Key::RightArrow);
        assert_eq!(cursor(&app), Some((id, 1)), "the window's keys work again");
    }

    #[test]
    fn f1_inside_dialog_clears_and_dialog_stays_open() {
        let app = test_app();
        let id = add_text(&app, "A", "a1");
        let op = opened(&app);
        show(&op, id, 0);
        op.open_dialog(DialogKind::Add(AddTab::Song));
        settle(&op);
        key(&op, Key::F1);
        assert!(app.live_view().frame.slide.is_none(), "F1 cleared the screen");
        assert_eq!(op.dialog(), Some(DialogKind::Add(AddTab::Song)));
        assert!(op.ui.get_dialog_open());
    }

    #[test]
    fn esc_closes_dialog_and_refocuses_keys() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2");
        let op = opened(&app);
        show(&op, id, 0);
        op.open_dialog(DialogKind::Phone);
        assert_eq!(op.ui.get_dialog_title(), "Phone remote");
        let closed = Rc::new(Cell::new(0));
        let seen = closed.clone();
        op.on_dialog_close(move |_| seen.set(seen.get() + 1));
        settle(&op);
        key(&op, Key::RightArrow);
        assert_eq!(cursor(&app), Some((id, 0)), "the dialog blocks the arrows");
        key(&op, Key::Escape);
        assert_eq!(op.dialog(), None);
        assert!(find(&op, "DialogShell::close-btn").is_none());
        assert_eq!(closed.get(), 1, "the close hook ran once");
        key(&op, Key::RightArrow);
        assert_eq!(cursor(&app), Some((id, 1)), "the keys have the keyboard again");
    }

    #[test]
    fn close_button_closes_and_open_swaps_in_place() {
        let app = test_app();
        let id = add_text(&app, "Welcome", "a1");
        let op = opened(&app);
        op.open_dialog(DialogKind::Add(AddTab::Song));
        let closed = Rc::new(Cell::new(0));
        let seen = closed.clone();
        op.on_dialog_close(move |_| seen.set(seen.get() + 1));
        op.open_dialog(DialogKind::ItemEdit(id));
        assert_eq!(op.ui.get_dialog_title(), "Welcome", "Item edit is titled with the item");
        assert_eq!(op.ui.get_dialog_kind(), "item-edit");
        assert!(!op.ui.get_dialog_wide());
        settle(&op);
        find(&op, "DialogShell::close-btn").unwrap().mock_single_click(PointerEventButton::Left);
        assert_eq!(op.dialog(), None);
        assert_eq!(closed.get(), 0, "a replaced dialog's close hook is dropped, as the web replaces `onclose`");
        op.open_dialog(DialogKind::Settings(SettingsTab::Help));
        assert!(op.ui.get_dialog_wide());
    }
}
