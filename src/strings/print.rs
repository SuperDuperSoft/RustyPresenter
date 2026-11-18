//! User-facing strings of printing the service (FEAT-6b). Declared from `src/print/mod.rs` with a `#[path]` line.
use crate::{tr, trn};

/// CCLI's licence wording stays in English, as the licence asks.
pub fn ccli_licence(n: &str) -> String { format!("CCLI License # {n}") }
pub fn ccli_song(n: &str) -> String { format!("CCLI Song # {n}") }
pub fn copyright(c: &str) -> String { if c.trim_start().starts_with('©') { c.trim().into() } else { format!("© {}", c.trim()) } }
pub fn order_line(labels: &str) -> String { tr!("Order: {sections}", sections = labels) }
pub fn slides(n: usize) -> String { trn!("{n} slide", "{n} slides", n) }
pub fn blank_title(label: &str) -> String { tr!("Blank — {label}", label = label) }
pub fn not_chosen() -> String { tr!("(not chosen yet)") }
pub fn no_lyrics() -> String { tr!("(no words)") }
pub fn no_verse_text() -> String { tr!("(the verse text isn't saved with this service)") }
pub fn missing_song() -> String { tr!("Missing song") }
pub fn missing_media() -> String { tr!("Missing media") }
pub fn missing_slides() -> String { tr!("Missing slides") }
pub fn video() -> String { tr!("Video") }
pub fn picture() -> String { tr!("Picture") }
pub fn audio() -> String { tr!("Audio") }
pub fn loops() -> String { tr!(" (loops)") }
pub fn order_heading() -> String { tr!("Service order") }
pub fn lyrics_heading() -> String { tr!("Words") }
pub fn nothing_to_print() -> String { tr!("No songs or Bible passages in this service.") }
pub fn empty_service() -> String { tr!("This service is empty.") }
pub fn printed(date: &str) -> String { tr!("Printed from RustyPresenter · {date}", date = date) }
/// The page's file name, without ".html". File names stay in English.
pub fn file_stem(name: &str, what: crate::model::PrintWhat) -> String {
    use crate::model::PrintWhat::*;
    format!("{name} – {}", match what { Order => "order", Lyrics => "lyrics", Both => "order and lyrics" })
}

// ---- the Print dialog and its toasts ----
pub fn opened() -> String { tr!("Opened in your web browser — use Print there (you can also save as PDF from the print window)") }
pub fn saved(file: &str) -> String { tr!("Saved {file}", file = file) }
pub fn cant_make(reason: &str) -> String { tr!("Couldn't make the page: {reason}.", reason = reason) }
pub fn cant_save(reason: &str) -> String { tr!("Couldn't save to that folder: {reason}.", reason = reason) }
pub fn not_opened(path: &str) -> String { tr!("The page is saved at {path} but no program opened it. Open it from there to print.", path = path) }
pub fn filter_page() -> String { tr!("Web page") }
pub fn title() -> String { tr!("Print or save") }
