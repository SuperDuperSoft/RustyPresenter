//! Settings → Bible (`operator.js:922-973`, after the `bible-esv` merge): every translation's status, the online
//! services a church connects with a key (`bible/key`), Bible file import (`bible/import/{ABBR}`, the raw file) and
//! removal (`bible/import-remove`), and the copyright lines of the sources in use.
use super::{confirm, settings::{Edit, copy, redraw_on_next_read, strings}, strings as dlg};
use crate::ui::{
    AlsoChip, SettingsBibleRow, SettingsCatalogRow, SettingsDlg, SettingsProviderRow,
    operator::{file_input::Filter, files, view::{BibleProvider, BibleSource, OperatorSnapshot}, window::Operator},
};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::{cell::{Cell, RefCell}, path::PathBuf, rc::Rc, time::Duration};

/// The rows the tab shows, so a button's index finds its translation or service.
#[derive(Default)]
pub struct BibleState {
    sources: RefCell<Vec<BibleSource>>,
    providers: RefCell<Vec<BibleProvider>>,
    /// The search rows on screen (`bible/catalog`), so Add/Remove find their Bible.
    catalog: RefCell<Vec<CatalogRow>>,
    /// Numbers each search; an answer to an older one is dropped (typing outruns the network).
    seq: Cell<u64>,
    /// Waits 150 ms after the last key before searching.
    typing: slint::Timer,
}

/// One row of `bible/catalog`'s answer: a `header`, or a Bible.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct CatalogRow { pub header: String, pub id: String, pub abbr: String, pub name: String, pub tag: String, pub state: String }

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct Catalog { rows: Vec<CatalogRow>, shown: usize, total: usize, keys: bool, notes: Vec<String>, listed: usize }

/// The line under the search box: what to type, how many matched, and any trouble reaching a service.
fn status(query: &str, c: &Catalog) -> String {
    let mut lines = Vec::new();
    if !c.keys { lines.push(strings::SEARCH_NEEDS_KEY.to_string()); }
    else if query.trim().is_empty() { lines.push(strings::search_hint(c.listed)); }
    else if c.total == 0 { lines.push(strings::no_match(query)); }
    else if c.total > c.shown { lines.push(strings::showing(c.shown, c.total)); }
    lines.extend(c.notes.iter().cloned());
    if c.keys && !query.trim().is_empty() { lines.push(strings::NOT_LISTED.into()); }
    lines.join(" ")
}

/// Searches now with what is in the box (`refresh`: fetch the lists again first). Called when the tab is drawn,
/// after Add/Remove, and 150 ms after typing stops.
pub(super) fn search(op: &Rc<Operator>, edit: &Rc<Edit>, refresh: bool) {
    let g = op.ui.global::<SettingsDlg>();
    let query = g.get_bible_query().to_string();
    let seq = edit.bible.seq.get() + 1;
    edit.bible.seq.set(seq);
    g.set_bible_searching(true);
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<Catalog>("bible/catalog", serde_json::json!({ "query": query, "refresh": refresh }), move |r| {
        let Some(op) = me.upgrade() else { return };
        if edit.bible.seq.get() != seq { return }
        let g = op.ui.global::<SettingsDlg>();
        g.set_bible_searching(false);
        match r {
            Ok(c) => show_catalog(&g, &edit, &query, c),
            Err(e) => op.toast(e, true),
        }
    });
}

fn show_catalog(g: &SettingsDlg, edit: &Edit, query: &str, c: Catalog) {
    g.set_bible_keys(c.keys);
    g.set_bible_catalog_status(status(query, &c).into());
    let rows: Vec<SettingsCatalogRow> = c.rows.iter().map(|r| SettingsCatalogRow {
        header: r.header.as_str().into(), abbr: r.abbr.as_str().into(), name: r.name.as_str().into(),
        tag: r.tag.as_str().into(), state: r.state.as_str().into(),
    }).collect();
    g.set_bible_catalog(ModelRc::new(VecModel::from(rows)));
    *edit.bible.catalog.borrow_mut() = c.rows;
}

/// A key in the search box: search once typing pauses.
pub(super) fn typed(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let (me, e) = (Rc::downgrade(op), edit.clone());
    edit.bible.typing.start(slint::TimerMode::SingleShot, Duration::from_millis(150), move || {
        if let Some(op) = me.upgrade() { search(&op, &e, false) }
    });
}

pub(super) fn refresh(op: &Rc<Operator>, edit: &Rc<Edit>) { search(op, edit, true) }

/// Add on a search row: one click, no questions (design §7.1).
pub(super) fn add_online(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(id) = edit.bible.catalog.borrow().get(i).map(|r| r.id.clone()).filter(|id| !id.is_empty()) else { return };
    #[derive(serde::Deserialize)]
    struct Added { abbr: String }
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<Added>("bible/online-add", serde_json::json!({ "id": id }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(a) => {
                redraw_on_next_read(&op, &edit);
                op.toast(strings::online_added(&a.abbr), false);
            }
            Err(e) => op.toast(e, true),
        }
    });
}

/// Remove on a search row.
pub(super) fn remove_from_search(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(r) = edit.bible.catalog.borrow().get(i).cloned().filter(|r| !r.id.is_empty()) else { return };
    remove_online(op, edit, r.id, r.abbr, false);
}

