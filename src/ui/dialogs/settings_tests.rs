use super::*;
use crate::ui::operator::files::testing::will_pick;
use crate::{
    app::{App, test_app},
    ui::{
        block_on,
        dialogs::{DialogKind, SettingsTab, settings_look::PREVIEW_DELAY},
        operator::{keys::{KeyCtx, route}, window::testing::{find, find_all, open as open_window, refresh}},
    },
};
use i_slint_backend_testing::mock_elapsed_time;
use slint::{Model, platform::PointerEventButton};
use std::{path::PathBuf, sync::Arc, time::Duration};

fn opened(app: &Arc<App>) -> Rc<Operator> {
    let op = open_window(app);
    op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
    slint::platform::update_timers_and_animations();
    refresh(&op);
    op
}

fn g(op: &Operator) -> SettingsDlg<'_> { op.ui.global::<SettingsDlg>() }

fn settle() { slint::platform::update_timers_and_animations(); }

fn click(op: &Operator, id: &str) {
    settle();
    find(op, id).unwrap_or_else(|| panic!("{id} not found")).mock_single_click(PointerEventButton::Left);
    settle();
}

fn toasts(op: &Operator) -> Vec<String> {
    find_all(op, "ToastStack::toast").iter().map(|t| t.accessible_label().unwrap_or_default().to_string()).collect()
}

fn pixels(img: &slint::Image) -> Vec<u8> { img.to_rgba8().map(|p| p.as_bytes().to_vec()).unwrap_or_default() }

fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rp-settings-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn top_bar_settings_opens_on_look() {
    let app = test_app();
    let op = opened(&app);
    op.ui.invoke_settings();
    assert_eq!(op.dialog(), Some(DialogKind::Settings(SettingsTab::Look)));
    assert_eq!(op.ui.get_dialog_title(), "Settings");
    assert!(op.ui.get_dialog_wide());
    assert!(find(&op, "SettingsDialog::save").is_some());
    assert_eq!(g(&op).get_tab(), 0);
}

#[test]
fn save_settings_round_trips_theme_and_lines() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Look);
    let f = g(&op);
    f.set_font_index(4);
    f.set_size(120);
    f.set_scripture_size(70);
    f.set_color("#ffcc00".into());
    f.set_position(1);
    f.set_shadow(false);
    f.set_uppercase(true);
    f.invoke_look_changed();
    f.set_lines(3);
    f.set_credits(2);
    f.set_ccli("12345".into());
    assert!(f.get_room_on_top(), "on by default");
    f.set_room_on_top(false);
    assert!(!f.get_keep_drawings() && f.get_stage_drawings(), "drawings: cleared on a new slide, shown on stage");
    f.set_keep_drawings(true);
    f.set_stage_drawings(false);
    assert_eq!((f.get_drawing_limit(), f.get_fade_secs().to_string()), (false, "3".to_string()), "no limits; ink fades after 3 s");
    f.set_drawing_limit(true);
    f.set_drawing_max_strokes("40".into());
    f.set_drawing_max_per_second("abc".into());
    f.set_fade_secs("90".into());
    assert!(f.get_section_keys(), "section keys on by default");
    f.set_section_keys(false);
    assert_eq!(f.get_audio_labels().row_count(), 1, "the default, before a list arrives");
    assert_eq!(f.get_audio_index(), 0);
    click(&op, "SettingsDialog::save");
    let s = app.data.lock().unwrap().settings.clone();
    assert_eq!((s.theme.font.as_str(), s.theme.size, s.theme.scripture_size), ("Arial", 120, 70));
    assert_eq!((s.theme.color.as_str(), s.theme.position.as_str(), s.theme.shadow, s.theme.uppercase), ("#ffcc00", "lower", false, true));
    assert_eq!((s.lines_per_slide, s.credits.as_str(), s.church_ccli.as_str()), (4, "none", "12345"));
    assert!(!s.room_on_top, "Settings → Video can let the room screen go behind other windows");
    assert!(s.keep_drawings && !s.stage_drawings, "Settings → Video: the two drawing choices");
    assert_eq!((s.drawing_limit, s.drawing_max_strokes, s.drawing_max_per_second, s.fade_secs), (true, 40, 30, 30), "not a number: unchanged; too many seconds: 30");
    assert_eq!(app.live.lock().unwrap().drawing.fade_after, std::time::Duration::from_secs(30));
    assert!(app.live.lock().unwrap().keep_drawing, "the live engine follows at once");
    assert!(!s.section_keys, "Settings → Songs can turn the section keys off");
    assert_eq!(op.dialog(), None, "Save closes the dialog");
    assert!(toasts(&op).contains(&"Settings saved".to_string()));

    // Opening again shows what was saved.
    refresh(&op);
    open(&op, SettingsTab::Songs);
    assert_eq!((g(&op).get_lines(), g(&op).get_credits(), g(&op).get_ccli().to_string()), (3, 2, "12345".to_string()));
    assert_eq!(g(&op).get_font_index(), 4);
}

