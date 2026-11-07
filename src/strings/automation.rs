//! FEAT-2's user-facing strings (FEAT-4 §7 convention: one strings file per feature).
//! Copied verbatim from the spec; strings marked *(plan copy)* can be reworded.
use crate::tr;

pub fn stale_try_again() -> String { tr!("The screen changed — try again.") }
pub fn cleared_try_again() -> String { tr!("The screen was cleared — try again.") }

pub fn no_start_time() -> String { tr!("Set a start time for this service first (Services → Starts at).") }

pub fn service_start_passed(at: &str) -> String { tr!("The service start time ({time}) has passed.", time = at) }

pub fn time_passed(at: &str) -> String { tr!("That time ({time}) has passed.", time = at) }

pub fn timer_needs_newer() -> String { tr!("This timer needs a newer RustyPresenter.") }
pub fn cant_pause_to_time() -> String { tr!("A countdown to a time can't be paused.") }

/// *(plan copy)*
pub fn duration_range() -> String { tr!("Choose a length from 1 second to 23:59:59.") }
/// *(plan copy)*
pub fn set_now_wrong_kind() -> String { tr!("This timer can't be set that way.") }

pub fn fill_in(field: &str) -> String { tr!("Fill in “{field}”.", field = field) }
pub fn message_too_long() -> String { tr!("That message is too long for the screen (200 letters at most).") }
pub fn unmatched_brace() -> String { tr!("This message has a { without a matching }.") }
pub fn message_showing() -> String { tr!("Message showing") }
pub fn practice_warning() -> String {
    tr!("Practice mode is on — this shows only in your preview. End practice to show it in the room.")
}

/// The built-in messages. Their `{Field}` gaps are the fields' names and stay as they are in a translation.
pub fn nursery_name() -> String { tr!("Nursery") }
pub fn nursery_text() -> String { tr!("Parent of child #{Number}, please come to the nursery.") }
pub fn vehicle_name() -> String { tr!("Vehicle") }
pub fn vehicle_text() -> String { tr!("Vehicle {Plate}: your lights are on.") }
pub fn custom_name() -> String { tr!("Custom") }
/// Only a field, no words.
pub const CUSTOM_TEXT: &str = "{Message}";

/// What a macro step points at that can be deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepTarget { Item, Timer, Overlay, ScreenView, Message, StageLayout }

/// One whole sentence per kind of thing, so a translation can put the words in its own order.
pub fn cant_run_deleted(name: &str, n: usize, thing: StepTarget) -> String {
    match thing {
        StepTarget::Item => tr!("Couldn't run “{name}”: step {step}'s item was deleted. Edit the macro.", name = name, step = n),
        StepTarget::Timer => tr!("Couldn't run “{name}”: step {step}'s timer was deleted. Edit the macro.", name = name, step = n),
        StepTarget::Overlay => tr!("Couldn't run “{name}”: step {step}'s overlay was deleted. Edit the macro.", name = name, step = n),
        StepTarget::ScreenView => {
            tr!("Couldn't run “{name}”: step {step}'s screen view was deleted. Edit the macro.", name = name, step = n)
        }
        StepTarget::Message => tr!("Couldn't run “{name}”: step {step}'s message was deleted. Edit the macro.", name = name, step = n),
        StepTarget::StageLayout => {
            tr!("Couldn't run “{name}”: step {step}'s stage layout was deleted. Edit the macro.", name = name, step = n)
        }
    }
}
pub fn cant_run_newer(name: &str, n: usize) -> String {
    tr!("Couldn't run “{name}”: step {step} needs a newer RustyPresenter.", name = name, step = n)
}
pub fn ran(name: &str) -> String { tr!("Ran “{name}”", name = name) }
pub fn ran_but_step_failed(name: &str, n: usize, reason: &str) -> String {
    tr!("Ran “{name}”, but step {step} didn't work: {reason}", name = name, step = n, reason = reason)
}
pub fn ran_but_skipped_shows(name: &str) -> String {
    tr!("Ran “{name}”, but skipped showing things because the screen was cleared.", name = name)
}
pub fn at_zero_changes_slides(timer: &str, name: &str) -> String {
    tr!("“{timer}” reached zero, but “{macro_name}” changes slides, so it didn't run.", timer = timer, macro_name = name)
}
pub fn at_zero_skipped_shows(timer: &str, name: &str) -> String {
    tr!("“{timer}” reached zero, but “{macro_name}” didn't show anything because the screen was cleared.", timer = timer, macro_name = name)
}

/// *(plan copy)*
pub fn advance_range() -> String { tr!("Choose 2 to 600 seconds.") }
pub fn needs_two_slides() -> String { tr!("Needs at least two slides.") }

pub fn service_starts_in() -> String { tr!("Service starts in") }
pub fn pre_service_timer_name() -> String { tr!("Pre-service") }
pub fn start_countdown() -> String { tr!("Start countdown") }
pub fn preservice_locked() -> String { tr!("Change it in the Pre-service tab") }
pub fn countdown_ended_nothing_else() -> String {
    tr!("Countdown ended — nothing else changed because the service has started.")
}
pub fn restart_for_new_time() -> String { tr!("Restart the countdown to use the new time.") }

pub fn service_start_name() -> String { tr!("Service start") }
pub fn sermon_name() -> String { tr!("Sermon") }
pub fn stopwatch_name() -> String { tr!("Stopwatch") }
