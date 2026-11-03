//! User-facing strings of the Screens dialog (Plan 3 Task 19), copied from `web/operator.js:793-828`.
//! Declared from `src/ui/dialogs/screens.rs` with a `#[path]` line, like `strings/dialogs.rs`.

/// The two cards.
pub const ROOM_TITLE: &str = "Room screen";
pub const ROOM_DESC: &str = "What the congregation sees: projector or TV.";
pub const STAGE_TITLE: &str = "Stage display (optional)";
pub const STAGE_DESC: &str = "For singers and the pastor: current and next lyrics, notes and a clock.";

/// The status chip.
pub const SHOWING: &str = "Showing";
pub const STARTING: &str = "Starting…";
pub const OFF: &str = "Off";

/// The toast after Show on this screen.
pub fn opened(title: &str) -> String { format!("{title} opened") }
