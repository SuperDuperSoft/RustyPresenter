use super::*;
use crate::{
    app::{App, test_app},
    model::{ItemKind, ServiceItem},
    ui::{
        dialogs::{SettingsTab, settings},
        operator::window::testing::{find, find_all, open as open_window, refresh},
    },
};
use i_slint_backend_testing::mock_elapsed_time;
use slint::{Model, platform::PointerEventButton};
use std::sync::Arc;

fn opened(app: &Arc<App>) -> Rc<Operator> {
    let op = open_window(app);
    op.window().set_size(slint::LogicalSize::new(1520.0, 1400.0));
    slint::platform::update_timers_and_animations();
    refresh(&op);
    op
}

fn settle() { slint::platform::update_timers_and_animations(); }

fn click(op: &Operator, id: &str) {
    settle();
    find(op, id).unwrap_or_else(|| panic!("{id} not found")).mock_single_click(PointerEventButton::Left);
    settle();
}

fn toasts(op: &Operator) -> Vec<String> {
    find_all(op, "ToastStack::toast").iter().map(|t| t.accessible_label().unwrap_or_default().to_string()).collect()
}

/// The question on top, if one is up.
fn ask_text(op: &Operator) -> Option<String> { op.ui.get_ask().row_data(0).map(|a| a.text.to_string()) }

fn answer(op: &Operator, ok: bool) {
    settle();
    click(op, if ok { "AskOverlay::ok" } else { "AskOverlay::cancel" });
}

fn names(op: &Operator) -> Vec<String> { g(op).get_rows().iter().map(|r| r.name.to_string()).collect() }

fn pixels(img: &slint::Image) -> Vec<u8> { img.to_rgba8().map(|p| p.as_bytes().to_vec()).unwrap_or_default() }

fn saved(app: &App, name: &str) -> SavedTheme {
    let t = SavedTheme { id: Uuid::new_v4(), name: name.into(), style: crate::model::Theme { weight: 700, ..Default::default() }, background: None };
    app.data.lock().unwrap().themes.push(t.clone());
    t
}

fn row_of(op: &Operator, name: &str) -> i32 { names(op).iter().position(|n| n == name).unwrap_or_else(|| panic!("{name} is listed")) as i32 }

#[test]
fn themes_tab_comes_after_look_and_lists_standard_first_with_star() {
    let app = test_app();
    saved(&app, "Christmas");
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    settle();
    let labels: Vec<String> = find_all(&op, "Tabs::tab").iter().map(|t| t.accessible_label().unwrap_or_default().to_string()).collect();
    assert_eq!(labels[..3], ["Look", "Themes", "Language"]);
    assert_eq!(op.ui.global::<SettingsDlg>().get_tab(), 1);
    let mut expect = vec!["Standard".to_string()];
    expect.extend(theme::builtin().into_iter().map(|b| b.name));
    expect.push("Christmas".into());
    assert_eq!(names(&op), expect, "Standard, the 15 built-ins, then saved themes");
    let last = expect.len() - 1;
    let rows: Vec<ThemeListRow> = g(&op).get_rows().iter().collect();
    assert!(rows[0].is_default && rows[0].standard);
    assert_eq!(rows[0].sub, "Church default — used whenever nothing else is picked");
    assert_eq!((rows[0].delete_label.as_str(), rows[1].delete_label.as_str(), rows[last].delete_label.as_str()), ("", "", "Delete"),
        "Standard and an unedited built-in can't be deleted");
    let stars = find_all(&op, "Star::ta");
    assert_eq!(stars.len(), 2, "the ready-made themes wait behind their button");
    assert!(rows[1].ready_made && !rows[0].ready_made && !rows[last].ready_made);
    assert_eq!(find(&op, "ThemeList::ready-made").unwrap().accessible_label().unwrap(), "Show 15 ready-made themes");
    assert!(find(&op, "ThemeList::new-theme").is_some());
    click(&op, "ThemeList::ready-made");
    let shown: Vec<String> = find_all(&op, "ThemeList::theme-row").iter().map(|r| r.accessible_label().unwrap().into()).collect();
    assert_eq!(shown[..3], ["Standard", "Lower third", "Midnight"]);
}

