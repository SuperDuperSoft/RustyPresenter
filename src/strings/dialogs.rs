//! User-facing strings of the dialog shell and its questions (Plan 3 Task 13), copied from `web/operator.js`.
//! Declared from `src/ui/dialogs/mod.rs` with a `#[path]` line until a shared `src/strings/mod.rs` exists.

/// Dialog titles (`operator.js:394, 705, 796, 810, 841, 873, 1004`).
pub const ADD: &str = "Add to service";
pub const NEW_SONG: &str = "New song";
pub const EDIT_SONG: &str = "Edit song";
pub const SERVICES: &str = "Services";
pub const SCREENS: &str = "Screens";
pub const PHONE: &str = "Phone remote";
pub const SETTINGS: &str = "Settings";

/// The buttons of the browser's `confirm()` and `prompt()`, which these overlays replace.
pub const OK: &str = "OK";
pub const CANCEL: &str = "Cancel";
