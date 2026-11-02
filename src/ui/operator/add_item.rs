//! `addItem()` (`operator.js:369-375`): one item goes at the end of the service and is selected. Shared by the Add
//! dialog, the song editor and adding files (Task 14), so it lives in the operator module, which owns the selection.
use super::window::Operator;
use crate::ui::dialogs::add_strings::ADDED;
use serde::Deserialize;
use serde_json::{Value, json};
use std::rc::Rc;
use uuid::Uuid;

#[derive(Deserialize)]
struct Added { id: Uuid }

/// `service/add {item, position: null}`, then the new item is selected and, unless `quiet`, the toast "Added to the
/// service". `done` runs only when the item went in; a failure is an error toast with the core's message (`guard()`),
/// so a dialog that waits for `done` to close stays open.
pub fn add_item(op: &Rc<Operator>, item: Value, quiet: bool, done: impl FnOnce(&Rc<Operator>, Uuid) + 'static) {
    let me = Rc::downgrade(op);
    op.ops.call::<Added>("service/add", json!({ "item": item, "position": null }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(Added { id }) => {
                op.select_added(id);
                if !quiet { op.toast(ADDED, false); }
                done(&op, id);
            }
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
