//! Turning files into library entries: finding out what a file is and what is in it.
#[path = "../strings/import.rs"]
pub mod strings;
pub mod libreoffice;
pub mod pdf;

/// What a cancelled import reports. No toast is shown for it.
pub const CANCELLED: &str = "Cancelled.";