#[test]
fn bible_verses_per_slide_round_trips_and_sits_with_the_scripture_size() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Look);
    assert_eq!(g(&op).get_bible_per_slide(), 0, "Automatic by default");
    assert!(find(&op, "SettingsLook::bible-per-slide").is_some());
    assert!(find(&op, "SettingsLook::scripture-size").is_some());
    g(&op).set_bible_per_slide(2);
    g(&op).set_scripture_size(90);
    click(&op, "SettingsDialog::save");
    let s = app.data.lock().unwrap().settings.clone();
    assert_eq!((s.bible_per_slide, s.theme.scripture_size), (2, 90));
    refresh(&op);
    open(&op, SettingsTab::Look);
    assert_eq!(g(&op).get_bible_per_slide(), 2, "opening again shows what was saved");
}

#[test]
fn save_settings_keeps_translation_and_displays_changed_meanwhile() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Songs);
    g(&op).set_ccli("777".into());
    block_on(app.do_action("settings/translation", br#"{"translation":"WEB"}"#)).unwrap();
    app.data.lock().unwrap().settings.screens.assign("audience", crate::model::ScreenTarget::Monitor(r"\\.\DISPLAY2".into()));
    refresh(&op);
    click(&op, "SettingsDialog::save");
    let s = app.data.lock().unwrap().settings.clone();
    assert_eq!(s.translation, "WEB", "the translation picked meanwhile is kept");
    assert_eq!(s.screens.monitors("audience"), [r"\\.\DISPLAY2"], "the screen chosen meanwhile is kept");
    assert_eq!(s.church_ccli, "777");
}

#[test]
fn cancel_discards_changes() {
    let app = test_app();
    let op = opened(&app);
    let before = app.data.lock().unwrap().settings.clone();
    open(&op, SettingsTab::Songs);
    g(&op).set_ccli("999".into());
    g(&op).set_lines(5);
    click(&op, "SettingsDialog::cancel");
    assert_eq!(op.dialog(), None);
    let after = app.data.lock().unwrap().settings.clone();
    assert_eq!((after.church_ccli, after.lines_per_slide), (before.church_ccli.clone(), before.lines_per_slide));
    open(&op, SettingsTab::Songs);
    assert_eq!(g(&op).get_ccli(), before.church_ccli.as_str(), "a new Settings starts from the saved settings");
}

#[test]
fn settings_keeps_typed_text_across_a_data_update() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Songs);
    g(&op).set_ccli("typed by hand".into());
    g(&op).set_lines(0);
    // A phone or a conversion changes the data meanwhile.
    app.data.lock().unwrap().settings.church_ccli = "from elsewhere".into();
    refresh(&op);
    refresh(&op);
    assert_eq!(g(&op).get_ccli(), "typed by hand");
    assert_eq!(g(&op).get_lines(), 0);
    assert_eq!(op.dialog(), Some(DialogKind::Settings(SettingsTab::Songs)));
    click(&op, "SettingsDialog::save");
    assert_eq!(app.data.lock().unwrap().settings.church_ccli, "typed by hand");
}

#[test]
fn look_preview_rerenders_after_change() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Look);
    let first = pixels(&g(&op).get_preview());
    assert!(!first.is_empty(), "Look draws its preview when it opens");
    g(&op).set_uppercase(true);
    g(&op).set_size(150);
    g(&op).invoke_look_changed();
    mock_elapsed_time(PREVIEW_DELAY - Duration::from_millis(10));
    assert_eq!(pixels(&g(&op).get_preview()), first, "not before 150 ms");
    mock_elapsed_time(Duration::from_millis(20));
    let second = pixels(&g(&op).get_preview());
    assert!(!second.is_empty());
    assert_ne!(second, first, "redrawn from the edits");
}

#[test]
fn look_preview_rebuilt_on_tab_return() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Look);
    settle();
    let first = pixels(&g(&op).get_preview());
    g(&op).set_shadow(false);
    g(&op).set_position(1);
    g(&op).invoke_look_changed();
    // Leave before the preview is drawn again: the image goes.
    let tabs = find_all(&op, "Tabs::tab");
    assert_eq!(tabs.len(), 9);
    tabs[1].mock_single_click(PointerEventButton::Left);
    settle();
    assert_eq!(g(&op).get_tab(), 1);
    assert!(pixels(&g(&op).get_preview()).is_empty(), "switching tabs drops the preview");
    mock_elapsed_time(PREVIEW_DELAY * 2);
    assert!(pixels(&g(&op).get_preview()).is_empty(), "the pending redraw was dropped too");
    find_all(&op, "Tabs::tab")[0].mock_single_click(PointerEventButton::Left);
    settle();
    let back = pixels(&g(&op).get_preview());
    assert!(!back.is_empty(), "Look draws it again");
    assert_ne!(back, first, "from the current edits, not a stale image");
}