#[test]
fn new_theme_saves_and_appears() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    click(&op, "ThemeList::new-theme");
    assert!(g(&op).get_editing());
    assert_eq!(g(&op).get_name(), "New theme");
    assert!(find(&op, "SettingsDialog::save").is_none(), "the footer saves the theme while editing");
    let f = g(&op);
    f.set_size(120);
    f.set_weight(2);
    f.set_align(0);
    f.set_position(0);
    f.set_outline(2);
    f.set_band(true);
    f.set_band_clear(30);
    f.invoke_changed();
    f.invoke_text_hex("#f5b94a".into());
    click(&op, "SettingsDialog::theme-save");
    let t = app.data.lock().unwrap().themes.iter().find(|t| t.name == "New theme").cloned().expect("saved");
    assert_eq!((t.style.size, t.style.weight, t.style.align, t.style.position.as_str()), (120, 700, TextAlign::Left, "top"));
    assert_eq!((t.style.outline, t.style.band, t.style.band_opacity, t.style.color.as_str()), (6, true, 70, "#f5b94a"));
    assert!(!g(&op).get_editing(), "back to the list");
    assert!(toasts(&op).contains(&"Theme saved".to_string()));
    assert!(names(&op).contains(&"New theme".to_string()));
    // A second New theme is not named the same.
    click(&op, "ThemeList::new-theme");
    assert_eq!(g(&op).get_name(), "New theme copy");
}

#[test]
fn a_duplicate_name_is_refused_and_the_editor_stays() {
    let app = test_app();
    saved(&app, "Christmas");
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    click(&op, "ThemeList::new-theme");
    g(&op).set_name("christmas".into());
    click(&op, "SettingsDialog::theme-save");
    assert!(toasts(&op).contains(&"There's already a theme called “christmas”.".to_string()));
    assert!(g(&op).get_editing());
}

#[test]
fn star_asks_then_moves_default() {
    let app = test_app();
    let t = saved(&app, "Christmas");
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    let i = row_of(&op, "Christmas");
    g(&op).invoke_make_default(i);
    settle();
    assert_eq!(ask_text(&op).as_deref(), Some("Make “Christmas” the church default? Everything without its own theme will use it. You can switch back any time."));
    answer(&op, true);
    assert_eq!(app.data.lock().unwrap().default_theme, Some(t.id));
    assert!(toasts(&op).contains(&"“Christmas” is now the church default".to_string()));
    let rows: Vec<ThemeListRow> = g(&op).get_rows().iter().collect();
    assert!(rows[row_of(&op, "Christmas") as usize].is_default);
    assert!(!rows[0].is_default, "Standard lost its star");
}

#[test]
fn delete_theme_asks_with_counts() {
    let app = test_app();
    let t = saved(&app, "Youth");
    {
        let mut d = app.data.lock().unwrap();
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "A".into(), body: "a".into(), layout: Default::default() }, background: None, notes: String::new(), theme: Some(t.id) };
        d.services[0].items.push(item);
    }
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    let i = row_of(&op, "Youth");
    assert_eq!(g(&op).get_rows().row_data(i as usize).unwrap().sub, "Used by 1 item");
    g(&op).invoke_delete(i);
    settle();
    assert_eq!(ask_text(&op).as_deref(), Some("Delete the theme “Youth”? 1 item uses it; they'll use the church default instead."));
    answer(&op, false);
    assert_eq!(app.data.lock().unwrap().themes.len(), 1, "Cancel keeps it");
    g(&op).invoke_delete(i);
    answer(&op, true);
    let d = app.data.lock().unwrap();
    assert!(d.themes.is_empty());
    assert_eq!(d.services[0].items[0].theme, None, "the item falls back to the church default");
    drop(d);
    assert!(toasts(&op).contains(&"Theme deleted".to_string()));
    assert!(!names(&op).contains(&"Youth".to_string()));
}

