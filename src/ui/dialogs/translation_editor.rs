//! Settings → Translations… (`ui/dialogs/translation-editor.slint`): the app's words in a language, English beside
//! the translation, with a search and filters. A change shows in the whole app (operator, dialogs, the phone) a
//! moment after typing (`APPLY_DELAY`) and is saved by itself to `<data>/translations/<code>.po`
//! (`crate::i18n::user`). "Add a language…" starts a new one from nothing or from a copy.
use super::DialogKind;
use crate::{
    i18n::{self, languages::{self, Language}, user::{self, Draft, Row}},
    tr,
    ui::{TransForm, TransLang, TransRow, TranslationEditorDlg, operator::{file_input::Filter, files, window::Operator}},
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, path::PathBuf, rc::{Rc, Weak}, time::Duration};

/// How long after the last key a change is used and saved.
pub const APPLY_DELAY: Duration = Duration::from_millis(400);
/// Rows drawn at once; "Show more" adds as many again.
const PAGE: usize = 200;

#[derive(Default)]
struct State {
    /// `None` while adding a language with no language open (the app is in English).
    draft: Option<Draft>,
    rows: Vec<Row>,
    /// The rows on show (search and filter), as indexes into `rows`.
    shown: Vec<usize>,
    limit: usize,
    /// The chosen row (an index into `rows`) and what its boxes hold now.
    selected: Option<usize>,
    boxes: Vec<String>,
    /// A change waits for `APPLY_DELAY`.
    timer: slint::Timer,
    // Add a language.
    add_list: Vec<&'static Language>,
    copy_codes: Vec<Option<String>>,
}

type St = Rc<RefCell<State>>;

fn folder(op: &Operator) -> PathBuf { op.app().store.root.join("translations") }

/// The user's translation files read again; one that cannot be read is said in a toast.
pub fn reload(op: &Operator) {
    i18n::load_user_folder(&folder(op));
    for p in i18n::take_problems() { op.toast(p, true) }
}

pub fn title() -> String { tr!("Translations") }

/// Opens the editor on `code` (the language in use when `None`). In English, it opens on "Add a language…".
pub fn open(op: &Rc<Operator>, code: Option<&str>) {
    reload(op);
    let code = code.map(str::to_string).unwrap_or_else(i18n::current);
    let st: St = Rc::new(RefCell::new(State { limit: PAGE, ..Default::default() }));
    op.open_dialog(DialogKind::Translations);
    wire(op, &st);
    let (me, s) = (Rc::downgrade(op), st.clone());
    op.on_dialog_close(move |_| if let Some(op) = me.upgrade() { apply(&op, &s) });
    if code == i18n::ENGLISH || !i18n::is_known(&code) { return show_add(op, &st) }
    load(op, &st, &code);
}

/// Opens the editor on "Add a language…".
pub fn open_add(op: &Rc<Operator>) {
    open(op, Some(i18n::ENGLISH));
}

fn g(op: &Operator) -> TranslationEditorDlg<'_> { op.ui.global::<TranslationEditorDlg>() }

fn wire(op: &Rc<Operator>, st: &St) {
    let g = g(op);
    let with = |f: fn(&Rc<Operator>, &St)| {
        let (me, st): (Weak<Operator>, St) = (Rc::downgrade(op), st.clone());
        move || if let Some(op) = me.upgrade() { f(&op, &st) }
    };
    let with_i = |f: fn(&Rc<Operator>, &St, usize)| {
        let (me, st): (Weak<Operator>, St) = (Rc::downgrade(op), st.clone());
        move |i: i32| if let Some(op) = me.upgrade() { f(&op, &st, i.max(0) as usize) }
    };
    let with_s = |f: fn(&Rc<Operator>, &St)| {
        let (me, st): (Weak<Operator>, St) = (Rc::downgrade(op), st.clone());
        move |_: slint::SharedString| if let Some(op) = me.upgrade() { f(&op, &st) }
    };
    g.on_search_changed(with_s(|op, st| { st.borrow_mut().limit = PAGE; refilter(op, st) }));
    g.on_filter_changed(with_i(|op, st, _| { st.borrow_mut().limit = PAGE; refilter(op, st) }));
    g.on_select(with_i(select));
    let (me, s) = (Rc::downgrade(op), st.clone());
    g.on_form_edited(move |k, text| if let Some(op) = me.upgrade() { edited(&op, &s, k.max(0) as usize, text.into()) });
    g.on_reset_entry(with(reset_entry));
    g.on_keep_entry(with(keep_entry));
    g.on_show_more(with(|op, st| { st.borrow_mut().limit += PAGE; refilter(op, st) }));
    g.on_use_language(with(use_language));
    g.on_mark_reviewed(with(mark_reviewed));
    g.on_export(with(export));
    g.on_import(with(import));
    g.on_add_language(with(|op, st| { apply(op, st); show_add(op, st) }));
    g.on_close(with(|op, _| op.close_dialog()));
    g.on_add_search_changed(with_s(fill_add));
    g.on_add_pick(with_i(|op, _, i| { let g = self::g(op); g.set_add_selected(i as i32); g.set_add_error("".into()) }));
    g.on_add_confirm(with(add_confirm));
    g.on_add_cancel(with(|op, st| {
        let code = st.borrow().draft.as_ref().map(|d| d.code.clone());
        match code { Some(c) => load(op, st, &c), None => op.close_dialog() }
    }));
}

