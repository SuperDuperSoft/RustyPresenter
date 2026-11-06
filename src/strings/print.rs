//! User-facing strings of printing the service (FEAT-6b). Declared from `src/print/mod.rs` with a `#[path]` line.

pub fn ccli_licence(n: &str) -> String { format!("CCLI License # {n}") }
pub fn ccli_song(n: &str) -> String { format!("CCLI Song # {n}") }
pub fn copyright(c: &str) -> String { if c.trim_start().starts_with('©') { c.trim().into() } else { format!("© {}", c.trim()) } }
pub fn order_line(labels: &str) -> String { format!("Order: {labels}") }
pub fn slides(n: usize) -> String { format!("{n} slide{}", if n == 1 { "" } else { "s" }) }
pub fn blank_title(label: &str) -> String { format!("Blank — {label}") }
pub const NOT_CHOSEN: &str = "(not chosen yet)";
pub const NO_LYRICS: &str = "(no lyrics)";
pub const NO_VERSE_TEXT: &str = "(the verse text isn't saved with this service)";
pub const MISSING_SONG: &str = "Missing song";
pub const MISSING_MEDIA: &str = "Missing media";
pub const MISSING_SLIDES: &str = "Missing slides";
pub const VIDEO: &str = "Video";
pub const PICTURE: &str = "Picture";
pub const AUDIO: &str = "Audio";
pub const LOOPS: &str = " (loops)";
pub const ORDER_HEADING: &str = "Service order";
pub const LYRICS_HEADING: &str = "Lyrics";
pub const NOTHING_TO_PRINT: &str = "No songs or Bible readings in this service.";
pub const EMPTY_SERVICE: &str = "This service is empty.";
pub fn printed(date: &str) -> String { format!("Printed from RustyPresenter · {date}") }
/// The page's file name, without ".html".
pub fn file_stem(name: &str, what: crate::model::PrintWhat) -> String {
    use crate::model::PrintWhat::*;
    format!("{name} – {}", match what { Order => "order", Lyrics => "lyrics", Both => "order and lyrics" })
}

// ---- the Print dialog and its toasts ----
pub const OPENED: &str = "Opened in your web browser — use Print there (you can also save as PDF from the print window)";
pub fn saved(file: &str) -> String { format!("Saved {file}") }
pub fn cant_make(reason: &str) -> String { format!("Couldn't make the page: {reason}.") }
pub fn cant_save(reason: &str) -> String { format!("Couldn't save to that folder: {reason}.") }
pub fn not_opened(path: &str) -> String { format!("The page is saved at {path} but no program opened it. Open it from there to print.") }
pub const FILTER_PAGE: &str = "Web page";
pub const TITLE: &str = "Print or save";
