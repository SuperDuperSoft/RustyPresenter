//! Settings → Bible (`operator.js:922-973`, after the `bible-esv` merge): every translation's status, the online
//! services a church connects with a key (`bible/key`), Bible file import (`bible/import/{ABBR}`, the raw file) and
//! removal (`bible/import-remove`), and the copyright lines of the sources in use.
use super::settings::{Edit, copy, redraw_on_next_read, strings};
use crate::{
    bible::{blocklist::normalise, catalog::strings as cat},
    render::Fonts,
    ui::{
        AlsoChip, SettingsBibleRow, SettingsCatalogRow, SettingsCreditRow, SettingsDlg, SettingsProviderRow,
        operator::{file_input::Filter, files, view::{BibleProvider, BibleSource, JobRow, OperatorSnapshot}, window::Operator},
    },
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{cell::{Cell, RefCell}, collections::{HashMap, HashSet}, path::PathBuf, rc::{Rc, Weak}};

/// The rows the tab shows, so a button's index finds its translation or service.
#[derive(Default)]
pub struct BibleState {
    sources: RefCell<Vec<BibleSource>>,
    providers: RefCell<Vec<BibleProvider>>,
    /// The Default Bible list's translation ids, in its order.
    defaults: RefCell<Vec<String>>,
    /// The Also show chips' translation ids, in their order.
    also: RefCell<Vec<String>>,
    pub(crate) catalog: CatalogState,
}

/// A row of `bible/catalog` (`App::bible_catalog`), in its order: grouped by language, the church's first.
#[derive(serde::Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct CatRow {
    pub id: String,
    pub abbr: String,
    pub title: String,
    pub lang: String,
    pub lang_name: String,
    pub lang_name_en: String,
    pub scope: String,
    pub apocrypha: bool,
    pub licence_on_download: bool,
    pub installed: bool,
    pub update: bool,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct Catalog { offline: bool, rows: Vec<CatRow> }

/// The library list of one open Settings dialog.
#[derive(Default)]
pub struct CatalogState {
    rows: RefCell<Vec<CatRow>>,
    offline: Cell<bool>,
    loading: Cell<bool>,
    loaded: Cell<bool>,
    error: RefCell<Option<String>>,
    /// The language filter's codes; the Select's index is one more (0 is "All languages").
    langs: RefCell<Vec<String>>,
    /// Each shown line's row, `None` for a heading.
    shown: RefCell<Vec<Option<usize>>>,
    /// Asked to install and not answered yet; and installed while this dialog was open.
    pending: RefCell<HashSet<String>>,
    added: RefCell<HashSet<String>>,
    failed: RefCell<HashMap<String, String>>,
    /// Rows checked for a missing font (spec §9.1), by id: the check shapes text, so each row is checked once.
    no_font: RefCell<HashMap<String, bool>>,
}

/// At most this many Bibles are listed at once; the search narrows the rest.
pub const MAX_SHOWN: usize = 200;

/// What the list shows for one query: lines (a heading, or a row index) and the note above them.
#[derive(Debug, PartialEq)]
pub struct Shown { pub lines: Vec<Option<usize>>, pub note: String }

/// Picks the rows for the search words (every word, folded like the catalog), Installed (`filter` 0) or All, and
/// a language (`""` for all), keeping the rows' order, with a heading line where the language changes.
pub fn select(rows: &[CatRow], query: &str, filter: i32, lang: &str) -> Shown {
    let words: Vec<String> = query.split_whitespace().map(normalise).filter(|w| !w.is_empty()).collect();
    let hit = |r: &CatRow| {
        if filter == 0 && !r.installed { return false }
        if !lang.is_empty() && r.lang != lang { return false }
        if words.is_empty() { return true }
        let hay = normalise(&format!("{} {} {} {} {} {}", r.title, r.abbr, r.lang_name, r.lang_name_en, r.lang, r.id));
        words.iter().all(|w| hay.contains(w.as_str()))
    };
    let matched: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| hit(r)).map(|(i, _)| i).collect();
    let mut lines = Vec::new();
    let mut last: Option<&str> = None;
    for &i in matched.iter().take(MAX_SHOWN) {
        if last != Some(rows[i].lang.as_str()) { lines.push(None); last = Some(rows[i].lang.as_str()); }
        lines.push(Some(i));
    }
    let note = if matched.is_empty() {
        if filter == 0 && words.is_empty() && lang.is_empty() { cat::none_installed().into() } else { cat::no_match().into() }
    } else if matched.len() > MAX_SHOWN { cat::showing(MAX_SHOWN, matched.len()) } else { String::new() };
    Shown { lines, note }
}

