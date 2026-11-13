//! Settings (`openSettings`, `operator.js:877-1007`): a wide dialog editing a clone of `Settings`, never redrawn by
//! `dialog_refresh`, so a phone or a conversion never wipes what the user typed. The form lives in the `SettingsDlg`
//! global (`ui/dialogs/settings-state.slint`); Look is in `settings_look`, Bible in `settings_bible`.
#[path = "../../strings/settings.rs"]
pub mod strings;

use super::{DialogKind, SettingsTab, about, background, confirm, settings_audio, settings_bible, settings_follow, settings_graphics, settings_language, settings_look, settings_media, settings_sections, settings_themes, strings as dlg};
use crate::{
    model::Settings,
    store::BackupInfo,
    ui::{SettingsBackupRow, SettingsDlg, operator::{view::OperatorSnapshot, window::Operator}},
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, rc::{Rc, Weak}};

/// What one open Settings dialog holds between callbacks: the edited clone and the lists its buttons index into.
pub struct Edit {
    pub(crate) settings: RefCell<Settings>,
    pub(crate) backups: RefCell<Vec<BackupInfo>>,
    pub(crate) look: settings_look::LookState,
    pub(crate) bible: settings_bible::BibleState,
    pub(crate) language: settings_language::LanguageState,
    pub(crate) themes: settings_themes::ThemesState,
}

/// The tab's index in the `Tabs` row.
pub fn tab_index(tab: SettingsTab) -> i32 {
    match tab {
        SettingsTab::Look => 0,
        SettingsTab::Themes => 1,
        SettingsTab::Language => 2,
        SettingsTab::Songs | SettingsTab::Sections => 3,
        SettingsTab::AutoFollow => 4,
        SettingsTab::Bible => 5,
        SettingsTab::Video => 6,
        SettingsTab::Media => 7,
        SettingsTab::Backups => 8,
        SettingsTab::Help => 9,
        SettingsTab::About => 10,
    }
}

/// `b.created.replace('_', ' at ').replace(/(\d\d)(\d\d)(\d\d)$/, '$1:$2') + ' UTC'` (`operator.js:988`).
pub fn backup_label(created: &str) -> String {
    // "143005…" ends as "14:30" when its last six characters are digits.
    let clock = |s: &str| -> String {
        let tail: Vec<char> = s.chars().rev().take(6).collect();
        if tail.len() == 6 && tail.iter().all(char::is_ascii_digit) {
            let cut = s.len() - 6;
            format!("{}{}:{}", &s[..cut], &s[cut..cut + 2], &s[cut + 2..cut + 4])
        } else { s.to_string() }
    };
    match created.split_once('_') {
        Some((day, time)) => crate::tr!("{day} at {time} UTC", day = day, time = clock(time)),
        None => crate::tr!("{time} UTC", time = clock(created)),
    }
}

/// Opens Settings on `tab`, editing a clone of the newest snapshot's settings. Nothing happens before the first read.
pub fn open(op: &Rc<Operator>, tab: SettingsTab) {
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    op.open_dialog(DialogKind::Settings(tab));
    let edit = Rc::new(Edit {
        settings: RefCell::new(snap.settings.clone()),
        backups: RefCell::default(),
        look: settings_look::LookState::default(),
        bible: settings_bible::BibleState::default(),
        language: settings_language::LanguageState::default(),
        themes: settings_themes::ThemesState::default(),
    });
    let g = op.ui.global::<SettingsDlg>();
    fill_form(&g, &edit.settings.borrow());
    g.set_palette(ModelRc::new(VecModel::from(background::palette())));
    g.set_tab(tab_index(tab));
    g.set_songs_page(i32::from(tab == SettingsTab::Sections));
    super::stage_layouts::fill_settings(&g, &snap.data.stage_layouts, snap.settings.stage.layout);
    about::show_licences(op, false);
    fill_pages(op, &edit, &snap);
    connect(op, &edit);
    settings_sections::open(op, &edit);
    settings_bible::remember(&edit);
    settings_bible::reset(&g);
    settings_audio::load(op);
    super::settings_record::load(op);
    settings_follow::load(op);
    if tab == SettingsTab::Look { settings_look::render_now(op, &edit) }
    if tab == SettingsTab::Bible { settings_bible::load_catalog(op, &edit) }
    let closing = edit.clone();
    op.on_dialog_close(move |op| { closing.look.stop(); closing.themes.stop(); super::settings_keys::stop(op); settings_follow::stop() });
}

