//! `addItem()` (`operator.js:369-375`): one item goes at the end of the service and is selected. Shared by the Add
//! dialog, the song editor and adding files (`files`), so it lives in the operator module, which owns the selection.
use super::{strings as tr, window::Operator};
use serde::Deserialize;
use serde_json::{Value, json};
use std::rc::Rc;
use uuid::Uuid;

/// `service/add`'s answer: the new id, and what to say when an Also Bible was left out (Nov 5 spec §3).
#[derive(Deserialize)]
struct Added { id: Uuid, #[serde(default)] toast: Option<String> }

/// `service/add {item, position: null}`, then the new item is selected and, unless `quiet`, the toast "Added to the
/// service". `done` gets the new id or the core's error; this does not toast an error itself, so adding files can
/// name the file in its own toast.
pub fn add_item(op: &Rc<Operator>, item: Value, quiet: bool, done: impl FnOnce(Result<Uuid, String>) + 'static) {
    let me = Rc::downgrade(op);
    op.ops.call::<Added>("service/add", json!({ "item": item, "position": null }), move |r| {
        let Some(op) = me.upgrade() else { return };
        let (r, left_out) = match r { Ok(Added { id, toast }) => (Ok(id), toast), Err(e) => (Err(e), None) };
        if let Ok(id) = r {
            op.select_added(id);
            if !quiet { op.toast(tr::added_to_service(), false); }
        }
        done(r);
        if let Some(t) = left_out { op.toast(t, true) }
    });
}

/// `add_item` for the dialogs: `done` runs only when the item went in; a failure is an error toast with the core's
/// message (`guard()`), so a dialog that waits for `done` to close stays open.
pub fn add_item_then(op: &Rc<Operator>, item: Value, quiet: bool, done: impl FnOnce(&Rc<Operator>, Uuid) + 'static) {
    // Filling a blank (FEAT-6a): the choice replaces it instead.
    let fill = op.state.borrow().fill;
    if let Some(blank) = fill { return crate::ui::dialogs::services::fill_blank(op, blank, item, done) }
    let me = Rc::downgrade(op);
    add_item(op, item, quiet, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(id) => done(&op, id),
            Err(e) => op.toast(e, true),
        }
    });
}

impl Operator {
    /// Selects an item that was just added. The core's `Data` update may already have been applied without it, so
    /// the marks and the grid are redrawn now; the next snapshot keeps the selection (`view::reselect`).
    pub(crate) fn select_added(&self, id: Uuid) {
        self.state.borrow_mut().selected = Some(id);
        self.mark_service();
        self.render_slides();
    }

    /// The selected service item, for tests in other modules.
    #[cfg(test)]
    pub(crate) fn selected_item(&self) -> Option<Uuid> { self.state.borrow().selected }

    /// Every toast shown so far, for tests in other modules (`OpState.toasts`).
    #[cfg(test)]
    pub(crate) fn toast_log(&self) -> Vec<(String, bool)> { self.state.borrow().toasts.clone() }
}