/// The languages of the filter, in the list's order (the church's first), as codes and labels.
pub fn languages(rows: &[CatRow]) -> (Vec<String>, Vec<String>) {
    let (mut codes, mut labels) = (Vec::<String>::new(), Vec::new());
    for r in rows {
        if !codes.contains(&r.lang) {
            codes.push(r.lang.clone());
            labels.push(cat::language(&r.lang_name_en, &r.lang_name, &r.lang));
        }
    }
    (codes, labels)
}

/// What the missing-font check shapes for a row: its title and its language's own name, both shown in the list.
/// The first verse is not checked yet: that needs the Bible read, and the list is drawn on the window's thread.
pub fn font_test_line(r: &CatRow) -> String { format!("{} {}", r.title, r.lang_name) }

/// Whether no font in `fonts` can draw some of the row's test line.
pub fn needs_font(fonts: &mut Fonts, r: &CatRow) -> bool { fonts.lacks_glyphs(&font_test_line(r)) }

thread_local! {
    /// The fonts the rows are checked with: the bundled faces and the system's, as slides use. Made on the first
    /// check, as loading the system fonts takes a moment.
    static FONTS: RefCell<Option<Fonts>> = const { RefCell::new(None) };
}

/// Checks rows with the bundled faces only from now on, on this thread: tests get the same answer on every machine.
#[cfg(test)]
pub(crate) fn check_with_bundled_fonts() { FONTS.with(|f| *f.borrow_mut() = Some(Fonts::bundled_only())) }

/// Whether no font here (the app's, as for Bible rows) can draw some of `text`. Latin letters are in every font, so
/// they need no check (which would load the system's fonts).
pub(crate) fn lacks_font(text: &str) -> bool {
    if text.chars().all(|c| (c as u32) < 0x250) { return false }
    FONTS.with(|f| f.borrow_mut().get_or_insert_with(Fonts::new).lacks_glyphs(text))
}

/// `needs_font` with the app's fonts, once per row id.
fn no_font(memo: &RefCell<HashMap<String, bool>>, r: &CatRow) -> bool {
    if let Some(&v) = memo.borrow().get(&r.id) { return v }
    let v = FONTS.with(|f| needs_font(f.borrow_mut().get_or_insert_with(Fonts::new), r));
    memo.borrow_mut().insert(r.id.clone(), v);
    v
}

/// One Bible line: its tag, the status line, the button and the bar. A running download (its job) wins; then a
/// failure from this dialog; then what is installed. `no_font` adds the missing-font warning, in every state.
#[allow(clippy::too_many_arguments)]
pub fn line(r: &CatRow, job: Option<&JobRow>, pending: bool, added: bool, failed: Option<&str>, offline: bool, same_abbr: bool,
    no_font: bool) -> SettingsCatalogRow {
    let mut tag = vec![cat::tag_offline(), r.scope.clone()];
    if r.apocrypha { tag.push(cat::includes_apocrypha().into()) }
    if r.licence_on_download && !r.installed { tag.push(cat::licence_on_download().into()) }
    let abbr = if same_abbr { format!("{} · {}", r.abbr, r.lang) } else { r.abbr.clone() };
    let mut row = SettingsCatalogRow {
        heading: SharedString::new(), abbr: abbr.into(), title: r.title.as_str().into(), tag: tag.join(" · ").into(),
        status: SharedString::new(), button: SharedString::new(), progress: -1.0, enabled: true, primary: false,
        font_warning: if no_font { cat::no_font().into() } else { SharedString::new() },
    };
    let running = job.filter(|j| j.error.is_none() && !j.done);
    if let Some(j) = running {
        let waiting = j.label == cat::waiting(&r.title);
        row.status = if waiting { cat::waiting_short().into() } else { cat::downloading(j.percent).into() };
        row.progress = if waiting { -1.0 } else { f32::from(j.percent) };
        row.button = cat::cancel().into();
        row.enabled = j.cancellable;
        return row;
    }
    if pending {
        row.status = cat::waiting_short().into();
        row.progress = 0.0;
        return row;
    }
    if let Some(e) = failed { row.status = e.into() }
    if r.installed && r.update {
        if row.status.is_empty() { row.status = cat::update_ready().into() }
        (row.button, row.primary, row.enabled) = (cat::update().into(), true, !offline);
    } else if r.installed {
        if added { row.status = cat::added_tag().into() }
        row.button = cat::remove().into();
    } else {
        (row.button, row.primary, row.enabled) = (cat::add().into(), true, !offline);
    }
    row
}

