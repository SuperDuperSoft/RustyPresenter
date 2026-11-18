//! `confirm()` and `prompt()` for the native operator (`ui/dialogs/ask.slint`): small overlays above the open
//! dialog, or above the bare window. While one is up, `OpState.asks > 0`, so the operator keys are off except
//! F1–F5 (`keys::route`), as the browser's own questions block every key. A question asked while one is up (from
//! inside its `done`, say) goes on top; the one under it comes back once it is answered.
use crate::ui::{AskRow, operator::window::Operator};
use slint::{ModelRc, VecModel};
use std::rc::Rc;

/// A question waiting to be answered, over the operator window or (`in_settings`) over the Settings window.
pub struct Ask {
    row: AskRow,
    done: Done,
    in_settings: bool,
}

thread_local! {
    /// Questions asked now go over the Settings window (`in_settings`).
    static IN_SETTINGS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `ask` (a `confirm`, `choose`, `prompt`…) so its question shows over the Settings window, where the person
/// is looking, instead of over the operator window behind it.
pub fn in_settings<R>(_op: &Operator, ask: impl FnOnce() -> R) -> R {
    let before = IN_SETTINGS.with(|f| f.replace(true));
    let r = ask();
    IN_SETTINGS.with(|f| f.set(before));
    r
}

enum Done {
    Confirm(Box<dyn FnOnce(bool)>),
    Prompt(Box<dyn FnOnce(Option<String>)>),
    Choose(Box<dyn FnOnce(Option<bool>)>),
    Choose3(Box<dyn FnOnce(Option<usize>)>),
}

/// Asks a yes/no question. `done(true)` on OK or Enter, `done(false)` on Cancel or Esc. `danger` draws OK in the
/// danger colour (a delete).
pub fn confirm(op: &Rc<Operator>, text: &str, ok_label: &str, cancel_label: &str, danger: bool, done: impl FnOnce(bool) + 'static) {
    let row = AskRow { prompt: false, text: text.into(), value: "".into(), ok_label: ok_label.into(), cancel_label: cancel_label.into(), other_label: "".into(), danger, ..AskRow::default() };
    push(op, Ask { row, done: Done::Confirm(Box::new(done)), in_settings: false });
}

/// Shows a report with one button; nothing depends on the answer.
pub fn notice(op: &Rc<Operator>, text: &str) {
    confirm(op, text, &super::strings::ok(), "", false, |_| {});
}

/// Asks which of two things to do, with Cancel as the way out of both. `done(Some(true))` on `first` or Enter,
/// `done(Some(false))` on `second`, `done(None)` on Cancel or Esc.
pub fn choose(op: &Rc<Operator>, text: &str, first: &str, second: &str, done: impl FnOnce(Option<bool>) + 'static) {
    let row = AskRow {
        prompt: false, text: text.into(), value: "".into(), ok_label: first.into(), other_label: second.into(),
        cancel_label: super::strings::cancel().into(), danger: false, ..AskRow::default()
    };
    push(op, Ask { row, done: Done::Choose(Box::new(done)), in_settings: false });
}

/// Asks which of three things to do, with Cancel as the way out (1.2). `done(Some(0))` on the first (or Enter),
/// `Some(1)` and `Some(2)` on the others, `None` on Cancel or Esc. With `third_why` the third is greyed, and the
/// reason shows under the question.
pub fn choose3(op: &Rc<Operator>, text: &str, [first, second, third]: [&str; 3], third_why: Option<&str>, done: impl FnOnce(Option<usize>) + 'static) {
    let row = AskRow {
        prompt: false, text: text.into(), value: "".into(), ok_label: first.into(), other_label: second.into(),
        cancel_label: super::strings::cancel().into(), danger: false, third_label: third.into(), third_enabled: third_why.is_none(),
        third_note: third_why.unwrap_or_default().into(),
    };
    push(op, Ask { row, done: Done::Choose3(Box::new(done)), in_settings: false });
}

/// Asks for a line of text, starting from `initial` (selected, so typing replaces it). `done(Some(text))` on OK
/// or Enter, even when empty (the browser's `prompt()` returns `""` then); `done(None)` on Cancel or Esc.
pub fn prompt(op: &Rc<Operator>, title: &str, initial: &str, done: impl FnOnce(Option<String>) + 'static) {
    let row = AskRow {
        prompt: true, text: title.into(), value: initial.into(),
        ok_label: super::strings::ok().into(), cancel_label: super::strings::cancel().into(), other_label: "".into(), danger: false,
        ..AskRow::default()
    };
    push(op, Ask { row, done: Done::Prompt(Box::new(done)), in_settings: false });
}

fn push(op: &Operator, mut ask: Ask) {
    // Over Settings when Settings asks, or when its window has the keyboard (the person is looking at it).
    ask.in_settings = super::settings::is_open(op) && (IN_SETTINGS.with(std::cell::Cell::get) || super::settings_window::has_focus(op));
    op.state.borrow_mut().dialogs.asks.push(ask);
    count(op);
    show_top(op);
}

/// `OpState.asks`: the questions over the operator window (one over Settings leaves the operator's keys on).
fn count(op: &Operator) {
    let mut st = op.state.borrow_mut();
    st.asks = st.dialogs.asks.iter().filter(|a| !a.in_settings).count();
}

/// The answer to the operator window's top question (from `AskOverlay`). The question is gone and the keyboard
/// handed back before `done` runs, so `done` may ask again, open a dialog or close one.
pub(crate) fn answer(op: &Operator, ok: bool, text: String) { answer_in(op, false, ok, text) }

/// The answer to the Settings window's top question.
pub(crate) fn answer_settings(op: &Operator, ok: bool, text: String) { answer_in(op, true, ok, text) }

fn answer_in(op: &Operator, settings: bool, ok: bool, text: String) {
    let Some(ask) = take(op, settings) else { return };
    match ask.done {
        Done::Confirm(done) => done(ok),
        Done::Prompt(done) => done(ok.then_some(text)),
        Done::Choose(done) => done(ok.then_some(true)),
        Done::Choose3(done) => done(ok.then_some(0)),
    }
}

/// The second answer to a `choose`, from its third button.
pub(crate) fn answer_other(op: &Operator) { other_in(op, false) }
pub(crate) fn answer_other_settings(op: &Operator) { other_in(op, true) }

fn other_in(op: &Operator, settings: bool) {
    match take(op, settings) {
        Some(Ask { done: Done::Choose(done), .. }) => done(Some(false)),
        Some(Ask { done: Done::Choose3(done), .. }) => done(Some(1)),
        _ => {}
    }
}

/// The third answer to a `choose3`. A greyed button never sends it.
pub(crate) fn answer_third(op: &Operator) { third_in(op, false) }
pub(crate) fn answer_third_settings(op: &Operator) { third_in(op, true) }

fn third_in(op: &Operator, settings: bool) {
    let enabled = op.state.borrow().dialogs.asks.iter().rev().find(|a| a.in_settings == settings).is_some_and(|a| a.row.third_enabled);
    if !enabled { return }
    if let Some(Ask { done: Done::Choose3(done), .. }) = take(op, settings) { done(Some(2)) }
}

/// The newest question over the operator window (`settings` false) or over Settings.
fn take(op: &Operator, settings: bool) -> Option<Ask> {
    let ask = {
        let mut st = op.state.borrow_mut();
        let at = st.dialogs.asks.iter().rposition(|a| a.in_settings == settings);
        at.map(|i| st.dialogs.asks.remove(i))
    };
    count(op);
    if ask.is_some() { show_top(op); }
    ask
}

/// The Settings window closed: its questions go unanswered (as Cancel would, without running anything).
pub(crate) fn clear_settings(op: &Operator) {
    op.state.borrow_mut().dialogs.asks.retain(|a| !a.in_settings);
    count(op);
    show_top(op);
}

/// Shows each window's newest waiting question, or gives the keyboard back to the dialog (or the window's keys).
/// A fresh one-row model makes a new overlay each time, whose `init` takes the keyboard.
fn show_top(op: &Operator) {
    let (top, top_settings, dialog) = {
        let st = op.state.borrow();
        let last = |s: bool| st.dialogs.asks.iter().rev().find(|a| a.in_settings == s).map(|a| a.row.clone());
        (last(false), last(true), st.dialog.is_some())
    };
    match top {
        Some(row) => op.ui.set_ask(ModelRc::new(VecModel::from(vec![row]))),
        None => {
            op.ui.set_ask(ModelRc::default());
            if dialog { op.ui.invoke_focus_dialog() } else { op.ui.invoke_focus_keys() }
        }
    }
    if super::settings::is_open(op) || top_settings.is_some() {
        let w = super::settings_window::win(op);
        match top_settings {
            Some(row) => w.set_ask(ModelRc::new(VecModel::from(vec![row]))),
            None => { w.set_ask(ModelRc::default()); w.invoke_focus_keys() }
        }
    }
}

