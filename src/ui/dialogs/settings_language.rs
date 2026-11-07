//! Settings → Language (I18N spec D7): "Use the computer's language", then every built-in language by its own name,
//! how much is translated, and a warning when no font here can draw its name. Picking one applies at once
//! (`settings/language`), like the default Bible; Cancel does not undo it.
use super::settings::{Edit, copy, redraw_on_next_read, strings};
use crate::{
    i18n::{self, LanguageInfo},
    ui::{SettingsDlg, SettingsLanguageRow, operator::window::Operator},
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};

/// The codes of the rows on show, by row (`""` = follow the computer).
#[derive(Default)]
pub struct LanguageState { codes: RefCell<Vec<String>> }

/// The rows for the `Settings.ui_language` value `setting`. `system` is the computer's language if the app has
/// it; `lacks_font` says whether no font here can draw a text.
pub fn rows(setting: &str, system: Option<&str>, langs: &[LanguageInfo], lacks_font: &mut dyn FnMut(&str) -> bool) -> Vec<(String, SettingsLanguageRow)> {
    let name_of = |code: &str| langs.iter().find(|l| l.code == code).map(|l| l.native_name.clone());
    let follow = name_of(system.unwrap_or(i18n::ENGLISH)).unwrap_or_else(|| "English".into());
    let mut out = vec![(String::new(), SettingsLanguageRow {
        title: strings::use_computer_language(&follow).into(), sub: "".into(), on: setting.is_empty(), font_warning: "".into(),
    })];
    out.extend(langs.iter().map(|l| {
        let title = if l.reviewed { l.native_name.clone() } else { strings::needs_review(&l.native_name) };
        let sub = if l.code == i18n::ENGLISH { String::new() } else { strings::language_sub(&l.english_name, l.percent) };
        let font_warning = if lacks_font(&l.native_name) { strings::no_font_for_language() } else { String::new() };
        (l.code.clone(), SettingsLanguageRow { title: title.into(), sub: sub.into(), on: setting == l.code, font_warning: font_warning.into() })
    }));
    out
}

/// Draws the list for the edited settings.
pub(super) fn fill(op: &Operator, edit: &Edit) {
    let setting = edit.settings.borrow().ui_language.clone();
    let rows = rows(&setting, i18n::system_language().as_deref(), &i18n::languages(), &mut |t| super::settings_bible::lacks_font(t));
    let (codes, rows): (Vec<String>, Vec<SettingsLanguageRow>) = rows.into_iter().unzip();
    *edit.language.codes.borrow_mut() = codes;
    op.ui.global::<SettingsDlg>().set_languages(ModelRc::new(VecModel::from(rows)));
}

/// A row's Use button: the whole app changes language now, and the Rust-made text is drawn again.
pub(super) fn choose(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(code) = edit.language.codes.borrow().get(i).cloned() else { return };
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<serde_json::Value>("settings/language", serde_json::json!({ "code": code }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(v) => {
                edit.settings.borrow_mut().ui_language = code;
                // The action may have run on another thread: this one's windows change here.
                i18n::sync_slint();
                fill(&op, &edit);
                redraw_on_next_read(&op, &edit);
                op.request_refresh();
                if let Some(t) = v["toast"].as_str() { op.toast(t, false) }
            }
            Err(e) => op.toast(e, true),
        }
    });
}

/// "Help translate RustyPresenter": the guide's address goes to the clipboard.
pub(super) fn help_translate(op: &Rc<Operator>, _edit: &Rc<Edit>) { copy(op, strings::TRANSLATING_URL, &strings::link_copied()) }

#[cfg(test)]
mod tests {
    use super::*;

    fn info(code: &str, native: &str, english: &str, percent: u8, reviewed: bool) -> LanguageInfo {
        LanguageInfo { code: code.into(), native_name: native.into(), english_name: english.into(), percent, reviewed }
    }

    #[test]
    fn rows_name_each_language_its_own_way() {
        let langs = [info("en", "English", "English", 100, true), info("es", "Español", "Spanish", 98, false), info("ko", "한국어", "Korean", 40, true)];
        let rows = rows("es", Some("es"), &langs, &mut |t| t == "한국어");
        let lines: Vec<(String, String, String, bool, String)> = rows.iter()
            .map(|(c, r)| (c.clone(), r.title.to_string(), r.sub.to_string(), r.on, r.font_warning.to_string())).collect();
        let warn = "Your computer may not have a font for this language.".to_string();
        assert_eq!(lines, [
            ("".into(), "Use the computer's language (Español)".into(), "".into(), false, "".into()),
            ("en".into(), "English".into(), "".into(), false, "".into()),
            ("es".into(), "Español (needs review)".into(), "Spanish · 98% translated".into(), true, "".into()),
            ("ko".into(), "한국어".into(), "Korean · 40% translated".into(), false, warn),
        ]);
        let rows = rows_for_follow(&langs);
        assert!(rows[0].1.on);
        assert_eq!(rows[0].1.title, "Use the computer's language (English)", "a computer language the app lacks shows English");
    }

    fn rows_for_follow(langs: &[LanguageInfo]) -> Vec<(String, SettingsLanguageRow)> { rows("", None, langs, &mut |_| false) }
}