thread_local! {
    /// The open Settings dialog's state, so job progress (which reaches the window, not the dialog) redraws the list.
    static OPEN: RefCell<Weak<super::settings::Edit>> = const { RefCell::new(Weak::new()) };
}

/// A new dialog starts with an empty search on All, every language, and no list until the Bible tab is shown.
pub(super) fn reset(g: &SettingsDlg) {
    g.set_catalog_search(SharedString::new());
    g.set_catalog_filter(1);
    g.set_catalog_languages(ModelRc::new(VecModel::from(vec![SharedString::from(cat::all_languages())])));
    g.set_catalog_language(0);
    g.set_catalog_note(SharedString::new());
    g.set_catalog_rows(ModelRc::default());
}

/// Puts `query` in the library's search box and redraws the list (the picker's "Find more Bibles…").
pub(super) fn search_for(op: &Operator, query: &str) {
    crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>().set_catalog_search(query.trim().into());
    if let Some(edit) = OPEN.with(|o| o.borrow().upgrade()) { render(op, &edit) }
}

pub(super) fn remember(edit: &Rc<super::settings::Edit>) { OPEN.with(|o| *o.borrow_mut() = Rc::downgrade(edit)) }

/// Loads the library list once per dialog, when the Bible tab is first shown. A list less than a day old comes from
/// disk; otherwise eBible is asked (spec §7.1, §13: only because the user opened the library).
pub(super) fn load_catalog(op: &Rc<Operator>, edit: &Rc<super::settings::Edit>) {
    let c = &edit.bible.catalog;
    if c.loading.get() || c.loaded.get() { return render(op, edit) }
    c.loading.set(true);
    render(op, edit);
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<Catalog>("bible/catalog", serde_json::json!({}), move |r| {
        let Some(op) = me.upgrade() else { return };
        let c = &edit.bible.catalog;
        c.loading.set(false);
        c.loaded.set(true);
        match r {
            Ok(list) => {
                c.offline.set(list.offline);
                *c.rows.borrow_mut() = list.rows;
                *c.error.borrow_mut() = None;
                let (codes, labels) = languages(&c.rows.borrow());
                *c.langs.borrow_mut() = codes;
                let mut options: Vec<SharedString> = vec![cat::all_languages().into()];
                options.extend(labels.into_iter().map(SharedString::from));
                let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
                g.set_catalog_languages(ModelRc::new(VecModel::from(options)));
                g.set_catalog_language(0);
            }
            Err(e) => *c.error.borrow_mut() = Some(e),
        }
        render(&op, &edit);
    });
}

