//! Settings → Bible (`operator.js:922-973`, after the `bible-esv` merge): every translation's status, the online
//! services a church connects with a key (`bible/key`), Bible file import (`bible/import/{ABBR}`, the raw file) and
//! removal (`bible/import-remove`), and the copyright lines of the sources in use.
use super::settings::{Edit, copy, redraw_on_next_read, strings};
use crate::ui::{
    SettingsBibleRow, SettingsDlg, SettingsProviderRow,
    operator::{file_input::Filter, files, view::{BibleProvider, BibleSource, OperatorSnapshot}, window::Operator},
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

/// The rows the tab shows, so a button's index finds its translation or service.
#[derive(Default)]
pub struct BibleState {
    sources: RefCell<Vec<BibleSource>>,
    providers: RefCell<Vec<BibleProvider>>,
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

pub(super) fn fill(g: &SettingsDlg, edit: &Edit, snap: &OperatorSnapshot) {
    let rows: Vec<SettingsBibleRow> = snap.bible_sources.iter().map(|b| {
        let (state, chip) = strings::chip(&b.status);
        SettingsBibleRow {
            translation: b.translation.as_str().into(), hint: hint(b, &snap.bible_providers).into(),
            chip: chip.into(), chip_state: state.into(), imported: b.imported,
        }
    }).collect();
    g.set_bible_sources(ModelRc::new(VecModel::from(rows)));
    let providers: Vec<SettingsProviderRow> = snap.bible_providers.iter().map(|p| SettingsProviderRow {
        label: p.label.as_str().into(), for_text: strings::provider_for(&p.translations).into(), key_set: p.key_set,
        signup_url: p.signup_url.as_str().into(), note: p.note.as_str().into(),
    }).collect();
    g.set_bible_providers(ModelRc::new(VecModel::from(providers)));
    let credits: Vec<SharedString> = snap.bible_sources.iter()
        .filter(|b| b.status == "online" || b.imported)
        .filter_map(|b| b.copyright.as_deref().filter(|c| !c.is_empty()).map(Into::into))
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
