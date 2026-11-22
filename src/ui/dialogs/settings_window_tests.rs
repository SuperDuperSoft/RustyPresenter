//! The Settings window (November 17, 2026 reorganisation): every page reachable, every setting on exactly one page,
//! "Find a setting", Show advanced settings, remembering the last page, and the window fitting at any size.
use super::*;
use crate::{app::{App, test_app}, ui::{SettingsWindow, operator::window::testing::{open as open_window, refresh}}};
use slint::Model;
use i_slint_backend_testing::{ElementHandle, ElementQuery};
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

fn opened(app: &Arc<App>) -> Rc<Operator> {
    crate::ui::ui_test_init();
    let op = open_window(app);
    refresh(&op);
    op
}

fn g(op: &Operator) -> SettingsDlg<'static> { win(op).global::<SettingsDlg>() }

fn settle() { for _ in 0..3 { slint::platform::update_timers_and_animations() } }

/// The rows the page shows now: key → (label, help).
fn rows_shown(w: &SettingsWindow) -> Vec<(String, String, String)> {
    // The query meets each row once per element it is made of: one entry per row.
    let rows: BTreeSet<(String, String, String)> = ElementQuery::from_root(w).match_descendants().match_type_name("SetRow").find_all().into_iter()
        .map(|e: ElementHandle| (
            e.accessible_id().unwrap_or_default().to_string(),
            e.accessible_label().unwrap_or_default().to_string(),
            e.accessible_description().unwrap_or_default().to_string(),
        )).filter(|r| !r.0.is_empty()).collect();
    rows.into_iter().collect()
}

#[test]
fn every_page_shows_exactly_its_rows_and_every_setting_is_on_one_page() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Look);
    // Tall enough that no page scrolls (the search leaves out rows scrolled out of view).
    win(&op).window().set_size(slint::LogicalSize::new(1280.0, 8000.0));
    g(&op).set_advanced(true);
    let listed = settings_find::rows();
    let keys: BTreeSet<&str> = listed.iter().map(|r| r.key).collect();
    assert_eq!(keys.len(), listed.len(), "every row's key is its own");
    let mut seen: BTreeMap<String, SettingsTab> = BTreeMap::new();
    for tab in SettingsTab::ALL {
        go(&op, tab, "");
        settle();
        assert_eq!(shown(&op), Some(tab), "{tab:?} can be shown");
        assert_eq!(g(&op).get_page_title(), settings_find::path(tab).as_str());
        let mut drawn = rows_shown(win(&op));
        drawn.sort();
        let mut want: Vec<(String, String, String)> = listed.iter().filter(|r| r.page == tab)
            .map(|r| (r.key.to_string(), settings_find::shown(&r.label), settings_find::shown(&r.help))).collect();
        want.sort();
        assert_eq!(drawn, want, "the rows of {tab:?} (a row on no page, on two, or with other words than settings_find has)");
        for (key, _, _) in drawn {
            if let Some(other) = seen.insert(key.clone(), tab) { panic!("{key} is on {other:?} and {tab:?}") }
        }
        // Without Show advanced settings, only the simple rows.
        g(&op).set_advanced(false);
        settle();
        let simple: BTreeSet<String> = rows_shown(win(&op)).into_iter().map(|r| r.0).collect();
        let want: BTreeSet<String> = listed.iter().filter(|r| r.page == tab && !r.advanced).map(|r| r.key.to_string()).collect();
        assert_eq!(simple, want, "{tab:?} without Show advanced settings");
        g(&op).set_advanced(true);
        settle();
    }
    assert_eq!(seen.len(), listed.len(), "every row in settings_find is on a page");
    // The sidebar lists every page once.
    let pages: Vec<i32> = slint::Model::iter(&g(&op).get_nav()).filter(|n| !n.heading).map(|n| n.page).collect();
    assert_eq!(pages, (0..SettingsTab::ALL.len() as i32).collect::<Vec<_>>());
}

#[test]
fn finding_phone_microphone_lands_on_its_row() {
    for words in ["phone microphone", "phone mic", "Phone Mic", "microphone phone", "lapel"] {
        let found = settings_find::search(words);
        assert!(found.iter().take(3).any(|f| f.key == "phone-mic"), "{words}: {found:?}");
    }
    let first = |w: &str| settings_find::search(w).first().map(|f| f.key).unwrap_or("none");
    assert_eq!(first("4K"), "record-size");
    assert_eq!(first("ndi"), "ndi");
    assert!(settings_find::search("chords").iter().any(|f| f.key == "stage-layout"));
    assert!(settings_find::search("key").iter().any(|f| f.key == "keyboard"));
    assert!(settings_find::search("stream key").iter().take(3).any(|f| f.key == "stream-to"));
    assert!(settings_find::search("backup").iter().any(|f| f.page == SettingsTab::Backups && f.key.is_empty()), "a page by its name");
    assert!(settings_find::search("zzqq").is_empty());
    assert_eq!(settings_find::search("phone microphone")[0].path, "Remote and network → Phone camera and microphone");

    // In the window: the words typed list it; picking it shows its page and lights the row up.
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Look);
    g(&op).invoke_find_edited("phone microphone".into());
    let rows = g(&op).get_find_rows();
    let at = slint::Model::iter(&rows).position(|r| r.key == "phone-mic").expect("listed");
    g(&op).invoke_find_pick(at as i32);
    settle();
    assert_eq!(shown(&op), Some(SettingsTab::PhoneCamMic));
    assert_eq!(g(&op).get_highlight(), "phone-mic");
    assert!(rows_shown(win(&op)).iter().any(|r| r.0 == "phone-mic"));
    // An advanced row turns Show advanced settings on.
    g(&op).set_advanced(false);
    go(&op, SettingsTab::StreamPicture, "record-custom-size");
    settle();
    assert!(g(&op).get_advanced());
    assert!(rows_shown(win(&op)).iter().any(|r| r.0 == "record-custom-size"));
    // A link button jumps the same way.
    go(&op, SettingsTab::StreamSound, "");
    settle();
    let link = testing::find(&op, "StreamSoundPage::phone-mic-link").expect("the link under the microphones");
    link.query_descendants().match_type_name("Btn").find_first().expect("its button").mock_single_click(slint::platform::PointerEventButton::Left);
    settle();
    assert_eq!((shown(&op), g(&op).get_highlight().to_string()), (Some(SettingsTab::PhoneCamMic), "phone-mic".to_string()));
}