/// Draws the library list from the state, the form's search, filter and language, and the jobs.
pub(super) fn render(op: &Operator, edit: &super::settings::Edit) {
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    let c = &edit.bible.catalog;
    g.set_catalog_loading(c.loading.get());
    if c.loading.get() {
        g.set_catalog_note(cat::list_loading().into());
        g.set_catalog_rows(ModelRc::default());
        return;
    }
    if let Some(e) = c.error.borrow().as_ref() {
        g.set_catalog_note(e.as_str().into());
        g.set_catalog_rows(ModelRc::default());
        c.shown.borrow_mut().clear();
        return;
    }
    let rows = c.rows.borrow();
    let lang = usize::try_from(g.get_catalog_language() - 1).ok().and_then(|i| c.langs.borrow().get(i).cloned()).unwrap_or_default();
    let shown = select(&rows, &g.get_catalog_search(), g.get_catalog_filter(), &lang);
    let jobs: Vec<JobRow> = op.state.borrow().snap.as_ref().map(|s| s.jobs.clone()).unwrap_or_default();
    let mut abbr_count: HashMap<String, usize> = HashMap::new();
    for &i in shown.lines.iter().flatten() { *abbr_count.entry(rows[i].abbr.to_lowercase()).or_default() += 1 }
    let (pending, added, failed) = (c.pending.borrow(), c.added.borrow(), c.failed.borrow());
    let lines: Vec<SettingsCatalogRow> = shown.lines.iter().enumerate().map(|(n, l)| match l {
        Some(i) => {
            let r = &rows[*i];
            let job = jobs.iter().rev().find(|j| j.subject.as_deref() == Some(r.id.as_str()));
            line(r, job, pending.contains(&r.id), added.contains(&r.id), failed.get(&r.id).map(String::as_str), c.offline.get(),
                abbr_count.get(&r.abbr.to_lowercase()).copied().unwrap_or(0) > 1, no_font(&c.no_font, r))
        }
        None => {
            let r = shown.lines.get(n + 1).copied().flatten().map(|i| &rows[i]);
            let heading = r.map(|r| cat::language(&r.lang_name_en, &r.lang_name, &r.lang)).unwrap_or_default();
            SettingsCatalogRow { heading: heading.into(), progress: -1.0, ..Default::default() }
        }
    }).collect();
    let note = if c.offline.get() {
        if shown.note.is_empty() { cat::catalog_offline() } else { format!("{} {}", cat::catalog_offline(), shown.note) }
    } else { shown.note.clone() };
    g.set_catalog_note(note.into());
    g.set_catalog_rows(ModelRc::new(VecModel::from(lines)));
    *c.shown.borrow_mut() = shown.lines;
}

/// A job's progress or end reached the window: redraw the list if Settings is open and has one.
pub fn jobs_changed(op: &Operator) {
    if !super::settings::is_open(op) { return }
    let Some(edit) = OPEN.with(|o| o.borrow().upgrade()) else { return };
    if edit.bible.catalog.loaded.get() { render(op, &edit) }
}

/// A row's one button: Add or Update (`bible/install`), Remove (`bible/import-remove`), Cancel (`job/cancel`).
pub(super) fn catalog_button(op: &Rc<Operator>, edit: &Rc<super::settings::Edit>, n: usize) {
    let c = &edit.bible.catalog;
    let Some(Some(i)) = c.shown.borrow().get(n).copied() else { return };
    let Some(r) = c.rows.borrow().get(i).cloned() else { return };
    let jobs: Vec<JobRow> = op.state.borrow().snap.as_ref().map(|s| s.jobs.clone()).unwrap_or_default();
    if let Some(j) = jobs.iter().rev().find(|j| j.subject.as_deref() == Some(r.id.as_str()) && j.error.is_none() && !j.done) {
        let id = j.id;
        op.ops().call::<serde_json::Value>("job/cancel", serde_json::json!({ "id": id }), |_| {});
        return;
    }
    if c.pending.borrow().contains(&r.id) { return }
    if r.installed && !r.update {
        let (me, edit) = (Rc::downgrade(op), edit.clone());
        op.ops().call::<serde_json::Value>("bible/import-remove", serde_json::json!({ "translation": r.id }), move |res| {
            let Some(op) = me.upgrade() else { return };
            match res {
                Ok(_) => {
                    let c = &edit.bible.catalog;
                    if let Some(row) = c.rows.borrow_mut().get_mut(i).filter(|x| x.id == r.id) { row.installed = false; row.update = false }
                    c.added.borrow_mut().remove(&r.id);
                    render(&op, &edit);
                    redraw_on_next_read(&op, &edit);
                    op.toast(cat::removed(&r.abbr), false);
                }
                Err(e) => op.toast(e, true),
            }
        });
        return;
    }
    if c.offline.get() { return }
    c.pending.borrow_mut().insert(r.id.clone());
    c.failed.borrow_mut().remove(&r.id);
    render(op, edit);
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<serde_json::Value>("bible/install", serde_json::json!({ "translation": r.id }), move |res| {
        let Some(op) = me.upgrade() else { return };
        let c = &edit.bible.catalog;
        c.pending.borrow_mut().remove(&r.id);
        match res {
            Ok(_) => {
                if let Some(row) = c.rows.borrow_mut().get_mut(i).filter(|x| x.id == r.id) { row.installed = true; row.update = false }
                c.added.borrow_mut().insert(r.id.clone());
                redraw_on_next_read(&op, &edit);
                op.toast(cat::added(&r.abbr), false);
            }
            // The job card shows the error too; the row keeps it so it is seen where the button was.
            Err(e) if e == crate::import::CANCELLED => {}
            Err(e) => { c.failed.borrow_mut().insert(r.id.clone(), e); }
        }
        render(&op, &edit);
    });
}