#[test]
fn missing_font_shows_note() {
    let app = test_app();
    app.data.lock().unwrap().settings.theme.font = "No Such Font".into();
    let op = opened(&app);
    open(&op, SettingsTab::Look);
    settle();
    assert_eq!(g(&op).get_font_note(), "“No Such Font” isn't installed on this computer, so slides use Noto Sans.");
    let note = find(&op, "SettingsLook::font-note").expect("the note under the font box");
    assert_eq!(note.accessible_label().unwrap_or_default(), g(&op).get_font_note());
    let fonts: Vec<String> = g(&op).get_fonts().iter().map(|f| f.to_string()).collect();
    assert_eq!(fonts[g(&op).get_font_index() as usize], "No Such Font", "the box shows the saved font");
    op.close_dialog();

    app.data.lock().unwrap().settings.theme.font = "Noto Sans".into();
    refresh(&op);
    open(&op, SettingsTab::Look);
    settle();
    assert_eq!(g(&op).get_font_note(), "");
    assert!(find(&op, "SettingsLook::font-note").is_none());
}

#[test]
fn choose_logo_imports_a_picture_and_remove_clears_it() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Look);
    settle();
    assert!(find(&op, "SettingsLook::no-logo").is_some());
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(40, 20, image::Rgba([200, 30, 30, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    will_pick(vec![temp_file("logo.png", &png)]);
    click(&op, "SettingsLook::choose-logo");
    assert!(g(&op).get_has_logo());
    assert!(!pixels(&g(&op).get_logo()).is_empty(), "the 44 px logo preview");
    assert!(find(&op, "SettingsLook::no-logo").is_none());
    click(&op, "SettingsDialog::save");
    let logo = app.data.lock().unwrap().settings.logo.clone().expect("logo saved");
    assert!(app.store.asset_path(&logo).exists());

    refresh(&op);
    open(&op, SettingsTab::Look);
    click(&op, "SettingsLook::remove-logo");
    assert!(!g(&op).get_has_logo());
    click(&op, "SettingsDialog::save");
    assert_eq!(app.data.lock().unwrap().settings.logo, None);
}

#[test]
fn default_background_pick_is_saved() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Look);
    let rows: Vec<_> = g(&op).get_backgrounds().iter().collect();
    assert_eq!(rows.len(), 14, "the six swatches, the custom colour, six gradients and Upload (no media, no Theme default)");
    assert!(rows[0].on, "black is the default");
    g(&op).invoke_pick_background(1);
    assert!(g(&op).get_backgrounds().row_data(1).unwrap().on);
    click(&op, "SettingsDialog::save");
    assert_eq!(app.data.lock().unwrap().settings.background, crate::model::Background::Color { color: "#0b1a33".into() });
}

#[test]
fn default_background_custom_colour_is_saved() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Look);
    g(&op).invoke_background_hex("#12AB3".into());
    g(&op).invoke_background_hex("#12AB34".into());
    assert!(g(&op).get_backgrounds().iter().all(|r| !r.on), "a custom colour is no tile");
    click(&op, "SettingsDialog::save");
    assert_eq!(app.data.lock().unwrap().settings.background, crate::model::Background::Color { color: "#12ab34".into() });
}

#[test]
fn text_color_from_the_grid_and_its_hex_field() {
    let app = test_app();
    let op = opened(&app);
    // Tall enough to show the whole Look page: `find` skips what the dialog's scroll box clips, and Text color sits
    // below the Bible verses group.
    op.window().set_size(slint::LogicalSize::new(1400.0, 1600.0));
    settle();
    open(&op, SettingsTab::Look);
    assert!(find(&op, "ColorGrid::hex").is_none(), "the grid opens from the swatch");
    click(&op, "ColorPicker::swatch");
    assert!(find(&op, "ColorGrid::hex").is_some());
    let before = g(&op).get_color();
    g(&op).invoke_text_hex("#12AB3".into());
    assert_eq!(g(&op).get_color(), before, "half a colour is ignored");
    g(&op).invoke_text_hex("#12AB34".into());
    assert_eq!(g(&op).get_color(), "#12ab34");
    g(&op).invoke_text_color(slint::Color::from_rgb_u8(255, 0, 0));
    assert_eq!(g(&op).get_color(), "#ff0000");
    click(&op, "SettingsDialog::save");
    assert_eq!(app.data.lock().unwrap().settings.theme.color, "#ff0000");
}

#[test]
fn backup_label_formats_like_web() {
    assert_eq!(backup_label("2026-11-01_140512"), "2026-11-01 at 14:05 UTC");
    assert_eq!(backup_label("2026-01-09_000000"), "2026-01-09 at 00:00 UTC");
    assert_eq!(backup_label("odd"), "odd UTC", "an unexpected stamp is shown as it is");
}