/// The edit page for `code`.
fn load(op: &Rc<Operator>, st: &St, code: &str) {
    let draft = Draft::open(code);
    {
        let mut s = st.borrow_mut();
        s.rows = draft.rows();
        s.draft = Some(draft);
        s.selected = None;
        s.boxes.clear();
        s.limit = PAGE;
    }
    let g = g(op);
    g.set_page("edit".into());
    g.set_search("".into());
    g.set_filter(0);
    g.set_selected(-1);
    header(op, st);
    refilter(op, st);
}

/// The language's name, how much is done, whether it is the one on show.
fn header(op: &Operator, st: &St) {
    let s = st.borrow();
    let Some(d) = &s.draft else { return };
    let c = i18n::catalog(&d.code);
    let g = g(op);
    let name = c.as_ref().map_or_else(|| d.code.clone(), |c| if c.native_name == c.english_name { c.native_name.clone() } else { format!("{} ({})", c.native_name, c.english_name) });
    g.set_language(name.into());
    let done = s.rows.iter().filter(|r| r.translated()).count();
    g.set_progress(progress(done, s.rows.len()).into());
    g.set_not_shown(i18n::current() != d.code);
    g.set_reviewed(c.is_some_and(|c| c.reviewed));
}

/// "1,203 of 1,654 translated".
pub fn progress(done: usize, total: usize) -> String { tr!("{done} of {total} translated", done = grouped(done), total = grouped(total)) }

fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 { out.push(',') }
        out.push(c);
    }
    out
}

/// The filters, in the order of the buttons.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Show { All, Untranslated, NeedsReview, Mine }

/// Whether a row is on show for a search and a filter.
pub fn matches(r: &Row, search: &str, show: Show) -> bool {
    let kept = match show {
        Show::All => true,
        Show::Untranslated => !r.translated(),
        Show::NeedsReview => r.needs_review(),
        Show::Mine => r.mine.is_some(),
    };
    if !kept { return false }
    let q = search.trim().to_lowercase();
    q.is_empty() || [r.id.as_str(), r.plural.as_deref().unwrap_or(""), r.ctx.as_str()].iter().any(|t| t.to_lowercase().contains(&q))
        || r.shown().iter().any(|t| t.to_lowercase().contains(&q))
}

fn row_state(r: &Row) -> &'static str {
    if r.needs_review() { "review" } else if r.mine.is_some() { "mine" } else if !r.translated() { "missing" } else { "" }
}

fn trans_row(r: &Row) -> TransRow {
    let shown = r.shown().into_iter().find(|t| !t.is_empty()).unwrap_or_default();
    let hint = [r.ctx.clone(), r.where_used()].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" · ");
    TransRow { english: r.id.replace('\n', " ⏎ ").into(), shown: shown.replace('\n', " ⏎ ").into(), hint: hint.into(), state: row_state(r).into() }
}