/// The line under a translation's name (`hint`, `operator.js:925-926`).
pub fn hint(b: &BibleSource, providers: &[BibleProvider]) -> String {
    match b.status.as_str() {
        "needs_key" => strings::connect_below(&b.providers.iter()
            .filter_map(|id| providers.iter().find(|p| p.id == *id).map(|p| p.label.clone())).collect::<Vec<_>>()),
        "unavailable" => strings::no_online_source().into(),
        _ => b.provider.clone().unwrap_or_default(),
    }
}

/// The Default Bible list: every Bible that can be used now, and the one in use even when it can't (a key was
/// removed), first, so the list never shows another Bible than the Bible box does. Ids and labels.
pub fn defaults(snap: &OperatorSnapshot) -> (Vec<String>, Vec<String>) {
    let mut ids = snap.available.clone();
    if !ids.contains(&snap.translation) { ids.insert(0, snap.translation.clone()) }
    let labels = ids.iter().map(|id| match snap.bible_sources.iter().find(|b| b.translation == *id) {
        Some(b) => cat::bible_option(b.label(), &b.name),
        None => id.clone(),
    }).collect();
    (ids, labels)
}

/// Every Bible in use that carries a notice or a licence: its credit line and the licence's link (spec §7.1).
pub fn credits(sources: &[BibleSource]) -> Vec<(String, String)> {
    sources.iter()
        .filter(|b| b.status == "online" || b.imported)
        .filter(|b| b.copyright.as_deref().is_some_and(|c| !c.is_empty()) || !b.licence.is_empty())
        .map(|b| (cat::credit(b.label(), b.copyright.as_deref().unwrap_or(""), &b.licence), b.licence_url.clone()))
        .collect()
}

/// Default Bible picked: `settings/translation` at once, as the Bible box's chips do, so Save and Cancel don't
/// touch it. A failure puts the list back to what is in use.
pub(super) fn default_bible(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(id) = edit.bible.defaults.borrow().get(i).cloned() else { return };
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<serde_json::Value>("settings/translation", serde_json::json!({ "translation": id }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => op.toast(cat::default_set(), false),
            Err(e) => op.toast(e, true),
        }
        redraw_on_next_read(&op, &edit);
    });
}

/// The Also show chips: every Bible usable now but the default one, ids and labels, and which are picked.
pub fn also_chips(snap: &OperatorSnapshot) -> Vec<(String, String, bool)> {
    snap.available.iter().filter(|id| !crate::bible::id::same_id(id, &snap.translation)).map(|id| {
        let label = snap.bible_sources.iter().find(|b| b.translation == *id).map_or_else(|| id.clone(), |b| b.label().to_string());
        let on = snap.settings.also_show.iter().any(|t| crate::bible::id::same_id(t, id));
        (id.clone(), label, on)
    }).collect()
}

