//! Settings → Help: the keyboard table, where any shortcut can be changed (N28), and the clear groups (N18). Both
//! edit `Edit.settings.keys` and are saved with the rest of the settings (Save settings).
use super::{confirm, settings::{Edit, strings as st}, strings as dlg};
use crate::{
    live::ClearLayer,
    sections::{SectionKind, SectionLook},
    shortcuts::{Action, Chord, Clash, ClearGroup, EditKey, KeySettings, Keymap, clashes, labels, reserved},
    ui::{ClearGroupRow, KeysDlg, SettingsDlg, SettingsHelpRow, operator::window::Operator},
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};

/// The layers a clear group can take down, in the editor's order.
pub const LAYERS: [ClearLayer; 8] = [ClearLayer::Slide, ClearLayer::Media, ClearLayer::Audio, ClearLayer::Logo, ClearLayer::Overlays,
    ClearLayer::Message, ClearLayer::Timers, ClearLayer::Drawing];

thread_local! {
    /// The open Settings and the action waiting for its new key.
    static CAPTURE: RefCell<Option<(Rc<Edit>, Action)>> = const { RefCell::new(None) };
}

/// Settings → Help is waiting for a new key: the window hands it every key (`keys::Operator::key`).
pub fn capturing() -> bool { CAPTURE.with(|c| c.borrow().is_some()) }

/// One row of the Help table. `action`: the shortcut it shows; `None` for a fixed key (Alt + 1–5, the section
/// keys, Esc).
#[derive(Clone, Debug, PartialEq)]
pub struct HelpRow { pub keys: String, pub what: String, pub action: Option<Action>, pub changed: bool }

/// What an action does, as the Help table says it.
pub fn what(action: Action, groups: &[ClearGroup]) -> String {
    use crate::shortcuts::strings as sh;
    match action {
        Action::Next => st::do_next(),
        Action::Back => st::do_back(),
        Action::Take => st::do_enter(),
        Action::FocusBible => st::do_bible(),
        Action::ClearAll => st::do_clear_all(),
        Action::ClearSlide => st::do_clear_text(),
        Action::ClearMedia => st::do_clear_media(),
        Action::ClearAudio => st::do_stop_audio(),
        Action::Logo => st::do_logo(),
        Action::ClearOverlays => crate::theme::strings::clear_overlays_help(),
        Action::ClearMessage => sh::clear_message(),
        Action::ClearTimers => sh::clear_timers(),
        Action::Group(id) => sh::clear_group(&groups.iter().find(|g| g.id == id).map(|g| g.name.clone()).unwrap_or_default()),
        Action::PasteLink => st::do_paste_link(),
        Action::Undo => st::do_undo(),
        Action::Redo => st::do_redo(),
        Action::Find => st::do_find(),
        Action::ShowMessage => crate::tr!("Show a message on the screen"),
        Action::FindMouse => sh::find_mouse(),
        Action::FollowAuto => sh::follow_auto(),
        Action::FollowHold => sh::follow_hold(),
    }
}

/// The section keys as the Help table shows them: "1–9  C  P  B  T  I  E  L  V" with the church's own letters.
pub fn section_keys_label(look: &SectionLook) -> String {
    use SectionKind::*;
    let letters = [Chorus, PreChorus, Bridge, Tag, Intro, Ending, Interlude, Verse].into_iter()
        .filter_map(|k| k.key_in(look)).map(|c| c.to_uppercase().to_string());
    std::iter::once("1–9".to_string()).chain(letters).collect::<Vec<_>>().join("  ")
}

/// The Help table (`operator.js:993`): every shortcut with its keys as the church set them, and the fixed keys.
pub fn help_rows(keys: &KeySettings, sections: &SectionLook) -> Vec<HelpRow> {
    let map = Keymap::new(keys);
    let fixed = |k: String, w: String| HelpRow { keys: k, what: w, action: None, changed: false };
    let mut rows = vec![];
    for (action, chords) in map.entries() {
        // The Bible chips and the section keys sit after the clear keys and groups, as before.
        if *action == Action::PasteLink {
            rows.push(fixed(st::key_alt_digits(), st::do_translation()));
            rows.push(fixed(section_keys_label(sections), st::do_sections()));
        }
        let changed = keys.changed.iter().any(|b| b.action == action.id());
        rows.push(HelpRow { keys: labels(chords), what: what(*action, &keys.clear_groups), action: Some(*action), changed });
    }
    rows.push(fixed(st::key_esc(), st::do_esc()));
    // The fixed Windows keys (`EditKey`), and ↑ ↓ in the service list.
    rows.push(fixed(labels(&[Chord::new("ArrowUp", false, false, false), Chord::new("ArrowDown", false, false, false)]), st::do_list_step()));
    for e in EditKey::ALL { rows.push(fixed(labels(&e.chords()), edit_what(e))) }
    rows
}

