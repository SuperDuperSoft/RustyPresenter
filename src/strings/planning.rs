//! User-facing strings of service planning (FEAT-6a): dates, Copy last week, templates, blanks and headers.
//! Declared from `src/planning/mod.rs` with a `#[path]` line, like `strings/bundle.rs`.
use crate::model::BlankKind;

/// The name of a service, or of a template, that has nothing else left once its date is taken off.
pub const SERVICE: &str = "Service";
pub const NO_DATE: &str = "No date";

/// Weekday names, Monday first (`chrono::Weekday::num_days_from_monday`).
pub const DAYS_SHORT: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
pub const DAYS_LONG: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
/// "Sundays", for a template's day.
pub const DAYS_PLURAL: [&str; 7] = ["Mondays", "Tuesdays", "Wednesdays", "Thursdays", "Fridays", "Saturdays", "Sundays"];
pub const ANY_DAY: &str = "Any day";
pub const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// What a blank asks for, as its label starts out.
pub fn blank_label(want: BlankKind) -> &'static str {
    match want {
        BlankKind::Song => "Song",
        BlankKind::Scripture => "Bible reading",
        BlankKind::Media => "Media",
        BlankKind::Deck => "Slides",
        BlankKind::Text => "Announcement",
        BlankKind::Any => "Something",
    }
}

/// A blank's title in the service list and on the phone.
pub fn blank_title(label: &str) -> String { if label.trim().is_empty() { "Blank".into() } else { format!("{} (blank)", label.trim()) } }
/// Under a blank's title in the service list.
pub const BLANK_SUBTITLE: &str = "Click Choose to fill this in";
pub const BLANK_CHOOSE_FIRST: &str = "This is a blank — choose what goes here first.";
/// The Add dialog's title while it fills a blank.
pub fn fill_title(label: &str) -> String { format!("Fill in: {label}") }

// ---- core errors ----
pub const NO_SERVICE_TO_COPY: &str = "There is no service to copy.";
pub const SERVICE_GONE: &str = "That service is gone.";
pub const TEMPLATE_GONE: &str = "That template is gone.";
pub const BAD_DATE: &str = "That date isn't valid.";
pub const BLANK_GONE: &str = "That blank is gone.";
pub const NOT_A_BLANK: &str = "That item is not a blank any more.";
pub const TEMPLATE_NEEDS_NAME: &str = "A template needs a name.";
pub const TEMPLATE_NEEDS_PARTS: &str = "A template needs at least one part.";
pub fn in_template(name: &str) -> String { format!("This is in the template “{name}”. Remove it from the template first.") }

// ---- Services dialog ----
pub fn blanks_chip(n: usize) -> String { format!("{n} blank{}", if n == 1 { "" } else { "s" }) }
pub fn show_older(n: usize) -> String { format!("Show {n} older service{}", if n == 1 { "" } else { "s" }) }
pub fn template_parts(parts: usize, blanks: usize) -> String {
    format!("{parts} part{}, {}", if parts == 1 { "" } else { "s" }, blanks_chip(blanks))
}
pub const NO_TEMPLATES: &str = "No templates yet. Use “Save as template” on a service to make one.";
pub const NEED_TEMPLATE: &str = "Save a service as a template first";
pub fn copy_week_question(date: &str, source: &str) -> String { format!("Make a service for {date} from “{source}”?") }
pub const COPY: &str = "Copy";
pub const COPY_WITH_BLANKS: &str = "Copy, with blanks for songs and Bible readings";
pub fn already_on_date(date: &str, name: &str) -> String { format!("There is already a service for {date} (“{name}”).") }
pub const OPEN_IT: &str = "Open it";
pub const MAKE_ANOTHER: &str = "Make another";
pub fn ready(name: &str, blanks: usize) -> String {
    match blanks {
        0 => format!("“{name}” is ready"),
        n => format!("“{name}” is ready — fill in {}", blanks_chip(n)),
    }
}
pub const DATE_PROMPT: &str = "Date of the service (leave empty for no date)";
pub fn date_example(example: &str) -> String { format!("Date of the service, like {example} (leave empty for no date)") }
pub const TEMPLATE_NAME_PROMPT: &str = "Name for the template";
pub fn template_saved(name: &str) -> String { format!("Template “{name}” saved") }
pub const RENAME_TEMPLATE: &str = "Rename template";
pub fn delete_template_question(name: &str) -> String { format!("Delete the template “{name}”? (Services made from it stay.)") }
pub const PICK_TEMPLATE: &str = "Which template?";
pub const NEW_FROM_TEMPLATE_PROMPT: &str = "Name for the new service";
pub fn template_day_question(name: &str) -> String { format!("Which day is the template “{name}” for?") }
pub const SUNDAYS: &str = "Sundays";

// ---- headers ----
pub const HEADER_PROMPT: &str = "Name for the header";
pub const HEADER_DEFAULT: &str = "Worship";
pub const RENAME_HEADER: &str = "Rename header";
pub const PICK_TEMPLATE_BELOW: &str = "Click “Make a service” on the template you want, under Templates.";
pub const BAD_DAY: &str = "Type a day of the week, like Sunday, or leave it empty for any day.";