/// The table from the search and filter.
fn refilter(op: &Operator, st: &St) {
    let g = g(op);
    let show = [Show::All, Show::Untranslated, Show::NeedsReview, Show::Mine][g.get_filter().clamp(0, 3) as usize];
    let search = g.get_search().to_string();
    let mut s = st.borrow_mut();
    let shown: Vec<usize> = s.rows.iter().enumerate().filter(|(_, r)| matches(r, &search, show)).map(|(i, _)| i).collect();
    let visible: Vec<TransRow> = shown.iter().take(s.limit).map(|&i| trans_row(&s.rows[i])).collect();
    g.set_more(if shown.len() > s.limit { tr!("Showing {shown} of {total}", shown = grouped(s.limit), total = grouped(shown.len())) } else { String::new() }.into());
    g.set_selected(s.selected.and_then(|sel| shown.iter().take(s.limit).position(|&i| i == sel)).map_or(-1, |p| p as i32));
    s.shown = shown;
    drop(s);
    g.set_rows(ModelRc::new(VecModel::from(visible)));
}

/// A row clicked: its English, notes and one box per form.
fn select(op: &Rc<Operator>, st: &St, i: usize) {
    apply(op, st);
    let Some(&idx) = st.borrow().shown.get(i) else { return };
    let (row, plurals) = {
        let mut s = st.borrow_mut();
        s.selected = Some(idx);
        let row = s.rows[idx].clone();
        s.boxes = row.shown();
        (row, s.draft.as_ref().map(Draft::plurals).unwrap_or_default())
    };
    let g = g(op);
    g.set_selected(i as i32);
    g.set_english(row.id.clone().into());
    g.set_english_plural(row.plural.clone().unwrap_or_default().into());
    g.set_context(row.ctx.clone().into());
    g.set_note(row.note.clone().into());
    g.set_where(if row.places.is_empty() { String::new() } else { tr!("Used in: {places}", places = row.where_used()) }.into());
    let mut boxes = st.borrow().boxes.clone();
    let labels: Vec<String> = if row.plural.is_some() {
        boxes.resize(plurals.count, String::new());
        user::plural_examples(&plurals).into_iter().map(|n| tr!("For {numbers}…", numbers = n)).collect()
    } else {
        boxes.resize(1, String::new());
        vec![String::new()]
    };
    st.borrow_mut().boxes = boxes.clone();
    let forms: Vec<TransForm> = labels.into_iter().zip(boxes).map(|(label, text)| TransForm { label: label.into(), text: text.into() }).collect();
    g.set_forms(ModelRc::new(VecModel::from(forms)));
    entry_state(op, st);
}

/// The chosen row's note and buttons.
fn entry_state(op: &Operator, st: &St) {
    let s = st.borrow();
    let Some(idx) = s.selected else { return };
    let r = &s.rows[idx];
    let g = g(op);
    g.set_problem(user::check(&r.id, r.plural.as_deref(), &s.boxes).unwrap_or_default().into());
    g.set_mine(r.mine.is_some());
    g.set_guess(r.mine.is_none() && r.builtin_guess);
}

/// Typing in a box: checked at once, used and saved after `APPLY_DELAY`.
fn edited(op: &Rc<Operator>, st: &St, k: usize, text: String) {
    {
        let mut s = st.borrow_mut();
        if s.selected.is_none() { return }
        if let Some(b) = s.boxes.get_mut(k) { *b = text }
    }
    entry_state(op, st);
    let (me, s) = (Rc::downgrade(op), st.clone());
    st.borrow().timer.start(slint::TimerMode::SingleShot, APPLY_DELAY, move || if let Some(op) = me.upgrade() { apply(&op, &s) });
}

/// Uses and saves what the boxes hold, if it changed.
fn apply(op: &Rc<Operator>, st: &St) {
    let changed = {
        let mut s = st.borrow_mut();
        s.timer.stop();
        let Some(idx) = s.selected else { return };
        let boxes = s.boxes.clone();
        let row = s.rows[idx].clone();
        if boxes == row.shown() || (row.mine.is_none() && boxes.iter().all(String::is_empty) && row.builtin.is_empty()) { return }
        let Some(d) = s.draft.as_mut() else { return };
        if boxes == row.builtin && row.mine.is_some() { d.reset(&row.ctx, &row.id) } else { d.set(&row.ctx, &row.id, &boxes) }
        true
    };
    if changed { save(op, st) }
}

