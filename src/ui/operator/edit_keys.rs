//! The standard Windows keys for the service list and the media library (`shortcuts::EditKey`): Delete, Copy, Cut,
//! Paste, Duplicate, Move up / down and ↑ ↓. Each runs what its button or menu item runs (the same core action, so
//! the same question and the same Undo); the Edit menu items call these too.
use super::window::{ListFocus, Operator};
use crate::{model::ServiceItem, shortcuts::EditKey, ui::dialogs::AddTab};
use serde::Deserialize;
use serde_json::json;
use std::{cell::RefCell, rc::Rc};
use uuid::Uuid;

thread_local! {
    /// The copied service item, and the clipboard's YouTube link when it was copied (a link copied later wins).
    static CLIP: RefCell<Option<(ServiceItem, Option<String>)>> = const { RefCell::new(None) };
}

/// `service/add`'s answer.
#[derive(Deserialize)]
struct Added { id: Uuid }

impl Operator {
    /// The selected service item and where it is in the list.
    fn selected_at(&self) -> Option<(usize, ServiceItem)> {
        let st = self.state.borrow();
        let id = st.selected?;
        st.snap.as_ref()?.items.iter().enumerate().find(|(_, i)| i.id == id).map(|(at, i)| (at, i.item.clone()))
    }

    /// An editing key or its Edit menu item; false when it had nothing to work on.
    pub(crate) fn edit_key(self: &Rc<Self>, key: EditKey) -> bool {
        let media = self.state.borrow().list == ListFocus::Media;
        if media {
            // The media library has one picked file: Delete asks, as the Add dialog's trash button does.
            let Some(file) = self.picked().filter(|_| key == EditKey::Delete) else { return false };
            crate::ui::dialogs::add_media::delete(self, AddTab::Media, file.id);
            return true;
        }
        let Some((at, item)) = self.selected_at() else { return false };
        match key {
            EditKey::Delete => self.remove_selected(at, item.id),
            EditKey::Copy => copy(item),
            EditKey::Cut => { copy(item.clone()); self.remove_selected(at, item.id) }
            EditKey::Duplicate => self.paste_item(item, at + 1),
            EditKey::MoveUp | EditKey::MoveDown => {
                let up = key == EditKey::MoveUp;
                let count = self.state.borrow().snap.as_ref().map_or(0, |s| s.items.len());
                if (up && at == 0) || (!up && at + 1 >= count) { return true }
                let to = if up { at - 1 } else { at + 1 };
                self.service_action("service/move", json!({ "id": item.id, "to": to }));
            }
            EditKey::NewService | EditKey::OpenService | EditKey::Print | EditKey::Settings => return false,
        }
        true
    }

    /// The row's remove button (`service/remove`, one Undo step), with the next row (else the one before) selected.
    fn remove_selected(self: &Rc<Self>, at: usize, id: Uuid) {
        let next = {
            let st = self.state.borrow();
            st.snap.as_ref().and_then(|s| s.items.get(at + 1).or(at.checked_sub(1).and_then(|p| s.items.get(p)))).map(|i| i.id)
        };
        self.state.borrow_mut().selected = next;
        self.service_action("service/remove", json!({ "id": id }));
        self.mark_service();
        self.render_slides();
    }