/// A chip clicked: picks or unpicks that Bible for every new passage (`settings/also`), at most `MAX_ALSO`.
pub(super) fn toggle_also(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(id) = edit.bible.also.borrow().get(i).cloned() else { return };
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    let mut picked = snap.settings.also_show.clone();
    if picked.iter().any(|t| crate::bible::id::same_id(t, &id)) {
        picked.retain(|t| !crate::bible::id::same_id(t, &id));
    } else if picked.len() >= crate::scripture::MAX_ALSO {
        op.toast(cat::also_full(), true);
        return;
    } else {
        picked.push(id);
    }
    set_also(op, edit, picked, snap.settings.parallel_layout);
}

/// Side by side or top and bottom, for every new passage.
pub(super) fn also_layout(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    let layout = if i == 1 { crate::scripture::ParallelLayout::Stacked } else { crate::scripture::ParallelLayout::SideBySide };
    set_also(op, edit, snap.settings.also_show.clone(), layout);
}

fn set_also(op: &Rc<Operator>, edit: &Rc<Edit>, also_show: Vec<String>, layout: crate::scripture::ParallelLayout) {
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<serde_json::Value>("settings/also", serde_json::json!({ "also_show": also_show, "parallel_layout": layout }), move |r| {
        let Some(op) = me.upgrade() else { return };
        if let Err(e) = r { op.toast(e, true) }
        redraw_on_next_read(&op, &edit);
    });
}

pub(super) fn fill(g: &SettingsDlg, edit: &Edit, snap: &OperatorSnapshot) {
    let chips = also_chips(snap);
    g.set_also_chips(ModelRc::new(VecModel::from(chips.iter().map(|(_, label, on)| AlsoChip { label: label.as_str().into(), on: *on }).collect::<Vec<_>>())));
    g.set_also_layout(i32::from(snap.settings.parallel_layout == crate::scripture::ParallelLayout::Stacked));
    *edit.bible.also.borrow_mut() = chips.into_iter().map(|(id, _, _)| id).collect();
    let (ids, labels) = defaults(snap);
    g.set_default_bible(ids.iter().position(|id| *id == snap.translation).unwrap_or(0) as i32);
    g.set_default_bibles(ModelRc::new(VecModel::from(labels.into_iter().map(SharedString::from).collect::<Vec<_>>())));
    *edit.bible.defaults.borrow_mut() = ids;
    let rows: Vec<SettingsBibleRow> = snap.bible_sources.iter().map(|b| {
        let (state, chip) = strings::chip(&b.status);
        SettingsBibleRow {
            translation: b.label().into(), hint: hint(b, &snap.bible_providers).into(),
            chip: chip.into(), chip_state: state.into(), imported: b.imported,
        }
    }).collect();
    g.set_bible_sources(ModelRc::new(VecModel::from(rows)));
    let providers: Vec<SettingsProviderRow> = snap.bible_providers.iter().map(|p| SettingsProviderRow {
        label: p.label.as_str().into(), for_text: strings::provider_for(&p.translations).into(), key_set: p.key_set,
        signup_url: p.signup_url.as_str().into(), note: p.note.as_str().into(),
    }).collect();
    g.set_bible_providers(ModelRc::new(VecModel::from(providers)));
    let credits: Vec<SettingsCreditRow> = credits(&snap.bible_sources).into_iter()
        .map(|(text, url)| SettingsCreditRow { text: text.into(), url: url.into() })
        .collect();
    g.set_bible_credits(ModelRc::new(VecModel::from(credits)));
    *edit.bible.sources.borrow_mut() = snap.bible_sources.clone();
    *edit.bible.providers.borrow_mut() = snap.bible_providers.clone();
}

/// Save key / Disconnect (an empty key): `bible/key`, then the tab again from a fresh read and a toast.
pub(super) fn save_key(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize, key: &str) {
    let Some(p) = edit.bible.providers.borrow().get(i).cloned() else { return };
    let key = key.trim().to_string();
    let connected = !key.is_empty();
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<serde_json::Value>("bible/key", serde_json::json!({ "provider": p.id, "key": key }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => {
                redraw_on_next_read(&op, &edit);
                op.toast(strings::key_saved(&p.label, connected), false);
            }
            Err(e) => op.toast(e, true),
        }
    });
}

