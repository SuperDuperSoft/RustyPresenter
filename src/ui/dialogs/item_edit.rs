//! Item edit (`openItemEdit`, `operator.js:720-796`; `ui/dialogs/item-edit.slint`). It works on a clone of the
//! service item and is never redrawn by `dialog_refresh`, so a phone or a conversion never wipes what was typed.
//! Save renames the media or slides when the name changed, then sends `service/update`; when that fails (the item
//! is gone, say) the dialog stays open with the core's message. A passage whose Bible was changed in its picker is
//! looked up again in that Bible first (Bible library spec §7.2); a failed lookup also keeps the dialog open.
use super::{DialogKind, background::{self, Picker, strings}};
use crate::{
    model::{EndAction, ItemKind, MediaStatus, ServiceItem, VerseText},
    ui::{ItemEdit, operator::{bible, view, window::Operator}},
};
use serde::Deserialize;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, rc::{Rc, Weak}};
use uuid::Uuid;

/// The library entry the name field renames, with the name it had when the dialog opened.
enum Name { Media { id: Uuid, was: String }, Deck { id: Uuid, was: String } }

struct Edit {
    item: ServiceItem,
    name: Option<Name>,
    picker: Option<Picker>,
    /// Scripture: the Bible its picker chose (the item's own until a pick).
    bible: Option<String>,
}

thread_local! {
    /// The open Item edit. The callbacks hold it weakly, so closing the dialog lets it (and the picker's thumbnail
    /// worker) go without touching the callbacks, which may be running at that moment.
    static OPEN: RefCell<Option<Rc<RefCell<Edit>>>> = const { RefCell::new(None) };
}

/// "When the video ends", in the order of the segments.
const ENDS: [EndAction; 3] = [EndAction::Stop, EndAction::Hold, EndAction::Loop];

/// Opens Item edit for service item `id` (the row's Edit button or the slides header's). Nothing happens when the
/// item is not in the newest snapshot.
pub fn open(op: &Rc<Operator>, id: Uuid) {
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    let Some(row) = snap.items.iter().find(|i| i.id == id) else { return };
    let item = row.item.clone();
    let g = op.ui.global::<ItemEdit>();
    g.set_kind(row.kind.as_str().into());
    g.set_song_exists(false);
    g.set_per_slide(0);
    g.set_size_mode(0);
    g.set_text_size(snap.data.settings.theme.scripture_size as i32);
    g.set_title("".into());
    g.set_body("".into());
    g.set_autoplay(true);
    g.set_end_index(0);
    g.set_looped(false);
    g.set_has_name(false);
    g.set_name("".into());
    g.set_not_ready(false);
    g.set_translation("".into());
    g.set_translation_changed(false);
    g.set_picker_search("".into());
    g.set_picker_rows(ModelRc::default());
    let mut name = None;
    let bible = match &item.kind { ItemKind::Scripture { translation, .. } => Some(translation.clone()), _ => None };
    match &item.kind {
        ItemKind::Song { song_id } => g.set_song_exists(snap.data.songs.iter().any(|s| s.id == *song_id)),
        ItemKind::Scripture { per_slide, text_size, .. } => {
            g.set_per_slide(i32::from(*per_slide));
            if *text_size > 0 {
                g.set_size_mode(1);
                g.set_text_size(*text_size as i32);
            }
        }
        ItemKind::Text { title, body } => {
            g.set_title(title.as_str().into());
            g.set_body(body.as_str().into());
        }
        ItemKind::Media { media_id, autoplay, end, looped } => {
            g.set_autoplay(*autoplay);
            g.set_end_index(ENDS.iter().position(|e| e == end).unwrap_or(0) as i32);
            g.set_looped(*looped);
            if let Some(m) = snap.data.media.iter().find(|m| m.id == *media_id) {
                g.set_has_name(true);
                g.set_name(m.name.as_str().into());
                name = Some(Name::Media { id: m.id, was: m.name.clone() });
                let chip = match &m.status {
                    MediaStatus::Failed { reason } => Some((strings::FAILED.to_string(), "bad", reason.clone())),
                    _ => None,
                };
                if let Some((text, state, reason)) = chip {
                    g.set_not_ready(true);
                    g.set_status_text(text.into());
                    g.set_status_state(state.into());
                    g.set_status_reason(reason.into());
                }
            }
        }
        ItemKind::Deck { deck_id } => if let Some(d) = snap.data.decks.iter().find(|d| d.id == *deck_id) {
            g.set_has_name(true);
            g.set_name(d.title.as_str().into());
            name = Some(Name::Deck { id: d.id, was: d.title.clone() });
        },
    }
    let has_background = matches!(item.kind, ItemKind::Song { .. } | ItemKind::Scripture { .. } | ItemKind::Text { .. });
    g.set_has_background(has_background);
    g.set_palette(ModelRc::new(VecModel::from(background::palette())));
    g.set_notes(item.notes.as_str().into());
    let picker = has_background.then(|| Picker::new(op, item.background.clone(), snap.data.media.clone(), true));
    let edit = Rc::new(RefCell::new(Edit { item, name, picker, bible }));

    op.open_dialog(DialogKind::ItemEdit(id));
    draw_choices(op, &edit);
    show_bible(op, &edit);
    wire(op, &edit);
    OPEN.with(|o| *o.borrow_mut() = Some(edit));
    op.on_dialog_close(|op| {
        OPEN.with(|o| o.borrow_mut().take());
        op.ui.global::<ItemEdit>().set_choices(ModelRc::default());
    });
}

