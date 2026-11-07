//! User-facing strings of Item edit and its background picker (Plan 3 Task 18), copied from `web/operator.js:701-796`.
//! Declared from `src/ui/dialogs/item_edit.rs` with a `#[path]` line until a shared `src/strings/mod.rs` exists.
use crate::tr;

/// `toast('Saved')` after Save (`operator.js:795`).
pub fn saved() -> String { tr!("Saved") }

/// The background picker's tiles (`operator.js:703-716`).
pub fn theme_default() -> String { tr!("Theme default") }
pub fn upload() -> String { tr!("+ Upload…") }
/// A video choice's caption: `'▶ ' + name`.
pub fn video_choice(name: &str) -> String { format!("▶ {name}") }
/// After **+ Upload…** imported every chosen file.
pub fn uploaded() -> String { tr!("Added to the media library — pick it here") }

/// `statusChip(m)` (`operator.js:464-470`) for media that is not ready.
pub fn failed() -> String { tr!("Failed") }
