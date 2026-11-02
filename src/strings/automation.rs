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

pub fn fill_in(field: &str) -> String { format!("Fill in “{field}”.") }
pub const MESSAGE_TOO_LONG: &str = "That message is too long for the screen (200 letters at most).";
pub const UNMATCHED_BRACE: &str = "This message has a { without a matching }.";
pub const MESSAGE_SHOWING: &str = "Message showing";
pub const PRACTICE_WARNING: &str =
    "Practice mode is on — this shows only in your preview. End practice to show it in the room.";

pub const NURSERY_NAME: &str = "Nursery";
pub const NURSERY_TEXT: &str = "Parent of child #{Number}, please come to the nursery.";
pub const VEHICLE_NAME: &str = "Vehicle";
pub const VEHICLE_TEXT: &str = "Vehicle {Plate}: your lights are on.";
pub const CUSTOM_NAME: &str = "Custom";
pub const CUSTOM_TEXT: &str = "{Message}";