#[test]
fn settings_remembers_its_last_page_and_advanced_rows() {
    let app = test_app();
    let op = opened(&app);
    op.ui.invoke_settings();
    assert_eq!(shown(&op), Some(SettingsTab::Look), "the first time: Look");
    go(&op, SettingsTab::StreamSound, "");
    g(&op).set_advanced(true);
    testing::click(&op, "SettingsWindow::cancel");
    assert!(!is_open(&op));
    g(&op).set_advanced(false);
    op.ui.invoke_settings();
    assert_eq!(shown(&op), Some(SettingsTab::StreamSound), "back on the last page");
    assert!(g(&op).get_advanced(), "Show advanced settings is remembered");
    let kept = settings_window::load(&app.store.root);
    assert_eq!((kept.page.as_str(), kept.advanced), ("StreamSound", true));
    // A menu item asking for a page gets it, with the window already open too.
    open(&op, SettingsTab::Bible);
    assert_eq!(shown(&op), Some(SettingsTab::Bible));
}

#[test]
fn the_window_fits_from_small_to_large_with_its_buttons_inside() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Look);
    g(&op).set_advanced(true);
    for (w, h) in [(640.0, 440.0), (1024.0, 768.0), (1920.0, 1080.0)] {
        win(&op).window().set_size(slint::LogicalSize::new(w, h));
        for tab in SettingsTab::ALL {
            go(&op, tab, "");
            settle();
            for id in ["SettingsWindow::save", "SettingsWindow::cancel", "SettingsWindow::find", "SettingsWindow::advanced", "SettingsWindow::body"] {
                let Some(e) = testing::find(&op, id) else { panic!("{id} on {tab:?}") };
                let (p, s) = (e.absolute_position(), e.size());
                assert!(p.x >= -0.5 && p.y >= -0.5 && p.x + s.width <= w + 0.5 && p.y + s.height <= h + 0.5,
                    "{id} at {p:?} {s:?} is outside the {w}×{h} window on {tab:?}");
            }
            // The page scrolls under the title, above the buttons.
            let body = testing::find(&op, "SettingsWindow::body").unwrap();
            let save = testing::find(&op, "SettingsWindow::save").unwrap();
            assert!(body.absolute_position().y + body.size().height <= save.absolute_position().y, "{tab:?} at {w}×{h}: the page ends above the buttons");
            assert!(body.size().height >= 100.0, "{tab:?} at {w}×{h}: room for the page");
        }
    }
}

#[test]
fn cancel_with_changes_asks_in_the_settings_window() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Songs);
    g(&op).set_lines(4);
    testing::click(&op, "SettingsWindow::cancel");
    assert!(is_open(&op), "asks first");
    assert_eq!(win(&op).get_ask().row_count(), 1, "the question is over Settings");
    assert_eq!(op.ui.get_ask().row_count(), 0, "not over the operator");
    crate::ui::dialogs::ask::answer_other_settings(&op);
    assert!(!is_open(&op), "Don't save closes");
    assert_ne!(app.data().settings.lines_per_slide, 5, "and keeps nothing");
    open(&op, SettingsTab::Songs);
    g(&op).set_lines(4);
    testing::click(&op, "SettingsWindow::cancel");
    crate::ui::dialogs::ask::answer_settings(&op, true, String::new());
    for _ in 0..50 { if !is_open(&op) { break } settle(); std::thread::sleep(std::time::Duration::from_millis(5)) }
    assert!(!is_open(&op));
    assert_eq!(app.data().settings.lines_per_slide, 5, "Save keeps it");
}

#[test]
fn a_size_typed_by_hand_is_saved_and_shown_again() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::StreamPicture);
    g(&op).set_advanced(true);
    settle();
    g(&op).set_record_width("1601".into());
    g(&op).set_record_height("901".into());
    g(&op).invoke_record_custom_size();
    assert_eq!(g(&op).get_record_size(), crate::record::Preset::ALL.len() as i32);
    assert_eq!(slint::Model::row_data(&g(&op).get_record_size_labels(), 5).unwrap(), "1600 × 900 (custom)");
    testing::click(&op, "SettingsWindow::save");
    for _ in 0..50 { if !is_open(&op) { break } settle(); std::thread::sleep(std::time::Duration::from_millis(5)) }
    assert_eq!(app.data().settings.record.size, crate::record::Preset::custom(1600, 900));
    refresh(&op);
    open(&op, SettingsTab::StreamPicture);
    assert_eq!(g(&op).get_record_size(), 5, "shown again as the typed size");
    // 4K warns (on most computers) but is allowed.
    g(&op).set_record_size(4);
    g(&op).invoke_record_size_changed();
    assert!(g(&op).get_size_warning().contains("Mbit/s"), "{}", g(&op).get_size_warning());
}
