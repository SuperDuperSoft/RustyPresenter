//! Settings (November 17, 2026 reorganisation, design `docs/superpowers/specs/2026-11-17-settings-design.md`): a
//! window of its own (`settings_window`) with a sidebar of categories and short pages, "Find a setting" and Show
//! advanced settings. It edits a clone of `Settings`, never redrawn by a read, so a phone or a conversion never wipes
//! what the user typed; Save keeps it, Cancel (or closing the window) leaves it. The form lives in the Settings
//! window's own `SettingsDlg` global (`ui/dialogs/settings-state.slint`); Look is in `settings_look`, Bible in
//! `settings_bible`, the search in `settings_find`.
#[path = "../../strings/settings.rs"]
pub mod strings;

use super::{SettingsTab, about, background, settings_audio, settings_bible, settings_find, settings_follow, settings_graphics, settings_language, settings_look, settings_media, settings_sections, settings_themes, settings_window::{self, win}, strings as dlg};
use crate::{
    model::Settings,
    store::BackupInfo,
    ui::{SettingsBackupRow, SettingsDlg, SettingsFindRow, SettingsNavRow, operator::{view::OperatorSnapshot, window::Operator}},
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

/// The page's place in the sidebar (what `SettingsDlg.tab` holds).
pub fn tab_index(tab: SettingsTab) -> i32 { SettingsTab::ALL.iter().position(|t| *t == tab).unwrap_or(0) as i32 }

/// The page at `SettingsDlg.tab`.
pub fn tab_at(i: i32) -> SettingsTab { usize::try_from(i).ok().and_then(SettingsTab::at).unwrap_or_default() }

thread_local! {
    /// The open Settings window's edits (none while it is closed).
    static EDIT: RefCell<Option<Rc<Edit>>> = const { RefCell::new(None) };
}

/// The edits of the open Settings window.
pub(crate) fn editing() -> Option<Rc<Edit>> { EDIT.with(|e| e.borrow().clone()) }

/// Whether the Settings window is open, and on which page.
pub fn shown(op: &Operator) -> Option<SettingsTab> {
    settings_window::is_open(op).then(|| tab_at(win(op).global::<SettingsDlg>().get_tab()))
}

/// Whether the Settings window is open.
pub fn is_open(op: &Operator) -> bool { settings_window::is_open(op) }

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

/// Opens Settings on `tab`, editing a clone of the newest snapshot's settings. Already open: it comes to the front on
/// that page, its edits kept. Nothing happens before the first read.
pub fn open(op: &Rc<Operator>, tab: SettingsTab) { open_at(op, Some(tab)) }

/// Opens Settings on the page it showed last (the Settings button and menu item).
pub fn open_last(op: &Rc<Operator>) { open_at(op, None) }

fn open_at(op: &Rc<Operator>, tab: Option<SettingsTab>) {
    if let Some(edit) = editing().filter(|_| settings_window::is_open(op)) {
        if let Some(tab) = tab { show_page(op, &edit, tab, "") }
        settings_window::raise(op);
        return;
    }
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    let remembered = settings_window::load(&op.app().store.root);
    let tab = tab.or_else(|| SettingsTab::from_key(&remembered.page)).unwrap_or_default();
    let edit = Rc::new(Edit {
        settings: RefCell::new(snap.settings.clone()),
        backups: RefCell::default(),
        look: settings_look::LookState::default(),
        bible: settings_bible::BibleState::default(),
        language: settings_language::LanguageState::default(),
        themes: settings_themes::ThemesState::default(),
    });
    EDIT.with(|e| *e.borrow_mut() = Some(edit.clone()));
    let w = win(op);
    let g = w.global::<SettingsDlg>();
    crate::ui::operator::slides::apply_section_colors(&w.global::<crate::ui::Theme>());
    fill_form(&g, &edit.settings.borrow());
    g.set_palette(ModelRc::new(VecModel::from(background::palette())));
    super::stage_layouts::fill_settings(&g, &snap.data.stage_layouts, snap.settings.stage.layout);
    about::show_licences(op, false);
    fill_nav(&g);
    g.set_find_text("".into());
    g.set_find_rows(ModelRc::default());
    g.set_highlight("".into());
    g.set_advanced(remembered.advanced);
    fill_pages(op, &edit, &snap);
    connect(op, &edit);
    super::songs_folder::connect(op);
    settings_sections::open(op, &edit);
    settings_bible::remember(&edit);
    settings_bible::reset(&g);
    settings_audio::load(op);
    super::settings_record::load(op);
    super::settings_record::wire_keys(op);
    super::settings_record::wire_size(op);
    super::settings_sound::wire(op);
    super::settings_stream_extras::load(op);
    settings_follow::load(op);
    super::settings_model::load(op);
    super::network::load(op);
    super::stream_more::load(op, &edit);
    super::settings_record::size_notes(op);
    settings_window::set_open(true);
    show_page(op, &edit, tab, "");
    settings_window::restore(op, &remembered);
    w.window().on_close_requested({
        let (me, edit) = (Rc::downgrade(op), edit.clone());
        move || {
            if let Some(op) = me.upgrade() { cancel(&op, &edit) }
            slint::CloseRequestResponse::KeepWindowShown
        }
    });
    if let Err(e) = w.show() { op.toast(e.to_string(), true) }
    w.invoke_focus_keys();
}

/// Closes the Settings window without saving: what it runs stops (the Look preview, listening for auto-follow, a
/// key being changed), and its place, page and Show advanced settings are kept for next time.
pub fn close(op: &Rc<Operator>) {
    if !settings_window::is_open(op) { return }
    let w = win(op);
    let g = w.global::<SettingsDlg>();
    settings_window::remember(op, &tab_at(g.get_tab()).key(), g.get_advanced());
    if let Some(edit) = EDIT.with(|e| e.borrow_mut().take()) {
        edit.look.stop();
        edit.themes.stop();
    }
    super::settings_keys::stop(op);
    settings_follow::stop();
    super::ask::clear_settings(op);
    settings_window::set_open(false);
    let _ = w.hide();
}

/// Shows `tab`, lighting up the row `key` there (none when empty). A row that only shows with Show advanced settings
/// turns that on.
pub fn go(op: &Rc<Operator>, tab: SettingsTab, key: &str) {
    if let Some(edit) = editing() { show_page(op, &edit, tab, key) }
}

fn show_page(op: &Rc<Operator>, edit: &Rc<Edit>, tab: SettingsTab, key: &str) {
    let g = win(op).global::<SettingsDlg>();
    if settings_find::find_row(key).is_some_and(|r| r.advanced) && !g.get_advanced() {
        g.set_advanced(true);
    }
    let i = tab_index(tab);
    if g.get_tab() != i || !settings_window::is_open(op) {
        g.set_tab(i);
        page_changed(op, edit, tab);
    }
    g.set_page_title(settings_find::path(tab).into());
    light(op, key);
}

/// Lights up a row for a moment (the page scrolls to it).
fn light(op: &Rc<Operator>, key: &str) {
    let g = win(op).global::<SettingsDlg>();
    g.set_highlight("".into());
    if key.is_empty() { return }
    g.set_highlight(key.into());
    let (me, key) = (Rc::downgrade(op), key.to_string());
    slint::Timer::single_shot(std::time::Duration::from_millis(2500), move || {
        if let Some(op) = me.upgrade() {
            let g = win(&op).global::<SettingsDlg>();
            if g.get_highlight() == key.as_str() { g.set_highlight("".into()) }
        }
    });
}

/// The sidebar's lines.
fn fill_nav(g: &SettingsDlg) {
    let rows: Vec<SettingsNavRow> = settings_find::nav().into_iter().map(|(title, page)| SettingsNavRow {
        title: title.into(), page: page.map_or(-1, tab_index), heading: page.is_none(),
    }).collect();
    g.set_nav(ModelRc::new(VecModel::from(rows)));
}

/// The sidebar, the page's title and what the search found again, in the language chosen now.
pub(crate) fn relabel(op: &Rc<Operator>) {
    let g = win(op).global::<SettingsDlg>();
    fill_nav(&g);
    g.set_page_title(settings_find::path(tab_at(g.get_tab())).into());
    let text = g.get_find_text();
    if !text.is_empty() { find(op, &text) }
}

/// "Find a setting": what matches the words typed.
fn find(op: &Rc<Operator>, text: &str) {
    let rows: Vec<SettingsFindRow> = settings_find::search(text).into_iter().map(|f| SettingsFindRow {
        title: f.title.into(), path: f.path.into(), page: tab_index(f.page), key: f.key.into(),
    }).collect();
    win(op).global::<SettingsDlg>().set_find_rows(ModelRc::new(VecModel::from(rows)));
}

fn find_pick(op: &Rc<Operator>, i: usize) {
    let g = win(op).global::<SettingsDlg>();
    let Some(row) = slint::Model::row_data(&g.get_find_rows(), i) else { return };
    go(op, tab_at(row.page), &row.key);
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
    g.set_songselect_inbox(s.songselect_inbox);
    settings_audio::fill(g, &[], &s.audio_device);
    settings_graphics::fill(g, s.graphics);
    settings_media::fill_form(g, &s.media, &[]);
    super::settings_record::fill_form(g, &s.record);
    super::settings_stream_extras::fill_form(g, &s.record);
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
    s.songselect_inbox = g.get_songselect_inbox();
    s.audio_device = settings_audio::read(g);
    settings_media::read_form(g, &mut s.media);
    super::settings_record::read_form(g, &mut s.record);
    super::settings_stream_extras::read_form(g, &mut s.record);
    settings_follow::read_form(g, &mut s.follow);
    super::stage_layouts::read_settings(g, s);
}

/// Draws the pages that show the core's state rather than the form: Bible, Video, Backups and Help. The web
/// rebuilds them from the newest `S` on every tab switch and after their own actions.
pub(crate) fn fill_pages(op: &Rc<Operator>, edit: &Rc<Edit>, snap: &OperatorSnapshot) {
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
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

    use crate::ui::operator::window::Operator;
    use i_slint_backend_testing::ElementHandle;

    /// An element of the Settings window by its id (`Component::element`).
    pub(crate) fn find(op: &Operator, id: &str) -> Option<ElementHandle> { ElementHandle::find_by_element_id(super::win(op), id).next() }

    pub(crate) fn find_all(op: &Operator, id: &str) -> Vec<ElementHandle> { ElementHandle::find_by_element_id(super::win(op), id).collect() }

    /// An element of the Settings window, or else of the operator window.
    pub(crate) fn find_any(op: &Operator, id: &str) -> Option<ElementHandle> {
        find(op, id).or_else(|| crate::ui::operator::window::testing::find(op, id))
    }

    pub(crate) fn find_all_any(op: &Operator, id: &str) -> Vec<ElementHandle> {
        let mut all = find_all(op, id);
        all.extend(crate::ui::operator::window::testing::find_all(op, id));
        all
    }

    /// Clicks an element of the Settings window.
    pub(crate) fn click(op: &Operator, id: &str) {
        find(op, id).unwrap_or_else(|| panic!("{id} is in the Settings window")).mock_single_click(slint::platform::PointerEventButton::Left);
        slint::platform::update_timers_and_animations();
    }
}

/// Sets every `SettingsDlg` callback for this dialog; each holds a `Weak`, so the window does not keep itself alive.
fn connect(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let w = win(op);
    let g = w.global::<SettingsDlg>();
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
    g.on_graphics_test(with(settings_graphics::start_test));
    g.on_graphics_test_use(with_i(settings_graphics::use_picked));
    g.on_restart_now(with(restart_now));
    g.on_help_translate(with(settings_language::help_translate));
    g.on_edit_translation(with(|op, _| super::translation_editor::open(op, None)));
    g.on_add_language(with(|op, _| super::translation_editor::open_add(op)));
    g.on_backup_now(with(backup_now));
    g.on_restore(with_i(restore));
    let me = Rc::downgrade(op);
    g.on_show_licences(move |show| if let Some(op) = me.upgrade() { about::show_licences(&op, show) });
    g.on_cancel(with(cancel));
    g.on_save(with(save));
    g.on_usage_report(with(usage_report));
    g.on_open_songselect(with(songselect));
    g.on_edit_own_scenes(with(own_scenes));
    g.on_credit_template_edited(with(|op, edit| {
        let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
        let mut s = edit.settings.borrow().clone();
        read_form(&g, &mut s);
        g.set_credit_sample(settings_sections::credit_sample(&s).into());
    }));
    g.on_cleanup_media(with(|op, _| settings_media::clean_up(op)));
    let me = Rc::downgrade(op);
    g.on_find_edited(move |t| if let Some(op) = me.upgrade() { find(&op, &t) });
    g.on_find_pick(with_i(|op, _, i| find_pick(op, i)));
    g.on_nav_pick(with_i(|op, edit, i| { if let Some(t) = SettingsTab::at(i) { show_page(op, edit, t, "") } }));
    let me = Rc::downgrade(op);
    g.on_go(move |page, key| if let Some(op) = me.upgrade() { if let Some(t) = usize::try_from(page).ok().and_then(SettingsTab::at) { go(&op, t, &key) } });
    g.on_advanced_changed(|_| {});
    g.on_pair_remote(with(|op, _| { super::phone::open(op); settings_window::raise_operator(op) }));
    g.on_open_screens(with(|op, _| { super::screens::open(op); settings_window::raise_operator(op) }));
    g.on_open_stage_layouts(with(|op, _| { super::stage_layouts::open(op); settings_window::raise_operator(op) }));
    let me = Rc::downgrade(op);
    w.on_ask_answer(move |ok, text| if let Some(op) = me.upgrade() { super::ask::answer_settings(&op, ok, text.to_string()) });
    let me = Rc::downgrade(op);
    w.on_ask_other(move || if let Some(op) = me.upgrade() { super::ask::answer_other_settings(&op) });
    let me = Rc::downgrade(op);
    w.on_ask_third(move || if let Some(op) = me.upgrade() { super::ask::answer_third_settings(&op) });
    settings_themes::connect(op, edit);
    super::settings_keys::connect(op, edit);
}

/// Another page: the Look preview stops; coming back to Look draws it again from the current edits
/// (`operator.js:937, 1000`). The state pages are rebuilt from the newest snapshot, as `draw()` does.
fn page_changed(op: &Rc<Operator>, edit: &Rc<Edit>, tab: SettingsTab) {
    settings_look::drop_preview(op, edit);
    settings_themes::close_editor(op, edit);
    if super::settings_keys::capturing() { super::settings_keys::stop(op) }
    if let Some(s) = snap(op) { fill_pages(op, edit, &s) }
    if tab == SettingsTab::Look { settings_look::render_now(op, edit) }
    if tab == SettingsTab::Bible { settings_bible::load_catalog(op, edit) }
}

fn tab_changed(op: &Rc<Operator>, edit: &Rc<Edit>, tab: usize) {
    if let Some(t) = SettingsTab::at(tab) { page_changed(op, edit, t) }
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
    super::ask::in_settings(op, || super::ask::confirm(op, &strings::restore_confirm(), &dlg::ok(), &dlg::cancel(), false, move |ok| {
        let Some(op) = me.upgrade().filter(|_| ok) else { return };
        let me = Rc::downgrade(&op);
        op.ops().call::<serde_json::Value>("backup/restore", serde_json::json!({ "name": name }), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(_) => {
                    close(&op);
                    op.toast(strings::backup_restored(), false);
                }
                Err(e) => op.toast(e, true),
            }
        });
    }));
}

