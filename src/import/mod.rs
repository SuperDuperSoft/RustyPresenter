//! Turning files into library entries: finding out what a file is and what is in it.
pub mod libreoffice;
pub mod pdf;
pub mod probe;
#[path = "../strings/import.rs"]
pub mod strings;

/// What a cancelled import reports. No toast is shown for it.
pub const CANCELLED: &str = "Cancelled.";