    /// A copy of `item` at `position`, selected.
    fn paste_item(self: &Rc<Self>, item: ServiceItem, position: usize) {
        let me = Rc::downgrade(self);
        self.ops.call::<Added>("service/add", json!({ "item": item, "position": position }), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r { Ok(Added { id }) => op.select_added(id), Err(e) => op.toast(e, true) }
        });
    }

    /// Ctrl+V or Edit → Paste: a YouTube link copied after the last item opens Download from YouTube (as before);
    /// else the copied service item goes in after the selected one (at the end with none).
    pub(crate) fn paste(self: &Rc<Self>) -> bool {
        use crate::ui::dialogs::add_youtube::{open_with_link, pasted_link};
        let link = pasted_link();
        let clip = CLIP.with(|c| c.borrow().clone());
        match (clip, link) {
            (Some((item, then)), link) if link.is_none() || link == then => {
                let position = self.selected_at().map_or_else(|| self.state.borrow().snap.as_ref().map_or(0, |s| s.items.len()), |(at, _)| at + 1);
                self.paste_item(item, position);
                true
            }
            (_, Some(link)) => { open_with_link(self, &link); true }
            _ => false,
        }
    }

    /// ↑ or ↓ in the service list: the item above or below is selected (as a click does); nothing goes live.
    pub(crate) fn step_selection(self: &Rc<Self>, down: bool) -> bool {
        let target = {
            let st = self.state.borrow();
            let Some(items) = st.snap.as_ref().map(|s| &s.items) else { return false };
            let at = st.selected.and_then(|id| items.iter().position(|i| i.id == id));
            let to = match (at, down) {
                (None, _) => 0,
                (Some(i), true) => (i + 1).min(items.len().saturating_sub(1)),
                (Some(i), false) => i.saturating_sub(1),
            };
            items.get(to).map(|i| i.id)
        };
        if let Some(id) = target { self.select_item(id) }
        true
    }
}

/// Ctrl+C: the item is kept for Ctrl+V, in this service or another.
fn copy(item: ServiceItem) {
    let link = crate::ui::dialogs::add_youtube::pasted_link();
    CLIP.with(|c| *c.borrow_mut() = Some((item, link)));
}

/// Forgets the copied item (each test starts with none).
#[cfg(test)]
pub(crate) fn clear_clip() { CLIP.with(|c| *c.borrow_mut() = None) }

