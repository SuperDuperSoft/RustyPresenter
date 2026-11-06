//! The Bible box in the top bar (`operator.js:93-102, 324-340`): type a reference and press Enter to add the
//! passage after the live item and arm it, so the next Enter outside the box shows it. The translation chips pick
//! the translation, saved in `Settings.translation` (product decision 3).
use super::{strings as tr, view, window::Operator};
use serde::Deserialize;
use serde_json::{Value, json};
use slint::Model;
use std::{rc::{Rc, Weak}, sync::Arc};
use uuid::Uuid;

/// `bible/lookup`'s answer. The verses pass through to `service/add` untouched.
#[derive(Deserialize)]
struct Found { label: String, verses: Value, translation: String }

#[derive(Deserialize)]
struct Added { id: Uuid }

/// Enter in the Bible box (`operator.js:325-336`): look the passage up, add it after the live item (or at the end),
/// clear the box, give the keys back, and select and arm the new item. A failure is an error toast with the core's
/// text, and the box keeps what was typed. Nothing happens while a dialog or question is open over the box.
pub fn submit(op: &Rc<Operator>, reference: String) {
    let reference = reference.trim().to_string();
    if reference.is_empty() || op.covered() { return }
    let translation = op.state.borrow().snap.as_ref().map(|s| s.translation.clone()).unwrap_or_else(|| "KJV".into());
    let me = Rc::downgrade(op);
    op.ops.call::<Found>("bible/lookup", json!({ "reference": reference, "translation": translation }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(found) => add(&op, found),
            Err(e) => op.toast(&e, true),
        }
    });
}

/// The second half of `submit`: the passage goes in with the translation the core returned, not the chip state
/// (`operator.js:330`).
fn add(op: &Rc<Operator>, found: Found) {
    let live_item = op.live_item();
    let position = op.state.borrow().snap.as_ref().and_then(|s| view::insert_position(&s.items, live_item));
    let Found { label, verses, translation } = found;
    let item = json!({ "kind": "scripture", "reference": label, "translation": translation, "verses": verses, "per_slide": 0 });
    let me = Rc::downgrade(op);
    op.ops.call::<Added>("service/add", json!({ "position": position, "item": item }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(Added { id }) => {
                op.ui.set_bible_text("".into());
                // Only the box gives up the keyboard (the web's `input.blur()`): a dialog or question opened while
                // the lookup ran keeps it.
                if !op.covered() { op.ui.invoke_focus_keys() }
                {
                    let mut st = op.state.borrow_mut();
                    st.selected = Some(id);
                    st.armed = Some(id);
                }
                // The core's `Data` update may already have been applied with the old selection.
                op.mark_service();
                op.render_slides();
                op.toast(&tr::bible_added(&label), false);
            }
            Err(e) => op.toast(&e, true),
        }
    });
}

/// A translation chip, or Alt+1…6 in the box. The chip (and the compact box's button) turns on at once and the
/// choice is saved through
/// `settings/translation` (product decision 3). A translation that is not connected is still picked, as in the
/// web; its lookup then fails with the core's message. The chip's click refocuses the box in `topbar.slint`.
pub fn pick(op: &Rc<Operator>, index: usize) {
    let name = {
        let mut st = op.state.borrow_mut();
        let Some(snap) = st.snap.as_mut() else { return };
        let Some(name) = snap.translations.get(index).cloned() else { return };
        Arc::make_mut(snap).translation = name.clone();
        name
    };
    let chips = op.ui.get_translations();
    for i in 0..chips.row_count() {
        let Some(mut chip) = chips.row_data(i) else { continue };
        let on = chip.name == name.as_str();
        if chip.on != on { chip.on = on; chips.set_row_data(i, chip); }
    }
    op.ui.set_translation(name.as_str().into());
    let me = Rc::downgrade(op);
    op.ops.call::<Value>("settings/translation", json!({ "translation": name }), move |r| {
        if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(&e, true) }
    });
}

