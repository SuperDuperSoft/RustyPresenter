//! Operator keyboard shortcuts (`operator.js:305-320`): which key does what, as pure functions, and the glue that
//! runs it for the window's key scope (`keys` in `ui/operator.slint`).
use super::window::Operator;
use crate::live::Command;
use slint::platform::Key;
use std::rc::{Rc, Weak};

#[derive(Clone, Debug, PartialEq)]
pub enum KeyAction {
    Clear(Command),
    FocusBible,
    Next,
    Prev,
    TakeArmedOrSelected,
    Blur,
    PickTranslation(usize),
    None,
}

/// Where the keyboard focus is. `dialog_open` is also true while a confirm or prompt overlay is up with no dialog
/// under it, because the web's `confirm()` and `prompt()` block every key too.
#[derive(Clone, Copy, Debug)]
pub struct KeyCtx {
    pub in_text_field: bool,
    pub in_bible_box: bool,
    pub dialog_open: bool,
}

/// The web name of a special key, from the private-use character Slint delivers for it. `None` means the text is
/// ordinary typing: route it as itself, with `key_name(text).unwrap_or(text)`.
pub fn key_name(text: &str) -> Option<&'static str> {
    let mut chars = text.chars();
    let (c, None) = (chars.next()?, chars.next()) else { return None };
    [
        (Key::RightArrow, "ArrowRight"),
        (Key::LeftArrow, "ArrowLeft"),
        (Key::DownArrow, "ArrowDown"),
        (Key::UpArrow, "ArrowUp"),
        (Key::PageDown, "PageDown"),
        (Key::PageUp, "PageUp"),
        (Key::Return, "Enter"),
        (Key::Escape, "Escape"),
        (Key::Space, " "),
        (Key::F1, "F1"),
        (Key::F2, "F2"),
        (Key::F3, "F3"),
        (Key::F4, "F4"),
        (Key::F5, "F5"),
    ]
    .into_iter()
    .find_map(|(key, name)| (char::from(key) == c).then_some(name))
}

/// What a key press does. The order of these checks is the behaviour; it follows the web handler, with the
/// Alt+digit translation shortcut added inside the Bible box.
pub fn route(key: &str, ctrl_or_cmd: bool, alt: bool, ctx: KeyCtx) -> KeyAction {
    let clear = match key {
        "F1" => Some(Command::ClearAll),
        "F2" => Some(Command::ClearSlide),
        "F3" => Some(Command::ClearMedia),
        "F4" => Some(Command::ClearAudio),
        "F5" => Some(Command::Logo),
        _ => None,
    };
    if let Some(command) = clear {
        return KeyAction::Clear(command);
    }
    if ctrl_or_cmd && key.eq_ignore_ascii_case("b") {
        return if ctx.dialog_open { KeyAction::None } else { KeyAction::FocusBible };
    }
    if ctx.dialog_open {
        return KeyAction::None;
    }
    if alt && ctx.in_bible_box && matches!(key.as_bytes(), [b'1'..=b'6']) {
        return KeyAction::PickTranslation(usize::from(key.as_bytes()[0] - b'1'));
    }
    if ctx.in_text_field || ctx.in_bible_box {
        return if key == "Escape" { KeyAction::Blur } else { KeyAction::None };
    }
    match key {
        "ArrowRight" | "ArrowDown" | "PageDown" | " " => KeyAction::Next,
        "ArrowLeft" | "ArrowUp" | "PageUp" => KeyAction::Prev,
        "Enter" => KeyAction::TakeArmedOrSelected,
        _ => KeyAction::None,
    }
}

impl Operator {
    /// One key press from the window's key scope; true when it was used (the scope then accepts it).
    fn key(self: &Rc<Self>, text: &str, ctrl_or_cmd: bool, alt: bool, in_text_field: bool, in_bible_box: bool) -> bool {
        let dialog_open = { let st = self.state.borrow(); st.dialog.is_some() || st.asks > 0 };
        let ctx = KeyCtx { in_text_field, in_bible_box, dialog_open };
        match route(key_name(text).unwrap_or(text), ctrl_or_cmd, alt, ctx) {
            KeyAction::Clear(cmd) => self.command(cmd, None),
            KeyAction::FocusBible => self.ui.invoke_focus_bible(),
            // Only the keys drop the Bible arm; the Back/Next buttons keep it (`operator.js:314-315, 343-344`).
            action @ (KeyAction::Next | KeyAction::Prev) => {
                self.state.borrow_mut().armed = None;
                self.step(action == KeyAction::Next);
            }
            KeyAction::TakeArmedOrSelected => self.take_armed_or_selected(),
            KeyAction::Blur => self.ui.invoke_focus_keys(),
            KeyAction::PickTranslation(i) => self.ui.invoke_pick_translation(i as i32),
            KeyAction::None => return false,
        }
        true
    }