#[test]
fn edited_builtin_offers_reset() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    g(&op).invoke_edit(row_of(&op, "Lower third"));
    settle();
    assert!(g(&op).get_editing());
    g(&op).set_size(70);
    click(&op, "SettingsDialog::theme-save");
    let i = row_of(&op, "Lower third") as usize;
    assert_eq!(g(&op).get_rows().row_data(i).unwrap().delete_label, "Reset");
    g(&op).invoke_delete(i as i32);
    settle();
    assert_eq!(ask_text(&op).as_deref(), Some("Put “Lower third” back the way it came? Your changes to it are lost."));
    answer(&op, true);
    assert!(app.data.lock().unwrap().themes.is_empty());
    assert_eq!(g(&op).get_rows().row_data(i).unwrap().delete_label, "");
}

#[test]
fn standard_edit_opens_look_and_duplicate_copies_it() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    g(&op).invoke_duplicate(0);
    settle();
    assert_eq!(g(&op).get_name(), "Standard copy");
    click(&op, "SettingsDialog::theme-cancel");
    assert!(!g(&op).get_editing());
    assert!(app.data.lock().unwrap().themes.is_empty(), "Cancel saves nothing");
    g(&op).invoke_edit(0);
    settle();
    assert_eq!(op.ui.global::<SettingsDlg>().get_tab(), 0, "Standard is the Look tab");
}

#[test]
fn theme_editor_preview_redraws_after_a_change() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    click(&op, "ThemeList::new-theme");
    let first = pixels(&g(&op).get_preview());
    assert!(!first.is_empty(), "drawn as the editor opens");
    g(&op).set_band(true);
    g(&op).set_position(2);
    g(&op).invoke_changed();
    assert_eq!(pixels(&g(&op).get_preview()), first, "not before the delay");
    mock_elapsed_time(settings_look::PREVIEW_DELAY * 2);
    settle();
    assert_ne!(pixels(&g(&op).get_preview()), first, "drawn again 150 ms later");
    // The other samples.
    let before = pixels(&g(&op).get_preview());
    g(&op).set_layout_index(LayoutKind::Scripture.index() as i32);
    g(&op).invoke_layout_picked();
    assert_ne!(pixels(&g(&op).get_preview()), before, "the Scripture sample");
}

#[test]
fn missing_font_note_in_theme_editor() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    click(&op, "ThemeList::new-theme");
    assert_eq!(g(&op).get_fonts().row_data(0).unwrap(), "Noto Sans", "the bundled font first");
    g(&op).set_font_index(0);
    g(&op).invoke_changed();
    mock_elapsed_time(std::time::Duration::from_millis(500));
    settle();
    assert_eq!(g(&op).get_font_note(), "", "Noto Sans is always there");
    assert_eq!(g(&op).get_font_hint(), "works on every computer");
    let missing = (0..g(&op).get_fonts().row_count()).find(|&i| g(&op).get_fonts().row_data(i).unwrap() == "Segoe UI").unwrap();
    g(&op).set_font_index(missing as i32);
    g(&op).invoke_changed();
    mock_elapsed_time(std::time::Duration::from_millis(500));
    settle();
    // Segoe UI is not installed on the Linux test machine; on Windows it is, and the note stays empty.
    if cfg!(not(windows)) { assert_eq!(g(&op).get_font_note(), "“Segoe UI” isn't installed on this computer, so slides use Noto Sans.") }
}

#[test]
fn theme_background_standard_gradient_and_darken() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    click(&op, "ThemeList::new-theme");
    let tiles: Vec<crate::ui::BgChoice> = g(&op).get_backgrounds().iter().collect();
    assert_eq!(tiles[0].label, "Standard background");
    assert!(tiles[0].on, "a new theme uses Standard's background");
    assert!(!g(&op).get_darken_shown());
    let midnight = tiles.iter().position(|t| t.tooltip == "Midnight").expect("the gradients are listed");
    g(&op).invoke_pick_background(midnight as i32);
    assert!(g(&op).get_darken_shown(), "Darken for a gradient");
    g(&op).set_darken(30);
    g(&op).invoke_changed();
    click(&op, "SettingsDialog::theme-save");
    let t = app.data.lock().unwrap().themes[0].clone();
    assert_eq!(t.background, Some(Background::Gradient { from: "#0b1a33".into(), to: "#000000".into(), angle: 180, dim: 30 }));
}

