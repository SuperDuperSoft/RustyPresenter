//! User-facing strings of the dialog shell and its questions (Plan 3 Task 13), copied from `web/operator.js`.
//! Declared from `src/ui/dialogs/mod.rs` with a `#[path]` line until a shared `src/strings/mod.rs` exists.
use crate::tr;

/// Dialog titles (`operator.js:394, 705, 796, 810, 841, 873, 1004`).
pub fn add() -> String { tr!("Add to service") }
pub fn new_song() -> String { tr!("New song") }
pub fn edit_song() -> String { tr!("Edit song") }
pub fn services() -> String { tr!("Services") }
pub fn screens() -> String { tr!("Screens") }
pub fn phone() -> String { tr!("Phone remote") }
pub fn settings() -> String { tr!("Settings") }

/// The buttons of the browser's `confirm()` and `prompt()`, which these overlays replace.
pub fn ok() -> String { tr!("OK") }
pub fn cancel() -> String { tr!("Cancel") }