    /// Enter: the armed Bible passage, else the selected item, goes live at its first slide. The arm is used up
    /// even when its item is gone (`operator.js:316-320`).
    fn take_armed_or_selected(self: &Rc<Self>) {
        let target = {
            let mut st = self.state.borrow_mut();
            let id = st.armed.take().or(st.selected);
            id.filter(|id| st.snap.as_ref().is_some_and(|s| s.items.iter().any(|i| i.id == *id && !i.slides.is_empty())))
        };
        if let Some(item_id) = target { self.command(Command::Show { item_id, slide: 0 }, None) }
    }
}

/// Connects the window's key scope. The callback holds a `Weak`, so the window does not keep itself alive.
pub(super) fn wire(op: &Rc<Operator>) {
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_key(move |text, ctrl, alt, in_text, in_bible| me.upgrade().is_some_and(|op| op.key(&text, ctrl, alt, in_text, in_bible)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::platform::Key;

    const PLAIN: KeyCtx = KeyCtx { in_text_field: false, in_bible_box: false, dialog_open: false };
    const TYPING: KeyCtx = KeyCtx { in_text_field: true, in_bible_box: false, dialog_open: false };
    const BIBLE: KeyCtx = KeyCtx { in_text_field: true, in_bible_box: true, dialog_open: false };
    const DIALOG: KeyCtx = KeyCtx { in_text_field: false, in_bible_box: false, dialog_open: true };

    #[test]
    fn f_keys_clear_even_while_typing_or_in_a_dialog() {
        let ctx = KeyCtx { in_text_field: true, in_bible_box: true, dialog_open: true };
        assert_eq!(route("F1", false, false, ctx), KeyAction::Clear(Command::ClearAll));
        assert_eq!(route("F2", false, false, ctx), KeyAction::Clear(Command::ClearSlide));
        assert_eq!(route("F3", false, false, ctx), KeyAction::Clear(Command::ClearMedia));
        assert_eq!(route("F4", false, false, ctx), KeyAction::Clear(Command::ClearAudio));
        assert_eq!(route("F5", false, false, ctx), KeyAction::Clear(Command::Logo));
    }

    #[test]
    fn ctrl_b_focuses_the_bible_box_from_anywhere_but_a_dialog() {
        assert_eq!(route("b", true, false, PLAIN), KeyAction::FocusBible);
        assert_eq!(route("b", true, false, TYPING), KeyAction::FocusBible);
        assert_eq!(route("B", true, false, PLAIN), KeyAction::FocusBible, "Caps Lock or Shift, like the web's toLowerCase");
    }

    #[test]
    fn ctrl_b_ignored_while_a_dialog_is_open() {
        assert_eq!(route("b", true, false, DIALOG), KeyAction::None);
    }

    #[test]
    fn plain_b_is_not_a_shortcut() {
        assert_eq!(route("b", false, false, PLAIN), KeyAction::None);
    }

    #[test]
    fn a_dialog_blocks_every_other_key() {
        for key in ["ArrowRight", "ArrowLeft", " ", "PageDown", "Enter", "Escape"] {
            assert_eq!(route(key, false, false, DIALOG), KeyAction::None, "{key:?}");
        }
    }

    #[test]
    fn alt_digit_picks_translation_only_in_bible_box() {
        assert_eq!(route("1", false, true, BIBLE), KeyAction::PickTranslation(0));
        assert_eq!(route("6", false, true, BIBLE), KeyAction::PickTranslation(5));
        assert_eq!(route("7", false, true, BIBLE), KeyAction::None);
        assert_eq!(route("0", false, true, BIBLE), KeyAction::None);
        assert_eq!(route("2", false, false, BIBLE), KeyAction::None, "plain digits are typed");
        assert_eq!(route("2", false, true, TYPING), KeyAction::None);
        assert_eq!(route("2", false, true, PLAIN), KeyAction::None);
        let dialog_over_box = KeyCtx { dialog_open: true, ..BIBLE };
        assert_eq!(route("2", false, true, dialog_over_box), KeyAction::None);
    }

    #[test]
    fn esc_blurs_text_field() {
        assert_eq!(route("Escape", false, false, TYPING), KeyAction::Blur);
        assert_eq!(route("Escape", false, false, BIBLE), KeyAction::Blur);
        assert_eq!(route("Escape", false, false, PLAIN), KeyAction::None);
    }

    #[test]
    fn arrows_go_to_the_text_field_while_typing() {
        assert_eq!(route("ArrowRight", false, false, TYPING), KeyAction::None);
        assert_eq!(route(" ", false, false, TYPING), KeyAction::None);
        assert_eq!(route("Enter", false, false, TYPING), KeyAction::None);
    }

    #[test]
    fn the_bible_box_counts_as_a_text_field_on_its_own() {
        // Its focus comes from `has-focus`, not `OperatorState`, so the caller may set only `in_bible_box`.
        let ctx = KeyCtx { in_text_field: false, in_bible_box: true, dialog_open: false };
        assert_eq!(route("ArrowRight", false, false, ctx), KeyAction::None);
        assert_eq!(route("Escape", false, false, ctx), KeyAction::Blur);
    }

    #[test]
    fn space_pagedown_down_right_are_next() {
        for key in ["ArrowRight", "ArrowDown", "PageDown", " "] {
            assert_eq!(route(key, false, false, PLAIN), KeyAction::Next, "{key:?}");
        }
    }

    #[test]
    fn pageup_up_left_are_prev() {
        for key in ["ArrowLeft", "ArrowUp", "PageUp"] {
            assert_eq!(route(key, false, false, PLAIN), KeyAction::Prev, "{key:?}");
        }
    }

    #[test]
    fn enter_takes_armed_or_selected() {
        assert_eq!(route("Enter", false, false, PLAIN), KeyAction::TakeArmedOrSelected);
    }

    #[test]
    fn other_keys_do_nothing() {
        assert_eq!(route("x", false, false, PLAIN), KeyAction::None);
        assert_eq!(route("F6", false, false, PLAIN), KeyAction::None);
    }

    #[test]
    fn slint_key_characters_map_to_route_names() {
        let name = |k: Key| key_name(&String::from(char::from(k)));
        assert_eq!(name(Key::RightArrow), Some("ArrowRight"));
        assert_eq!(name(Key::LeftArrow), Some("ArrowLeft"));
        assert_eq!(name(Key::UpArrow), Some("ArrowUp"));
        assert_eq!(name(Key::DownArrow), Some("ArrowDown"));
        assert_eq!(name(Key::PageUp), Some("PageUp"));
        assert_eq!(name(Key::PageDown), Some("PageDown"));
        assert_eq!(name(Key::Return), Some("Enter"));
        assert_eq!(name(Key::Escape), Some("Escape"));
        assert_eq!(name(Key::Space), Some(" "));
        assert_eq!(name(Key::F1), Some("F1"));
        assert_eq!(name(Key::F2), Some("F2"));
        assert_eq!(name(Key::F3), Some("F3"));
        assert_eq!(name(Key::F4), Some("F4"));
        assert_eq!(name(Key::F5), Some("F5"));
    }

    #[test]
    fn ordinary_text_has_no_special_name() {
        // The caller routes `key_name(text).unwrap_or(text)`, so typed text passes through as itself.
        assert_eq!(key_name("b"), None);
        assert_eq!(key_name("1"), None);
        assert_eq!(key_name(""), None);
        assert_eq!(key_name(&String::from(char::from(Key::Home))), None);
    }
}

/// The key scope in a real window: key events on the testing backend, actions run inline.
#[cfg(test)]
mod window_tests {
    use crate::{
        app::{App, LiveUpdate, test_app},
        live::Command,
        model::{ItemKind, ServiceItem},
        ui::operator::window::{DialogKind, Operator, testing::{find, open, refresh}},
    };
    use slint::{ComponentHandle, SharedString, platform::{Key, PointerEventButton, WindowEvent}};
    use std::{cell::Cell, rc::Rc, sync::Arc};
    use uuid::Uuid;

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

    /// Hands the operator the core's live view (no follower in tests).
    fn live(op: &Operator) { op.apply_live(LiveUpdate::new(op.app().live_view())); }

    fn show(op: &Operator, item_id: Uuid, slide: usize) {
        op.ops().cmd(Command::Show { item_id, slide }, None, |o| assert!(o.ok, "{o:?}"));
        live(op);
    }

    fn cursor(app: &App) -> Option<(Uuid, usize)> { app.live_view().cursor.map(|c| (c.item_id, c.slide)) }

    fn press(op: &Operator, text: impl Into<SharedString>) {
        let text = text.into();
        op.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        op.window().dispatch_event(WindowEvent::KeyReleased { text });
    }

    fn key(op: &Operator, k: Key) { press(op, char::from(k)) }

    /// `text` while `modifier` is held down.
    fn chord(op: &Operator, modifier: Key, text: &str) {
        op.window().dispatch_event(WindowEvent::KeyPressed { text: char::from(modifier).into() });
        press(op, text);
        op.window().dispatch_event(WindowEvent::KeyReleased { text: char::from(modifier).into() });
    }

    fn type_text(op: &Operator, text: &str) { for c in text.chars() { press(op, c) } }

    fn click_bible_box(op: &Operator) {
        find(op, "TopBar::bible-input").expect("Bible input").mock_single_click(PointerEventButton::Left);
    }

    #[test]
    fn arrow_keys_step_from_the_start() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2");
        let op = opened(&app);
        show(&op, id, 0);
        key(&op, Key::RightArrow);
        assert_eq!(cursor(&app), Some((id, 1)), "the window's key scope has focus from the start");
        live(&op);
        key(&op, Key::PageUp);
        assert_eq!(cursor(&app), Some((id, 0)));
    }

    #[test]
    fn f1_in_bible_box_clears_and_types_nothing() {
        let app = test_app();
        let id = add_text(&app, "A", "a1");
        let op = opened(&app);
        show(&op, id, 0);
        click_bible_box(&op);
        type_text(&op, "John");
        key(&op, Key::F1);
        assert!(app.live_view().frame.slide.is_none(), "F1 cleared the screen");
        assert_eq!(op.ui.get_bible_text(), "John");
    }

    #[test]
    fn arrows_in_bible_box_go_to_the_box() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2");
        let op = opened(&app);
        show(&op, id, 0);
        click_bible_box(&op);
        type_text(&op, "Jn");
        key(&op, Key::LeftArrow);
        key(&op, Key::RightArrow);
        type_text(&op, " ");
        assert_eq!(cursor(&app), Some((id, 0)), "the slide did not move");
        assert_eq!(op.ui.get_bible_text(), "Jn ");
    }

    #[test]
    fn alt_2_in_bible_box_picks_translation_and_types_nothing() {
        let app = test_app();
        let op = opened(&app);
        let picked = Rc::new(Cell::new(None));
        let p = picked.clone();
        op.ui.on_pick_translation(move |i| p.set(Some(i)));
        click_bible_box(&op);
        type_text(&op, "Ps 23");
        chord(&op, Key::Alt, "2");
        assert_eq!(picked.get(), Some(1));
        assert_eq!(op.ui.get_bible_text(), "Ps 23");
    }

    #[test]
    fn alt_2_outside_the_bible_box_picks_nothing() {
        let app = test_app();
        let op = opened(&app);
        let picked = Rc::new(Cell::new(None));
        let p = picked.clone();
        op.ui.on_pick_translation(move |i| p.set(Some(i)));
        chord(&op, Key::Alt, "2");
        assert_eq!(picked.get(), None);
    }

    #[test]
    fn ctrl_b_focuses_and_selects_bible_text() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2");
        let op = opened(&app);
        show(&op, id, 0);
        click_bible_box(&op);
        type_text(&op, "John");
        key(&op, Key::Escape);
        key(&op, Key::RightArrow);
        assert_eq!(cursor(&app), Some((id, 1)), "Esc left the box, so the arrow is Next again");
        chord(&op, Key::Control, "b");
        type_text(&op, "Mark");
        assert_eq!(op.ui.get_bible_text(), "Mark", "the whole text was selected, so typing replaced it");
        assert_eq!(cursor(&app), Some((id, 1)));
    }

    #[test]
    fn ctrl_b_with_dialog_open_leaves_bible_box_alone() {
        let app = test_app();
        let op = opened(&app);
        op.state.borrow_mut().dialog = Some(DialogKind::Settings);
        chord(&op, Key::Control, "b");
        type_text(&op, "x");
        assert_eq!(op.ui.get_bible_text(), "", "the Bible box did not take focus");
    }

    #[test]
    fn two_nexts_from_same_base_advance_once() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2\n\na3");
        let op = opened(&app);
        show(&op, id, 0);
        let version = app.live_view().version;
        // Both presses come before the first live update reaches the window, so both carry the same base.
        key(&op, Key::RightArrow);
        key(&op, Key::RightArrow);
        assert_eq!(app.live_view().version, version + 1);
        assert_eq!(cursor(&app), Some((id, 1)), "the room advanced once");
        assert!(op.state.borrow().toasts.is_empty(), "the stale second Next is silent: {:?}", op.state.borrow().toasts);
    }

    #[test]
    fn right_arrow_with_dialog_open_does_nothing() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2");
        let op = opened(&app);
        show(&op, id, 0);
        op.state.borrow_mut().dialog = Some(DialogKind::Add);
        for k in [Key::RightArrow, Key::Space, Key::Return] { key(&op, k) }
        assert_eq!(cursor(&app), Some((id, 0)));
        // The F-keys still work.
        key(&op, Key::F1);
        assert!(app.live_view().frame.slide.is_none());
    }

    #[test]
    fn right_arrow_with_a_question_open_does_nothing() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2");
        let op = opened(&app);
        show(&op, id, 0);
        op.state.borrow_mut().asks = 1;
        key(&op, Key::RightArrow);
        assert_eq!(cursor(&app), Some((id, 0)), "a confirm or prompt with no dialog under it blocks the keys too");
        op.state.borrow_mut().asks = 0;
        key(&op, Key::RightArrow);
        assert_eq!(cursor(&app), Some((id, 1)));
    }

    #[test]
    fn enter_shows_the_armed_item_then_the_selected_one() {
        let app = test_app();
        let selected = add_text(&app, "A", "a1");
        let armed = add_text(&app, "B", "b1");
        let op = opened(&app);
        {
            let mut st = op.state.borrow_mut();
            st.selected = Some(selected);
            st.armed = Some(armed);
        }
        key(&op, Key::Return);
        assert_eq!(cursor(&app), Some((armed, 0)));
        assert_eq!(op.state.borrow().armed, None, "Enter used the arm up");
        live(&op);
        key(&op, Key::Return);
        assert_eq!(cursor(&app), Some((selected, 0)));
    }

    #[test]
    fn enter_on_an_armed_item_that_is_gone_does_nothing() {
        let app = test_app();
        let selected = add_text(&app, "A", "a1");
        let op = opened(&app);
        {
            let mut st = op.state.borrow_mut();
            st.selected = Some(selected);
            st.armed = Some(Uuid::new_v4());
        }
        key(&op, Key::Return);
        assert_eq!(cursor(&app), None, "like the web, a missing armed item does not fall back to the selection");
        assert_eq!(op.state.borrow().armed, None);
    }

    #[test]
    fn next_and_prev_keys_clear_the_arm() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2");
        let op = opened(&app);
        show(&op, id, 0);
        op.state.borrow_mut().armed = Some(id);
        key(&op, Key::DownArrow);
        assert_eq!(op.state.borrow().armed, None);
        live(&op);
        op.state.borrow_mut().armed = Some(id);
        key(&op, Key::UpArrow);
        assert_eq!(op.state.borrow().armed, None);
        assert_eq!(cursor(&app), Some((id, 0)));
    }

    #[test]
    fn clicking_a_button_returns_focus_to_the_keys() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2\n\na3");
        let op = opened(&app);
        show(&op, id, 0);
        click_bible_box(&op);
        find(&op, "LiveColumn::next-btn").unwrap().mock_single_click(PointerEventButton::Left);
        live(&op);
        assert_eq!(cursor(&app), Some((id, 1)));
        key(&op, Key::RightArrow);
        assert_eq!(cursor(&app), Some((id, 2)), "the Bible box lost focus to the click, as in the web");
        assert!(!op.ui.global::<crate::ui::OperatorState>().get_focus_in_text());
    }
}