/// What a fixed Windows key does, as the Help table says it.
pub fn edit_what(e: EditKey) -> String {
    match e {
        EditKey::Delete => st::do_delete(),
        EditKey::Copy => st::do_copy(),
        EditKey::Cut => st::do_cut(),
        EditKey::Duplicate => st::do_duplicate(),
        EditKey::MoveUp => st::do_move_up(),
        EditKey::MoveDown => st::do_move_down(),
        EditKey::NewService => st::do_new_service(),
        EditKey::OpenService => st::do_open_service(),
        EditKey::Print => st::do_print(),
        EditKey::Settings => st::do_settings(),
    }
}

/// Draws the table and the groups from the edited settings.
pub(crate) fn fill(op: &Operator, edit: &Edit) {
    let (keys, sections) = { let s = edit.settings.borrow(); (s.keys.clone(), s.sections.clone()) };
    let rows: Vec<SettingsHelpRow> = help_rows(&keys, &sections).into_iter().map(|r| SettingsHelpRow {
        keys: r.keys.into(), what: r.what.into(), id: r.action.map(|a| a.id()).unwrap_or_default().into(), changed: r.changed,
    }).collect();
    op.ui.global::<SettingsDlg>().set_help(ModelRc::new(VecModel::from(rows)));
    let k = op.ui.global::<KeysDlg>();
    k.set_any_changed(!keys.changed.is_empty());
    k.set_layer_names(ModelRc::new(VecModel::from(LAYERS.iter().map(|l| layer_name(*l).into()).collect::<Vec<slint::SharedString>>())));
    let groups: Vec<ClearGroupRow> = keys.clear_groups.iter().map(|g| ClearGroupRow {
        name: g.name.as_str().into(),
        layers: ModelRc::new(VecModel::from(LAYERS.iter().map(|l| g.layers.contains(l)).collect::<Vec<bool>>())),
    }).collect();
    k.set_groups(ModelRc::new(VecModel::from(groups)));
}

fn layer_name(l: ClearLayer) -> String {
    match l {
        ClearLayer::Slide => tr::text(),
        ClearLayer::Media => tr::media(),
        ClearLayer::Audio => tr::audio(),
        ClearLayer::Logo => st::do_logo(),
        ClearLayer::Overlays => tr::overlays(),
        ClearLayer::Message => tr::message(),
        ClearLayer::Timers => tr::timers(),
        ClearLayer::Drawing => tr::drawing(),
    }
}

/// The action on Help row `i` of the edited settings.
fn row_action(edit: &Edit, i: i32) -> Option<Action> {
    let s = edit.settings.borrow();
    help_rows(&s.keys, &s.sections).get(usize::try_from(i).ok()?)?.action
}

/// Waits for the next key for row `i`'s action. The menu bar's shortcuts are off meanwhile.
fn start(op: &Rc<Operator>, edit: &Rc<Edit>, i: i32) {
    let Some(action) = row_action(edit, i) else { return };
    CAPTURE.with(|c| *c.borrow_mut() = Some((edit.clone(), action)));
    let k = op.ui.global::<KeysDlg>();
    k.set_capturing(i);
    k.set_capture_text(tr::press_for(&what(action, &edit.settings.borrow().keys.clear_groups)).into());
    crate::ui::operator::help::rerender_keys(op);
}

/// Stops waiting for a key (Esc, Cancel, a key taken, or the dialog closing).
pub(crate) fn stop(op: &Operator) {
    CAPTURE.with(|c| *c.borrow_mut() = None);
    op.ui.global::<KeysDlg>().set_capturing(-1);
    crate::ui::operator::help::rerender_keys(op);
}

/// The key pressed while waiting. Esc cancels; Tab is refused; a key that clashes asks first.
pub fn captured(op: &Rc<Operator>, chord: Chord) {
    let Some((edit, action)) = CAPTURE.with(|c| c.borrow().clone()) else { return };
    if chord.key == "Escape" { return stop(op) }
    if reserved(&chord) { return op.toast(tr::not_tab(), false) }
    stop(op);
    let section_keys = op.ui.global::<SettingsDlg>().get_section_keys();
    let (found, groups) = {
        let s = edit.settings.borrow();
        (clashes(&Keymap::new(&s.keys), action, &chord, section_keys.then_some(&s.sections)), s.keys.clear_groups.clone())
    };
    let text = clash_text(&chord, action, &found, &groups);
    let me = Rc::downgrade(op);
    let apply = move || {
        let Some(op) = me.upgrade() else { return };
        edit.settings.borrow_mut().keys.assign(action, chord);
        fill(&op, &edit);
    };
    if found.is_empty() { return apply() }
    confirm(op, &text, &tr::use_it(), &dlg::cancel(), false, move |ok| if ok { apply() });
}

