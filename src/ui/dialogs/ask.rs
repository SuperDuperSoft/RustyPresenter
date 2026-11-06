//! `confirm()` and `prompt()` for the native operator (`ui/dialogs/ask.slint`): small overlays above the open
//! dialog, or above the bare window. While one is up, `OpState.asks > 0`, so the operator keys are off except
//! F1–F5 (`keys::route`), as the browser's own questions block every key. A question asked while one is up (from
//! inside its `done`, say) goes on top; the one under it comes back once it is answered.
use crate::ui::{AskRow, operator::window::Operator};
use slint::{ModelRc, VecModel};
use std::rc::Rc;

/// A question waiting to be answered.
pub struct Ask {
    row: AskRow,
    done: Done,
}

enum Done {
    Confirm(Box<dyn FnOnce(bool)>),
    Prompt(Box<dyn FnOnce(Option<String>)>),
    Choose(Box<dyn FnOnce(Choice)>),
}

/// The answer to `choose`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice { Ok, Other, Cancel }

/// Asks a yes/no question. `done(true)` on OK or Enter, `done(false)` on Cancel or Esc. `danger` draws OK in the
/// danger colour (a delete).
pub fn confirm(op: &Rc<Operator>, text: &str, ok_label: &str, cancel_label: &str, danger: bool, done: impl FnOnce(bool) + 'static) {
    let row = AskRow { prompt: false, text: text.into(), value: "".into(), ok_label: ok_label.into(), cancel_label: cancel_label.into(), danger, other_label: "".into() };
    push(op, Ask { row, done: Done::Confirm(Box::new(done)) });
}

/// A confirm with a third button, `other_label`, between Cancel and OK. `done(Ok)` on OK or Enter, `done(Other)`
/// on the third button, `done(Cancel)` on Cancel or Esc.
pub fn choose(op: &Rc<Operator>, text: &str, ok_label: &str, other_label: &str, done: impl FnOnce(Choice) + 'static) {
    let row = AskRow {
        prompt: false, text: text.into(), value: "".into(), ok_label: ok_label.into(),
        cancel_label: super::strings::CANCEL.into(), danger: false, other_label: other_label.into(),
    };
    push(op, Ask { row, done: Done::Choose(Box::new(done)) });
}

/// Asks for a line of text, starting from `initial` (selected, so typing replaces it). `done(Some(text))` on OK
/// or Enter, even when empty (the browser's `prompt()` returns `""` then); `done(None)` on Cancel or Esc.
pub fn prompt(op: &Rc<Operator>, title: &str, initial: &str, done: impl FnOnce(Option<String>) + 'static) {
    let row = AskRow {
        prompt: true, text: title.into(), value: initial.into(),
        ok_label: super::strings::OK.into(), cancel_label: super::strings::CANCEL.into(), danger: false, other_label: "".into(),
    };
    push(op, Ask { row, done: Done::Prompt(Box::new(done)) });
}

fn push(op: &Operator, ask: Ask) {
    {
        let mut st = op.state.borrow_mut();
        st.dialogs.asks.push(ask);
        st.asks = st.dialogs.asks.len();
    }
    show_top(op);
}

/// The answer to the top question (from `AskOverlay`). The question is gone and the keyboard handed back before
/// `done` runs, so `done` may ask again, open a dialog or close one.
pub(crate) fn answer(op: &Operator, ok: bool, text: String) {
    let Some(ask) = pop(op) else { return };
    match ask.done {
        Done::Confirm(done) => done(ok),
        Done::Prompt(done) => done(ok.then_some(text)),
        Done::Choose(done) => done(if ok { Choice::Ok } else { Choice::Cancel }),
    }
}

/// The third button of the top question (from `AskOverlay`); only a `choose` has one.
pub(crate) fn answer_other(op: &Operator) {
    let Some(ask) = pop(op) else { return };
    match ask.done {
        Done::Choose(done) => done(Choice::Other),
        Done::Confirm(done) => done(false),
        Done::Prompt(done) => done(None),
    }
}

/// Takes the top question off and shows the one under it.
fn pop(op: &Operator) -> Option<Ask> {
    let ask = {
        let mut st = op.state.borrow_mut();
        let ask = st.dialogs.asks.pop();
        st.asks = st.dialogs.asks.len();
        ask
    };
    show_top(op);
    ask
}

/// Shows the newest waiting question, or gives the keyboard back to the dialog (or the window's keys).
/// A fresh one-row model makes a new overlay each time, whose `init` takes the keyboard.
fn show_top(op: &Operator) {
    let (top, dialog) = {
        let st = op.state.borrow();
        (st.dialogs.asks.last().map(|a| a.row.clone()), st.dialog.is_some())
    };
    match top {
        Some(row) => op.ui.set_ask(ModelRc::new(VecModel::from(vec![row]))),
        None => {
            op.ui.set_ask(ModelRc::default());
            if dialog { op.ui.invoke_focus_dialog() } else { op.ui.invoke_focus_keys() }
        }
    }
}
