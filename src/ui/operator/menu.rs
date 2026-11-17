//! The menu bar (`MenuBar` in `ui/operator.slint`, spec `2026-11-06-ui-menu-design.md`). Most items call the window
//! callback their button uses; the ones with no window-level callback (Add or Settings on a given tab, Undo, Find,
//! Print, Zoom, Exit…) send an id through `menu(string)`, turned into a `MenuAction` here.
use super::{view::OperatorSnapshot, window::Operator};
use crate::ui::dialogs::{AddTab, SettingsTab, add, settings};
use slint::{Model, ModelRc, SharedString, VecModel, platform::WindowEvent};
use std::rc::{Rc, Weak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Add(AddTab),
    Settings(SettingsTab),
    /// Edit → Undo (`false`) or Redo (`true`).
    Undo(bool),
    /// Edit → Quick search… (N14): songs, media, slide sets and services in one box.
    Find,
    /// File → Print service order… (the Print dialog, FEAT-6b).
    Print,
    /// Help → Open logs folder.
    Logs,
    /// View → Zoom in (1), Zoom out (-1), Actual size (0).
    Zoom(i8),
    /// The Overlays dialog (FEAT-1 §3.6).
    Overlays,
    /// The same as the window's close button.
    Exit,
    /// Window → Stage layouts… (FEAT-3).
    StageLayouts,
    /// Window → Automation… (FEAT-2).
    Automation,
    /// Window → Walk-in music… (N13).
    Music,
    /// Window → Calendar… (N17): the Automation dialog's Calendar tab.
    Calendar,
    /// Edit → Find a verse… (FEAT-5).
    FindVerse,
    /// Help → Find my mouse (N35, also Alt+M).
    FindMouse,
    /// Help → Tutorials: short how-tos, written to a page and opened in the browser (N35).
    Tutorials,
    /// Help → Report a problem…: a text file saying what to send and where the logs are (N35).
    ReportProblem,
    /// File → Clean up unused media… (N23).
    CleanUpMedia,
    /// View → Listen in headphones, or Stop listening (N21).
    Listen,
    /// View → Show slides as (N37).
    SlideView(super::slides::SlideView),
    /// Edit → About this Bible… (N6): the chosen Bible's licence and where it comes from.
    BibleInfo,
    /// Settings → Translations…: the translation editor on the language in use.
    Translations,
    /// File → Import / Export's words, pictures, playlist and theme items (N32).
    Transfer(super::transfer::Transfer),
    /// File → Start recording, or Stop recording (N22; also the Record button).
    Record,
    /// Settings → Network… (N31).
    Network,
    /// File → Add to service → Designed slides… (N26): the slide editor on new slides.
    AddDesign,
    /// Window → Action palette (N15): opens or closes it in the slides column.
    ActionPalette,
    /// Window → Start pre-service (N16): the Start countdown macro, or its tab when it isn't set up yet.
    StartPreService,
    /// Edit → Cut, Copy, Duplicate, Delete, Move up / down: the selected service item (or media file).
    Edit(crate::shortcuts::EditKey),
    /// Edit → Paste: the copied service item, or a copied YouTube link.
    Paste,
    /// File → Go Live, or Stop the livestream (also the Live button; the first-time helper when nothing is set up).
    GoLive,
}