/// Saves the user file, shows it everywhere, and draws the table again.
fn save(op: &Rc<Operator>, st: &St) {
    let result = {
        let mut s = st.borrow_mut();
        let Some(d) = s.draft.as_ref() else { return };
        let r = d.save(&folder(op));
        let rows = d.rows();
        s.rows = rows;
        r
    };
    if let Err(e) = result { op.toast(tr!("The translation could not be saved: {reason}", reason = e), true) }
    changed_everywhere(op);
    header(op, st);
    refilter(op, st);
    entry_state(op, st);
}

/// The new words in every window and on the phone.
fn changed_everywhere(op: &Operator) {
    i18n::sync_slint();
    op.request_refresh();
    op.app().broadcast_data();
}

fn reset_entry(op: &Rc<Operator>, st: &St) {
    let i = {
        let mut s = st.borrow_mut();
        s.timer.stop();
        let Some(idx) = s.selected else { return };
        let row = s.rows[idx].clone();
        if let Some(d) = s.draft.as_mut() { d.reset(&row.ctx, &row.id) }
        s.boxes = row.builtin.clone();
        s.shown.iter().position(|&x| x == idx)
    };
    save(op, st);
    if let Some(i) = i { select(op, st, i) }
}

/// "This is right": a built-in guess becomes the user's own.
fn keep_entry(op: &Rc<Operator>, st: &St) {
    {
        let mut s = st.borrow_mut();
        let Some(idx) = s.selected else { return };
        let row = s.rows[idx].clone();
        if let Some(d) = s.draft.as_mut() { d.set(&row.ctx, &row.id, &row.builtin) }
    }
    save(op, st);
}

fn use_language(op: &Rc<Operator>, st: &St) {
    apply(op, st);
    let Some(code) = st.borrow().draft.as_ref().map(|d| d.code.clone()) else { return };
    let (me, st) = (Rc::downgrade(op), st.clone());
    op.ops().call::<serde_json::Value>("settings/language", serde_json::json!({ "code": code }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(v) => {
                changed_everywhere(&op);
                header(&op, &st);
                if let Some(t) = v["toast"].as_str() { op.toast(t, false) }
            }
            Err(e) => op.toast(e, true),
        }
    });
}

fn mark_reviewed(op: &Rc<Operator>, st: &St) {
    apply(op, st);
    if let Some(d) = st.borrow_mut().draft.as_mut() { d.mark_reviewed() }
    save(op, st);
    op.toast(tr!("Marked as reviewed. Thank you!"), false);
}

/// "Export…": the whole language as one file, to share or send to the project.
fn export(op: &Rc<Operator>, st: &St) {
    apply(op, st);
    let Some(code) = st.borrow().draft.as_ref().map(|d| d.code.clone()) else { return };
    let me = Rc::downgrade(op);
    files::pick_save(op, Filter::Translation, &format!("{code}.po"), move |path| {
        let Some(op) = me.upgrade() else { return };
        match std::fs::write(&path, user::export(&code)) {
            Ok(()) => op.toast(tr!("Saved {file}. Send it to the RustyPresenter project to share it with other churches.", file = path.file_name().unwrap_or_default().to_string_lossy()), false),
            Err(e) => op.toast(tr!("The file could not be saved: {reason}", reason = e), true),
        }
    });
}

/// "Import…": a `.po` someone shared; its texts that differ from the built-in ones become the user's.
fn import(op: &Rc<Operator>, st: &St) {
    apply(op, st);
    let code = st.borrow().draft.as_ref().map(|d| d.code.clone());
    let (me, st) = (Rc::downgrade(op), st.clone());
    files::pick(op, Filter::Translation, false, move |paths| {
        let Some(op) = me.upgrade() else { return };
        let Some(path) = paths.first() else { return };
        let read = std::fs::read(path).map_err(|e| e.to_string())
            .and_then(|b| String::from_utf8(b).map_err(|_| tr!("it is not saved as UTF-8 text")))
            .and_then(|text| user::import(&folder(&op), &text, code.as_deref()));
        match read {
            Ok(code) => {
                changed_everywhere(&op);
                load(&op, &st, &code);
                let name = i18n::catalog(&code).map_or(code, |c| c.native_name.clone());
                op.toast(tr!("Imported {language}", language = name), false);
            }
            Err(e) => op.toast(e, true),
        }
    });
}

// ---- Add a language ----