fn wire(op: &Rc<Operator>, edit: &Rc<RefCell<Edit>>) {
    let g = op.ui.global::<ItemEdit>();
    let with = |f: fn(&Rc<Operator>, &Rc<RefCell<Edit>>)| {
        let (me, edit): (Weak<Operator>, _) = (Rc::downgrade(op), Rc::downgrade(edit));
        move || if let (Some(op), Some(edit)) = (me.upgrade(), edit.upgrade()) { f(&op, &edit) }
    };
    g.on_save(with(save));
    g.on_remove(with(remove));
    g.on_edit_song(with(edit_song));
    g.on_upload(with(|op, edit| {
        let (me, edit) = (op.me.clone(), Rc::downgrade(edit));
        background::upload(op, move |files| {
            let (Some(op), Some(edit)) = (me.upgrade(), edit.upgrade()) else { return };
            if let Some(p) = edit.borrow_mut().picker.as_mut() { p.add_media(files) }
            draw_choices(&op, &edit);
        });
    }));
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_pick_background(move |i| {
        let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) else { return };
        let picked = usize::try_from(i).ok().zip(e.borrow_mut().picker.as_mut()).is_some_and(|(i, p)| p.pick(i));
        if picked { draw_choices(&op, &e) }
    });
    g.on_picker_opening(with(|op, edit| show_bible(op, edit)));
    g.on_picker_accept(with(|op, edit| if let Some(id) = bible::first_picked(&op.ui.global::<ItemEdit>().get_picker_rows()) { pick_bible(op, edit, id) }));
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_picker_search_changed(move |_| if let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) { show_bible(&op, &e) });
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_pick_bible(move |id| if let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) { pick_bible(&op, &e, id.into()) });
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_toggle_favourite(move |id| if let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) {
        bible::toggle_favourite(&op, id.into());
        show_bible(&op, &e);
    });
    let me = Rc::downgrade(op);
    g.on_find_more_bibles(move |q| if let Some(op) = me.upgrade() { super::settings::open_bible_search(&op, &q) });
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_custom_color(move |c| if let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) { set_color(&op, &e, background::to_hex(c)) });
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_custom_hex(move |t| if let (Some(op), Some(e), Some(hex)) = (me.upgrade(), e.upgrade(), background::parse_hex(&t)) { set_color(&op, &e, hex) });
}

fn set_color(op: &Operator, edit: &Rc<RefCell<Edit>>, hex: String) {
    if let Some(p) = edit.borrow_mut().picker.as_mut() { p.set_color(hex) }
    draw_choices(op, edit);
}

/// The Bible chip, the note under it and the picker's lines (keeping its search). A Bible no longer installed
/// shows as its id.
fn show_bible(op: &Operator, edit: &Rc<RefCell<Edit>>) {
    let e = edit.borrow();
    let (Some(chosen), ItemKind::Scripture { translation, .. }) = (e.bible.as_ref(), &e.item.kind) else { return };
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    let g = op.ui.global::<ItemEdit>();
    g.set_translation(view::bible_labels(&snap).get(chosen).cloned().unwrap_or_else(|| chosen.clone()).into());
    g.set_translation_changed(!crate::bible::id::same_id(chosen, translation));
    let (rows, note) = bible::picker_rows(&snap, &g.get_picker_search(), chosen);
    g.set_picker_rows(rows);
    g.set_picker_note(note);
}