/// The keys and menu items in a real window: key events on the testing backend, actions run inline.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, test_app},
        model::{ItemKind, MediaFile, MediaKind, MediaStatus},
        ui::operator::window::testing::{open, refresh},
    };
    use slint::{Model, SharedString, platform::{Key, WindowEvent}};
    use std::sync::Arc;

    fn add_texts(app: &App, titles: &[&str]) -> Vec<Uuid> {
        let mut d = app.data.lock().unwrap();
        titles.iter().map(|t| {
            let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: t.to_string(), body: format!("{t} body"), layout: Default::default() }, background: None, notes: String::new(), theme: None };
            d.services[0].items.push(item.clone());
            item.id
        }).collect()
    }

    fn titles(app: &App) -> Vec<String> {
        app.data.lock().unwrap().services[0].items.iter().map(|i| match &i.kind { ItemKind::Text { title, .. } => title.clone(), _ => String::new() }).collect()
    }

    fn opened(app: &Arc<App>) -> Rc<Operator> {
        clear_clip();
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1400.0, 900.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        op
    }

    fn press(op: &Operator, text: impl Into<SharedString>) {
        let text = text.into();
        op.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        op.window().dispatch_event(WindowEvent::KeyReleased { text });
    }

    fn key(op: &Operator, k: Key) { press(op, char::from(k)) }

    /// `text` while `modifier` is held down.
    fn chord(op: &Operator, modifier: Key, text: impl Into<SharedString>) {
        op.window().dispatch_event(WindowEvent::KeyPressed { text: char::from(modifier).into() });
        press(op, text);
        op.window().dispatch_event(WindowEvent::KeyReleased { text: char::from(modifier).into() });
    }

    /// A click on a service row.
    fn click_row(op: &Operator, id: Uuid) { op.ui.invoke_service_select(id.to_string().into()) }

    #[test]
    fn delete_removes_the_selected_item_and_ctrl_z_brings_it_back() {
        let app = test_app();
        let ids = add_texts(&app, &["A", "B", "C"]);
        let op = opened(&app);
        click_row(&op, ids[1]);
        key(&op, Key::Delete);
        assert_eq!(titles(&app), ["A", "C"], "no question for a service item");
        refresh(&op);
        assert_eq!(op.selected_item(), Some(ids[2]), "the next item is selected");
        key(&op, Key::Backspace);
        assert_eq!(titles(&app), ["A"], "Backspace too");
        refresh(&op);
        chord(&op, Key::Control, "z");
        assert_eq!(titles(&app), ["A", "C"], "one Undo step each");
        chord(&op, Key::Control, "z");
        assert_eq!(titles(&app), ["A", "B", "C"]);
    }

    #[test]
    fn delete_in_a_text_box_edits_the_text() {
        let app = test_app();
        let ids = add_texts(&app, &["A", "B"]);
        let op = opened(&app);
        click_row(&op, ids[0]);
        crate::ui::operator::window::testing::find(&op, "TopBar::bible-input").unwrap()
            .mock_single_click(slint::platform::PointerEventButton::Left);
        for c in "Jn".chars() { press(&op, c) }
        key(&op, Key::Backspace);
        key(&op, Key::LeftArrow);
        key(&op, Key::Delete);
        assert_eq!(op.ui.get_bible_text(), "");
        assert_eq!(titles(&app), ["A", "B"], "nothing was removed");
        chord(&op, Key::Control, "c");
        chord(&op, Key::Control, "d");
        chord(&op, Key::Alt, char::from(Key::DownArrow));
        assert_eq!(titles(&app), ["A", "B"]);
    }

    #[test]
    fn copy_cut_paste_and_duplicate_service_items() {
        let app = test_app();
        let ids = add_texts(&app, &["A", "B", "C"]);
        let op = opened(&app);
        click_row(&op, ids[0]);
        chord(&op, Key::Control, "c");
        click_row(&op, ids[1]);
        chord(&op, Key::Control, "v");
        assert_eq!(titles(&app), ["A", "B", "A", "C"], "pasted after the selected item");
        refresh(&op);
        let pasted = app.data.lock().unwrap().services[0].items[2].id;
        assert_ne!(pasted, ids[0], "a new item");
        assert_eq!(op.selected_item(), Some(pasted), "and selected");
        click_row(&op, ids[2]);
        chord(&op, Key::Control, "x");
        assert_eq!(titles(&app), ["A", "B", "A"]);
        refresh(&op);
        click_row(&op, ids[0]);
        chord(&op, Key::Control, "v");
        assert_eq!(titles(&app), ["A", "C", "B", "A"], "the cut item goes in after the selected one");
        refresh(&op);
        click_row(&op, ids[1]);
        chord(&op, Key::Control, "d");
        assert_eq!(titles(&app), ["A", "C", "B", "B", "A"], "Ctrl+D: a copy right after");
        chord(&op, Key::Control, "z");
        assert_eq!(titles(&app), ["A", "C", "B", "A"], "undone in one step");
    }

    #[test]
    fn paste_goes_into_another_service() {
        let app = test_app();
        let ids = add_texts(&app, &["A"]);
        let op = opened(&app);
        click_row(&op, ids[0]);
        chord(&op, Key::Control, "c");
        let other = app.edit(|d| {
            let mut s = d.services[0].clone();
            s.id = Uuid::new_v4();
            s.items.clear();
            let id = s.id;
            d.services.push(s);
            d.current_service = Some(id);
            Ok(id)
        }).unwrap();
        refresh(&op);
        chord(&op, Key::Control, "v");
        let d = app.data.lock().unwrap();
        assert_eq!(d.services.iter().find(|s| s.id == other).unwrap().items.len(), 1);
        assert_eq!(d.services[0].items.len(), 1);
    }

    #[test]
    fn alt_up_and_down_move_the_selected_item() {
        let app = test_app();
        let ids = add_texts(&app, &["A", "B", "C"]);
        let op = opened(&app);
        click_row(&op, ids[0]);
        chord(&op, Key::Alt, char::from(Key::DownArrow));
        assert_eq!(titles(&app), ["B", "A", "C"]);
        refresh(&op);
        chord(&op, Key::Control, char::from(Key::DownArrow));
        assert_eq!(titles(&app), ["B", "C", "A"]);
        refresh(&op);
        chord(&op, Key::Alt, char::from(Key::DownArrow));
        assert_eq!(titles(&app), ["B", "C", "A"], "already last");
        chord(&op, Key::Alt, char::from(Key::UpArrow));
        assert_eq!(titles(&app), ["B", "A", "C"]);
        assert_eq!(op.selected_item(), Some(ids[0]), "the item stays selected");
    }

    #[test]
    fn up_and_down_move_the_selection_after_a_click_in_the_list_and_never_the_screen() {
        let app = test_app();
        let ids = add_texts(&app, &["A", "B", "C"]);
        let op = opened(&app);
        op.ops().cmd(crate::live::Command::Show { item_id: ids[0], slide: 0 }, None, |o| assert!(o.ok));
        op.apply_live(crate::app::LiveUpdate::new(app.live_view()));
        let version = app.live_view().version;
        click_row(&op, ids[0]);
        key(&op, Key::DownArrow);
        key(&op, Key::DownArrow);
        assert_eq!(op.selected_item(), Some(ids[2]));
        key(&op, Key::DownArrow);
        assert_eq!(op.selected_item(), Some(ids[2]), "stays on the last");
        key(&op, Key::UpArrow);
        assert_eq!(op.selected_item(), Some(ids[1]));
        assert_eq!(app.live_view().version, version, "the room screen did not move");
        // Showing an item gives the arrows back to the live screen.
        op.ui.invoke_service_show_first(ids[1].to_string().into());
        assert_eq!(op.state.borrow().list, ListFocus::Live);
    }

    #[test]
    fn delete_on_a_picked_media_file_asks_first() {
        let app = test_app();
        let file = MediaFile { id: Uuid::new_v4(), name: "pic.png".into(), kind: MediaKind::Image, asset: "pic.png".into(), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() };
        app.data.lock().unwrap().media.push(file.clone());
        let ids = add_texts(&app, &["A"]);
        let op = opened(&app);
        click_row(&op, ids[0]);
        op.ui.invoke_bin_pick(file.id.to_string().into());
        key(&op, Key::Delete);
        assert_eq!(op.ui.get_ask().row_data(0).map(|a| a.text.to_string()).as_deref(), Some("Delete “pic.png” from the library?"));
        assert_eq!(titles(&app), ["A"], "the service item stays");
        key(&op, Key::Return);
        assert!(app.data.lock().unwrap().media.is_empty());
    }

    #[test]
    fn edit_menu_items_run_the_same_actions() {
        let app = test_app();
        let ids = add_texts(&app, &["A", "B"]);
        let op = opened(&app);
        click_row(&op, ids[0]);
        op.ui.invoke_menu("duplicate".into());
        assert_eq!(titles(&app), ["A", "A", "B"]);
        refresh(&op);
        op.ui.invoke_menu("delete".into());
        assert_eq!(titles(&app), ["A", "B"]);
        refresh(&op);
        op.ui.invoke_menu("copy".into());
        op.ui.invoke_menu("paste".into());
        assert_eq!(titles(&app).len(), 3);
    }

    #[test]
    fn ctrl_n_o_p_and_comma_open_their_menu_items() {
        use crate::ui::dialogs::{DialogKind, SettingsTab};
        let app = test_app();
        add_texts(&app, &["A"]);
        let op = opened(&app);
        chord(&op, Key::Control, ",");
        assert_eq!(op.dialog(), Some(DialogKind::Settings(SettingsTab::Look)));
        key(&op, Key::Escape);
        assert_eq!(op.dialog(), None, "Esc closes it");
        chord(&op, Key::Control, "p");
        assert!(matches!(op.dialog(), Some(DialogKind::Print(_))), "{:?}", op.dialog());
        key(&op, Key::Escape);
        assert_eq!(op.dialog(), None);
        chord(&op, Key::Control, "n");
        assert_eq!(op.ui.get_ask().row_count(), 1, "New service… asks for the name");
    }
}