fn show_add(op: &Rc<Operator>, st: &St) {
    let g = g(op);
    g.set_page("add".into());
    g.set_add_search("".into());
    g.set_add_custom(false);
    g.set_add_code("".into());
    g.set_add_native("".into());
    g.set_add_english("".into());
    g.set_add_plural(languages::find("en-GB").map_or("", |l| l.plural_forms).into());
    g.set_add_error("".into());
    let mut options = vec![tr!("Nothing (the English shows until you type)")];
    let mut codes = vec![None];
    for l in i18n::languages().into_iter().skip(1) {
        options.push(tr!("A copy of {language}", language = l.native_name));
        codes.push(Some(l.code));
    }
    g.set_add_copy_options(ModelRc::new(VecModel::from(options.into_iter().map(Into::into).collect::<Vec<slint::SharedString>>())));
    g.set_add_copy(0);
    st.borrow_mut().copy_codes = codes;
    fill_add(op, st);
}

/// The languages of the list that the app does not have yet, matching the search.
fn fill_add(op: &Rc<Operator>, st: &St) {
    let g = g(op);
    let q = g.get_add_search().trim().to_lowercase();
    let list: Vec<&'static Language> = languages::LANGUAGES.iter()
        .filter(|l| !i18n::is_known(l.code))
        .filter(|l| q.is_empty() || [l.code, l.native, l.english].iter().any(|t| t.to_lowercase().contains(&q)))
        .collect();
    let rows: Vec<TransLang> = list.iter().map(|l| TransLang { title: l.native.into(), sub: format!("{} · {}", l.english, l.code).into() }).collect();
    st.borrow_mut().add_list = list;
    g.set_add_selected(-1);
    g.set_add_langs(ModelRc::new(VecModel::from(rows)));
}