/// A row of the picker: the Bible Save looks the passage up in.
fn pick_bible(op: &Operator, edit: &Rc<RefCell<Edit>>, id: String) {
    let known = op.state.borrow().snap.as_ref().is_some_and(|s| s.translations.contains(&id));
    if !known || edit.borrow().bible.is_none() { return }
    edit.borrow_mut().bible = Some(id);
    show_bible(op, edit);
}

/// `bible/lookup`'s answer.
#[derive(Deserialize)]
struct Found { label: String, verses: Vec<VerseText>, translation: String }

/// Redraws the background tiles (a pick, an upload, or a thumbnail that arrived).
fn draw_choices(op: &Operator, edit: &Rc<RefCell<Edit>>) {
    let (me, e) = (op.me.clone(), Rc::downgrade(edit));
    let redraw = move || if let (Some(op), Some(edit)) = (me.upgrade(), e.upgrade()) { draw_choices(&op, &edit) };
    let rows = edit.borrow().picker.as_ref().map(|p| p.rows(redraw)).unwrap_or_default();
    op.ui.global::<ItemEdit>().set_choices(ModelRc::new(VecModel::from(rows)));
}

/// The clone with what the dialog changed.
fn collect(edit: &Edit, g: &ItemEdit) -> ServiceItem {
    let mut item = edit.item.clone();
    match &mut item.kind {
        ItemKind::Scripture { per_slide, text_size, .. } => {
            *per_slide = u8::try_from(g.get_per_slide()).unwrap_or(0);
            *text_size = if g.get_size_mode() == 1 { g.get_text_size().clamp(36, 120) as u32 } else { 0 };
        }
        ItemKind::Text { title, body } => {
            *title = g.get_title().into();
            *body = g.get_body().into();
        }
        ItemKind::Media { autoplay, end, looped, .. } => match g.get_kind().as_str() {
            "video" => {
                *autoplay = g.get_autoplay();
                *end = usize::try_from(g.get_end_index()).ok().and_then(|i| ENDS.get(i).copied()).unwrap_or(*end);
            }
            "audio" => *looped = g.get_looped(),
            _ => {}
        },
        ItemKind::Song { .. } | ItemKind::Deck { .. } => {}
    }
    if let Some(p) = &edit.picker { item.background = p.value.clone() }
    item.notes = g.get_notes().into();
    item
}

/// `parts.saveName`: a rename when the name changed and is not blank, sent trimmed.
fn rename(name: &Name, typed: &str) -> Option<(&'static str, serde_json::Value)> {
    let trimmed = typed.trim();
    match name {
        Name::Media { id, was } if !trimmed.is_empty() && typed != was => Some(("media/rename", serde_json::json!({ "id": id, "name": trimmed }))),
        Name::Deck { id, was } if !trimmed.is_empty() && typed != was => Some(("deck/rename", serde_json::json!({ "id": id, "title": trimmed }))),
        _ => None,
    }
}

/// Save: look the passage up again if its Bible changed, rename if needed, then `service/update`, then close and
/// toast "Saved". Any failure is an error toast with the core's message, and the dialog stays open.
fn save(op: &Rc<Operator>, edit: &Rc<RefCell<Edit>>) {
    let g = op.ui.global::<ItemEdit>();
    let (item, rename, chosen) = {
        let e = edit.borrow();
        (collect(&e, &g), e.name.as_ref().and_then(|n| rename(n, &g.get_name())), e.bible.clone())
    };
    let Some(lookup) = relookup(&item, chosen.as_deref()) else { return rename_then_update(op, item, rename) };
    let me = Rc::downgrade(op);
    op.ops().call::<Found>("bible/lookup", lookup, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(found) => rename_then_update(&op, with_passage(item, found), rename),
            Err(e) => op.toast(e, true),
        }
    });
}