/// The action of a `menu("…")` id; `None` for an unknown id.
pub fn action(id: &str) -> Option<MenuAction> {
    use MenuAction::{Add, Settings};
    if let Some(t) = super::transfer::action(id) { return Some(MenuAction::Transfer(t)) }
    Some(match id {
        "add-song" => Add(AddTab::Song),
        "add-bible" => Add(AddTab::Scripture),
        "add-media" => Add(AddTab::Media),
        "add-slides" => Add(AddTab::Deck),
        "add-text" => Add(AddTab::Text),
        "settings-themes" => Settings(SettingsTab::Themes),
        "overlays" => MenuAction::Overlays,
        "settings-songs" => Settings(SettingsTab::Songs),
        "settings-sections" => Settings(SettingsTab::Sections),
        "settings-bible" => Settings(SettingsTab::Bible),
        "settings-video" => Settings(SettingsTab::Video),
        "settings-media" => Settings(SettingsTab::Media),
        "media-cleanup" => MenuAction::CleanUpMedia,
        "listen" => MenuAction::Listen,
        "settings-backups" => Settings(SettingsTab::Backups),
        "settings-help" => Settings(SettingsTab::Help),
        "settings-about" => Settings(SettingsTab::About),
        "undo" => MenuAction::Undo(false),
        "redo" => MenuAction::Undo(true),
        "find" => MenuAction::Find,
        "print" => MenuAction::Print,
        "logs" => MenuAction::Logs,
        "zoom-in" => MenuAction::Zoom(1),
        "zoom-out" => MenuAction::Zoom(-1),
        "zoom-reset" => MenuAction::Zoom(0),
        "exit" => MenuAction::Exit,
        "stage-layouts" => MenuAction::StageLayouts,
        "automation" => MenuAction::Automation,
        "music" => MenuAction::Music,
        "calendar" => MenuAction::Calendar,
        "find-verse" => MenuAction::FindVerse,
        "find-mouse" => MenuAction::FindMouse,
        "tutorials" => MenuAction::Tutorials,
        "report-problem" => MenuAction::ReportProblem,
        "slides-grid" => MenuAction::SlideView(super::slides::SlideView::Grid),
        "slides-easy" => MenuAction::SlideView(super::slides::SlideView::Easy),
        "slides-table" => MenuAction::SlideView(super::slides::SlideView::Table),
        "bible-info" => MenuAction::BibleInfo,
        "translations" => MenuAction::Translations,
        "record" => MenuAction::Record,
        "network" => MenuAction::Network,
        "add-design" => MenuAction::AddDesign,
        "action-palette" => MenuAction::ActionPalette,
        "start-preservice" => MenuAction::StartPreService,
        "delete" => MenuAction::Edit(crate::shortcuts::EditKey::Delete),
        "cut" => MenuAction::Edit(crate::shortcuts::EditKey::Cut),
        "copy" => MenuAction::Edit(crate::shortcuts::EditKey::Copy),
        "duplicate" => MenuAction::Edit(crate::shortcuts::EditKey::Duplicate),
        "move-up" => MenuAction::Edit(crate::shortcuts::EditKey::MoveUp),
        "move-down" => MenuAction::Edit(crate::shortcuts::EditKey::MoveDown),
        "paste" => MenuAction::Paste,
        "go-live" => MenuAction::GoLive,
        _ => return None,
    })
}

/// View → Zoom's sizes, smallest first; `ZOOMS[ZOOM_AT_100]` is 100 %.
pub const ZOOMS: [f32; 6] = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5];
const ZOOM_AT_100: i8 = 2;

/// The window's size the operator UI needs at the least (`min-width`/`min-height` in `ui/operator.slint`).
const MIN_SIZE: (f32, f32) = (1024.0, 640.0);

impl Operator {
    fn menu(self: &Rc<Self>, id: &str) {
        match action(id) {
            Some(MenuAction::Add(tab)) => add::open_add(self, tab),
            Some(MenuAction::Settings(tab)) => settings::open(self, tab),
            Some(MenuAction::Undo(redo)) => self.undo(redo),
            Some(MenuAction::Find) => self.find(),
            Some(MenuAction::Print) => self.print(),
            Some(MenuAction::Logs) => self.open_logs(),
            Some(MenuAction::Zoom(step)) => self.zoom(step),
            Some(MenuAction::Overlays) => crate::ui::dialogs::overlays::open(self),
            Some(MenuAction::Exit) => self.window().dispatch_event(WindowEvent::CloseRequested),
            Some(MenuAction::StageLayouts) => crate::ui::dialogs::stage_layouts::open(self),
            Some(MenuAction::Automation) => crate::ui::dialogs::automation::open(self),
            Some(MenuAction::Music) => crate::ui::dialogs::music::open(self),
            Some(MenuAction::Calendar) => {
                crate::ui::dialogs::automation::open(self);
                crate::ui::dialogs::automation::goto(self, crate::ui::dialogs::automation::CALENDAR_TAB, -1);
            }
            Some(MenuAction::FindVerse) => crate::ui::dialogs::find_verse::open(self, ""),
            Some(MenuAction::FindMouse) => self.find_mouse(),
            Some(MenuAction::Tutorials) => self.tutorials(),
            Some(MenuAction::ReportProblem) => self.report_problem(),
            Some(MenuAction::CleanUpMedia) => crate::ui::dialogs::settings_media::clean_up(self),
            Some(MenuAction::Listen) => self.listen(),
            Some(MenuAction::SlideView(view)) => self.set_slide_view(view, None),
            Some(MenuAction::BibleInfo) => {
                let chosen = self.state.borrow().snap.as_ref().map(|s| s.translation.clone());
                if let Some(id) = chosen { super::bible_extras::info(self, &id) }
            }
            Some(MenuAction::Translations) => crate::ui::dialogs::translation_editor::open(self, None),
            Some(MenuAction::Transfer(t)) => self.transfer(t),
            Some(MenuAction::Record) => self.toggle_recording(),
            Some(MenuAction::Network) => crate::ui::dialogs::network::open(self),
            Some(MenuAction::AddDesign) => crate::ui::dialogs::design_editor::open(self, None),
            Some(MenuAction::ActionPalette) => super::action_palette::toggle(self),
            Some(MenuAction::StartPreService) => self.start_preservice(),
            Some(MenuAction::Edit(key)) => { self.edit_key(key); }
            Some(MenuAction::Paste) => { self.paste(); }
            Some(MenuAction::GoLive) => self.go_live(),
            None => {}
        }
    }