pub(super) fn copy_link(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(url) = edit.bible.providers.borrow().get(i).map(|p| p.signup_url.clone()) else { return };
    copy(op, &url, &strings::link_copied());
}

/// Import a Bible file: pick `.xml .usx .osis .zef` files and import each under the short name typed (or the file's
/// own, `-`), one after another; the first failure stops the rest, as the web's loop throws.
pub(super) fn import(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    files::pick(op, Filter::Bibles, true, move |paths| if let Some(op) = me.upgrade() { import_files(&op, &edit, paths) });
}

pub(crate) fn import_files(op: &Rc<Operator>, edit: &Rc<Edit>, mut paths: Vec<PathBuf>) {
    if paths.is_empty() { return redraw_on_next_read(op, edit) }
    let path = paths.remove(0);
    // The web box has `maxlength: 10`.
    let short: String = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>().get_bible_short().trim().chars().take(10).collect::<String>().to_uppercase();
    let name = if short.is_empty() { "-".to_string() } else { short };
    #[derive(serde::Deserialize)]
    struct Added { abbreviation: String, verses: u64 }
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call_file::<Added>(format!("bible/import/{name}"), path, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(a) => {
                op.toast(strings::bible_added(&a.abbreviation, a.verses), false);
                import_files(&op, &edit, paths);
            }
            Err(e) => {
                redraw_on_next_read(&op, &edit);
                op.toast(if e.is_empty() { strings::import_failed() } else { e }, true);
            }
        }
    });
}

pub(super) fn remove_import(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(t) = edit.bible.sources.borrow().get(i).map(|b| b.translation.clone()) else { return };
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<serde_json::Value>("bible/import-remove", serde_json::json!({ "translation": t }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => {
                redraw_on_next_read(&op, &edit);
                op.toast(strings::bible_removed(&t), false);
            }
            Err(e) => op.toast(e, true),
        }
    });
}

#[cfg(test)]
mod catalog_tests {
    use super::*;

    fn row(id: &str, abbr: &str, title: &str, lang: &str, installed: bool) -> CatRow {
        CatRow { id: id.into(), abbr: abbr.into(), title: title.into(), lang: lang.into(), lang_name_en: lang.into(),
            scope: "Full Bible".into(), installed, ..Default::default() }
    }

    fn job(subject: &str, label: &str, percent: u8) -> JobRow {
        JobRow { id: uuid::Uuid::new_v4(), label: label.into(), percent, error: None, note: None, cancellable: true, done: false,
            subject: Some(subject.into()) }
    }

    #[test]
    fn select_puts_a_heading_where_the_language_changes_and_cuts_long_lists() {
        let rows = vec![row("e:a", "A", "Alpha", "eng", false), row("e:b", "B", "Beta", "eng", true), row("e:c", "C", "Gamma", "spa", false)];
        assert_eq!(select(&rows, "", 1, "").lines, [None, Some(0), Some(1), None, Some(2)]);
        assert_eq!(select(&rows, "", 0, "").lines, [None, Some(1)], "Installed");
        assert_eq!(select(&rows, "", 1, "spa").lines, [None, Some(2)]);
        assert_eq!(select(&rows, "gamma spa", 1, "").lines, [None, Some(2)], "every word, across fields");
        assert_eq!(select(&rows, "gamma eng", 1, "").note, cat::no_match());
        let many: Vec<CatRow> = (0..MAX_SHOWN + 5).map(|i| row(&format!("e:{i}"), "X", "X", "eng", false)).collect();
        let s = select(&many, "", 1, "");
        assert_eq!(s.lines.len(), MAX_SHOWN + 1);
        assert_eq!(s.note, cat::showing(MAX_SHOWN, MAX_SHOWN + 5));
    }

