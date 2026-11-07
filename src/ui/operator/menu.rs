//! The menu bar (`MenuBar` in `ui/operator.slint`, spec `2026-11-06-ui-menu-design.md`). Most items call the window
//! callback their button uses; the few with no window-level callback (Add or Settings on a given tab, Exit) send an
//! id through `menu(string)`, turned into a `MenuAction` here.
use super::window::Operator;
use crate::ui::dialogs::{AddTab, SettingsTab, add, settings};
use std::rc::{Rc, Weak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Add(AddTab),
    Settings(SettingsTab),
    /// The same as the window's close button.
    Exit,
    /// Window → Stage layouts… (FEAT-3).
    StageLayouts,
}

/// The action of a `menu("…")` id; `None` for an unknown id.
pub fn action(id: &str) -> Option<MenuAction> {
    use MenuAction::{Add, Settings};
    Some(match id {
        "add-song" => Add(AddTab::Song),
        "add-bible" => Add(AddTab::Scripture),
        "add-media" => Add(AddTab::Media),
        "add-slides" => Add(AddTab::Deck),
        "add-text" => Add(AddTab::Text),
        "settings-songs" => Settings(SettingsTab::Songs),
        "settings-bible" => Settings(SettingsTab::Bible),
        "settings-video" => Settings(SettingsTab::Video),
        "settings-backups" => Settings(SettingsTab::Backups),
        "settings-help" => Settings(SettingsTab::Help),
        "settings-about" => Settings(SettingsTab::About),
        "exit" => MenuAction::Exit,
        "stage-layouts" => MenuAction::StageLayouts,
        _ => return None,
    })
}

impl Operator {
    fn menu(self: &Rc<Self>, id: &str) {
        match action(id) {
            Some(MenuAction::Add(tab)) => add::open_add(self, tab),
            Some(MenuAction::Settings(tab)) => settings::open(self, tab),
            Some(MenuAction::Exit) => self.window().dispatch_event(slint::platform::WindowEvent::CloseRequested),
            Some(MenuAction::StageLayouts) => crate::ui::dialogs::stage_layouts::open(self),
            None => {}
        }
    }
}