#[test]
fn library_adds_a_gradient_renames_and_removes_it() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    assert!(find(&op, "ThemeList::no-backgrounds").is_some());
    click(&op, "ThemeList::add-color");
    assert!(g(&op).get_adding());
    g(&op).set_add_kind(1);
    g(&op).set_add_angle(2);
    g(&op).invoke_add_from_hex("#ff8800".into());
    g(&op).set_add_name("Sunrise".into());
    click(&op, "ThemeList::add-save");
    let e = app.data.lock().unwrap().backgrounds[0].clone();
    assert_eq!((e.name.as_str(), &e.background), ("Sunrise", &Background::Gradient { from: "#ff8800".into(), to: "#000000".into(), angle: 90, dim: 0 }));
    assert!(toasts(&op).contains(&"Added to backgrounds".to_string()));
    let rows: Vec<BgLibraryRow> = g(&op).get_library().iter().collect();
    assert_eq!((rows[0].name.as_str(), rows[0].sub.as_str(), rows[0].has_image), ("Sunrise", "Gradient", true));

    g(&op).invoke_rename_background(0);
    settle();
    assert_eq!(ask_text(&op).as_deref(), Some("New name for this background"));
    crate::ui::dialogs::ask::answer(&op, true, "Dawn".into());
    settle();
    assert_eq!(app.data.lock().unwrap().backgrounds[0].name, "Dawn");

    g(&op).invoke_remove_background(0);
    settle();
    assert_eq!(ask_text(&op).as_deref(), Some("Remove “Dawn” from backgrounds? Items that use it keep it."));
    answer(&op, true);
    assert!(app.data.lock().unwrap().backgrounds.is_empty());
    assert_eq!(g(&op).get_library().row_count(), 0);
}

#[test]
fn add_picture_imports_it_as_a_background() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    let dir = std::env::temp_dir().join(format!("rp-themes-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Sky.png");
    image::RgbaImage::from_pixel(8, 4, image::Rgba([10, 20, 200, 255])).save(&path).unwrap();
    crate::ui::operator::files::testing::will_pick(vec![path]);
    click(&op, "ThemeList::add-pictures");
    let d = app.data.lock().unwrap();
    assert_eq!(d.backgrounds.len(), 1);
    assert_eq!(d.backgrounds[0].name, "Sky");
    assert!(matches!(&d.backgrounds[0].background, Background::Image { asset, .. } if *asset == d.media[0].asset));
    drop(d);
    assert!(toasts(&op).contains(&"Added to backgrounds".to_string()));
    assert_eq!(g(&op).get_library().row_data(0).unwrap().sub, "Picture");
    // It is pickable in the theme editor, with the library's name.
    click(&op, "ThemeList::new-theme");
    assert!(g(&op).get_backgrounds().iter().any(|t| t.label == "Sky" && t.kind == "image"));
}

#[test]
fn filter_box_shows_for_a_long_list_and_keeps_each_rows_buttons() {
    let app = test_app();
    saved(&app, "Christmas");
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    settle();
    op.close_dialog();
    for n in ["Easter", "Youth", "Funeral", "Wedding", "Christmas Eve"] { saved(&app, n); }
    refresh(&op);
    settings::open(&op, SettingsTab::Themes);
    settle();
    find(&op, "ThemeList::themes-filter").expect("six themes or more get a filter box").set_accessible_value("christmas");
    settle();
    let shown: Vec<String> = find_all(&op, "ThemeList::theme-row").iter().map(|r| r.accessible_label().unwrap().into()).collect();
    assert_eq!(shown, ["Christmas red", "Christmas", "Christmas Eve"]);
    // The third row shown is "Christmas Eve": its Edit opens that theme, not the list's third.
    find_all(&op, "ThemeList::edit").swap_remove(2).mock_single_click(PointerEventButton::Left);
    settle();
    assert!(g(&op).get_editing());
    assert_eq!(g(&op).get_name(), "Christmas Eve");
}

// ---- N25: layouts and snap guides ----