#[test]
fn backup_now_toasts_and_lists_it() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Backups);
    settle();
    assert!(find(&op, "SettingsBackups::no-backups").is_some());
    assert!(g(&op).get_backups_folder().ends_with("\\backups"));
    click(&op, "SettingsBackups::backup-now");
    assert_eq!(toasts(&op), ["Backup made"]);
    refresh(&op);
    settle();
    let rows: Vec<_> = g(&op).get_backups().iter().collect();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].label.ends_with(" UTC"));
    assert_eq!(rows[0].counts, "0 songs · 1 services · 0 files");
}

#[test]
fn restore_asks_then_restores() {
    let app = test_app();
    let data = app.data.lock().unwrap().clone();
    app.store.backup(&data).unwrap();
    let op = opened(&app);
    op.ops().call::<serde_json::Value>("song/save", serde_json::json!({ "id": uuid::Uuid::nil(), "title": "After the backup", "lyrics": "la" }), |r| { r.unwrap(); });
    refresh(&op);
    open(&op, SettingsTab::Backups);
    settle();
    click(&op, "SettingsBackups::restore");
    let ask = op.ui.get_ask().row_data(0).expect("a confirm");
    assert_eq!(ask.text, "Restore this backup? Your current songs and services are replaced (a copy of them is kept).");
    // Cancel keeps everything.
    click(&op, "AskOverlay::cancel");
    assert_eq!(app.data.lock().unwrap().songs.len(), 1);
    assert_eq!(op.dialog(), Some(DialogKind::Settings(SettingsTab::Backups)));
    click(&op, "SettingsBackups::restore");
    click(&op, "AskOverlay::ok");
    assert!(app.data.lock().unwrap().songs.is_empty(), "the backup's data is back");
    assert_eq!(op.dialog(), None);
    assert_eq!(toasts(&op), ["Backup restored"]);
}

#[test]
fn help_lists_every_bound_key() {
    let rows = help_rows();
    let same = |a: &KeyAction, b: &KeyAction| std::mem::discriminant(a) == std::mem::discriminant(b) && match (a, b) {
        (KeyAction::Clear(x), KeyAction::Clear(y)) => x == y,
        _ => true,
    };
    let plain = KeyCtx { in_text_field: false, in_bible_box: false, dialog_open: false };
    let typing = KeyCtx { in_text_field: true, in_bible_box: false, dialog_open: false };
    let bible = KeyCtx { in_text_field: true, in_bible_box: true, dialog_open: false };
    // Each key a row names does what the row says, where it applies.
    for row in &rows {
        for &(key, ctrl, alt) in row.binds {
            let got = [plain, typing, bible].iter().map(|c| route(key, ctrl, alt, *c)).find(|a| *a != KeyAction::None);
            assert!(got.as_ref().is_some_and(|a| same(a, &row.action)), "{} ({key:?}) is {got:?}, Help says {:?}", row.keys, row.action);
        }
    }
    // Every key that does something is in Help.
    let mut keys: Vec<(String, bool, bool)> = ["ArrowRight", "ArrowLeft", "ArrowDown", "ArrowUp", "PageDown", "PageUp", "Enter", "Escape", " ", "F1", "F2", "F3", "F4", "F5", "F6", "Tab", "Home", "End"]
        .iter().map(|k| (k.to_string(), false, false)).collect();
    for c in ('a'..='z').chain('0'..='9') {
        for (ctrl, alt) in [(false, false), (true, false), (false, true)] { keys.push((c.to_string(), ctrl, alt)) }
    }
    for (key, ctrl, alt) in keys {
        for ctx in [plain, typing, bible] {
            let action = route(&key, ctrl, alt, ctx);
            if action == KeyAction::None || (ctx.in_text_field && action == KeyAction::None) { continue }
            let row = rows.iter().find(|r| same(&r.action, &action)).unwrap_or_else(|| panic!("{key:?} ctrl={ctrl} alt={alt} does {action:?}, not in Help"));
            let named = row.binds.iter().any(|(k, c, a)| k.eq_ignore_ascii_case(&key) && *c == ctrl && *a == alt)
                || (row.keys == strings::key_alt_digits() && alt && ('1'..='5').contains(&key.chars().next().unwrap()));
            assert!(named, "{key:?} ctrl={ctrl} alt={alt} is not named in the Help row {:?}", row.keys);
        }
    }
    // The dialog shows the same rows, with the ↓/↑ wording.
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Help);
    settle();
    let shown: Vec<(String, String)> = g(&op).get_help().iter().map(|r| (r.keys.to_string(), r.what.to_string())).collect();
    assert_eq!(shown, rows.iter().map(|r| (r.keys.to_string(), r.what.to_string())).collect::<Vec<_>>());
    assert_eq!(shown[0], ("→  ↓  Space  Page Down".to_string(), "Next".to_string()));
    assert_eq!(shown[1], ("←  ↑  Page Up".to_string(), "Back".to_string()));
    assert!(shown.contains(&("Alt + 1–5".to_string(), "Pick a Bible chip (in the Bible box)".to_string())));
    assert_eq!(find_all(&op, "SettingsHelp::help-row").len(), rows.len());
}