fn add_confirm(op: &Rc<Operator>, st: &St) {
    let g = g(op);
    let (code, native, english, plural) = if g.get_add_custom() {
        let code = g.get_add_code().trim().to_string();
        let native = g.get_add_native().trim().to_string();
        let english = g.get_add_english().trim().to_string();
        let native = if native.is_empty() { code.clone() } else { native };
        let english = if english.is_empty() { native.clone() } else { english };
        (code, native, english, g.get_add_plural().trim().to_string())
    } else {
        let picked = usize::try_from(g.get_add_selected()).ok().and_then(|i| st.borrow().add_list.get(i).copied());
        let Some(l) = picked else { return g.set_add_error(tr!("Pick a language from the list, or tick “My language is not in the list”.").into()) };
        (l.code.to_string(), l.native.to_string(), l.english.to_string(), l.plural_forms.to_string())
    };
    let copy = st.borrow().copy_codes.get(g.get_add_copy().max(0) as usize).cloned().flatten();
    match user::add_language(&folder(op), &code, &native, &english, &plural, copy.as_deref()) {
        Ok(()) => {
            changed_everywhere(op);
            load(op, st, &code);
            op.toast(tr!("{language} added. Type your words, then pick it in Settings → Language.", language = native), false);
        }
        Err(e) => g.set_add_error(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dialogs::settings::testing::{find_any as find, find_all_any as find_all};
    use crate::{
        app::{App, test_app},
        ui::{SettingsDlg, dialogs::{SettingsTab, settings}, operator::{files::testing::will_pick, window::testing::{open as open_window, refresh}}},
    };
    use i_slint_backend_testing::mock_elapsed_time;
    use slint::Model;
    use std::sync::Arc;

    fn opened(app: &Arc<App>) -> Rc<Operator> {
        crate::i18n::testing::reset();
        let op = open_window(app);
        op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        op
    }

    fn settle() { slint::platform::update_timers_and_animations(); }

    /// Puts this thread back in English when a test ends, even a failed one.
    struct BackToEnglish;
    impl Drop for BackToEnglish {
        fn drop(&mut self) { let _ = crate::i18n::set_language("en"); }
    }

    /// The Slint side's words on the Language page: its row's name (an `@tr` text), in the place the old tab row had it.
    fn tab_labels(op: &Operator) -> Vec<String> {
        settle();
        let row = find_all(op, "SettingsLanguage::language-set").first().and_then(|r| r.accessible_label()).unwrap_or_default().to_string();
        vec![String::new(), String::new(), row]
    }

    /// The row on show whose English is `english`, after searching for it.
    fn pick(op: &Rc<Operator>, english: &str) -> usize {
        let g = g(op);
        g.set_search(english.into());
        g.invoke_search_changed(english.into());
        let i = g.get_rows().iter().position(|r| r.english == english).unwrap_or_else(|| panic!("{english} not listed"));
        g.invoke_select(i as i32);
        i
    }

    fn typed(op: &Rc<Operator>, k: i32, text: &str) {
        g(op).invoke_form_edited(k, text.into());
        mock_elapsed_time(APPLY_DELAY + Duration::from_millis(10));
        settle();
    }

    #[test]
    fn an_edit_shows_in_the_slint_window_without_a_restart() {
        let app = test_app();
        let op = opened(&app);
        let _en = BackToEnglish;
        crate::i18n::set_language("es").unwrap();
        settings::open(&op, SettingsTab::Language);
        assert_eq!(tab_labels(&op)[2], "Idioma", "built in");
        crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>().invoke_edit_translation();
        assert_eq!(op.dialog(), Some(DialogKind::Translations));
        let g = g(&op);
        assert_eq!(g.get_page(), "edit");
        assert_eq!(g.get_language(), "Español (Spanish)");
        let rows = Draft::open("es").rows();
        let done = rows.iter().filter(|r| r.translated()).count();
        assert_eq!(done, rows.len(), "Spanish is built in and complete");
        assert_eq!(g.get_progress(), progress(done, rows.len()), "the count, in the language on show");
        assert!(!g.get_not_shown());

        pick(&op, "Language");
        assert_eq!(g.get_forms().row_data(0).unwrap().text, "Idioma", "the box starts with the built-in text");
        g.invoke_form_edited(0, "Lengua {x}".into());
        let problem = crate::i18n::user::check("Language", None, &["Lengua {x}".into()]).unwrap();
        assert!(problem.starts_with("{x} ") && !problem.contains("English"), "in Spanish: {problem}");
        assert_eq!(g.get_problem(), problem, "checked while typing");
        typed(&op, 0, "Lengua");
        assert_eq!(g.get_problem(), "");
        assert!(g.get_mine());
        let saved = std::fs::read_to_string(app.store.root.join("translations/es.po")).unwrap();
        assert!(saved.contains("msgid \"Language\"\nmsgstr \"Lengua\""), "saved by itself: {saved}");

        // The Slint text changed with no restart.
        g.invoke_close();
        settings::open(&op, SettingsTab::Language);
        assert_eq!(tab_labels(&op)[2], "Lengua", "the patched Slint core asks the app first");
        assert_eq!(tr!("Language"), "Lengua", "and the Rust side");

        // Reset goes back to the built-in text, at once.
        crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>().invoke_edit_translation();
        pick(&op, "Language");
        g.invoke_reset_entry();
        assert!(!g.get_mine());
        g.invoke_close();
        settings::open(&op, SettingsTab::Language);
        assert_eq!(tab_labels(&op)[2], "Idioma");
    }

    #[test]
    fn a_new_language_from_the_picker_shows_in_slint() {
        let app = test_app();
        let op = opened(&app);
        let _en = BackToEnglish;
        op.ui.invoke_menu("translations".into());
        assert_eq!(op.dialog(), Some(DialogKind::Translations));
        let g = g(&op);
        assert_eq!(g.get_page(), "add", "English has nothing to edit: the picker opens");
        assert!(g.get_add_langs().row_count() >= 80, "the built-in languages are left out");
        g.set_add_search("pols".into());
        g.invoke_add_search_changed("pols".into());
        assert_eq!(g.get_add_langs().row_data(0).unwrap().title, "Polski");
        g.invoke_add_confirm();
        assert!(g.get_add_error().starts_with("Pick a language"), "{}", g.get_add_error());
        g.invoke_add_pick(0);
        let options: Vec<String> = g.get_add_copy_options().iter().map(|s| s.to_string()).collect();
        let ru = options.iter().position(|o| o == "A copy of Русский").expect("copy of Russian offered");
        g.set_add_copy(ru as i32);
        g.invoke_add_confirm();
        assert_eq!(g.get_page(), "edit", "{}", g.get_add_error());
        assert!(g.get_language().starts_with("Polski"));
        assert!(g.get_not_shown());
        assert!(crate::i18n::languages().iter().any(|l| l.code == "pl"));

        g.invoke_use_language();
        assert_eq!(crate::i18n::current(), "pl");
        assert_eq!(app.data.lock().unwrap().settings.ui_language, "pl");
        pick(&op, "Language");
        assert_eq!(g.get_forms().row_data(0).unwrap().text, "Язык", "copied from Russian");
        typed(&op, 0, "Język");
        // Plural boxes are labelled with numbers from the Polish rule (in the words on show, copied from Russian).
        pick(&op, "{n} song");
        let labels: Vec<String> = g.get_forms().iter().map(|f| f.label.to_string()).collect();
        let want: Vec<String> = ["1", "2, 3, 4", "0, 5, 6"].iter().map(|n| tr!("For {numbers}…", numbers = n)).collect();
        assert_eq!(labels, want);
        assert_eq!(labels[0], "Для 1…", "Polish shows the copied Russian words");
        g.invoke_close();
        settings::open(&op, SettingsTab::Language);
        assert_eq!(tab_labels(&op)[2], "Język", "a language Slint never had, through the hook");
        let rows: Vec<crate::ui::SettingsLanguageRow> = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>().get_languages().iter().collect();
        assert!(rows.iter().any(|r| r.title.starts_with("Polski (") && r.on), "listed (marked for review, in the copied words) and in use");
    }

    #[test]
    fn export_import_and_review_buttons() {
        let app = test_app();
        let op = opened(&app);
        let _en = BackToEnglish;
        crate::i18n::set_language("de").unwrap();
        crate::ui::dialogs::translation_editor::open(&op, None);
        let g = g(&op);
        pick(&op, "Language");
        typed(&op, 0, "Sprache!");
        g.invoke_mark_reviewed();
        assert!(g.get_reviewed());
        let thanks = tr!("Marked as reviewed. Thank you!");
        assert_ne!(thanks, "Marked as reviewed. Thank you!", "German is on show");
        assert!(op.toast_log().iter().any(|(t, _)| *t == thanks), "{:?}", op.toast_log());
        let out = crate::store::test_dir().join("de.po");
        will_pick(vec![out.clone()]);
        g.invoke_export();
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("\"X-Reviewed: yes\\n\"") && text.contains("msgstr \"Sprache!\""), "{}", &text[..400.min(text.len())]);

        // Into another church's data folder.
        let other = test_app();
        let op2 = opened(&other);
        crate::i18n::set_language("de").unwrap();
        crate::ui::dialogs::translation_editor::open(&op2, None);
        will_pick(vec![out]);
        super::g(&op2).invoke_import();
        let imported = tr!("Imported {language}", language = "Deutsch");
        assert!(op2.toast_log().iter().any(|(t, _)| *t == imported), "{:?}", op2.toast_log());
        assert_eq!(tr!("Language"), "Sprache!");
        assert!(super::g(&op2).get_reviewed());
        assert!(find(&op2, "EditPage::trans-close").is_some());
    }

    #[test]
    fn progress_and_filters() {
        assert_eq!(progress(1203, 1654), "1,203 of 1,654 translated");
        assert_eq!(progress(0, 12), "0 of 12 translated");
        let row = |mine: Option<&str>, builtin: &str, guess: bool| Row {
            ctx: String::new(), id: "Delete {name}".into(), plural: None, note: String::new(), places: vec![],
            builtin: if builtin.is_empty() { vec![] } else { vec![builtin.into()] }, builtin_guess: guess, mine: mine.map(|m| vec![m.into()]),
        };
        let missing = row(None, "", false);
        let built = row(None, "Borrar {name}", false);
        let guess = row(None, "Borrar {name}", true);
        let mine = row(Some("Quitar {name}"), "Borrar {name}", false);
        let broken = row(Some("Quitar"), "", false);
        let on = |show: Show| [&missing, &built, &guess, &mine, &broken].iter().map(|r| matches(r, "", show)).collect::<Vec<_>>();
        assert_eq!(on(Show::All), [true; 5]);
        assert_eq!(on(Show::Untranslated), [true, false, true, false, true]);
        assert_eq!(on(Show::NeedsReview), [false, false, true, false, true]);
        assert_eq!(on(Show::Mine), [false, false, false, true, true]);
        assert!(matches(&mine, "QUITAR", Show::All) && matches(&mine, "delete", Show::All) && !matches(&mine, "song", Show::All));
        assert_eq!([&missing, &built, &guess, &mine, &broken].map(row_state), ["missing", "", "review", "mine", "review"]);
    }
}
