//! FEAT-2's user-facing strings (FEAT-4 §7 convention: one strings file per feature).
//! Copied verbatim from the spec; strings marked *(plan copy)* can be reworded.

pub const STALE_TRY_AGAIN: &str = "The screen changed — try again.";
pub const CLEARED_TRY_AGAIN: &str = "The screen was cleared — try again.";

pub const NO_START_TIME: &str = "Set a start time for this service first (Services → Starts at).";

pub fn service_start_passed(at: &str) -> String { format!("The service start time ({at}) has passed.") }

pub fn time_passed(at: &str) -> String { format!("That time ({at}) has passed.") }

pub const TIMER_NEEDS_NEWER: &str = "This timer needs a newer RustyPresenter.";
pub const CANT_PAUSE_TO_TIME: &str = "A countdown to a time can't be paused.";

/// *(plan copy)*
pub const DURATION_RANGE: &str = "Choose a length from 1 second to 23:59:59.";
/// *(plan copy)*
pub const SET_NOW_WRONG_KIND: &str = "This timer can't be set that way.";