/// Connects the menu bar's `menu` callback. It holds a `Weak`, so the window does not keep itself alive.
pub(super) fn wire(op: &Rc<Operator>) {
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_menu(move |id| if let Some(op) = me.upgrade() { op.menu(&id) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::Command;
    use crate::ui::operator::keys::{KeyAction, KeyCtx, route};

    const WINDOW: &str = include_str!("../../../ui/operator.slint");

    /// The `MenuBar { … }` block of the window.
    fn menu_bar() -> &'static str {
        let start = WINDOW.find("MenuBar {").expect("the window has a MenuBar");
        let mut depth = 0;
        for (i, c) in WINDOW[start..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => { depth -= 1; if depth == 0 { return &WINDOW[start..start + i + 1] } }
                _ => {}
            }
        }
        panic!("the MenuBar block is not closed")
    }

    /// Each `MenuItem { … }` of the bar: its body text.
    fn items() -> Vec<&'static str> {
        let bar = menu_bar();
        bar.match_indices("MenuItem {").map(|(at, _)| {
            let body = &bar[at..];
            &body[..body.find("\n            }").or_else(|| body.find('}')).map_or(body.len(), |e| e)]
        }).collect()
    }

    /// What follows `key:` up to the end of its line.
    fn field<'a>(item: &'a str, key: &str) -> Option<&'a str> {
        item.lines().map(str::trim).find_map(|l| l.strip_prefix(key)).map(|v| v.trim().trim_end_matches(';'))
    }

    #[test]
    fn menu_ids_map_to_their_tabs() {
        assert_eq!(action("add-song"), Some(MenuAction::Add(AddTab::Song)));
        assert_eq!(action("add-bible"), Some(MenuAction::Add(AddTab::Scripture)));
        assert_eq!(action("add-media"), Some(MenuAction::Add(AddTab::Media)));
        assert_eq!(action("add-slides"), Some(MenuAction::Add(AddTab::Deck)));
        assert_eq!(action("add-text"), Some(MenuAction::Add(AddTab::Text)));
        assert_eq!(action("settings-songs"), Some(MenuAction::Settings(SettingsTab::Songs)));
        assert_eq!(action("settings-bible"), Some(MenuAction::Settings(SettingsTab::Bible)));
        assert_eq!(action("settings-video"), Some(MenuAction::Settings(SettingsTab::Video)));
        assert_eq!(action("settings-backups"), Some(MenuAction::Settings(SettingsTab::Backups)));
        assert_eq!(action("settings-help"), Some(MenuAction::Settings(SettingsTab::Help)));
        assert_eq!(action("settings-about"), Some(MenuAction::Settings(SettingsTab::About)));
        assert_eq!(action("exit"), Some(MenuAction::Exit));
        assert_eq!(action("stage-layouts"), Some(MenuAction::StageLayouts));
        assert_eq!(action("settings-nope"), None);
        assert_eq!(action(""), None);
    }

    #[test]
    fn every_menu_id_in_the_window_has_an_action() {
        let ids: Vec<&str> = menu_bar().match_indices("root.menu(\"").map(|(at, m)| {
            let rest = &menu_bar()[at + m.len()..];
            &rest[..rest.find('"').unwrap()]
        }).collect();
        assert!(ids.len() >= 12, "the bar sends its tab and exit ids: {ids:?}");
        for id in ids { assert!(action(id).is_some(), "{id:?} has no action") }
    }

    #[test]
    fn every_menu_title_is_translatable() {
        let bar = menu_bar();
        let titles: Vec<&str> = bar.lines().map(str::trim).filter_map(|l| l.strip_prefix("title:")).collect();
        assert!(titles.len() >= 30, "found {} titles", titles.len());
        for t in titles {
            let t = t.trim();
            assert!(t.starts_with("@tr(") || t.contains("? @tr(") , "{t:?} is not translatable");
        }
    }

    #[test]
    fn shortcuts_match_the_keys() {
        // Only keys that `keys::route` handles the same in every focus state may be bound (spec §2).
        let anywhere = KeyCtx { in_text_field: true, in_bible_box: true, dialog_open: false };
        let mut seen = Vec::new();
        for item in items() {
            let Some(keys) = field(item, "shortcut:") else { continue };
            let call = field(item, "activated =>").unwrap_or_else(|| panic!("{item} has no action"));
            let (key, ctrl) = match keys {
                "@keys(F1)" => ("F1", false),
                "@keys(F2)" => ("F2", false),
                "@keys(F3)" => ("F3", false),
                "@keys(F4)" => ("F4", false),
                "@keys(F5)" => ("F5", false),
                "@keys(Control + B)" => ("b", true),
                other => panic!("{other} must not be a menu shortcut: its key depends on focus"),
            };
            let expected = match route(key, ctrl, false, anywhere) {
                KeyAction::Clear(Command::ClearAll) => "root.live-clear(\"clear_all\")",
                KeyAction::Clear(Command::ClearSlide) => "root.live-clear(\"clear_slide\")",
                KeyAction::Clear(Command::ClearMedia) => "root.live-clear(\"clear_media\")",
                KeyAction::Clear(Command::ClearAudio) => "root.live-clear(\"clear_audio\")",
                KeyAction::Clear(Command::Logo) => "root.live-clear(\"logo\")",
                KeyAction::FocusBible => "root.focus-bible()",
                other => panic!("{key}: {other:?}"),
            };
            assert!(call.contains(expected), "{keys} runs {call}, the key runs {expected}");
            if ctrl {
                // The key does nothing over a dialog, so the item is off then and the key falls through.
                assert_eq!(field(item, "enabled:"), Some("!root.covered"), "{keys}");
            } else {
                assert!(field(item, "enabled:").is_none(), "{keys} works over dialogs too");
            }
            seen.push(key);
        }
        seen.sort();
        assert_eq!(seen, ["F1", "F2", "F3", "F4", "F5", "b"]);
    }
}

