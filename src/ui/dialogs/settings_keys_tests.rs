use super::*;
use crate::{
    app::{App, test_app},
    live::Command,
    shortcuts::BUILTIN_GROUP,
    ui::{
        dialogs::{DialogKind, SettingsTab, settings},
        operator::{keys::{KeyAction, KeyCtx, route, route_in}, window::testing::{find_all, open as open_window, refresh}},
    },
};
use slint::{Model, platform::{Key, PointerEventButton, WindowEvent}};
use std::sync::Arc;

const PLAIN: KeyCtx = KeyCtx { in_text_field: false, in_bible_box: false, dialog_open: false };
const TYPING: KeyCtx = KeyCtx { in_text_field: true, in_bible_box: false, dialog_open: false };
const BIBLE: KeyCtx = KeyCtx { in_text_field: true, in_bible_box: true, dialog_open: false };

fn opened(app: &Arc<App>) -> Rc<Operator> {
    let op = open_window(app);
    op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
    slint::platform::update_timers_and_animations();
    refresh(&op);
    op
}

fn settle() { slint::platform::update_timers_and_animations(); }

fn click(op: &Operator, id: &str, n: usize) {
    settle();
    let all = find_all(op, id);
    all.get(n).unwrap_or_else(|| panic!("{id} #{n} of {}", all.len())).mock_single_click(PointerEventButton::Left);
    settle();
}

fn press(op: &Operator, text: &str) {
    op.window().dispatch_event(WindowEvent::KeyPressed { text: text.into() });
    op.window().dispatch_event(WindowEvent::KeyReleased { text: text.into() });
}

fn key(k: Key) -> String { char::from(k).into() }

fn shown(op: &Operator) -> Vec<(String, String)> {
    op.ui.global::<SettingsDlg>().get_help().iter().map(|r| (r.keys.to_string(), r.what.to_string())).collect()
}

/// The Help row of `what`.
fn row_of(op: &Operator, what: &str) -> usize {
    shown(op).iter().position(|(_, w)| w == what).unwrap_or_else(|| panic!("no row {what}"))
}

/// The Change button of row `row` (only shortcut rows have one).
fn change(op: &Operator, row: usize) {
    let ids: Vec<bool> = op.ui.global::<SettingsDlg>().get_help().iter().map(|r| !r.id.is_empty()).collect();
    let n = ids[..row].iter().filter(|b| **b).count();
    click(op, "SettingsHelp::change", n);
}

#[test]
fn help_lists_every_key_with_what_it_does() {
    let rows = help_rows(&KeySettings::default());
    fn s(r: &HelpRow) -> (&str, &str) { (r.keys.as_str(), r.what.as_str()) }
    assert_eq!(s(&rows[0]), ("→  ↓  Space  Page Down", "Next"));
    assert_eq!(s(&rows[1]), ("←  ↑  Page Up", "Back"));
    for (keys, what) in [("F1", "Clear all"), ("F2", "Clear text"), ("F7", "Clear the message"), ("F8", "Clear the timers"),
        ("F9", "Clear group: Everything but the background"), ("Alt + 1–5", "Pick a Bible chip (in the Bible box)"),
        ("Ctrl + Shift + M", "Show a message on the screen"), ("Alt + M", "Find my mouse"), ("Esc", "Leave a text box")] {
        assert!(rows.iter().any(|r| s(r) == (keys, what)), "{keys} {what}");
    }
    // Each shortcut's keys do what its row says, where the shortcut works.
    let map = Keymap::default();
    for row in rows.iter().filter(|r| r.action.is_some()) {
        let action = row.action.unwrap();
        for chord in map.keys(action) {
            let got = [PLAIN, TYPING, BIBLE].iter().map(|c| route_in(chord, *c, &map)).find(|a| *a != KeyAction::None);
            assert!(got.is_some(), "{} does nothing", row.keys);
        }
    }
    // Every key that does something is in Help.
    let mut keys: Vec<(String, bool, bool)> = ["ArrowRight", "ArrowLeft", "ArrowDown", "ArrowUp", "PageDown", "PageUp", "Enter", "Escape", " ",
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "Tab", "Home", "End"]
        .iter().map(|k| (k.to_string(), false, false)).collect();
    for c in ('a'..='z').chain('0'..='9') {
        for (ctrl, alt) in [(false, false), (true, false), (false, true)] { keys.push((c.to_string(), ctrl, alt)) }
    }
    for (key, ctrl, alt) in keys {
        for ctx in [PLAIN, TYPING, BIBLE] {
            let action = route(&key, ctrl, alt, ctx);
            let named = match &action {
                KeyAction::None | KeyAction::Blur => true,
                KeyAction::PickTranslation(_) => alt && ('1'..='5').contains(&key.chars().next().unwrap()),
                KeyAction::JumpSection(_) => ('1'..='9').contains(&key.chars().next().unwrap()) || st::KEY_SECTIONS.to_lowercase().contains(&key),
                _ => map.action_for(&Chord::new(&key, ctrl, alt, false)).is_some_and(|a| rows.iter().any(|r| r.action == Some(a))),
            };
            assert!(named, "{key:?} ctrl={ctrl} alt={alt} does {action:?}, not in Help");
        }
    }
}

