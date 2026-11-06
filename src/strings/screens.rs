//! User-facing strings of Screen Configuration.
//! Declared from `src/ui/dialogs/screens.rs` with a `#[path]` line, like `strings/dialogs.rs`.

/// The two lists.
pub const AUDIENCE: &str = "Audience";
pub const STAGE: &str = "Stage";
pub const AUDIENCE_SCREEN: &str = "Audience screen";
pub const STAGE_SCREEN: &str = "Stage screen";

/// A row's status chip.
pub const SHOWING: &str = "Showing";
pub const OFF: &str = "Off";
pub const NO_OUTPUT: &str = "No output";
pub const UNPLUGGED: &str = "Unplugged";

/// The Output choices.
pub const OUTPUT_NONE: &str = "None";
pub const OUTPUT_WINDOW: &str = "In a window (for testing)";
pub const OUTPUT_GONE: &str = "Not connected right now";

/// The notes under the settings.
pub const ON_MAIN: &str = "This is the screen you are working on. Turning this on covers it.";
pub fn mirrors(first: &str) -> String { format!("Shows the same picture as {first}. Sound only plays from {first}.") }

pub const REMOVE: &str = "Remove";
pub fn remove_question(name: &str) -> String { format!("Remove “{name}”?") }
