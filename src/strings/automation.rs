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

pub fn cant_run_deleted(name: &str, n: usize, thing: &str) -> String {
    format!("Couldn't run “{name}”: step {n}'s {thing} was deleted. Edit the macro.")
}
pub fn cant_run_newer(name: &str, n: usize) -> String {
    format!("Couldn't run “{name}”: step {n} needs a newer RustyPresenter.")
}
pub fn ran(name: &str) -> String { format!("Ran “{name}”") }
pub fn ran_but_step_failed(name: &str, n: usize, reason: &str) -> String {
    format!("Ran “{name}”, but step {n} didn't work: {reason}")
}
pub fn ran_but_skipped_shows(name: &str) -> String {
    format!("Ran “{name}”, but skipped showing things because the screen was cleared.")
}
pub fn at_zero_changes_slides(timer: &str, name: &str) -> String {
    format!("“{timer}” reached zero, but “{name}” changes slides, so it didn't run.")
}
pub fn at_zero_skipped_shows(timer: &str, name: &str) -> String {
    format!("“{timer}” reached zero, but “{name}” didn't show anything because the screen was cleared.")
}

/// *(plan copy)*
pub const ADVANCE_RANGE: &str = "Choose 2 to 600 seconds.";
pub const NEEDS_TWO_SLIDES: &str = "Needs at least two slides.";

pub const SERVICE_STARTS_IN: &str = "Service starts in";
pub const PRE_SERVICE_TIMER_NAME: &str = "Pre-service";
pub const START_COUNTDOWN: &str = "Start countdown";
pub const PRESERVICE_LOCKED: &str = "Change it in the Pre-service tab";
pub const COUNTDOWN_ENDED_NOTHING_ELSE: &str =
    "Countdown ended — nothing else changed because the service has started.";
pub const RESTART_FOR_NEW_TIME: &str = "Restart the countdown to use the new time.";

pub const SERVICE_START_NAME: &str = "Service start";
pub const SERMON_NAME: &str = "Sermon";
pub const STOPWATCH_NAME: &str = "Stopwatch";