/// The rename (if any), then `service/update` with `item`.
fn rename_then_update(op: &Rc<Operator>, item: ServiceItem, rename: Option<(&'static str, serde_json::Value)>) {
    let id = item.id;
    let body = serde_json::to_value(&item).unwrap_or_default();
    let update = move |op: &Rc<Operator>| {
        let me = Rc::downgrade(op);
        op.ops().call::<serde_json::Value>("service/update", body, move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(_) => {
                    if op.dialog() == Some(DialogKind::ItemEdit(id)) { op.close_dialog() }
                    op.toast(strings::SAVED, false);
                }
                Err(e) => op.toast(e, true),
            }
        });
    };
    match rename {
        None => update(op),
        Some((action, body)) => {
            let me = Rc::downgrade(op);
            op.ops().call::<serde_json::Value>(action, body, move |r| {
                let Some(op) = me.upgrade() else { return };
                match r {
                    Ok(_) => update(&op),
                    Err(e) => op.toast(e, true),
                }
            });
        }
    }
}

/// The lookup Save needs first: the passage's reference in the Bible the picker chose, when that is another Bible.
fn relookup(item: &ServiceItem, chosen: Option<&str>) -> Option<serde_json::Value> {
    let (ItemKind::Scripture { reference, translation, .. }, Some(chosen)) = (&item.kind, chosen) else { return None };
    (!crate::bible::id::same_id(chosen, translation)).then(|| serde_json::json!({ "reference": reference, "translation": chosen }))
}

/// `item` with the passage as the lookup found it in the new Bible (its label, the Bible's id and the verses).
fn with_passage(mut item: ServiceItem, found: Found) -> ServiceItem {
    if let ItemKind::Scripture { reference, translation, verses, .. } = &mut item.kind {
        *reference = found.label;
        *translation = found.translation;
        *verses = found.verses;
    }
    item
}

/// **Remove from service** (no confirm, as on the web), then close.
fn remove(op: &Rc<Operator>, edit: &Rc<RefCell<Edit>>) {
    let id = edit.borrow().item.id;
    let me = Rc::downgrade(op);
    op.ops().call::<serde_json::Value>("service/remove", serde_json::json!({ "id": id }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => if op.dialog() == Some(DialogKind::ItemEdit(id)) { op.close_dialog() },
            Err(e) => op.toast(e, true),
        }
    });
}