    #[test]
    fn line_has_one_button_for_each_state() {
        let mut r = row("ebible:x", "X", "X Bible", "eng", false);
        let l = line(&r, None, false, false, None, false, false, false);
        assert_eq!((l.button.as_str(), l.primary, l.enabled, l.progress), ("Add", true, true, -1.0));
        assert_eq!(l.tag, "Offline · Full Bible");
        assert!(!line(&r, None, false, false, None, true, false, false).enabled, "Add is off while offline");
        assert_eq!(line(&r, None, false, false, None, false, true, false).abbr, "X · eng", "equal abbreviations told apart");
        let w = line(&r, Some(&job("ebible:x", "Waiting to download X Bible…", 0)), true, false, None, false, false, false);
        assert_eq!((w.button.as_str(), w.status.as_str(), w.progress), ("Cancel", "Waiting", -1.0));
        let d = line(&r, Some(&job("ebible:x", "Downloading X Bible…", 37)), true, false, None, false, false, false);
        assert_eq!((d.status.as_str(), d.progress), ("Downloading… 37%", 37.0));
        let mut failed = job("ebible:x", "Downloading X Bible…", 37);
        failed.error = Some("Couldn't download X Bible.".into());
        assert_eq!(line(&r, Some(&failed), false, false, Some("Couldn't download X Bible."), false, false, false).button, "Add",
            "a failed job gives the button back");
        r.installed = true;
        let i = line(&r, None, false, true, None, false, false, false);
        assert_eq!((i.button.as_str(), i.status.as_str(), i.primary), ("Remove", "Added", false));
        r.update = true;
        assert_eq!(line(&r, None, false, false, None, false, false, false).button, "Update");
    }

    #[test]
    fn a_credit_names_the_bible_once_and_adds_the_licence_once() {
        assert_eq!(cat::credit("RVR", "Copyright © Ejemplo", "CC BY 4.0"), "RVR: Copyright © Ejemplo (CC BY 4.0)");
        assert_eq!(cat::credit("NET", "NET Bible® copyright © Biblical Studies Press", ""), "NET Bible® copyright © Biblical Studies Press");
        assert_eq!(cat::credit("WEB", "", "Public domain"), "WEB (Public domain)");
        assert_eq!(cat::credit("X", "X: CC BY 4.0", "CC BY 4.0"), "X: CC BY 4.0", "not twice");
        let src = |t: &str, status: &str, imported: bool, copyright: Option<&str>, licence: &str| BibleSource {
            translation: t.into(), status: status.into(), imported, copyright: copyright.map(Into::into), licence: licence.into(), ..Default::default()
        };
        let list = [src("KJV", "offline", false, None, ""), src("ESV", "needs_key", false, Some("ESV ©"), ""), src("ESV", "online", false, Some("ESV ©"), ""),
            src("e:x", "offline", true, None, ""), src("e:y", "offline", true, None, "Public domain")];
        assert_eq!(credits(&list).into_iter().map(|c| c.0).collect::<Vec<_>>(), ["ESV ©", "e:y (Public domain)"],
            "in use and with something to say");
    }

    #[test]
    fn a_row_warns_when_no_font_draws_its_title_or_language() {
        let mut f = Fonts::bundled_only();
        let mut r = row("ebible:amh", "AMH", "መጽሐፍ ቅዱስ", "amh", false);
        assert!(needs_font(&mut f, &r), "the title");
        r.title = "Amharic Bible".into();
        assert!(!needs_font(&mut f, &r));
        r.lang_name = "አማርኛ".into();
        assert!(needs_font(&mut f, &r), "the language's own name, shown in its heading");
        let l = line(&r, None, false, false, None, false, false, true);
        assert_eq!((l.font_warning.as_str(), l.button.as_str()), (cat::no_font().as_str(), "Add"), "shown before Add, and Add still works");
        let d = line(&r, Some(&job("ebible:amh", "Downloading Amharic Bible…", 10)), true, false, None, false, false, true);
        assert_eq!(d.font_warning, cat::no_font(), "and while downloading");
        assert!(line(&r, None, false, false, None, false, false, false).font_warning.is_empty());
    }

    #[test]
    fn languages_keep_the_list_order_with_both_names() {
        let mut a = row("e:a", "A", "A", "spa", false);
        a.lang_name_en = "Spanish".into();
        a.lang_name = "Español".into();
        let rows = vec![row("e:b", "B", "B", "eng", false), a.clone(), a];
        assert_eq!(languages(&rows), (vec!["eng".to_string(), "spa".into()], vec!["eng".to_string(), "Spanish · Español".into()]));
    }
}
