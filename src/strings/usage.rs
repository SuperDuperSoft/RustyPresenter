//! User-facing strings of the song use log and its report (FEAT-7 §3.3, §5.5, §8).
//! Declared from `src/usage/mod.rs` with a `#[path]` line, like `strings/phone.rs`.

/// The dialog's title.
pub const TITLE: &str = "Song use report";

/// The report's header.
pub fn license_line(license: &str) -> String { format!("CCLI license #: {license}") }
pub const NO_LICENSE: &str = "Add your church's CCLI license number in Settings → Songs.";

/// The Mark button and its question.
pub fn mark_reported(to: &str) -> String { format!("Mark as reported through {to}") }
pub fn mark_confirm(to: &str) -> String { format!("Next time, the report starts the day after {to}.") }
pub const MARK: &str = "Mark as reported";
pub fn marked(to: &str) -> String { format!("Marked as reported through {to}.") }

/// Removing a Details entry.
pub const REMOVE_CONFIRM: &str = "Remove this entry? Use this for rehearsals that shouldn't count.";
pub const REMOVE: &str = "Remove";
pub const REMOVED: &str = "Entry removed";

/// Settings → Songs' link, when Settings has changes not saved yet.
pub const SAVE_FIRST: &str = "Save your settings changes and open the report?";
pub const SAVE_AND_OPEN: &str = "Save and open";

/// Errors.
pub const FROM_AFTER_TO: &str = "Pick a From date before the To date.";
pub const BAD_DATE: &str = "Type dates as year-month-day, like 2026-11-01.";
pub fn save_failed(reason: &str) -> String { format!("Couldn't save the report: {reason}.") }

/// After an export.
pub fn saved_to(path: &str) -> String { format!("Saved {path}") }

/// The save dialog's file type.
pub const FILTER_CSV: &str = "Spreadsheet (CSV)";

/// The "streamed" mark in Details.
pub const STREAMED: &str = "streamed";

/// The spreadsheet's header row and licence line.
pub const CSV_HEADER: [&str; 6] = ["Title", "CCLI Song #", "Writers", "Times shown", "Times streamed", "Last used"];
pub const CSV_LICENSE: &str = "CCLI License #";

/// The printable page.
pub fn page_range(from: &str, to: &str) -> String { format!("{from} to {to}") }
pub const PAGE_NO_SONGS: &str = "No songs in these dates.";
pub const PAGE_PRINT_HINT: &str = "Print this page, or save it as a PDF from your browser's Print window.";
pub const FOOTER: &str = "Enter these numbers at CCLI's reporting site: projected uses go under “Digital”, streamed uses under your Streaming License. “Streamed” means a livestream page or window was open, not that you were on air.";