/// **Edit song** swaps the dialog for the song editor (`openSongEditor(song)`).
fn edit_song(op: &Rc<Operator>, edit: &Rc<RefCell<Edit>>) {
    let ItemKind::Song { song_id } = edit.borrow().item.kind else { return };
    super::song_editor::open_song_editor(op, Some(song_id), false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, test_app},
        model::{Background, MediaFile, MediaKind, Song},
        ui::operator::window::testing::{find, find_all, open as open_window, refresh},
    };
    use std::sync::Arc;

    fn opened(app: &Arc<App>) -> Rc<Operator> {
        let op = open_window(app);
        op.window().set_size(slint::LogicalSize::new(1400.0, 900.0));
        refresh(&op);
        op
    }

    fn push(app: &App, kind: ItemKind) -> Uuid {
        let item = ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new() };
        app.data.lock().unwrap().services[0].items.push(item.clone());
        item.id
    }

    fn item(app: &App, id: Uuid) -> Option<ServiceItem> {
        app.data.lock().unwrap().services[0].items.iter().find(|i| i.id == id).cloned()
    }

    fn media(app: &App, name: &str, kind: MediaKind) -> Uuid {
        let m = MediaFile { id: Uuid::new_v4(), name: name.into(), kind, asset: format!("{}.bin", Uuid::new_v4()), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() };
        app.data.lock().unwrap().media.push(m.clone());
        m.id
    }

    fn settle() { slint::platform::update_timers_and_animations(); }

    fn click(op: &Operator, id: &str) {
        settle();
        find(op, id).unwrap_or_else(|| panic!("{id} not found")).invoke_accessible_default_action();
        settle();
    }

    fn press(op: &Operator, id: &str) {
        settle();
        find(op, id).unwrap_or_else(|| panic!("{id} not found")).mock_single_click(slint::platform::PointerEventButton::Left);
        settle();
    }

    fn type_into(op: &Operator, id: &str, text: &str) {
        settle();
        find(op, id).unwrap_or_else(|| panic!("{id} not found")).set_accessible_value(text);
        settle();
    }

    fn toasts(op: &Operator) -> Vec<(String, bool)> {
        use slint::Model;
        op.ui.get_toasts().iter().map(|t| (t.text.to_string(), t.err)).collect()
    }

    fn g(op: &Operator) -> ItemEdit<'_> { op.ui.global::<ItemEdit>() }

    #[test]
    fn edit_buttons_open_item_edit() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "Welcome".into(), body: "Hi".into() });
        let op = opened(&app);
        op.ui.invoke_service_edit(id.to_string().into());
        assert_eq!(op.dialog(), Some(DialogKind::ItemEdit(id)));
        assert_eq!(op.ui.get_dialog_title(), "Welcome");
        assert_eq!(g(&op).get_title(), "Welcome");
        op.close_dialog();
        op.ui.invoke_service_select(id.to_string().into());
        op.ui.invoke_slides_edit();
        assert_eq!(op.dialog(), Some(DialogKind::ItemEdit(id)), "the slides header's Edit opens the selected item");
    }

    #[test]
    fn scripture_per_slide_saved() {
        let app = test_app();
        let id = push(&app, ItemKind::Scripture { reference: "John 3:16".into(), translation: "KJV".into(), verses: vec![], per_slide: 0, text_size: 0 });
        let op = opened(&app);
        open(&op, id);
        assert_eq!(g(&op).get_kind(), "scripture");
        assert!(g(&op).get_has_background());
        g(&op).set_per_slide(2);
        press(&op, "ItemEditDialog::save");
        let ItemKind::Scripture { per_slide, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!(per_slide, 2);
        assert_eq!(op.dialog(), None);
        assert_eq!(toasts(&op).last(), Some(&("Saved".to_string(), false)));
    }

    fn kjv_john_3_16(app: &App) -> Uuid {
        let verses = vec![VerseText { reference: "John 3:16".into(), text: "old text".into() }];
        push(app, ItemKind::Scripture { reference: "John 3:16".into(), translation: "KJV".into(), verses, per_slide: 2, text_size: 0 })
    }

    #[test]
    fn scripture_bible_change_looks_the_verses_up_again_on_save() {
        let app = test_app();
        let id = kjv_john_3_16(&app);
        let op = opened(&app);
        open(&op, id);
        assert_eq!(g(&op).get_translation(), "KJV");
        assert!(!g(&op).get_translation_changed());
        press(&op, "ItemEditDialog::bible-pick");
        let rows = find_all(&op, "BiblePickerList::picker-row");
        assert!(!rows.is_empty(), "the picker is open");
        let web = rows.iter().find(|r| r.accessible_label().unwrap_or_default() == "WEB").expect("WEB listed");
        web.mock_single_click(slint::platform::PointerEventButton::Left);
        settle();
        assert_eq!(g(&op).get_translation(), "WEB");
        assert!(g(&op).get_translation_changed(), "the note says Save looks it up again");
        assert!(item(&app, id).is_some_and(|i| matches!(i.kind, ItemKind::Scripture { ref translation, .. } if translation == "KJV")), "nothing changes before Save");
        press(&op, "ItemEditDialog::save");
        let ItemKind::Scripture { reference, translation, verses, per_slide, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!((reference.as_str(), translation.as_str(), per_slide), ("John 3:16", "WEB", 2));
        assert_eq!(verses.len(), 1);
        assert_ne!(verses[0].text, "old text", "the verse is WEB's now");
        assert_eq!(op.dialog(), None);
        assert_eq!(toasts(&op).last(), Some(&("Saved".to_string(), false)));
        assert_eq!(app.data.lock().unwrap().settings.translation, "KJV", "the Bible box keeps its Bible");
    }

    #[test]
    fn scripture_failed_relookup_keeps_the_dialog_and_the_item() {
        let app = test_app();
        let id = kjv_john_3_16(&app);
        let op = opened(&app);
        open(&op, id);
        g(&op).invoke_pick_bible("NIV".into());
        assert_eq!(g(&op).get_translation(), "NIV");
        press(&op, "ItemEditDialog::save");
        assert!(toasts(&op).last().is_some_and(|t| t.1), "an error toast: {:?}", toasts(&op));
        assert_eq!(op.dialog(), Some(DialogKind::ItemEdit(id)), "the dialog stays open");
        let ItemKind::Scripture { translation, verses, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!((translation.as_str(), verses[0].text.as_str()), ("KJV", "old text"));
        g(&op).invoke_pick_bible("KJV".into());
        assert!(!g(&op).get_translation_changed(), "back to its own Bible");
        g(&op).invoke_pick_bible("nothing-like-this".into());
        assert_eq!(g(&op).get_translation(), "KJV", "an unknown Bible is not picked");
    }

    #[test]
    fn a_plain_save_does_not_look_the_passage_up() {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Scripture { reference: "John 3:16".into(), translation: "ebible:spaRV1909".into(), verses: vec![], per_slide: 0, text_size: 0 }, background: None, notes: String::new() };
        assert_eq!(relookup(&item, Some("ebible:spaRV1909")), None);
        assert_eq!(relookup(&item, None), None);
        assert_eq!(relookup(&item, Some("KJV")), Some(serde_json::json!({ "reference": "John 3:16", "translation": "KJV" })));
    }

    #[test]
    fn scripture_text_size_saved_and_automatic_again() {
        let app = test_app();
        let id = push(&app, ItemKind::Scripture { reference: "John 3:16".into(), translation: "KJV".into(), verses: vec![], per_slide: 0, text_size: 0 });
        let op = opened(&app);
        open(&op, id);
        assert_eq!(g(&op).get_size_mode(), 0, "Automatic until chosen");
        assert_eq!(g(&op).get_text_size(), 64, "the slider starts at the Settings size");
        g(&op).set_size_mode(1);
        g(&op).set_text_size(100);
        press(&op, "ItemEditDialog::save");
        assert!(matches!(item(&app, id).unwrap().kind, ItemKind::Scripture { text_size: 100, .. }));
        refresh(&op);
        open(&op, id);
        assert_eq!((g(&op).get_size_mode(), g(&op).get_text_size()), (1, 100));
        g(&op).set_size_mode(0);
        press(&op, "ItemEditDialog::save");
        assert!(matches!(item(&app, id).unwrap().kind, ItemKind::Scripture { text_size: 0, .. }));
    }

    #[test]
    fn video_end_action_saved() {
        let app = test_app();
        let m = media(&app, "Intro", MediaKind::Video);
        let id = push(&app, ItemKind::Media { media_id: m, autoplay: true, end: EndAction::Stop, looped: false });
        let op = opened(&app);
        open(&op, id);
        assert_eq!(g(&op).get_kind(), "video");
        assert!(!g(&op).get_has_background(), "video items have no background picker");
        assert!(find(&op, "ItemEditDialog::looped").is_none());
        click(&op, "ItemEditDialog::autoplay");
        assert!(!g(&op).get_autoplay(), "the checkbox turned off");
        g(&op).set_end_index(1);
        press(&op, "ItemEditDialog::save");
        let ItemKind::Media { autoplay, end, looped, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!((autoplay, end, looped), (false, EndAction::Hold, false));

        // Audio: "Repeat until stopped".
        let a = media(&app, "Hymn", MediaKind::Audio);
        let id = push(&app, ItemKind::Media { media_id: a, autoplay: true, end: EndAction::Stop, looped: false });
        refresh(&op);
        open(&op, id);
        assert_eq!(g(&op).get_kind(), "audio");
        click(&op, "ItemEditDialog::looped");
        press(&op, "ItemEditDialog::save");
        let ItemKind::Media { looped, end, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!((looped, end), (true, EndAction::Stop));
    }

    #[test]
    fn text_item_title_and_body_saved() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "Welcome".into(), body: "Hi".into() });
        let op = opened(&app);
        open(&op, id);
        type_into(&op, "ItemEditDialog::title-box", "Offering");
        type_into(&op, "ItemEditDialog::body-box", "Give\n\nThanks");
        type_into(&op, "ItemEditDialog::notes-box", "Slow down");
        press(&op, "ItemEditDialog::save");
        let it = item(&app, id).unwrap();
        let ItemKind::Text { title, body } = it.kind else { panic!() };
        assert_eq!((title.as_str(), body.as_str(), it.notes.as_str()), ("Offering", "Give\n\nThanks", "Slow down"));
    }

    #[test]
    fn background_pick_and_custom_colour_saved() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "A".into(), body: "a".into() });
        let op = opened(&app);
        open(&op, id);
        let tiles = || find_all(&op, "BackgroundPicker::choice");
        settle();
        assert_eq!(tiles().len(), 9, "default, six swatches, custom, upload");
        assert!(tiles()[0].accessible_checked().unwrap_or(false), "Theme default is on");
        tiles()[2].invoke_accessible_default_action();
        settle();
        assert!(tiles()[2].accessible_checked().unwrap_or(false));
        assert!(!tiles()[0].accessible_checked().unwrap_or(true));
        press(&op, "ItemEditDialog::save");
        assert_eq!(item(&app, id).unwrap().background, Some(Background::Color { color: "#0b1a33".into() }));

        // The custom colour: its swatch opens the grid and the hex field.
        open(&op, id);
        settle();
        tiles()[7].invoke_accessible_default_action();
        settle();
        type_into(&op, "ColorGrid::hex", "#12AB3");
        type_into(&op, "ColorGrid::hex", "#12AB34");
        assert!(tiles().iter().all(|t| !t.accessible_checked().unwrap_or(false)), "a custom colour is no tile");
        press(&op, "ItemEditDialog::save");
        assert_eq!(item(&app, id).unwrap().background, Some(Background::Color { color: "#12ab34".into() }));
    }

    #[test]
    fn upload_imports_every_chosen_file() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "A".into(), body: "a".into() });
        let op = opened(&app);
        open(&op, id);
        let dir = std::env::temp_dir().join(format!("rp-upload-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut paths = Vec::new();
        for (n, px) in [("one.png", 1u8), ("two.png", 2)] {
            let p = dir.join(n);
            image::RgbaImage::from_pixel(4, 4, image::Rgba([px, 0, 0, 255])).save(&p).unwrap();
            paths.push(p);
        }
        crate::ui::operator::files::testing::will_pick(paths);
        settle();
        let tiles = find_all(&op, "BackgroundPicker::choice");
        tiles.last().unwrap().invoke_accessible_default_action();
        settle();
        let names: Vec<String> = app.data.lock().unwrap().media.iter().map(|m| m.name.clone()).collect();
        assert_eq!(names, ["one", "two"], "the core names media without the extension");
        assert_eq!(toasts(&op).last(), Some(&("Added to the media library — pick it here".to_string(), false)));
        let tiles = find_all(&op, "BackgroundPicker::choice");
        assert_eq!(tiles.len(), 11, "both pictures are pickable at once");
        assert_eq!(tiles[8].accessible_label().unwrap(), "one");
        tiles[9].invoke_accessible_default_action();
        settle();
        press(&op, "ItemEditDialog::save");
        let asset = app.data.lock().unwrap().media[1].asset.clone();
        assert_eq!(item(&app, id).unwrap().background, Some(Background::Image { asset }));
        assert_eq!(op.dialog(), None);
    }

    #[test]
    fn rename_media_then_update() {
        let app = test_app();
        let m = media(&app, "clip.png", MediaKind::Image);
        let id = push(&app, ItemKind::Media { media_id: m, autoplay: true, end: EndAction::Stop, looped: false });
        let op = opened(&app);
        open(&op, id);
        assert_eq!(g(&op).get_name(), "clip.png");
        type_into(&op, "ItemEditDialog::name-box", "  Sunrise  ");
        type_into(&op, "ItemEditDialog::notes-box", "Hold here");
        press(&op, "ItemEditDialog::save");
        assert_eq!(app.data.lock().unwrap().media[0].name, "Sunrise", "renamed, trimmed");
        assert_eq!(item(&app, id).unwrap().notes, "Hold here", "then updated");
        assert_eq!(op.dialog(), None);

        // A blank name keeps the old one; the item is still saved.
        refresh(&op);
        open(&op, id);
        type_into(&op, "ItemEditDialog::name-box", "   ");
        press(&op, "ItemEditDialog::save");
        assert_eq!(app.data.lock().unwrap().media[0].name, "Sunrise");

        // Slides: "Name" renames the deck.
        let deck = crate::model::Deck { id: Uuid::new_v4(), title: "Announcements".into(), slides: vec![] };
        app.data.lock().unwrap().decks.push(deck.clone());
        let id = push(&app, ItemKind::Deck { deck_id: deck.id });
        refresh(&op);
        open(&op, id);
        type_into(&op, "ItemEditDialog::name-box", "Notices");
        press(&op, "ItemEditDialog::save");
        assert_eq!(app.data.lock().unwrap().decks[0].title, "Notices");
    }

    #[test]
    fn item_edit_keeps_typed_text_across_a_data_update() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "Welcome".into(), body: "Hi".into() });
        let op = opened(&app);
        open(&op, id);
        type_into(&op, "ItemEditDialog::title-box", "Typed title");
        type_into(&op, "ItemEditDialog::notes-box", "Typed notes");
        // A phone adds an item meanwhile: a new read arrives.
        let other = push(&app, ItemKind::Text { title: "From phone".into(), body: "x".into() });
        refresh(&op);
        settle();
        assert_eq!(g(&op).get_title(), "Typed title");
        assert_eq!(g(&op).get_notes(), "Typed notes");
        assert_eq!(op.dialog(), Some(DialogKind::ItemEdit(id)));
        press(&op, "ItemEditDialog::save");
        let ItemKind::Text { title, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!(title, "Typed title");
        assert!(item(&app, other).is_some(), "the other change stays");
    }

    #[test]
    fn save_on_removed_item_shows_that_item_is_gone_and_stays_open() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "Welcome".into(), body: "Hi".into() });
        let op = opened(&app);
        open(&op, id);
        app.data.lock().unwrap().services[0].items.retain(|i| i.id != id);
        press(&op, "ItemEditDialog::save");
        assert_eq!(toasts(&op).last(), Some(&("That item is gone.".to_string(), true)));
        assert_eq!(op.dialog(), Some(DialogKind::ItemEdit(id)), "the dialog stays open");
        assert!(op.ui.get_dialog_open());
    }

    #[test]
    fn remove_from_service_closes() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "Welcome".into(), body: "Hi".into() });
        let op = opened(&app);
        open(&op, id);
        press(&op, "ItemEditDialog::remove");
        assert!(item(&app, id).is_none());
        assert_eq!(op.dialog(), None);
        assert!(toasts(&op).is_empty(), "no toast, as on the web");
    }

    #[test]
    fn cancel_closes_without_saving() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "Welcome".into(), body: "Hi".into() });
        let op = opened(&app);
        open(&op, id);
        type_into(&op, "ItemEditDialog::title-box", "Changed");
        press(&op, "ItemEditDialog::cancel");
        assert_eq!(op.dialog(), None);
        let ItemKind::Text { title, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!(title, "Welcome");
    }

    #[test]
    fn song_item_offers_edit_song_or_says_deleted() {
        let app = test_app();
        let song = Song { id: Uuid::new_v4(), title: "Amazing Grace".into(), writers: String::new(), ccli: String::new(), copyright: String::new(), lyrics: "Verse 1\nAmazing grace".into(), arrangement: vec![] };
        app.data.lock().unwrap().songs.push(song.clone());
        let id = push(&app, ItemKind::Song { song_id: song.id });
        let gone = push(&app, ItemKind::Song { song_id: Uuid::new_v4() });
        let op = opened(&app);
        open(&op, gone);
        settle();
        assert!(!g(&op).get_song_exists());
        assert!(find(&op, "ItemEditDialog::edit-song").is_none());
        open(&op, id);
        settle();
        assert!(g(&op).get_song_exists());
        press(&op, "ItemEditDialog::edit-song");
        assert_eq!(op.dialog(), Some(DialogKind::SongEditor { song_id: Some(song.id), add_after: false }));
    }

    #[test]
    fn not_ready_media_shows_its_status() {
        let app = test_app();
        let m = media(&app, "odd.mkv", MediaKind::Video);
        app.data.lock().unwrap().media[0].status = MediaStatus::Failed { reason: "Broken file".into() };
        let id = push(&app, ItemKind::Media { media_id: m, autoplay: true, end: EndAction::Stop, looped: false });
        let op = opened(&app);
        open(&op, id);
        assert!(g(&op).get_not_ready());
        assert_eq!((g(&op).get_status_text().as_str(), g(&op).get_status_state().as_str(), g(&op).get_status_reason().as_str()), ("Failed", "bad", "Broken file"));
    }
}