#[test]
fn clash_questions_say_what_the_key_does_now() {
    let groups = KeySettings::default().clear_groups;
    let f2 = Chord::parse("F2").unwrap();
    assert_eq!(clash_text(&f2, Action::ClearMessage, &[Clash::Action(Action::ClearSlide)], &groups),
        "F2 is already the key for “Clear text”. It will move here.\n\nUse F2 for “Clear the message”?");
    let c = Chord::parse("C").unwrap();
    assert!(clash_text(&c, Action::Logo, &[Clash::SectionKey], &groups).starts_with("C also jumps the live song"));
}

#[test]
fn change_a_key_then_save_and_the_window_uses_it() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Help);
    settle();
    let row = row_of(&op, "Clear text");
    change(&op, row);
    assert!(capturing());
    assert_eq!(op.ui.global::<KeysDlg>().get_capturing(), row as i32);
    assert_eq!(op.ui.global::<KeysDlg>().get_capture_text(), "Press the new key for “Clear text”");
    // The menu bar lets go of its shortcuts meanwhile, so F11 cannot run anything.
    assert_eq!(op.ui.global::<crate::ui::MenuKeys>().get_clear_all().to_string(), "");
    press(&op, &key(Key::Shift));
    assert!(capturing(), "Shift alone is not a key yet");
    press(&op, &key(Key::F11));
    assert!(!capturing());
    assert_eq!(shown(&op)[row].0, "F11");
    assert!(op.ui.global::<KeysDlg>().get_any_changed());
    assert!(op.ui.global::<SettingsDlg>().get_help().row_data(row).unwrap().changed);
    assert_eq!(op.ui.global::<crate::ui::MenuKeys>().get_clear_all(), slint::Keys::from_parts(["F1"]).unwrap(), "the menu has its keys again");
    // Not saved yet: the window still has F2.
    assert_eq!(op.keymap().action_for(&Chord::parse("F11").unwrap()), None);
    click(&op, "SettingsDialog::save", 0);
    refresh(&op);
    assert_eq!(op.dialog(), None);
    assert_eq!(app.data.lock().unwrap().settings.keys.changed.len(), 1);
    assert_eq!(route_in(&Chord::parse("F11").unwrap(), PLAIN, &op.keymap()), KeyAction::Clear(Command::ClearSlide));
    assert_eq!(route_in(&Chord::parse("F2").unwrap(), PLAIN, &op.keymap()), KeyAction::None);
    assert_eq!(op.ui.global::<crate::ui::MenuKeys>().get_clear_slide(), slint::Keys::from_parts(["F11"]).unwrap());
}

#[test]
fn esc_cancels_and_a_clash_asks_before_moving_the_key() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Help);
    settle();
    let row = row_of(&op, "Clear the message");
    change(&op, row);
    press(&op, &key(Key::Escape));
    assert!(!capturing());
    assert_eq!(op.dialog(), Some(DialogKind::Settings(SettingsTab::Help)), "Esc cancelled the change, not the dialog");
    assert_eq!(shown(&op)[row].0, "F7");
    change(&op, row);
    let version = app.live_view().version;
    press(&op, &key(Key::F2));
    assert_eq!(app.live_view().version, version, "F2 did not clear the screen while it was being picked");
    assert_eq!(op.state.borrow().asks, 1, "asks first");
    click(&op, "AskOverlay::ok", 0);
    assert_eq!(shown(&op)[row].0, "F2");
    assert_eq!(shown(&op)[row_of(&op, "Clear text")].0, "", "Clear text has no key now");
    click(&op, "SettingsHelp::reset-all", 0);
    assert_eq!(shown(&op)[row].0, "F7");
    assert!(!op.ui.global::<KeysDlg>().get_any_changed());
}

#[test]
fn clear_groups_are_edited_here_and_run_from_their_key() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Help);
    settle();
    let k = op.ui.global::<KeysDlg>();
    assert_eq!(k.get_groups().row_count(), 1);
    let builtin = k.get_groups().row_data(0).unwrap();
    assert_eq!(builtin.name, "Everything but the background");
    assert_eq!(builtin.layers.iter().collect::<Vec<_>>(), [true, false, true, true, true, true, true], "all but Media & background");
    click(&op, "SettingsHelp::add-group", 0);
    assert_eq!(k.get_groups().row_count(), 2);
    k.invoke_group_renamed(1, "Words only ".into());
    k.invoke_group_layer(1, 5, true);
    let row = row_of(&op, "Clear group: Words only");
    change(&op, row);
    press(&op, &key(Key::F10));
    click(&op, "SettingsDialog::save", 0);
    refresh(&op);
    let groups = app.data.lock().unwrap().settings.keys.clear_groups.clone();
    assert_eq!(groups[1].name, "Words only");
    assert_eq!(groups[1].layers, [ClearLayer::Slide, ClearLayer::Message]);
    assert_eq!(route_in(&Chord::parse("F10").unwrap(), TYPING, &op.keymap()), KeyAction::ClearGroup(groups[1].id));
    assert_eq!(route_in(&Chord::parse("F9").unwrap(), TYPING, &op.keymap()), KeyAction::ClearGroup(BUILTIN_GROUP));
    // Deleting a group takes its key away too.
    settings::open(&op, SettingsTab::Help);
    settle();
    click(&op, "SettingsHelp::delete-group", 1);
    click(&op, "SettingsDialog::save", 0);
    refresh(&op);
    let keys = app.data.lock().unwrap().settings.keys.clone();
    assert_eq!(keys.clear_groups.len(), 1);
    assert!(keys.changed.is_empty());
}