/// The menu in a real window: actions run inline on the testing backend.
#[cfg(test)]
mod window_tests {
    use crate::{
        app::{LiveUpdate, test_app},
        model::{ItemKind, ServiceItem},
        ui::{dialogs::{AddTab, DialogKind, SettingsTab}, operator::window::testing::{open, refresh}},
    };
    use slint::platform::{Key, WindowEvent};
    use uuid::Uuid;

    #[test]
    fn menu_opens_settings_on_a_tab() {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        op.ui.invoke_menu("settings-about".into());
        assert_eq!(op.dialog(), Some(DialogKind::Settings(SettingsTab::About)));
        op.close_dialog();
        op.ui.invoke_menu("settings-help".into());
        assert_eq!(op.dialog(), Some(DialogKind::Settings(SettingsTab::Help)));
    }

    #[test]
    fn menu_opens_add_on_a_tab() {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        op.ui.invoke_menu("add-bible".into());
        assert_eq!(op.dialog(), Some(DialogKind::Add(AddTab::Scripture)));
    }

    #[test]
    fn menu_exit_closes_the_window() {
        let app = test_app();
        let op = open(&app);
        assert!(op.window().is_visible());
        op.ui.invoke_menu("exit".into());
        assert!(!op.window().is_visible(), "the same close request as the window's X");
    }

    #[test]
    fn menu_items_greyed_until_ready_and_while_covered() {
        let app = test_app();
        let op = open(&app);
        assert!(!op.ui.get_menu_ready(), "nothing opens before the first read");
        refresh(&op);
        assert!(op.ui.get_menu_ready());
        assert!(op.ui.get_menu_can_save(), "there is a current service");
        assert!(!op.ui.get_menu_can_edit(), "nothing selected");
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "A".into(), body: "a".into() }, background: None, notes: String::new(), theme: None };
        app.data.lock().unwrap().services[0].items.push(item);
        refresh(&op);
        assert!(op.ui.get_menu_can_edit(), "the new item is selected");
        op.ui.invoke_settings();
        assert!(!op.ui.get_menu_ready() && !op.ui.get_menu_can_edit() && !op.ui.get_menu_can_save(), "a dialog covers the window");
        op.close_dialog();
        assert!(op.ui.get_menu_ready());
        { let mut d = app.data.lock().unwrap(); d.services.clear(); d.current_service = None; }
        refresh(&op);
        assert!(!op.ui.get_menu_can_save(), "no service to save");
    }

    #[test]
    fn f2_runs_through_the_menu_bar_and_clears_text() {
        let app = test_app();
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "A".into(), body: "a1".into() }, background: None, notes: String::new(), theme: None };
        app.data.lock().unwrap().services[0].items.push(item.clone());
        let op = open(&app);
        refresh(&op);
        op.ops().cmd(crate::live::Command::Show { item_id: item.id, slide: 0 }, None, |o| assert!(o.ok, "{o:?}"));
        op.apply_live(LiveUpdate::new(app.live_view()));
        assert!(app.live_view().frame.slide.is_some());
        // The window's key scope would also clear; count it, and leave the key unused there.
        let scope = std::rc::Rc::new(std::cell::Cell::new(0));
        let s = scope.clone();
        op.ui.on_key(move |_, _, _, _, _| { s.set(s.get() + 1); false });
        let f2: slint::SharedString = char::from(Key::F2).into();
        op.window().dispatch_event(WindowEvent::KeyPressed { text: f2.clone() });
        op.window().dispatch_event(WindowEvent::KeyReleased { text: f2 });
        assert!(app.live_view().frame.slide.is_none(), "F2 cleared the text");
        assert_eq!(scope.get(), 0, "the menu bar took the key before the window's key scope");
    }
}