/// "Find more Bibles…" in the Bible picker: Settings on the Bible tab, with the picker's search in the library's
/// search box (spec §7.2).
pub fn open_bible_search(op: &Rc<Operator>, query: &str) {
    open(op, SettingsTab::Bible);
    settings_bible::search_for(op, query);
}

/// Puts the edited settings into the form (`in-out` fields of `SettingsDlg`).
fn fill_form(g: &SettingsDlg, s: &Settings) {
    settings_look::fill_form(g, s);
    g.set_lines(i32::from(s.lines_per_slide.clamp(1, 6)) - 1);
    g.set_credits_on(s.credits != "none");
    g.set_credits(match s.credits.as_str() { "first" => 1, "last" => 2, _ => 0 });
    g.set_credit_template(s.credit_template.as_str().into());
    g.set_credit_usual(crate::slides::DEFAULT_CREDIT_TEMPLATE.into());
    g.set_credit_sample(settings_sections::credit_sample(s).into());
    g.set_ccli(s.church_ccli.as_str().into());
    g.set_room_on_top(s.room_on_top);
    g.set_screens_at_start(s.screens_at_start);
    g.set_show_performance(s.show_performance);
    g.set_keep_drawings(s.keep_drawings);
    g.set_stage_drawings(s.stage_drawings);
    g.set_drawing_limit(s.drawing_limit);
    g.set_drawing_max_strokes(s.drawing_max_strokes.to_string().into());
    g.set_drawing_max_per_second(s.drawing_max_per_second.to_string().into());
    g.set_fade_secs(s.fade_secs.to_string().into());
    g.set_section_keys(s.section_keys);
    settings_audio::fill(g, &[], &s.audio_device);
    settings_graphics::fill(g, s.graphics);
    settings_media::fill_form(g, &s.media, &[]);
    super::settings_record::fill_form(g, &s.record);
    settings_follow::fill_form(g, &s.follow, &[]);
}

/// Reads the form back into `s` (Save and every Look change).
pub(crate) fn read_form(g: &SettingsDlg, s: &mut Settings) {
    settings_look::read_form(g, s);
    s.lines_per_slide = (g.get_lines().clamp(0, 5) + 1) as u8;
    s.credits = if g.get_credits_on() { ["both", "first", "last"][g.get_credits().clamp(0, 2) as usize] } else { "none" }.into();
    s.credit_template = g.get_credit_template().trim().into();
    s.church_ccli = g.get_ccli().into();
    s.room_on_top = g.get_room_on_top();
    s.screens_at_start = g.get_screens_at_start();
    s.show_performance = g.get_show_performance();
    s.keep_drawings = g.get_keep_drawings();
    s.stage_drawings = g.get_stage_drawings();
    s.drawing_limit = g.get_drawing_limit();
    // A number kept to its range; anything else leaves the setting as it was.
    let num = |t: slint::SharedString, r: std::ops::RangeInclusive<u32>, old: u32| t.trim().parse::<u32>().map_or(old, |n| n.clamp(*r.start(), *r.end()));
    use crate::live::drawing::{FADE_SECS_RANGE, actions::{MAX_PER_SECOND_RANGE, MAX_STROKES_RANGE}};
    s.drawing_max_strokes = num(g.get_drawing_max_strokes(), MAX_STROKES_RANGE, s.drawing_max_strokes);
    s.drawing_max_per_second = num(g.get_drawing_max_per_second(), MAX_PER_SECOND_RANGE, s.drawing_max_per_second);
    s.fade_secs = num(g.get_fade_secs(), FADE_SECS_RANGE, s.fade_secs);
    s.section_keys = g.get_section_keys();
    s.audio_device = settings_audio::read(g);
    settings_media::read_form(g, &mut s.media);
    super::settings_record::read_form(g, &mut s.record);
    settings_follow::read_form(g, &mut s.follow);
    super::stage_layouts::read_settings(g, s);
}