#[test]
fn bible_tab_shows_sources_keys_and_import() {
    let app = test_app();
    let mut secrets = crate::bible::sources::Secrets::default();
    secrets.keys.insert("esv".into(), "k".into());
    secrets.save(&app.store.root).unwrap();
    let op = opened(&app);
    open(&op, SettingsTab::Bible);
    settle();
    let sources: Vec<_> = g(&op).get_bible_sources().iter().collect();
    for t in ["KJV", "WEB", "BSB"] {
        let row = sources.iter().find(|r| r.translation == t).unwrap_or_else(|| panic!("{t}"));
        assert_eq!((row.chip.as_str(), row.chip_state.as_str()), ("Ready — offline", "ok"), "{t}");
    }
    let esv = sources.iter().find(|r| r.translation == "ESV").unwrap();
    assert_eq!(esv.chip, "Ready — online");
    let needs = sources.iter().find(|r| r.chip == "Needs a free key").expect("a translation that needs a key");
    assert!(needs.hint.starts_with("Connect ") && needs.hint.ends_with(" below"), "{}", needs.hint);
    let providers: Vec<_> = g(&op).get_bible_providers().iter().collect();
    let i = providers.iter().position(|p| p.label == "ESV API (Crossway)").unwrap();
    assert!(providers[i].key_set);
    assert!(providers[i].for_text.starts_with("For ESV"));

    // Disconnect: an empty key removes it.
    g(&op).invoke_save_key(i as i32, "".into());
    assert!(crate::bible::sources::Secrets::load(&app.store.root).keys.is_empty());
    assert_eq!(toasts(&op), ["ESV API (Crossway) disconnected"]);
    refresh(&op);
    assert!(!g(&op).get_bible_providers().row_data(i).unwrap().key_set, "redrawn from the fresh read");

    // Import a file under the typed short name.
    let file = br#"<?xml version="1.0"?><XMLBIBLE biblename="Test"><INFORMATION><title>Holman Test</title><identifier>HCSB</identifier><rights>(c) Holman</rights></INFORMATION>
        <BIBLEBOOK bnumber="43"><CHAPTER cnumber="3"><VERS vnumber="16">For God loved.</VERS><VERS vnumber="17">For God sent.</VERS></CHAPTER></BIBLEBOOK></XMLBIBLE>"#;
    g(&op).set_bible_short(" hcsb ".into());
    will_pick(vec![temp_file("holman.xml", file)]);
    g(&op).invoke_import_bible();
    assert_eq!(toasts(&op).last().map(String::as_str), Some("Added HCSB (2 verses)"));
    refresh(&op);
    let sources: Vec<_> = g(&op).get_bible_sources().iter().collect();
    let hcsb = sources.iter().position(|r| r.translation == "HCSB").expect("the imported Bible is listed");
    assert!(sources[hcsb].imported);
    assert!(g(&op).get_bible_credits().iter().any(|c| c.text.contains("(c) Holman")));

    g(&op).invoke_remove_import(hcsb as i32);
    assert_eq!(toasts(&op).last().map(String::as_str), Some("HCSB removed"));
    refresh(&op);
    assert!(!g(&op).get_bible_sources().iter().any(|r| r.imported));
    assert_eq!(op.dialog(), Some(DialogKind::Settings(SettingsTab::Bible)), "Bible actions keep Settings open");
}

#[test]
fn video_tab_shows_player_status_not_ffmpeg() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Video);
    settle();
    assert!(find(&op, "SettingsVideo::player").is_some(), "the player line");
    assert!(find(&op, "SettingsVideo::drawing").is_some(), "the drawing line");
    for gone in ["SettingsVideo::missing", "SettingsVideo::found", "SettingsVideo::check-again"] {
        assert!(find(&op, gone).is_none(), "{gone}");
    }
}

#[test]
fn the_graphics_picker_lists_this_builds_choices_and_saves_at_once() {
    use crate::{model::Graphics, video::graphics};
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Video);
    settle();
    assert!(find(&op, "SettingsVideo::graphics").is_some(), "the picker");
    assert!(find(&op, "SettingsVideo::graphics-now").is_some(), "the line saying what is in use");
    let labels: Vec<String> = g(&op).get_graphics_labels().iter().map(|l| l.to_string()).collect();
    let want: Vec<String> = graphics::available(graphics::Build::this()).into_iter().map(graphics::label).collect();
    assert_eq!(labels, want, "only what this build can draw with");
    assert_eq!(labels.first().map(String::as_str), Some("Automatic (recommended)"));
    assert_eq!(labels.last().map(String::as_str), Some("Software (safe mode)"));
    assert_eq!(g(&op).get_graphics_index(), 0, "Auto by default");
    assert_eq!(g(&op).get_graphics_now(), "In use now: Software", "tests never choose a backend");

    let last = labels.len() as i32 - 1;
    g(&op).set_graphics_index(last);
    g(&op).invoke_graphics_changed(last);
    settle();
    assert_eq!(app.data.lock().unwrap().settings.graphics, Graphics::Software);
    assert!(toasts(&op).contains(&"Restart RustyPresenter to use the new graphics setting.".to_string()));
    assert_eq!(op.dialog(), Some(DialogKind::Settings(SettingsTab::Video)), "the dialog stays open");

    // Save keeps it, and opening again shows it.
    click(&op, "SettingsDialog::save");
    assert_eq!(app.data.lock().unwrap().settings.graphics, Graphics::Software);
    refresh(&op);
    open(&op, SettingsTab::Video);
    assert_eq!(g(&op).get_graphics_index(), last);
}