/// The settings as the form has them now (every page, the advanced rows too).
pub(crate) fn current(op: &Operator, edit: &Edit) -> Settings {
    let mut s = edit.settings.borrow().clone();
    read_form(&win(op).global::<SettingsDlg>(), &mut s);
    super::stream_more::read_into(op, &mut s.record);
    s
}

/// Whether anything was changed and not saved yet.
pub(crate) fn changed(op: &Operator, edit: &Edit) -> bool {
    current(op, edit) != *edit.settings.borrow() || super::network::changed(op)
}

/// Cancel, Esc or the window's close button: closes at once, or asks first when something was changed.
fn cancel(op: &Rc<Operator>, edit: &Rc<Edit>) {
    if !changed(op, edit) { return close(op) }
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    super::ask::in_settings(op, || super::ask::choose(op, &crate::tr!("Save your changes to the settings?"), &crate::tr!("Save"), &crate::tr!("Don't save"), move |answer| {
        let Some(op) = me.upgrade() else { return };
        match answer {
            Some(true) => save(&op, &edit),
            Some(false) => close(&op),
            None => {}
        }
    }));
}

/// Save settings: the form over the clone, with the translation and screens taken from the **newest** snapshot, so
/// a translation picked meanwhile (`settings/translation`) or a monitor chosen in Screens is never undone. Then the
/// network rows (`network/save`), and the window closes.
fn save(op: &Rc<Operator>, edit: &Rc<Edit>) { save_then(op, edit, true, |_| {}) }