#[test]
fn editor_lists_the_ten_layouts_and_shows_the_picked_ones_box() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    click(&op, "ThemeList::new-theme");
    assert_eq!(g(&op).get_layout_names().row_count(), 10);
    assert_eq!(g(&op).get_layout_index(), LayoutKind::Lyrics.index() as i32, "Lyrics first, like the old preview");
    assert_eq!((g(&op).get_frame_x(), g(&op).get_frame_w()), (57, 886), "the screen inside its margins");
    assert!(find(&op, "ThemeEditor::text-frame").is_some());
    g(&op).set_layout_index(LayoutKind::Points.index() as i32);
    g(&op).invoke_layout_picked();
    assert_eq!((g(&op).get_frame_x(), g(&op).get_frame_y(), g(&op).get_layout_size(), g(&op).get_layout_align()), (120, 110, 80, 0));
    assert!(!g(&op).get_layout_changed());
}

#[test]
fn dragging_the_box_snaps_to_guides_and_saves_with_the_theme() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    click(&op, "ThemeList::new-theme");
    g(&op).set_name("Sermon".into());
    g(&op).set_layout_index(LayoutKind::MainPoint.index() as i32);
    g(&op).invoke_layout_picked();
    let before = pixels(&g(&op).get_preview());
    // Main point's box is x 100, w 800. Moved 20 left nothing is near a guide; 40 left, its left edge (60) snaps to
    // the margin at 57.
    g(&op).invoke_drag_start(500.0, 500.0);
    g(&op).invoke_drag_to(-20.0, 0.0);
    assert_eq!(g(&op).get_frame_x(), 80, "no guide is near");
    assert_eq!(g(&op).get_guides_x().row_count(), 0);
    g(&op).invoke_drag_to(-40.0, 0.0);
    assert_eq!(g(&op).get_frame_x(), 57, "the left edge snaps to the margin");
    assert_eq!(g(&op).get_guides_x().iter().collect::<Vec<i32>>(), [57]);
    g(&op).invoke_drag_done();
    assert_eq!(g(&op).get_guides_x().row_count(), 0, "guides show only while dragging");
    assert_ne!(pixels(&g(&op).get_preview()), before, "drawn again in the new place");
    assert!(g(&op).get_layout_changed());
    // The corner makes it bigger; with snapping off it goes exactly where it is dragged.
    g(&op).set_snap(false);
    g(&op).invoke_drag_start(857.0, 780.0);
    g(&op).invoke_drag_to(-3.0, 20.0);
    g(&op).invoke_drag_done();
    assert_eq!((g(&op).get_frame_w(), g(&op).get_frame_h()), (797, 580));
    g(&op).set_layout_size(140);
    g(&op).invoke_layout_edited();
    click(&op, "SettingsDialog::theme-save");
    let t = app.data.lock().unwrap().themes.iter().find(|t| t.name == "Sermon").cloned().expect("saved");
    let l = t.style.layouts.get(LayoutKind::MainPoint);
    assert_eq!(l.frame, Some(layouts::Frame { x: 57, y: 220, w: 797, h: 580 }));
    assert_eq!(l.size, 140);
    assert!(!t.style.layouts.changed(LayoutKind::Title), "only the edited layout is stored");
}

#[test]
fn a_press_outside_the_box_drags_nothing_and_reset_puts_a_layout_back() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Themes);
    click(&op, "ThemeList::new-theme");
    g(&op).set_layout_index(LayoutKind::Quote.index() as i32);
    g(&op).invoke_layout_picked();
    g(&op).invoke_drag_start(20.0, 20.0);
    g(&op).invoke_drag_to(300.0, 300.0);
    assert_eq!(g(&op).get_frame_x(), 140, "outside the box");
    g(&op).set_layout_place(0);
    g(&op).invoke_layout_edited();
    assert!(g(&op).get_layout_changed());
    click(&op, "ThemeEditor::reset-layout");
    assert!(!g(&op).get_layout_changed());
    assert_eq!(g(&op).get_layout_place(), 1);
}

#[test]
fn every_layout_has_a_preview_sample() {
    for k in LayoutKind::ALL {
        let (slide, kind) = sample(k);
        assert!(matches!(slide.content, SlideContent::Text { ref text, .. } if !text.is_empty()), "{k:?}");
        assert_eq!(kind == "scripture", k == LayoutKind::Scripture);
    }
}