#[test]
fn the_sound_output_is_saved_and_shown_again() {
    let app = test_app();
    let op = opened(&app);
    app.data.lock().unwrap().settings.audio_device = "wasapi/{board}".into();
    refresh(&op);
    open(&op, SettingsTab::Video);
    assert_eq!(g(&op).get_audio_index(), 1, "kept though no list came");
    click(&op, "SettingsDialog::save");
    assert_eq!(app.data.lock().unwrap().settings.audio_device, "wasapi/{board}");
    refresh(&op);
    open(&op, SettingsTab::Video);
    g(&op).set_audio_index(0);
    click(&op, "SettingsDialog::save");
    assert_eq!(app.data.lock().unwrap().settings.audio_device, "");
}

/// A saved eBible list checked just now, so `bible/catalog` reads it from disk and asks no one.
fn seed_catalog(app: &App) {
    let dir = app.store.root.join("bibles/catalog");
    std::fs::create_dir_all(&dir).unwrap();
    let head = "languageCode,translationId,languageName,languageNameInEnglish,title,Redistributable,Copyright,UpdateDate,OTbooks,NTbooks,DCbooks,textDirection,downloadable,shortTitle,script\n";
    let rows = [
        "spa,spaRV1909,Español,Spanish,Santa Biblia Reina-Valera 1909,True,public domain,2025-01-02,39,27,0,ltr,True,Reina-Valera 1909,Latn",
        "eng,engwebp,English,English,World English Bible,True,public domain,2026-03-01,39,27,0,ltr,True,World English Bible,Latn",
        "eng,engbrent,English,English,Brenton Septuagint in English,True,public domain,2024-01-01,39,0,15,ltr,True,Brenton,Latn",
        "grc,grcbrent,Ελληνικά,Ancient Greek,Brenton Greek Septuagint,True,public domain,2024-01-01,39,0,15,ltr,True,Brenton,Grek",
        "amh,amh2005,አማርኛ,Amharic,መጽሐፍ ቅዱስ,True,public domain,2024-01-01,39,27,0,ltr,True,Amharic Bible,Ethi",
    ];
    std::fs::write(dir.join("ebible-translations.csv"), format!("{head}{}\n", rows.join("\n"))).unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    std::fs::write(dir.join("ebible.json"), format!(r#"{{"etag":"","last_modified":"","checked_at":{now}}}"#)).unwrap();
}

fn catalog_lines(op: &Operator) -> Vec<String> {
    g(op).get_catalog_rows().iter()
        .map(|r| if r.heading.is_empty() { format!("{} {} [{}]", r.abbr, r.title, r.button) } else { format!("# {}", r.heading) })
        .collect()
}

#[test]
fn bible_library_lists_searches_and_filters_the_saved_catalog() {
    let app = test_app();
    seed_catalog(&app);
    let op = opened(&app);
    open(&op, SettingsTab::Look);
    settle();
    assert_eq!(g(&op).get_catalog_rows().row_count(), 0, "nothing is asked for before the Bible tab is shown");
    g(&op).set_tab(tab_index(SettingsTab::Bible));
    g(&op).invoke_tab_changed(tab_index(SettingsTab::Bible));
    settle();
    let lines = catalog_lines(&op);
    assert_eq!(lines[0], "# English", "the church's language first: {lines:?}");
    assert!(lines.iter().any(|l| l.contains(" World English Bible [Add]")), "{lines:?}");
    let greek = lines.iter().position(|l| l.starts_with("# Ancient Greek")).expect("Greek heading");
    let spanish = lines.iter().position(|l| l.starts_with("# Spanish")).expect("Spanish heading");
    assert!(greek < spanish, "then English names A to Z: {lines:?}");
    assert!(lines.iter().any(|l| l == "# Spanish · Español"), "{lines:?}");
    assert!(find(&op, "SettingsBible::catalog-search").is_some());

    // Search folds accents and case, and needs every word.
    g(&op).set_catalog_search("REINA valéra".into());
    g(&op).invoke_catalog_changed();
    let lines = catalog_lines(&op);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[1].ends_with("Santa Biblia Reina-Valera 1909 [Add]"), "{lines:?}");
    g(&op).set_catalog_search("brenton".into());
    g(&op).invoke_catalog_changed();
    assert_eq!(catalog_lines(&op).len(), 4, "two languages, a heading each");

    // A language.
    let langs: Vec<String> = g(&op).get_catalog_languages().iter().map(|s| s.to_string()).collect();
    assert_eq!(langs[0], "All languages");
    let grc = langs.iter().position(|l| l.starts_with("Ancient Greek")).unwrap();
    g(&op).set_catalog_language(grc as i32);
    g(&op).invoke_catalog_changed();
    let lines = catalog_lines(&op);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0], "# Ancient Greek · Ελληνικά");
    assert!(lines[1].contains("Brenton Greek Septuagint [Add]"), "{lines:?}");

    // Installed: nothing yet.
    g(&op).set_catalog_search("".into());
    g(&op).set_catalog_language(0);
    g(&op).set_catalog_filter(0);
    g(&op).invoke_catalog_changed();
    assert_eq!(g(&op).get_catalog_rows().row_count(), 0);
    assert!(g(&op).get_catalog_note().starts_with("No Bibles added yet"));
}