/// Draws the pages that show the core's state rather than the form: Bible, Video, Backups and Help. The web
/// rebuilds them from the newest `S` on every tab switch and after their own actions.
pub(crate) fn fill_pages(op: &Rc<Operator>, edit: &Rc<Edit>, snap: &OperatorSnapshot) {
    let g = op.ui.global::<SettingsDlg>();
    settings_look::fill_backgrounds(op, edit, snap);
    settings_themes::fill(op, edit, snap);
    settings_bible::fill(&g, edit, snap);
    g.set_backups_folder(strings::backups_folder(&snap.data_folder).into());
    let rows: Vec<SettingsBackupRow> = snap.backups.iter().map(|b| SettingsBackupRow {
        label: backup_label(&b.created).into(), counts: strings::backup_counts(b.songs, b.services, b.assets).into(),
    }).collect();
    g.set_backups(ModelRc::new(VecModel::from(rows)));
    *edit.backups.borrow_mut() = snap.backups.clone();
    super::settings_keys::fill(op, edit);
    about::fill(op, snap);
    settings_language::fill(op, edit);
}

/// Redraws the state pages once the next read arrives (the web's `await refresh(); redraw()`).
pub(crate) fn redraw_on_next_read(op: &Operator, edit: &Rc<Edit>) {
    let edit = edit.clone();
    op.on_next_read(move |op| {
        let snap = op.state.borrow().snap.clone();
        if let Some(snap) = snap { fill_pages(op, &edit, &snap) }
    });
}

/// The newest snapshot (the web `S`).
pub(crate) fn snap(op: &Operator) -> Option<std::sync::Arc<OperatorSnapshot>> { op.state.borrow().snap.clone() }

thread_local! {
    /// The Slint thread's one clipboard, kept for the window's life: on Linux (X11) the copied text lives only as long
    /// as the `arboard::Clipboard` that set it, and dropping the last one waits on the UI thread for a clipboard manager.
    static CLIPBOARD: RefCell<Option<arboard::Clipboard>> = const { RefCell::new(None) };
}

/// Copies text with `arboard` (Slint 1.18 has no public clipboard-write API) and toasts `done_text`.
pub(crate) fn copy(op: &Operator, text: &str, done_text: &str) {
    let set = CLIPBOARD.with(|c| {
        let mut c = c.borrow_mut();
        let clipboard = match c.as_mut() { Some(clipboard) => clipboard, None => c.insert(arboard::Clipboard::new()?) };
        clipboard.set_text(text.to_string())
    });
    match set {
        Ok(()) => op.toast(done_text, false),
        Err(e) => op.toast(strings::copy_failed(&e.to_string()), true),
    }
}

/// The clipboard's text, if it holds any (Download from YouTube fills its link box from it). Tests read a stand-in.
pub(crate) fn paste_text() -> Option<String> {
    #[cfg(test)]
    return testing::CLIPBOARD.with(|c| c.borrow().clone());
    #[cfg(not(test))]
    CLIPBOARD.with(|c| {
        let mut c = c.borrow_mut();
        let clipboard = match c.as_mut() { Some(clipboard) => clipboard, None => c.insert(arboard::Clipboard::new().ok()?) };
        clipboard.get_text().ok()
    })
}

#[cfg(test)]
pub(crate) mod testing {
    use std::cell::RefCell;