/// The question a clash asks: what the key does now, then whether to use it anyway.
pub fn clash_text(chord: &Chord, action: Action, found: &[Clash], groups: &[ClearGroup]) -> String {
    let key = chord.label();
    let mut lines: Vec<String> = found.iter().map(|c| match c {
        Clash::Action(other) => tr::taken(&key, &what(*other, groups)),
        Clash::SectionKey => tr::section_key(&key),
        Clash::BibleChip => tr::bible_chip(&key),
        Clash::TextEditing => tr::text_editing(&key),
    }).collect();
    lines.push(tr::use_for(&key, &what(action, groups)));
    lines.join("\n\n")
}

/// Connects the table's buttons and the group editor.
pub(crate) fn connect(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let k = op.ui.global::<KeysDlg>();
    let with = |f: fn(&Rc<Operator>, &Rc<Edit>, i32)| {
        let (me, e) = (Rc::downgrade(op), edit.clone());
        move |i: i32| if let Some(op) = me.upgrade() { f(&op, &e, i) }
    };
    k.on_change(with(start));
    k.on_reset(with(|op, edit, i| {
        if let Some(a) = row_action(edit, i) { edit.settings.borrow_mut().keys.reset(a); fill(op, edit) }
    }));
    let unit = |f: fn(&Rc<Operator>, &Rc<Edit>)| {
        let (me, e) = (Rc::downgrade(op), edit.clone());
        move || if let Some(op) = me.upgrade() { f(&op, &e) }
    };
    k.on_reset_all(unit(|op, edit| { edit.settings.borrow_mut().keys.changed.clear(); fill(op, edit) }));
    let me = Rc::downgrade(op);
    k.on_cancel_capture(move || if let Some(op) = me.upgrade() { stop(&op) });
    let (me, e) = (Rc::downgrade(op), edit.clone());
    k.on_group_renamed(move |i, name| {
        if me.upgrade().is_none() { return }
        if let Some(g) = e.settings.borrow_mut().keys.clear_groups.get_mut(i as usize) { g.name = name.trim().to_string() }
    });
    let (me, e) = (Rc::downgrade(op), edit.clone());
    k.on_group_layer(move |gi, li, on| {
        let Some(op) = me.upgrade() else { return };
        let Some(layer) = LAYERS.get(li as usize).copied() else { return };
        if let Some(g) = e.settings.borrow_mut().keys.clear_groups.get_mut(gi as usize) {
            g.layers.retain(|l| *l != layer);
            if on { g.layers.push(layer); g.layers.sort() }
        }
        fill(&op, &e);
    });
    k.on_group_add(unit(|op, edit| { edit.settings.borrow_mut().keys.clear_groups.push(new_group()); fill(op, edit) }));
    k.on_group_delete(with(|op, edit, i| {
        let mut s = edit.settings.borrow_mut();
        if let Ok(i) = usize::try_from(i) && i < s.keys.clear_groups.len() {
            let gone = s.keys.clear_groups.remove(i);
            s.keys.reset(Action::Group(gone.id));
        }
        drop(s);
        fill(op, edit);
    }));
}

/// The Change button of Help row `row` (for the screenshot example).
pub fn press_change(op: &Operator, row: usize) { op.ui.global::<KeysDlg>().invoke_change(row as i32) }

/// Add a clear group (for the screenshot example).
pub fn press_add_group(op: &Operator) { op.ui.global::<KeysDlg>().invoke_group_add() }

/// A new group: just the text, to change from there.
fn new_group() -> ClearGroup { ClearGroup { id: uuid::Uuid::new_v4(), name: tr::new_group(), layers: vec![ClearLayer::Slide] } }

mod tr {
    use crate::tr;
    pub fn text() -> String { tr!("Text") }
    pub fn media() -> String { tr!("Media & background") }
    pub fn audio() -> String { tr!("Audio") }
    pub fn overlays() -> String { tr!("Overlays (pictures, clocks, signs)") }
    pub fn message() -> String { tr!("Message") }
    pub fn timers() -> String { tr!("Timers") }
    pub fn drawing() -> String { tr!("Phone drawing") }
    pub fn new_group() -> String { tr!("New clear group") }
    pub fn press_for(what: &str) -> String { tr!("Press the new key for “{what}”", what = what) }
    pub fn not_tab() -> String { tr!("Tab moves between buttons, so it cannot be a shortcut. Press another key.") }
    pub fn use_it() -> String { tr!("Use it") }
    pub fn taken(key: &str, what: &str) -> String { tr!("{key} is already the key for “{what}”. It will move here.", key = key, what = what) }
    pub fn section_key(key: &str) -> String { tr!("{key} also jumps the live song to a section (Settings → Songs → Section keys).", key = key) }
    pub fn bible_chip(key: &str) -> String { tr!("{key} also picks a Bible chip while you type in the Bible box.", key = key) }
    pub fn text_editing(key: &str) -> String { tr!("Text boxes use {key} for editing. It will do this instead, even while you type.", key = key) }
    pub fn use_for(key: &str, what: &str) -> String { tr!("Use {key} for “{what}”?", key = key, what = what) }
}

#[cfg(test)]
#[path = "settings_keys_tests.rs"]
mod tests;