#[test]
fn bible_library_warns_on_a_row_no_font_can_draw() {
    let app = test_app();
    seed_catalog(&app);
    crate::ui::dialogs::settings_bible::check_with_bundled_fonts();
    let op = opened(&app);
    open(&op, SettingsTab::Bible);
    settle();
    let rows: Vec<_> = g(&op).get_catalog_rows().iter().filter(|r| r.heading.is_empty()).collect();
    let warned: Vec<&str> = rows.iter().filter(|r| !r.font_warning.is_empty()).map(|r| r.title.as_str()).collect();
    assert_eq!(warned, ["መጽሐፍ ቅዱስ"], "Latin and Greek are drawn by the bundled Noto Sans; Ethiopic isn't");
    let amh = rows.iter().find(|r| r.title == "መጽሐፍ ቅዱስ").unwrap();
    assert_eq!((amh.font_warning.as_str(), amh.button.as_str()), ("Your computer may not have a font for this language.", "Add"));
    assert!(find(&op, "SettingsBible::font-warning").is_some(), "the warning is on the page");
}

#[test]
fn bible_library_row_follows_its_download_job() {
    let app = test_app();
    seed_catalog(&app);
    let op = opened(&app);
    open(&op, SettingsTab::Bible);
    settle();
    let web = |op: &Operator| g(op).get_catalog_rows().iter().find(|r| r.title == "World English Bible").unwrap();
    assert_eq!((web(&op).button.as_str(), web(&op).progress), ("Add", -1.0));

    let (job, cancel) = app.job_start_cancellable("Waiting to download World English Bible…");
    app.job_subject(job, "ebible:engwebp");
    refresh(&op);
    assert_eq!((web(&op).button.as_str(), web(&op).status.as_str()), ("Cancel", "Waiting"));
    app.job_label(job, "Downloading World English Bible…");
    app.job_progress(job, 40);
    refresh(&op);
    assert_eq!((web(&op).status.as_str(), web(&op).progress), ("Downloading… 40%", 40.0));

    let n = g(&op).get_catalog_rows().iter().position(|r| r.title == "World English Bible").unwrap();
    g(&op).invoke_catalog_button(n as i32);
    assert!(cancel.is_cancelled(), "Cancel cancels the row's job");
}

/// An eBible download as `bible/install` leaves it: prefixed id, its own abbreviation, licence and source.
fn install_download(app: &App) {
    use crate::bible::{import::{ImportedBible, install}, library::{BibleMeta, Licence, Source}};
    let meta = BibleMeta {
        id: "ebible:spaRVR".into(), abbr: "RVR".into(), name: "Reina Valera Revisada".into(), lang: "spa".into(),
        lang_name_en: "Spanish".into(), copyright: "Copyright © 2020 Sociedad Ejemplo".into(),
        licence: Licence::Cc { kind: "by-sa".into(), version: "4.0".into(), url: "https://creativecommons.org/licenses/by-sa/4.0/".into() },
        source: Source::Ebible { update_date: "2026-03-01".into() }, ..Default::default()
    };
    let verses = [((42u8, 3u16, 16u16), "Porque de tal manera amó Dios al mundo.".to_string())].into_iter().collect();
    install(&app.store.root, ImportedBible { abbreviation: meta.id.clone(), name: meta.name.clone(), copyright: meta.copyright.clone(), verses, meta: Some(meta) }).unwrap();
}