    thread_local! {
        /// What `paste_text` reads in tests, so they never touch the real clipboard.
        pub(crate) static CLIPBOARD: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    pub(crate) fn set_clipboard(text: Option<&str>) { CLIPBOARD.with(|c| *c.borrow_mut() = text.map(str::to_string)) }
}

/// Sets every `SettingsDlg` callback for this dialog; each holds a `Weak`, so the window does not keep itself alive.
fn connect(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let g = op.ui.global::<SettingsDlg>();
    let with = |f: fn(&Rc<Operator>, &Rc<Edit>)| {
        let (me, edit): (Weak<Operator>, Rc<Edit>) = (Rc::downgrade(op), edit.clone());
        move || if let Some(op) = me.upgrade() { f(&op, &edit) }
    };
    let with_i = |f: fn(&Rc<Operator>, &Rc<Edit>, usize)| {
        let (me, edit): (Weak<Operator>, Rc<Edit>) = (Rc::downgrade(op), edit.clone());
        move |i: i32| if let Some(op) = me.upgrade() { f(&op, &edit, i.max(0) as usize) }
    };
    g.on_tab_changed(with_i(tab_changed));
    g.on_look_changed(with(settings_look::changed));
    g.on_choose_logo(with(settings_look::choose_logo));
    g.on_remove_logo(with(settings_look::remove_logo));
    g.on_pick_background(with_i(settings_look::pick_background));
    g.on_upload_background(with(settings_look::upload_background));
    let with_color = |f: fn(&Rc<Operator>, &Rc<Edit>, String)| {
        let (me, edit): (Weak<Operator>, Rc<Edit>) = (Rc::downgrade(op), edit.clone());
        move |c: slint::Color| if let Some(op) = me.upgrade() { f(&op, &edit, background::to_hex(c)) }
    };
    // A hex field's text is kept only when it is a whole `#rrggbb`.
    let with_hex = |f: fn(&Rc<Operator>, &Rc<Edit>, String)| {
        let (me, edit): (Weak<Operator>, Rc<Edit>) = (Rc::downgrade(op), edit.clone());
        move |t: slint::SharedString| if let (Some(op), Some(hex)) = (me.upgrade(), background::parse_hex(&t)) { f(&op, &edit, hex) }
    };
    g.on_text_color(with_color(settings_look::text_color));
    g.on_text_hex(with_hex(settings_look::text_color));
    g.on_background_color(with_color(settings_look::background_color));
    g.on_background_hex(with_hex(settings_look::background_color));
    let (me, e) = (Rc::downgrade(op), edit.clone());
    g.on_save_key(move |i, key| if let Some(op) = me.upgrade() { settings_bible::save_key(&op, &e, i.max(0) as usize, &key) });
    g.on_copy_link(with_i(settings_bible::copy_link));
    g.on_import_bible(with(settings_bible::import));
    g.on_remove_import(with_i(settings_bible::remove_import));
    g.on_catalog_changed(with(|op, edit| settings_bible::render(op, edit)));
    g.on_catalog_button(with_i(settings_bible::catalog_button));
    g.on_default_bible_changed(with_i(settings_bible::default_bible));
    g.on_toggle_also(with_i(settings_bible::toggle_also));
    g.on_also_layout_changed(with_i(settings_bible::also_layout));
    g.on_choose_language(with_i(settings_language::choose));
    g.on_graphics_changed(with_i(settings_graphics::choose));
    g.on_help_translate(with(settings_language::help_translate));
    g.on_edit_translation(with(|op, _| super::translation_editor::open(op, None)));
    g.on_add_language(with(|op, _| super::translation_editor::open_add(op)));
    g.on_backup_now(with(backup_now));
    g.on_restore(with_i(restore));
    let me = Rc::downgrade(op);
    g.on_show_licences(move |show| if let Some(op) = me.upgrade() { about::show_licences(&op, show) });
    g.on_cancel(with(|op, _| op.close_dialog()));
    g.on_save(with(save));
    g.on_usage_report(with(usage_report));
    g.on_credit_template_edited(with(|op, edit| {
        let g = op.ui.global::<SettingsDlg>();
        let mut s = edit.settings.borrow().clone();
        read_form(&g, &mut s);
        g.set_credit_sample(settings_sections::credit_sample(&s).into());
    }));
    g.on_cleanup_media(with(|op, _| settings_media::clean_up(op)));
    settings_themes::connect(op, edit);
    super::settings_keys::connect(op, edit);
}

/// Switching tabs drops the Look preview; coming back to Look draws it again from the current edits
/// (`operator.js:937, 1000`). The state pages are rebuilt from the newest snapshot, as `draw()` does.
fn tab_changed(op: &Rc<Operator>, edit: &Rc<Edit>, tab: usize) {
    settings_look::drop_preview(op, edit);
    settings_themes::close_editor(op, edit);
    if super::settings_keys::capturing() { super::settings_keys::stop(op) }
    if let Some(s) = snap(op) { fill_pages(op, edit, &s) }
    if tab == tab_index(SettingsTab::Look) as usize { settings_look::render_now(op, edit) }
    if tab == tab_index(SettingsTab::Bible) as usize { settings_bible::load_catalog(op, edit) }
}

fn backup_now(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<serde_json::Value>("backup/now", serde_json::json!({}), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => {
                redraw_on_next_read(&op, &edit);
                op.toast(strings::backup_made(), false);
            }
            Err(e) => op.toast(e, true),
        }
    });
}