/// `bible/online-remove`: asks first when saved services use the Bible (their passages keep their verses).
fn remove_online(op: &Rc<Operator>, edit: &Rc<Edit>, id: String, abbr: String, confirmed: bool) {
    #[derive(serde::Deserialize)]
    struct Removed { used: usize, removed: bool }
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<Removed>("bible/online-remove", serde_json::json!({ "id": id, "confirm": confirmed }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(Removed { removed: true, .. }) => {
                redraw_on_next_read(&op, &edit);
                op.toast(strings::bible_removed(&abbr), false);
            }
            Ok(Removed { used, .. }) => {
                let me = Rc::downgrade(&op);
                confirm(&op, &strings::used_in_services(&abbr, used), strings::REMOVE, dlg::CANCEL, true, move |ok| {
                    if let Some(op) = me.upgrade().filter(|_| ok) { remove_online(&op, &edit, id, abbr, true) }
                });
            }
            Err(e) => op.toast(e, true),
        }
    });
}

/// The line under a translation's name (`hint`, `operator.js:925-926`).
pub fn hint(b: &BibleSource, providers: &[BibleProvider]) -> String {
    match b.status.as_str() {
        "needs_key" => strings::connect_below(&b.providers.iter()
            .filter_map(|id| providers.iter().find(|p| p.id == *id).map(|p| p.label.clone())).collect::<Vec<_>>()),
        "unavailable" => strings::NO_ONLINE_SOURCE.into(),
        _ => b.provider.clone().unwrap_or_default(),
    }
}

/// The "Also show" chips: every Bible the church can use except the chosen one, picked ones on.
pub(super) fn fill_also(g: &SettingsDlg, edit: &Edit, snap: &OperatorSnapshot) {
    let picked = edit.settings.borrow().also_show.clone();
    let chips: Vec<AlsoChip> = snap.available.iter().filter(|t| **t != snap.translation).map(|t| AlsoChip {
        name: t.as_str().into(), label: snap.translation_label(t).into(), on: picked.contains(t),
    }).collect();
    g.set_also_chips(ModelRc::new(VecModel::from(chips)));
}

/// A chip clicked: on or off. A third pick replaces the oldest, so it never holds more than two.
pub(super) fn toggle_also(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let g = op.ui.global::<SettingsDlg>();
    let Some(chip) = g.get_also_chips().row_data(i) else { return };
    let name = chip.name.to_string();
    {
        let mut s = edit.settings.borrow_mut();
        if let Some(at) = s.also_show.iter().position(|t| *t == name) { s.also_show.remove(at); }
        else {
            s.also_show.push(name);
            while s.also_show.len() > crate::model::MAX_BIBLES - 1 { s.also_show.remove(0); }
        }
    }
    if let Some(snap) = op.state.borrow().snap.clone() { fill_also(&g, edit, &snap) }
}

pub(super) fn fill(g: &SettingsDlg, edit: &Edit, snap: &OperatorSnapshot) {
    fill_also(g, edit, snap);
    let rows: Vec<SettingsBibleRow> = snap.bible_sources.iter().map(|b| {
        let (state, chip) = strings::chip(&b.status);
        SettingsBibleRow {
            translation: b.shown().into(), hint: hint(b, &snap.bible_providers).into(),
            // The Remove button: an imported file, or a Bible added from the search.
            chip: chip.into(), chip_state: state.into(), imported: b.imported || b.added,
        }
    }).collect();
    g.set_bible_sources(ModelRc::new(VecModel::from(rows)));
    let providers: Vec<SettingsProviderRow> = snap.bible_providers.iter().map(|p| SettingsProviderRow {
        label: p.label.as_str().into(), for_text: strings::provider_for(&p.translations).into(), key_set: p.key_set,
        signup_url: p.signup_url.as_str().into(), note: p.note.as_str().into(),
    }).collect();
    g.set_bible_providers(ModelRc::new(VecModel::from(providers)));
    let mut credits: Vec<SharedString> = snap.bible_sources.iter()
        .filter(|b| b.status == "online" || b.imported || b.added)
        .filter_map(|b| b.copyright.as_deref().filter(|c| !c.is_empty()).map(|c| if b.added { format!("{}: {c}", b.shown()) } else { c.to_string() }.into()))
        .collect();
    // The providers' own lines, for the Bibles added from their lists.
    let from = |p: &str| snap.bible_sources.iter().any(|b| b.added && b.providers.iter().any(|x| x == p));
    if from("youversion") { credits.push(crate::bible::strings::CREDIT_YOUVERSION.into()); }
    if from("api_bible") { credits.push(crate::bible::strings::CREDIT_API_BIBLE.into()); }
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
    copy(op, &url, strings::LINK_COPIED);
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
    let short: String = op.ui.global::<SettingsDlg>().get_bible_short().trim().chars().take(10).collect::<String>().to_uppercase();
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
                op.toast(if e.is_empty() { strings::IMPORT_FAILED.to_string() } else { e }, true);
            }
        }
    });
}

pub(super) fn remove_import(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(b) = edit.bible.sources.borrow().get(i).cloned() else { return };
    if b.added { return remove_online(op, edit, b.translation.clone(), b.shown().to_string(), false) }
    let t = b.translation;
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