#[test]
fn default_bible_lists_what_can_be_used_and_is_applied_at_once() {
    let app = test_app();
    install_download(&app);
    let op = opened(&app);
    open(&op, SettingsTab::Bible);
    settle();
    assert!(find(&op, "SettingsBible::default-bible").is_some());
    let options: Vec<String> = g(&op).get_default_bibles().iter().map(|s| s.to_string()).collect();
    assert!(options.contains(&"KJV — King James Version".to_string()), "{options:?}");
    assert!(options.contains(&"RVR — Reina Valera Revisada".to_string()), "a download by its abbreviation: {options:?}");
    assert!(!options.iter().any(|o| o.starts_with("ESV")), "a Bible that needs a key isn't offered: {options:?}");
    let kjv = options.iter().position(|o| o.starts_with("KJV")).unwrap();
    assert_eq!(g(&op).get_default_bible(), kjv as i32, "the one in use");

    let rvr = options.iter().position(|o| o.starts_with("RVR")).unwrap();
    g(&op).set_default_bible(rvr as i32);
    g(&op).invoke_default_bible_changed(rvr as i32);
    assert_eq!(app.data.lock().unwrap().settings.translation, "ebible:spaRVR", "applied before Save, by its id");
    assert_eq!(toasts(&op).last().map(String::as_str), Some("Default Bible changed"));
    click(&op, "SettingsDialog::cancel");
    assert_eq!(app.data.lock().unwrap().settings.translation, "ebible:spaRVR", "Cancel doesn't undo it");
    refresh(&op);
    open(&op, SettingsTab::Bible);
    settle();
    let options: Vec<String> = g(&op).get_default_bibles().iter().map(|s| s.to_string()).collect();
    assert!(options[g(&op).get_default_bible() as usize].starts_with("RVR"), "shown again: {options:?}");
}

#[test]
fn credits_show_each_downloaded_bible_with_its_licence_and_link() {
    let app = test_app();
    install_download(&app);
    let op = opened(&app);
    open(&op, SettingsTab::Bible);
    settle();
    let credits: Vec<_> = g(&op).get_bible_credits().iter().collect();
    let c = credits.iter().find(|c| c.text.contains("Sociedad Ejemplo")).unwrap_or_else(|| panic!("{credits:?}"));
    assert_eq!(c.text, "RVR: Copyright © 2020 Sociedad Ejemplo (CC BY-SA 4.0)");
    assert_eq!(c.url, "https://creativecommons.org/licenses/by-sa/4.0/");
    let sources: Vec<_> = g(&op).get_bible_sources().iter().collect();
    let row = sources.iter().find(|r| r.translation == "RVR").unwrap_or_else(|| panic!("listed by abbreviation"));
    assert_eq!(row.hint, "Reina Valera Revisada — downloaded from eBible.org");
    assert!(row.imported, "it can still be removed here");
}

#[test]
fn language_tab_lists_languages_and_switches_the_app_at_once() {
    let app = test_app();
    let op = opened(&app);
    open(&op, SettingsTab::Language);
    settle();
    assert_eq!(g(&op).get_tab(), 2, "Language comes right after Look and Themes");
    let tab_labels = || find_all(&op, "Tabs::tab").iter().map(|t| t.accessible_label().unwrap_or_default().to_string()).collect::<Vec<_>>();
    assert_eq!(tab_labels()[..3], ["Look", "Themes", "Language"]);
    let rows: Vec<crate::ui::SettingsLanguageRow> = g(&op).get_languages().iter().collect();
    assert!(rows[0].title.starts_with("Use the computer's language ("), "{:?}", rows[0].title);
    assert!(rows[0].on, "new data follows the computer");
    assert_eq!(rows[1].title, "English");
    let es = rows.iter().position(|r| r.title == "Español (needs review)").expect("Spanish is listed by its own name");
    assert!(rows[es].sub.starts_with("Spanish · ") && rows[es].sub.ends_with("% translated"), "{:?}", rows[es].sub);
    // The search skips rows scrolled out of view, so this counts the rows on screen (the chosen one is first).
    let shown = find_all(&op, "SettingsLanguage::language-row").len();
    assert!(shown > 2 && shown <= rows.len(), "{shown} of {} rows", rows.len());
    assert_eq!(find_all(&op, "SettingsLanguage::use-language").len(), shown - 1, "every row but the chosen one has Use");

    g(&op).invoke_choose_language(es as i32);
    settle();
    assert_eq!(crate::i18n::current(), "es");
    assert_eq!(app.data.lock().unwrap().settings.ui_language, "es", "saved at once");
    assert!(op.toast_log().iter().any(|(t, _)| t == "Idioma cambiado"), "the toast is in Spanish: {:?}", op.toast_log());
    assert_eq!(format!("{:?}", slint::select_bundled_translation("es")), "Ok(())", "Spanish is bundled into the Slint side");
    assert_eq!(tab_labels()[2], "Idioma", "the window's own words changed without a restart");
    assert!(g(&op).get_languages().row_data(es).unwrap().on);

    // Cancel keeps it; English again from its row.
    g(&op).invoke_cancel();
    open(&op, SettingsTab::Language);
    settle();
    g(&op).invoke_choose_language(1);
    settle();
    assert_eq!((crate::i18n::current().as_str(), tab_labels()[2].as_str()), ("en", "Language"));
    assert_eq!(app.data.lock().unwrap().settings.ui_language, "en");
}