fn restore(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(name) = edit.backups.borrow().get(i).map(|b| b.name.clone()) else { return };
    let me = Rc::downgrade(op);
    confirm(op, &strings::restore_confirm(), &dlg::ok(), &dlg::cancel(), false, move |ok| {
        let Some(op) = me.upgrade().filter(|_| ok) else { return };
        let me = Rc::downgrade(&op);
        op.ops().call::<serde_json::Value>("backup/restore", serde_json::json!({ "name": name }), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(_) => {
                    op.close_dialog();
                    op.toast(strings::backup_restored(), false);
                }
                Err(e) => op.toast(e, true),
            }
        });
    });
}

/// Save settings: the form over the clone, with the translation and screens taken from the **newest** snapshot, so
/// a translation picked meanwhile (`settings/translation`) or a monitor chosen in Screens is never undone.
fn save(op: &Rc<Operator>, edit: &Rc<Edit>) { save_then(op, edit, |_| {}) }

/// The Song use report from Settings → Songs: opens at once, or, with changes not saved yet, asks to save them first
/// (FEAT-7 §3.3).
fn usage_report(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let mut s = edit.settings.borrow().clone();
    read_form(&op.ui.global::<SettingsDlg>(), &mut s);
    if s == *edit.settings.borrow() { return super::usage::open(op) }
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    confirm(op, &crate::usage::strings::save_first(), &crate::usage::strings::save_and_open(), &dlg::cancel(), false, move |ok| {
        let Some(op) = me.upgrade().filter(|_| ok) else { return };
        save_then(&op, &edit, super::usage::open);
    });
}

/// `save`, then `then` once the settings are saved and the dialog closed.
fn save_then(op: &Rc<Operator>, edit: &Rc<Edit>, then: impl FnOnce(&Rc<Operator>) + 'static) {
    let mut s = edit.settings.borrow().clone();
    read_form(&op.ui.global::<SettingsDlg>(), &mut s);
    if let Some(problem) = s.sections.problem() { return op.toast(problem, true) }
    if let Some(latest) = snap(op) {
        s.translation = latest.settings.translation.clone();
        s.screens = latest.settings.screens.clone();
    }
    let body = serde_json::to_value(&s).unwrap_or_default();
    let me = Rc::downgrade(op);
    op.ops().call::<serde_json::Value>("settings", body, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => {
                op.close_dialog();
                op.toast(strings::saved(), false);
                then(&op);
            }
            Err(e) => op.toast(e, true),
        }
    });
}

/// Connects the top bar's Settings button.
pub(crate) fn wire(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    op.ui.on_settings(move || if let Some(op) = me.upgrade() { open(&op, SettingsTab::Look) });
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