/// Asks to save the changes first (in the Settings window), then runs `then`; at once with nothing changed.
fn saved_first(op: &Rc<Operator>, edit: &Rc<Edit>, question: String, yes: String, then: fn(&Rc<Operator>)) {
    if !changed(op, edit) { return then(op) }
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    super::ask::in_settings(op, || super::ask::confirm(op, &question, &yes, &dlg::cancel(), false, move |ok| {
        let Some(op) = me.upgrade().filter(|_| ok) else { return };
        save_then(&op, &edit, false, then);
    }));
}

/// The Song use report from Settings → Songs: opens at once (in the operator window), or, with changes not saved
/// yet, asks to save them first (FEAT-7 §3.3).
fn usage_report(op: &Rc<Operator>, edit: &Rc<Edit>) {
    saved_first(op, edit, crate::usage::strings::save_first(), crate::usage::strings::save_and_open(), |op| {
        super::usage::open(op);
        settings_window::raise_operator(op);
    });
}

/// "Your scenes…" in Livestream → Scenes: the scenes editor opens in the operator window (it saves the scenes
/// itself, and Save here keeps them as it found them).
fn own_scenes(op: &Rc<Operator>, _edit: &Rc<Edit>) {
    super::stream_scenes::open(op);
    settings_window::raise_operator(op);
}

