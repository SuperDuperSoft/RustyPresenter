//! FEAT-2's user-facing strings (FEAT-4 §7 convention: one strings file per feature).
//! Copied verbatim from the spec; strings marked *(plan copy)* can be reworded.
use crate::{tr, trn};

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

// ---- Part B (plan copy unless the spec has the words) ----
pub fn too_many_room_timers() -> String { tr!("Up to 4 timers can be on the screen at once.") }
/// *(plan copy)*
pub fn timer_gone() -> String { tr!("That timer was deleted.") }
/// *(plan copy)*
pub fn message_gone() -> String { tr!("That message was deleted.") }
/// *(plan copy)*
pub fn macro_gone() -> String { tr!("That macro was deleted.") }
/// *(plan copy)* Never seen by people: the App runs macros before the engine.
pub fn macro_needs_app() -> String { tr!("Macros run in the app.") }
pub fn too_fast() -> String { tr!("Too fast — try again.") }
pub fn not_allowed_from_phone() -> String { tr!("Not allowed from the phone.") }
/// *(plan copy)*
pub fn name_length() -> String { tr!("Give it a name of 1 to 40 letters.") }
/// *(plan copy)*
pub fn too_many_timers() -> String { tr!("You can have up to 50 timers.") }
/// *(plan copy)*
pub fn too_many_messages() -> String { tr!("You can have up to 100 messages.") }
/// *(plan copy)*
pub fn too_many_macros() -> String { tr!("You can have up to 100 macros.") }
/// *(plan copy)*
pub fn too_many_steps() -> String { tr!("A macro can have up to 30 steps.") }
/// *(plan copy)*
pub fn at_zero_macro_missing() -> String { tr!("That macro was deleted. Pick another one.") }
pub fn at_zero_overlay_only() -> String {
    tr!("A timer can only run a macro that shows or hides things (timers, messages, overlays).")
}
pub fn used_by(name: &str, n: usize) -> String {
    trn!("“{name}” is used by {n} macro. Delete it anyway? Those steps will stop working.",
        "“{name}” is used by {n} macros. Delete it anyway? Those steps will stop working.", n, name = name)
}
/// *(plan copy)*
pub fn used_at_zero(name: &str, n: usize) -> String {
    trn!("“{name}” runs when {n} timer reaches zero. Delete it anyway? Those timers will do nothing at zero.",
        "“{name}” runs when {n} timers reach zero. Delete it anyway? Those timers will do nothing at zero.", n, name = name)
}
/// *(plan copy)* Stage layouts that follow the slides come with the stage layout editor (FEAT-3).
pub fn stage_auto_not_yet() -> String { tr!("The stage layout can't follow the slides yet.") }
/// *(plan copy)* FEAT-7 screen views.
pub fn screen_views_not_yet() -> String { tr!("Screen views aren't available yet.") }
pub fn confirm_run(name: &str) -> String { tr!("Run “{name}”?", name = name) }

// N13: walk-in music.
pub fn music_empty() -> String { tr!("Add some songs to the walk-in music first.") }
pub fn music_track_gone() -> String { tr!("That song isn't in the walk-in music any more.") }
pub fn music_too_many() -> String { tr!("The walk-in music can hold 500 songs at most.") }
pub fn music_failed(why: &str) -> String { tr!("The walk-in music stopped: {reason}", reason = why) }

// N17: the calendar.
pub fn calendar_macro_gone(name: &str) -> String { tr!("“{name}” runs a macro that was deleted.", name = name) }
pub fn calendar_no_days() -> String { tr!("Pick at least one day.") }
pub fn calendar_pick_what() -> String { tr!("Pick what to do.") }
pub fn calendar_too_many() -> String { tr!("The calendar can hold 50 entries at most.") }
pub fn calendar_ran(name: &str) -> String { tr!("Calendar: ran “{name}”.", name = name) }
