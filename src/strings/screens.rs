//! User-facing strings of Screen Configuration.
//! Declared from `src/ui/dialogs/screens.rs` with a `#[path]` line, like `strings/dialogs.rs`.
use crate::tr;

/// The two lists.
pub fn audience() -> String { tr!("Audience") }
pub fn stage() -> String { tr!("Stage") }
pub fn audience_screen() -> String { tr!("Audience screen") }
pub fn stage_screen() -> String { tr!("Stage screen") }

/// A row's status chip.
pub fn showing() -> String { tr!("Showing") }
pub fn off() -> String { tr!("Off") }
pub fn no_output() -> String { tr!("No output") }
pub fn unplugged() -> String { tr!("Unplugged") }

/// The Output choices.
pub fn output_none() -> String { tr!("None") }
pub fn output_window() -> String { tr!("In a window (for testing)") }
pub fn output_gone() -> String { tr!("Not connected right now") }

/// The notes under the settings.
pub fn on_main() -> String { tr!("This is the screen you are working on. Turning this on covers it.") }
pub fn mirrors(first: &str) -> String { tr!("Shows the same picture as {screen}. Sound only plays from {screen}.", screen = first) }

pub fn remove() -> String { tr!("Remove") }
pub fn remove_question(name: &str) -> String { tr!("Remove “{name}”?", name = name) }