/// "Songs from SongSelect…" in Songs → SongSelect: opens in the operator window.
fn songselect(op: &Rc<Operator>, _edit: &Rc<Edit>) {
    super::songselect::open(op);
    settings_window::raise_operator(op);
}

/// "Restart now" (`recovery::restart`): at once, or, with other changes not saved yet, asks to save them first.
fn restart_now(op: &Rc<Operator>, edit: &Rc<Edit>) {
    saved_first(op, edit, crate::tr!("Save your other changes and restart?"), crate::tr!("Save and restart"), |_| crate::recovery::restart::request());
}

/// Saves, then runs `then` once the settings (and the network rows) are saved; `close_after`: the window closes.
fn save_then(op: &Rc<Operator>, edit: &Rc<Edit>, close_after: bool, then: fn(&Rc<Operator>)) {
    let mut s = current(op, edit);
    if let Some(problem) = s.sections.problem() { return op.toast(problem, true) }
    // The network rows are checked before anything is saved (a port that is not a number).
    let network = match super::network::body_if_changed(op) {
        Ok(n) => n,
        Err(e) => { show_page(op, edit, SettingsTab::OtherGear, "tcp"); return super::network::show_error(op, &e) }
    };
    if let Some(latest) = snap(op) {
        s.translation = latest.settings.translation.clone();
        s.screens = latest.settings.screens.clone();
        // Kept by Your scenes, which saves them itself, and by `network/save` below.
        s.record.own_scenes = latest.settings.record.own_scenes.clone();
        s.network = latest.settings.network.clone();
    }
    let body = serde_json::to_value(&s).unwrap_or_default();
    // 1.3: a new sound input for auto-follow is listened to at once.
    let input_changed = s.follow.input != edit.settings.borrow().follow.input;
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<serde_json::Value>("settings", body, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => {
                crate::ui::operator::follow::listen(&op, input_changed);
                *edit.settings.borrow_mut() = s;
                let done = move |op: &Rc<Operator>| {
                    if close_after { close(op) }
                    op.toast(strings::saved(), false);
                    then(op);
                };
                match network {
                    Some(body) => super::network::save(&op, body, done),
                    None => done(&op),
                }
            }
            Err(e) => op.toast(e, true),
        }
    });
}

/// Connects the top bar's Settings button.
pub(crate) fn wire(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    op.ui.on_settings(move || if let Some(op) = me.upgrade() { open_last(&op) });
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "settings_window_tests.rs"]
mod window_tests;
