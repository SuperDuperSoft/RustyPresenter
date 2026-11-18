//! User-facing strings of the song use log and its report (FEAT-7 §3.3, §5.5, §8).
//! Declared from `src/usage/mod.rs` with a `#[path]` line, like `strings/phone.rs`.
use crate::tr;

/// The dialog's title.
pub fn title() -> String { tr!("Song use report") }

/// The report's header.
pub fn license_line(license: &str) -> String { tr!("CCLI licence #: {license}", license = license) }
pub fn no_license() -> String { tr!("Add your church's CCLI licence number in Settings → Songs → Slides and credits.") }

/// The Mark button and its question.
pub fn mark_reported(to: &str) -> String { tr!("Mark as reported through {date}", date = to) }
pub fn mark_confirm(to: &str) -> String { tr!("Next time, the report starts the day after {date}.", date = to) }
pub fn mark() -> String { tr!("Mark as reported") }
pub fn marked(to: &str) -> String { tr!("Marked as reported through {date}.", date = to) }

/// Removing a Details entry.
pub fn remove_confirm() -> String { tr!("Remove this entry? Use this for rehearsals that shouldn't count.") }
pub fn remove() -> String { tr!("Remove") }
pub fn removed() -> String { tr!("Entry removed") }

/// Settings → Songs' link, when Settings has changes not saved yet.
pub fn save_first() -> String { tr!("Save your settings changes and open the report?") }
pub fn save_and_open() -> String { tr!("Save and open") }

/// Errors.
pub fn from_after_to() -> String { tr!("Pick a From date before the To date.") }
pub fn bad_date() -> String { tr!("Type dates as year-month-day, like 2026-11-01.") }
pub fn save_failed(reason: &str) -> String { tr!("Couldn't save the report: {reason}.", reason = reason) }

/// After an export.
pub fn saved_to(path: &str) -> String { tr!("Saved {path}", path = path) }

/// The save dialog's file type.
pub fn filter_csv() -> String { tr!("Spreadsheet (CSV)") }

/// The "streamed" mark in Details.
pub fn streamed() -> String { tr!("streamed") }

/// The spreadsheet's header row and licence line. CCLI's licence wording stays in English.
pub fn csv_header() -> [String; 6] {
    [tr!("Title"), tr!("CCLI Song #"), tr!("Writers"), tr!("Times shown"), tr!("Times streamed"), tr!("Last used")]
}
pub const CSV_LICENSE: &str = "CCLI License #";

/// The printable page.
pub fn page_range(from: &str, to: &str) -> String { tr!("{from} to {to}", from = from, to = to) }
pub fn page_no_songs() -> String { tr!("No songs in these dates.") }
pub fn page_print_hint() -> String { tr!("Print this page, or save it as a PDF from your browser's Print window.") }
pub fn footer() -> String {
    tr!("Enter these numbers at CCLI's reporting site: projected uses go under “Digital”, streamed uses under your Streaming License. “Streamed” means a livestream page or window was open, not that you were on air.")
}
