//! User-facing strings of service planning (FEAT-6a): dates, Copy last week, templates, blanks and headers.
//! Declared from `src/planning/mod.rs` with a `#[path]` line, like `strings/bundle.rs`.
use crate::model::BlankKind;
use crate::{tr, trc, trn};

/// The name of a service, or of a template, that has nothing else left once its date is taken off.
pub fn service() -> String { tr!("Service") }
pub fn no_date() -> String { tr!("No date") }

/// Weekday names, Monday first (`chrono::Weekday::num_days_from_monday`).
pub fn days_short() -> [String; 7] {
    [tr!("Mon"), tr!("Tue"), tr!("Wed"), tr!("Thu"), tr!("Fri"), tr!("Sat"), tr!("Sun")]
}
pub fn days_long() -> [String; 7] {
    [tr!("Monday"), tr!("Tuesday"), tr!("Wednesday"), tr!("Thursday"), tr!("Friday"), tr!("Saturday"), tr!("Sunday")]
}
/// "Sundays", for a template's day.
pub fn days_plural() -> [String; 7] {
    [tr!("Mondays"), tr!("Tuesdays"), tr!("Wednesdays"), tr!("Thursdays"), tr!("Fridays"), tr!("Saturdays"), tr!("Sundays")]
}
pub fn any_day() -> String { tr!("Any day") }
/// What a person may type for a template's day to mean no fixed day. Also "Any day" works.
pub fn any_typed() -> String { trc!("typed answer: no fixed day", "any") }
pub fn months() -> [String; 12] {
    [
        tr!("January"), tr!("February"), tr!("March"), tr!("April"), tr!("May"), tr!("June"), tr!("July"), tr!("August"),
        tr!("September"), tr!("October"), tr!("November"), tr!("December"),
    ]
}

/// What a blank asks for, as its label starts out.
pub fn blank_label(want: BlankKind) -> String {
    match want {
        BlankKind::Song => tr!("Song"),
        BlankKind::Scripture => tr!("Bible reading"),
        BlankKind::Media => tr!("Media"),
        BlankKind::Deck => tr!("Slides"),
        BlankKind::Text => tr!("Announcement"),
        BlankKind::Any => tr!("Something"),
    }
}

/// A blank's title in the service list and on the phone.
pub fn blank_title(label: &str) -> String {
    if label.trim().is_empty() { tr!("Blank") } else { tr!("{label} (blank)", label = label.trim()) }
}
/// Under a blank's title in the service list.
pub fn blank_subtitle() -> String { tr!("Click Choose to fill this in") }
pub fn blank_choose_first() -> String { tr!("This is a blank — choose what goes here first.") }
/// The Add dialog's title while it fills a blank.
pub fn fill_title(label: &str) -> String { tr!("Fill in: {label}", label = label) }

// ---- core errors ----
pub fn no_service_to_copy() -> String { tr!("There is no service to copy.") }
pub fn service_gone() -> String { tr!("That service is gone.") }
pub fn template_gone() -> String { tr!("That template is gone.") }
pub fn bad_date() -> String { tr!("That date isn't valid.") }
pub fn blank_gone() -> String { tr!("That blank is gone.") }
pub fn not_a_blank() -> String { tr!("That item is not a blank any more.") }
pub fn template_needs_name() -> String { tr!("A template needs a name.") }
pub fn template_needs_parts() -> String { tr!("A template needs at least one part.") }
pub fn in_template(name: &str) -> String { tr!("This is in the template “{name}”. Remove it from the template first.", name = name) }

// ---- Services dialog ----
pub fn blanks_chip(n: usize) -> String { trn!("{n} blank", "{n} blanks", n) }
pub fn show_older(n: usize) -> String { trn!("Show {n} older service", "Show {n} older services", n) }
pub fn template_parts(parts: usize, blanks: usize) -> String {
    trn!("{n} part, {blanks}", "{n} parts, {blanks}", parts, blanks = blanks_chip(blanks))
}
pub fn no_templates() -> String { tr!("No templates yet. Use “Save as template” on a service to make one.") }
pub fn need_template() -> String { tr!("Save a service as a template first") }
pub fn copy_week_question(date: &str, source: &str) -> String {
    tr!("Make a service for {date} from “{service}”?", date = date, service = source)
}
pub fn copy() -> String { tr!("Copy") }
pub fn copy_with_blanks() -> String { tr!("Copy, with blanks for songs and Bible readings") }
pub fn already_on_date(date: &str, name: &str) -> String {
    tr!("There is already a service for {date} (“{name}”).", date = date, name = name)
}
pub fn open_it() -> String { tr!("Open it") }
pub fn make_another() -> String { tr!("Make another") }
pub fn ready(name: &str, blanks: usize) -> String {
    match blanks {
        0 => tr!("“{name}” is ready", name = name),
        n => tr!("“{name}” is ready — fill in {blanks}", name = name, blanks = blanks_chip(n)),
    }
}
pub fn date_prompt() -> String { tr!("Date of the service (leave empty for no date)") }
pub fn date_example(example: &str) -> String { tr!("Date of the service, like {example} (leave empty for no date)", example = example) }
pub fn template_name_prompt() -> String { tr!("Name for the template") }
pub fn template_saved(name: &str) -> String { tr!("Template “{name}” saved", name = name) }
pub fn rename_template() -> String { tr!("Rename template") }
pub fn delete_template_question(name: &str) -> String { tr!("Delete the template “{name}”? (Services made from it stay.)", name = name) }
pub fn pick_template() -> String { tr!("Which template?") }
pub fn new_from_template_prompt() -> String { tr!("Name for the new service") }
pub fn template_day_question(name: &str) -> String { tr!("Which day is the template “{name}” for?", name = name) }
pub fn sundays() -> String { tr!("Sundays") }

// ---- headers ----
pub fn header_prompt() -> String { tr!("Name for the header") }
pub fn header_default() -> String { tr!("Worship") }
pub fn rename_header() -> String { tr!("Rename header") }
pub fn pick_template_below() -> String { tr!("Click “Make a service” on the template you want, under Templates.") }
pub fn bad_day() -> String { tr!("Type a day of the week, like Sunday, or leave it empty for any day.") }