    /// N16: one click sets up pre-service (music, loop, countdown and stage layout, as the Pre-service tab says).
    fn start_preservice(self: &Rc<Self>) {
        let id = crate::automation::preservice::PRE_SERVICE_MACRO_ID;
        let set_up = self.state.borrow().snap.as_ref().is_some_and(|s| s.data.automation.macros.iter().any(|m| m.id == id));
        if set_up { return super::automation::run_macro(self, &id.to_string()) }
        crate::ui::dialogs::automation::open(self);
        crate::ui::dialogs::automation::goto(self, 3, -1);
        self.toast(&crate::tr!("Choose what pre-service does, then click Save."), false);
    }

    /// Edit → Undo or Redo (also Ctrl+Z / Ctrl+Y): the toast says what was undone. Like any edit, it never changes
    /// the live screen (`App::undo`).
    pub(super) fn undo(self: &Rc<Self>, redo: bool) {
        let me = Rc::downgrade(self);
        self.ops.undo(redo, move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(text) => op.toast(text, false),
                // "Nothing to undo." is not a fault.
                Err(e) => { let nothing = e == crate::undo::nothing(redo); op.toast(e, !nothing) }
            }
        });
    }

    /// Edit → Quick search… (also Ctrl+F, N14): one box for songs, media, slide sets and services. Nothing over a dialog.
    pub(super) fn find(self: &Rc<Self>) { crate::ui::dialogs::quick_search::open(self) }

    /// File → Print service order…: the Print dialog for the open service.
    fn print(self: &Rc<Self>) {
        let service = self.state.borrow().snap.as_ref().and_then(|s| s.service_id);
        if let Some(id) = service { crate::ui::dialogs::print::open(self, id) }
    }

    /// Help → Open logs folder: the data folder's `logs`, where the app's log and crash notes go.
    fn open_logs(&self) {
        let dir = self.app().store.root.join("logs");
        if std::fs::create_dir_all(&dir).is_err() || !crate::report_page::open(&dir) {
            self.toast(tr::logs_not_opened(&dir.display().to_string()), true);
        }
    }

    /// View → Zoom: the whole operator window drawn larger or smaller (`step` 1 in, -1 out, 0 back to 100 %). The
    /// window keeps its size on screen, so a zoom that would leave the UI less room than it needs is refused.
    fn zoom(&self, step: i8) {
        let now = self.state.borrow().zoom_step;
        let next = if step == 0 { 0 } else { (now + step).clamp(-ZOOM_AT_100, ZOOMS.len() as i8 - 1 - ZOOM_AT_100) };
        if next == now { return }
        let window = self.window();
        let base = window.scale_factor() / zoom_of(now);
        let scale = base * zoom_of(next);
        let size = window.size();
        if (size.width as f32 / scale) < MIN_SIZE.0 || (size.height as f32 / scale) < MIN_SIZE.1 {
            self.toast(tr::window_too_small_to_zoom(), false);
            return;
        }
        window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
        window.dispatch_event(WindowEvent::Resized { size: size.to_logical(scale) });
        self.state.borrow_mut().zoom_step = next;
        self.ui.set_zoom_percent((zoom_of(next) * 100.0).round() as i32);
    }

    /// View → Listen in headphones (N21): turns Listen on or off and says where the sound goes now.
    fn listen(self: &Rc<Self>) {
        let on = !self.ui.get_listening();
        let me = Rc::downgrade(self);
        self.ops.call::<serde_json::Value>("sound/listen", serde_json::json!({ "on": on }), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(_) => {
                    op.ui.set_listening(on);
                    op.toast(if on { tr::listening() } else { tr::not_listening() }, false);
                }
                Err(e) => op.toast(e, true),
            }
        });
    }

    /// File → Open recent: the `i`th file there.
    fn open_recent(self: &Rc<Self>, i: usize) {
        let path = self.state.borrow().snap.as_ref().and_then(|s| s.settings.recent_files.get(i).cloned());
        if let Some(path) = path { crate::ui::dialogs::services::open_path(self, path) }
    }
}