/// Connects the box and its chips. The callbacks hold a `Weak`, so the window does not keep itself alive.
pub(super) fn wire(op: &Rc<Operator>) {
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_bible_submit(move |text| if let Some(op) = me.upgrade() { submit(&op, text.into()) });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_pick_translation(move |i| if let (Some(op), Ok(i)) = (me.upgrade(), usize::try_from(i)) { pick(&op, i) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, LiveUpdate, test_app},
        live::Command,
        model::{ItemKind, ServiceItem},
        ui::operator::window::testing::{find, find_all, open, refresh},
    };
    use slint::{SharedString, platform::{Key, PointerEventButton, WindowEvent}};
    use std::sync::Arc;
    use uuid::Uuid;

    fn add_text(app: &App, title: &str) -> Uuid {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: title.into(), body: "x".into() }, background: None, notes: String::new() };
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

    fn show(op: &Operator, item_id: Uuid) {
        op.ops().cmd(Command::Show { item_id, slide: 0 }, None, |o| assert!(o.ok, "{o:?}"));
        live(op);
    }

    fn ids(app: &App) -> Vec<Uuid> { app.data.lock().unwrap().services[0].items.iter().map(|i| i.id).collect() }

    fn cursor(app: &App) -> Option<(Uuid, usize)> { app.live_view().cursor.map(|c| (c.item_id, c.slide)) }

    fn press(op: &Operator, text: impl Into<SharedString>) {
        let text = text.into();
        op.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        op.window().dispatch_event(WindowEvent::KeyReleased { text });
    }

    /// Clicks into the box, types `reference` and presses Enter, as the operator would.
    fn type_and_enter(op: &Operator, reference: &str) {
        find(op, "TopBar::bible-input").expect("Bible input").mock_single_click(PointerEventButton::Left);
        for c in reference.chars() { press(op, c) }
        press(op, char::from(Key::Return));
    }

    fn last_toast(op: &Operator) -> Option<(String, bool)> { op.state.borrow().toasts.last().cloned() }

    fn chip_index(op: &Operator, name: &str) -> usize {
        op.state.borrow().snap.as_ref().unwrap().translations.iter().position(|t| t == name).expect("translation listed")
    }

    #[test]
    fn bible_enter_inserts_after_live_item_and_arms_it() {
        let app = test_app();
        let (a, b) = (add_text(&app, "A"), add_text(&app, "B"));
        let op = opened(&app);
        show(&op, a);
        type_and_enter(&op, "John 3:16");
        let now = ids(&app);
        assert_eq!(now.len(), 3);
        assert_eq!((now[0], now[2]), (a, b), "the passage goes right after the live item");
        let new = now[1];
        let st = op.state.borrow();
        assert_eq!((st.selected, st.armed), (Some(new), Some(new)));
        drop(st);
        assert_eq!(op.ui.get_bible_text(), "", "the box is cleared");
        assert_eq!(last_toast(&op), Some(("John 3:16 added — press Enter to show it".into(), false)));
    }

    #[test]
    fn bible_enter_appends_when_nothing_live() {
        let app = test_app();
        let a = add_text(&app, "A");
        let op = opened(&app);
        type_and_enter(&op, "John 3:16");
        let now = ids(&app);
        assert_eq!(now.len(), 2);
        assert_eq!(now[0], a);
        assert_eq!(op.state.borrow().armed, Some(now[1]));
    }

    #[test]
    fn bible_item_uses_translation_from_lookup() {
        let app = test_app();
        let op = opened(&app);
        pick(&op, chip_index(&op, "WEB"));
        type_and_enter(&op, "jn 3:16");
        let data = app.data.lock().unwrap();
        match &data.services[0].items[0].kind {
            ItemKind::Scripture { reference, translation, verses, .. } => {
                assert_eq!(translation, "WEB");
                assert_eq!(reference, "John 3:16", "the item carries the label the lookup returned");
                assert_eq!(verses.len(), 1);
            }
            other => panic!("not scripture: {other:?}"),
        }
    }

    #[test]
    fn second_enter_takes_armed_passage_live() {
        let app = test_app();
        let a = add_text(&app, "A");
        let op = opened(&app);
        show(&op, a);
        type_and_enter(&op, "Psalm 23:1-2");
        let new = ids(&app)[1];
        refresh(&op);
        assert_eq!(cursor(&app), Some((a, 0)), "the first Enter only adds and arms");
        press(&op, char::from(Key::Return));
        assert_eq!(cursor(&app), Some((new, 0)));
        assert_eq!(op.state.borrow().armed, None);
    }

    #[test]
    fn next_key_clears_the_arm() {
        let app = test_app();
        let op = opened(&app);
        type_and_enter(&op, "John 3:16");
        refresh(&op);
        assert!(op.state.borrow().armed.is_some());
        press(&op, char::from(Key::RightArrow));
        assert_eq!(op.state.borrow().armed, None);
    }

    #[test]
    fn unknown_book_shows_core_error_and_keeps_text() {
        let app = test_app();
        let op = opened(&app);
        type_and_enter(&op, "Hezekiah 1:1");
        assert_eq!(last_toast(&op), Some(("I don't recognize the book \"Hezekiah\".".into(), true)));
        assert_eq!(op.ui.get_bible_text(), "Hezekiah 1:1");
        assert!(ids(&app).is_empty());
        assert_eq!(op.state.borrow().armed, None);
    }

    #[test]
    fn unavailable_translation_errors_with_core_message() {
        let app = test_app();
        let op = opened(&app);
        let core = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
            .block_on(app.do_action("bible/lookup", br#"{"reference":"John 3:16","translation":"NIV"}"#)).unwrap_err().message;
        pick(&op, chip_index(&op, "NIV"));
        type_and_enter(&op, "John 3:16");
        assert_eq!(last_toast(&op), Some((core, true)));
        assert_eq!(op.ui.get_bible_text(), "John 3:16");
        assert!(ids(&app).is_empty());
    }

    #[test]
    fn available_chip_tooltip_is_its_name() {
        let app = test_app();
        let op = opened(&app);
        // Wide enough for every translation's button (narrower, they collapse: `ui::operator::fit`).
        op.window().set_size(slint::LogicalSize::new(1440.0, 900.0));
        slint::platform::update_timers_and_animations();
        let chips = find_all(&op, "TopBar::translation-chip");
        let tip = |name: &str| chips.iter().find(|c| c.accessible_label().unwrap_or_default() == name)
            .and_then(|c| c.accessible_description()).unwrap_or_default().to_string();
        assert_eq!(tip("KJV"), "KJV");
        assert_eq!(tip("NIV"), "NIV isn't connected yet (Settings → Bible). You can still paste its verses with Add → Bible.");
    }

    #[test]
    fn translation_choice_survives_restart() {
        let app = test_app();
        let op = opened(&app);
        pick(&op, chip_index(&op, "NIV"));
        let on = |op: &Operator| slint::Model::iter(&op.ui.get_translations()).filter(|c| c.on).map(|c| c.name.to_string()).collect::<Vec<_>>();
        assert_eq!(on(&op), ["NIV"], "the chip turns on at once");
        assert_eq!(app.data.lock().unwrap().settings.translation, "NIV");
        drop(op);
        let again = opened(&app);
        assert_eq!(on(&again), ["NIV"]);
    }

    #[test]
    fn chip_click_picks_and_refocuses_the_box() {
        let app = test_app();
        let op = opened(&app);
        // The first pointer event lays the chips out; a position read before it can be stale.
        op.window().dispatch_event(WindowEvent::PointerMoved { position: slint::LogicalPosition::new(0.0, 0.0) });
        let chips = find_all(&op, "TopBar::translation-chip");
        let web = chips.iter().find(|c| c.accessible_label().unwrap_or_default() == "WEB").expect("WEB chip");
        web.mock_single_click(PointerEventButton::Left);
        assert_eq!(app.data.lock().unwrap().settings.translation, "WEB");
        press(&op, "J");
        assert_eq!(op.ui.get_bible_text(), "J", "typing goes to the Bible box");
    }

    #[test]
    fn empty_input_does_nothing() {
        let app = test_app();
        let op = opened(&app);
        submit(&op, "   ".into());
        assert!(ids(&app).is_empty());
        assert!(op.state.borrow().toasts.is_empty());
    }
}