fn zoom_of(step: i8) -> f32 { ZOOMS[(step + ZOOM_AT_100) as usize] }

/// What the menu shows from a new snapshot: Undo / Redo and the recent files.
pub(super) fn render(op: &Operator, s: &OperatorSnapshot) {
    op.ui.set_can_undo(s.can_undo);
    op.ui.set_can_redo(s.can_redo);
    let names: Vec<SharedString> = s.settings.recent_files.iter().map(|p| recent_name(p).into()).collect();
    if op.ui.get_recent_files().iter().ne(names.iter().cloned()) { op.ui.set_recent_files(ModelRc::new(VecModel::from(names))) }
    super::help::render_keys(op, &crate::shortcuts::Keymap::new(&s.settings.keys), &s.settings.keys.clear_groups);
}

/// A recent file as File → Open recent lists it: its name without `.rpservice`, then its folder.
pub fn recent_name(path: &std::path::Path) -> String {
    let name = path.file_stem().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    match path.parent().and_then(|p| p.file_name()) {
        Some(folder) => format!("{name}  ({})", folder.to_string_lossy()),
        None => name,
    }
}

mod tr {
    use crate::tr;
    pub fn logs_not_opened(dir: &str) -> String { tr!("Could not open the logs folder. It is at {path}", path = dir) }
    pub fn window_too_small_to_zoom() -> String { tr!("Make the window bigger to zoom in further.") }
    pub fn listening() -> String { tr!("Listening: the sound goes to your headphones only. The room hears nothing.") }
    pub fn not_listening() -> String { tr!("The sound goes to the room again.") }
}

/// Connects the menu bar's `menu` callback. It holds a `Weak`, so the window does not keep itself alive.
pub(super) fn wire(op: &Rc<Operator>) {
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_menu(move |id| if let Some(op) = me.upgrade() { op.menu(&id) });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_open_recent(move |i| if let Some(op) = me.upgrade() { op.open_recent(i as usize) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::operator::keys::{KeyAction, KeyCtx};

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
        assert_eq!(action("settings-themes"), Some(MenuAction::Settings(SettingsTab::Themes)));
        assert_eq!(action("overlays"), Some(MenuAction::Overlays));
        assert_eq!(action("settings-songs"), Some(MenuAction::Settings(SettingsTab::Songs)));
        assert_eq!(action("settings-sections"), Some(MenuAction::Settings(SettingsTab::Sections)));
        assert_eq!(action("settings-bible"), Some(MenuAction::Settings(SettingsTab::Bible)));
        assert_eq!(action("settings-video"), Some(MenuAction::Settings(SettingsTab::Video)));
        assert_eq!(action("settings-backups"), Some(MenuAction::Settings(SettingsTab::Backups)));
        assert_eq!(action("settings-help"), Some(MenuAction::Settings(SettingsTab::Help)));
        assert_eq!(action("settings-about"), Some(MenuAction::Settings(SettingsTab::About)));
        assert_eq!(action("undo"), Some(MenuAction::Undo(false)));
        assert_eq!(action("redo"), Some(MenuAction::Undo(true)));
        assert_eq!(action("find"), Some(MenuAction::Find));
        assert_eq!(action("print"), Some(MenuAction::Print));
        assert_eq!(action("logs"), Some(MenuAction::Logs));
        assert_eq!(action("zoom-in"), Some(MenuAction::Zoom(1)));
        assert_eq!(action("zoom-out"), Some(MenuAction::Zoom(-1)));
        assert_eq!(action("zoom-reset"), Some(MenuAction::Zoom(0)));
        assert_eq!(action("exit"), Some(MenuAction::Exit));
        assert_eq!(action("stage-layouts"), Some(MenuAction::StageLayouts));
        assert_eq!(action("add-design"), Some(MenuAction::AddDesign));
        assert_eq!(action("automation"), Some(MenuAction::Automation));
        assert_eq!(action("find-verse"), Some(MenuAction::FindVerse));
        assert_eq!(action("find-mouse"), Some(MenuAction::FindMouse));
        assert_eq!(action("tutorials"), Some(MenuAction::Tutorials));
        assert_eq!(action("report-problem"), Some(MenuAction::ReportProblem));
        assert_eq!(action("settings-media"), Some(MenuAction::Settings(SettingsTab::Media)));
        assert_eq!(action("media-cleanup"), Some(MenuAction::CleanUpMedia));
        assert_eq!(action("listen"), Some(MenuAction::Listen));
        assert_eq!(action("record"), Some(MenuAction::Record));
        assert_eq!(action("settings-nope"), None);
        assert_eq!(action(""), None);
    }

    #[test]
    fn every_menu_id_in_the_window_has_an_action() {
        let ids: Vec<&str> = menu_bar().match_indices("root.menu(\"").map(|(at, m)| {
            let rest = &menu_bar()[at + m.len()..];
            &rest[..rest.find('"').unwrap()]
        }).collect();
        assert!(ids.len() >= 20, "the bar sends its tab, undo, zoom and exit ids: {ids:?}");
        for id in ids { assert!(action(id).is_some(), "{id:?} has no action") }
    }

    #[test]
    fn every_menu_title_is_translatable() {
        let bar = menu_bar();
        let titles: Vec<&str> = bar.lines().map(str::trim).filter_map(|l| l.strip_prefix("title:")).collect();
        assert!(titles.len() >= 30, "found {} titles", titles.len());
        for t in titles {
            let t = t.trim();
            // `file`: Open recent's file names, which are not words to translate.
            // `g.name`: the church's own clear group names.
            assert!(t.starts_with("@tr(") || t.contains("? @tr(") || t == "file;" || t == "g.name;", "{t:?} is not translatable");
        }
    }

    #[test]
    fn shortcuts_match_the_keys() {
        // Each item with a shortcut takes it from `MenuKeys` (the church's keys, N28), runs what the key runs, and is
        // on exactly where the key works (`shortcuts::Reach`).
        use crate::shortcuts::{Action, Reach};
        let names = [
            ("MenuKeys.undo", Action::Undo, "root.menu(\"undo\")"),
            ("MenuKeys.redo", Action::Redo, "root.menu(\"redo\")"),
            ("MenuKeys.find", Action::Find, "root.menu(\"find\")"),
            ("MenuKeys.bible", Action::FocusBible, "root.focus-bible()"),
            ("MenuKeys.clear-all", Action::ClearAll, "root.live-clear(\"clear_all\")"),
            ("MenuKeys.clear-slide", Action::ClearSlide, "root.live-clear(\"clear_slide\")"),
            ("MenuKeys.clear-media", Action::ClearMedia, "root.live-clear(\"clear_media\")"),
            ("MenuKeys.clear-audio", Action::ClearAudio, "root.live-clear(\"clear_audio\")"),
            ("MenuKeys.logo", Action::Logo, "root.live-clear(\"logo\")"),
            ("MenuKeys.clear-overlays", Action::ClearOverlays, "root.live-clear(\"clear_overlays\")"),
            ("MenuKeys.clear-message", Action::ClearMessage, "root.live-clear(\"clear_message\")"),
            ("MenuKeys.clear-timers", Action::ClearTimers, "root.live-clear(\"clear_timers\")"),
            ("MenuKeys.find-mouse", Action::FindMouse, "root.menu(\"find-mouse\")"),
            ("MenuKeys.paste", Action::PasteLink, "root.menu(\"paste\")"),
            ("g.keys", Action::Group(crate::shortcuts::BUILTIN_GROUP), "root.clear-group(i)"),
        ];
        // The fixed Windows keys (`EditKey`): Free ones like Undo, the File and Settings ones like Find.
        use crate::shortcuts::EditKey;
        let edits = [
            ("MenuKeys.delete", EditKey::Delete, "root.menu(\"delete\")"),
            ("MenuKeys.cut", EditKey::Cut, "root.menu(\"cut\")"),
            ("MenuKeys.copy", EditKey::Copy, "root.menu(\"copy\")"),
            ("MenuKeys.duplicate", EditKey::Duplicate, "root.menu(\"duplicate\")"),
            ("MenuKeys.move-up", EditKey::MoveUp, "root.menu(\"move-up\")"),
            ("MenuKeys.move-down", EditKey::MoveDown, "root.menu(\"move-down\")"),
            ("MenuKeys.new-service", EditKey::NewService, "ServicesDialogState.new-service()"),
            ("MenuKeys.open-file", EditKey::OpenService, "ServicesDialogState.open-file()"),
            ("MenuKeys.print", EditKey::Print, "root.menu(\"print\")"),
            ("MenuKeys.settings", EditKey::Settings, "root.settings()"),
        ];
        let mut seen = Vec::new();
        for item in items() {
            let Some(keys) = field(item, "shortcut:") else { continue };
            let call = field(item, "activated =>").unwrap_or_else(|| panic!("{item} has no action"));
            if let Some(&(_, e, expected)) = edits.iter().find(|(n, ..)| *n == keys) {
                assert!(call.contains(expected), "{keys} runs {call}, not {expected}");
                let enabled = field(item, "enabled:").unwrap_or_default();
                assert!(enabled.starts_with("root.menu-"), "{keys}: off while a dialog covers the window: {enabled}");
                if !e.menu_only() {
                    assert!(enabled.contains("!OperatorState.focus-in-text"), "{keys}: a text box keeps the key: {enabled}");
                    let chord = e.chords()[0].clone();
                    let plain = KeyCtx { in_text_field: false, in_bible_box: false, dialog_open: false, in_list: false };
                    assert_eq!(crate::ui::operator::keys::route_in(&chord, plain, &Default::default()), KeyAction::Edit(e), "{keys}");
                }
                seen.push(keys);
                continue;
            }
            let &(_, action, expected) = names.iter().find(|(n, ..)| *n == keys).unwrap_or_else(|| panic!("{keys} is not a MenuKeys shortcut"));
            assert!(call.contains(expected), "{keys} runs {call}, the key runs {expected}");
            // The key itself does the same thing.
            let menu_key = crate::shortcuts::Keymap::default().menu_key(action).cloned().expect("a default key");
            let anywhere = KeyCtx { in_text_field: true, in_bible_box: true, dialog_open: false, in_list: false };
            let plain = KeyCtx { in_text_field: false, in_bible_box: false, dialog_open: false, in_list: false };
            assert_ne!(crate::ui::operator::keys::route_in(&menu_key, plain, &Default::default()), KeyAction::None, "{keys}");
            let enabled = field(item, "enabled:").unwrap_or_default();
            match action.reach() {
                Reach::Free => {
                    assert_eq!(crate::ui::operator::keys::route_in(&menu_key, anywhere, &Default::default()), KeyAction::None);
                    assert!(enabled.contains("!OperatorState.focus-in-text") && enabled.starts_with("root.menu-ready"), "{keys}: {enabled}");
                }
                // The key does nothing over a dialog, so the item is off then and the key falls through.
                Reach::Window => assert!(enabled == "!root.covered" || enabled == "root.menu-ready", "{keys}: {enabled}"),
                Reach::Anywhere => assert!(enabled.is_empty(), "{keys} works over dialogs too"),
            }
            seen.push(keys);
        }
        assert_eq!(seen.len(), names.len() + edits.len(), "{seen:?}");
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
    use slint::{ComponentHandle, Model, platform::{Key, WindowEvent}};
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
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "A".into(), body: "a".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None };
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

    fn text_item(title: &str) -> ServiceItem {
        ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: title.into(), body: "a".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None }
    }

    fn ctrl(op: &crate::ui::operator::window::Operator, c: &str) {
        let control: slint::SharedString = char::from(Key::Control).into();
        op.window().dispatch_event(WindowEvent::KeyPressed { text: control.clone() });
        op.window().dispatch_event(WindowEvent::KeyPressed { text: c.into() });
        op.window().dispatch_event(WindowEvent::KeyReleased { text: c.into() });
        op.window().dispatch_event(WindowEvent::KeyReleased { text: control });
    }

    #[test]
    fn menu_undo_and_redo_say_what_they_did() {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        assert!(!op.ui.get_can_undo() && !op.ui.get_can_redo());
        app.edit(|d| { d.services[0].items.push(text_item("Welcome")); Ok(()) }).unwrap();
        refresh(&op);
        assert!(op.ui.get_can_undo());
        op.ui.invoke_menu("undo".into());
        assert!(app.data.lock().unwrap().services[0].items.is_empty());
        refresh(&op);
        assert!(!op.ui.get_can_undo() && op.ui.get_can_redo());
        op.ui.invoke_menu("redo".into());
        assert_eq!(app.data.lock().unwrap().services[0].items.len(), 1);
        op.ui.invoke_menu("redo".into());
        assert_eq!(op.toast_log(), [("Undone: adding “Welcome”".to_string(), false), ("Redone: adding “Welcome”".to_string(), false),
            ("Nothing to redo.".to_string(), false)]);
    }

    #[test]
    fn ctrl_z_undoes_through_the_menu_bar_but_not_in_a_text_box() {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        app.edit(|d| { d.services[0].items.push(text_item("Welcome")); Ok(()) }).unwrap();
        refresh(&op);
        op.ui.global::<crate::ui::OperatorState>().set_focus_in_text(true);
        ctrl(&op, "z");
        assert_eq!(app.data.lock().unwrap().services[0].items.len(), 1, "the text box keeps Ctrl+Z");
        op.ui.global::<crate::ui::OperatorState>().set_focus_in_text(false);
        ctrl(&op, "z");
        assert!(app.data.lock().unwrap().services[0].items.is_empty());
        refresh(&op);
        ctrl(&op, "y");
        assert_eq!(app.data.lock().unwrap().services[0].items.len(), 1);
    }

    #[test]
    fn find_opens_quick_search_and_ctrl_f_too() {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        op.ui.invoke_menu("find".into());
        assert_eq!(op.dialog(), Some(DialogKind::QuickSearch));
        op.close_dialog();
        ctrl(&op, "f");
        assert_eq!(op.dialog(), Some(DialogKind::QuickSearch));
    }

    #[test]
    fn view_menu_switches_the_slides_view() {
        use crate::ui::operator::slides::SlideView;
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        assert_eq!(crate::ui::operator::menu::action("slides-table"), Some(crate::ui::operator::menu::MenuAction::SlideView(SlideView::Table)));
        op.ui.invoke_menu("slides-easy".into());
        assert_eq!((op.slide_view(), op.ui.get_slides_view()), (SlideView::Easy, 1));
        op.ui.invoke_menu("slides-table".into());
        assert_eq!((op.slide_view(), op.ui.get_slides_view()), (SlideView::Table, 2));
        op.ui.invoke_menu("slides-grid".into());
        assert_eq!(op.slide_view(), SlideView::Grid);
        let bar = include_str!("../../../ui/operator.slint");
        assert!(bar.contains("root.menu(\"slides-easy\")") && bar.contains("root.menu(\"slides-table\")"));
    }

    #[test]
    fn find_a_verse_and_automation_open_from_the_menu() {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        op.ui.invoke_menu("find-verse".into());
        assert_eq!(op.dialog(), Some(DialogKind::FindVerse));
        op.close_dialog();
        op.ui.invoke_menu("automation".into());
        assert_eq!(op.dialog(), Some(DialogKind::Automation));
        op.ui.invoke_menu("music".into());
        assert_eq!(op.dialog(), Some(DialogKind::Music));
        op.ui.invoke_menu("calendar".into());
        assert_eq!(op.dialog(), Some(DialogKind::Automation));
        assert_eq!(op.ui.global::<crate::ui::AutomationDlg>().get_tab(), crate::ui::dialogs::automation::CALENDAR_TAB);
        let bar = include_str!("../../../ui/operator.slint");
        assert!(bar.contains("root.menu(\"find-verse\")") && bar.contains("root.menu(\"automation\")"), "the bar has both items");
    }

    #[test]
    fn print_opens_the_print_dialog_for_the_open_service() {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        let id = app.data.lock().unwrap().current_service.unwrap();
        op.ui.invoke_menu("print".into());
        assert_eq!(op.dialog(), Some(DialogKind::Print(id)));
    }

    #[test]
    fn song_use_report_opens_from_the_menu() {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        op.ui.invoke_usage_report();
        assert_eq!(op.dialog(), Some(DialogKind::Usage));
    }

    #[test]
    fn open_logs_folder_opens_the_data_folders_logs() {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        op.ui.invoke_menu("logs".into());
        let logs = app.store.root.join("logs");
        assert!(logs.is_dir());
        assert!(crate::report_page::testing::OPENED.lock().unwrap().contains(&logs));
    }

    #[test]
    fn open_recent_lists_opened_files_and_opens_one() {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        assert_eq!(op.ui.get_recent_files().row_count(), 0);
        let file = app.store.root.join("Easter.rpservice");
        let id = app.data.lock().unwrap().current_service.unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(app.export_service(id, file.clone())).unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(app.import_service(file.clone())).unwrap();
        refresh(&op);
        let names: Vec<String> = op.ui.get_recent_files().iter().map(|s| s.to_string()).collect();
        assert_eq!(names, [super::recent_name(&file)]);
        assert!(names[0].starts_with("Easter  ("), "{names:?}");
        let before = app.data.lock().unwrap().services.len();
        op.ui.invoke_open_recent(0);
        assert_eq!(app.data.lock().unwrap().services.len(), before + 1, "opened again");
        assert_eq!(app.data.lock().unwrap().settings.recent_files, [file]);
    }

    #[test]
    fn recent_files_keep_five_newest_first() {
        let mut s = crate::model::Settings::default();
        for i in 0..7 { s.opened_file(std::path::Path::new(&format!("/a/{i}.rpservice"))) }
        s.opened_file(std::path::Path::new("/a/3.rpservice"));
        let names: Vec<String> = s.recent_files.iter().map(|p| p.file_stem().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["3", "6", "5", "4", "2"]);
    }

    #[test]
    fn zoom_scales_the_window_and_comes_back() {
        let app = test_app();
        let op = open(&app);
        op.window().set_size(slint::LogicalSize::new(1600.0, 900.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        let base = op.window().scale_factor();
        op.ui.invoke_menu("zoom-in".into());
        slint::platform::update_timers_and_animations();
        assert!((op.window().scale_factor() - base * 1.1).abs() < 1e-4);
        assert_eq!(op.ui.get_zoom_percent(), 110);
        assert!(!op.ui.get_compact(), "1600 / 1.1 is still a wide window");
        op.ui.invoke_menu("zoom-in".into());
        slint::platform::update_timers_and_animations();
        assert_eq!(op.ui.get_zoom_percent(), 125);
        assert!(op.ui.get_compact() && !op.ui.get_narrow(), "the window lays itself out 1280 wide");
        op.ui.invoke_menu("zoom-in".into());
        assert_eq!(op.ui.get_zoom_percent(), 125, "900 / 1.5 is lower than the window allows");
        assert!(op.toast_log().iter().any(|(t, _)| t == "Make the window bigger to zoom in further."));
        op.ui.invoke_menu("zoom-reset".into());
        assert_eq!(op.ui.get_zoom_percent(), 100);
        assert!((op.window().scale_factor() - base).abs() < 1e-4);
        op.ui.invoke_menu("zoom-out".into());
        op.ui.invoke_menu("zoom-out".into());
        op.ui.invoke_menu("zoom-out".into());
        assert_eq!(op.ui.get_zoom_percent(), 80, "80 % is the smallest");
    }

    #[test]
    fn f2_runs_through_the_menu_bar_and_clears_text() {
        let app = test_app();
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "A".into(), body: "a1".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None };
        app.data.lock().unwrap().services[0].items.push(item.clone());
        let op = open(&app);
        refresh(&op);
        op.ops().cmd(crate::live::Command::Show { item_id: item.id, slide: 0 }, None, |o| assert!(o.ok, "{o:?}"));
        op.apply_live(LiveUpdate::new(app.live_view()));
        assert!(app.live_view().frame.slide.is_some());
        // The window's key scope would also clear; count it, and leave the key unused there.
        let scope = std::rc::Rc::new(std::cell::Cell::new(0));
        let s = scope.clone();
        op.ui.on_key(move |_, _, _, _, _, _| { s.set(s.get() + 1); false });
        let f2: slint::SharedString = char::from(Key::F2).into();
        op.window().dispatch_event(WindowEvent::KeyPressed { text: f2.clone() });
        op.window().dispatch_event(WindowEvent::KeyReleased { text: f2 });
        assert!(app.live_view().frame.slide.is_none(), "F2 cleared the text");
        assert_eq!(scope.get(), 0, "the menu bar took the key before the window's key scope");
    }
}
