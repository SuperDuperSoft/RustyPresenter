//! User-facing strings of the Add dialog and the song editor (Plan 3 Tasks 16–17), copied from `web/operator.js`.
//! Declared from `src/ui/dialogs/mod.rs` with a `#[path]` line until a shared `src/strings/mod.rs` exists.
use crate::{tr, trn};

/// `searchList()` (`operator.js:399-410`).
pub fn nothing_matches() -> String { tr!("Nothing matches.") }
pub fn nothing_yet() -> String { tr!("Nothing here yet.") }

/// The Song tab's search placeholder (`operator.js:425`).
pub fn search_songs(n: usize) -> String { trn!("Search {n} song…", "Search {n} songs…", n) }
/// The Media tab's search placeholder (`operator.js:470`).
pub fn search_files(n: usize) -> String { trn!("Search {n} file…", "Search {n} files…", n) }
/// The Slides tab's search placeholder (`operator.js:497`).
pub fn search_slides() -> String { tr!("Search slides…") }
pub fn install_libreoffice() -> String { tr!("To import PowerPoint or Keynote files, install LibreOffice (free). PDF files work without it.") }
pub fn copied() -> String { tr!("Copied") }

/// The end of an EasyWorship import (spec §9.3): how many came in, and every song that did not, with why.
pub fn library_report(report: &crate::app::LibraryImport) -> String {
    const SHOWN: usize = 10;
    let more = |n: usize| if n > SHOWN { tr!("…and {n} more.", n = n - SHOWN) } else { String::new() };
    let mut text = trn!("Imported {n} song.", "Imported {n} songs.", report.added);
    let duplicates = report.duplicates.len();
    if duplicates > 0 {
        let titles = report.duplicates[..duplicates.min(SHOWN)].join(", ");
        text += "\n\n";
        text += &trn!("{n} already in the library: {titles}", "{n} already in the library: {titles}", duplicates, titles = titles);
        if duplicates > SHOWN { text += &format!(" {}", more(duplicates)) }
    }
    if !report.skipped.is_empty() {
        text += "\n\n";
        text += &tr!("Not imported:");
        for (title, why) in report.skipped.iter().take(SHOWN) { text += &format!("\n{title}: {why}") }
        if report.skipped.len() > SHOWN { text += &format!("\n{}", more(report.skipped.len())) }
    }
    text
}

/// Where EasyWorship keeps its libraries unless someone moved them. The folder picker starts here.
pub const EASYWORSHIP_FOLDER: &str = r"C:\Users\Public\Documents\Softouch\Easyworship";
pub fn reading_easyworship() -> String { tr!("Reading the EasyWorship library…") }
pub fn libreoffice_found() -> String { tr!("LibreOffice found!") }
pub fn libreoffice_not_found() -> String { tr!("Not found yet — finish installing, then click Check again.") }

/// A song row's subtitle part (`operator.js:421`).
pub fn ccli(n: &str) -> String { format!("CCLI #{n}") }

/// `addScripture()`'s toast (`operator.js:433`).
pub fn bible_added(label: &str) -> String { tr!("{passage} added", passage = label) }

/// `addText()` (`operator.js:507`).
pub fn give_it_a_title() -> String { tr!("Give it a title.") }

/// The Media tab (`operator.js:458-467`).
pub fn ready() -> String { tr!("Ready") }
pub fn failed() -> String { tr!("Failed") }
pub fn cant_be_played() -> String { tr!("This file can't be played.") }
pub fn checking_again() -> String { tr!("Checking the file again…") }
/// The trash button's tooltip (`operator.js:484`).
pub fn delete_from_library_tip() -> String { tr!("Delete from library") }
pub fn delete_from_library(name: &str) -> String { tr!("Delete “{name}” from the library?", name = name) }
/// A media row's kind for a video or audio file while libmpv is missing (native spec §5).
pub fn player_missing() -> String { tr!("The video player is missing — reinstall RustyPresenter") }

/// The Slides tab (`operator.js:494-496`).
pub fn slide_count(n: usize) -> String { trn!("{n} slide", "{n} slides", n) }
pub fn delete_deck(title: &str) -> String { tr!("Delete “{name}”?", name = title) }

/// The question's buttons. The web's `confirm()` has OK and Cancel.
pub fn ok() -> String { tr!("OK") }
pub fn cancel() -> String { tr!("Cancel") }

/// The song editor (`operator.js:689-708`).
pub fn song_saved() -> String { tr!("Song saved") }
/// New: an Order name that matches no section (the core leaves it out without a word).
pub fn not_in_lyrics(names: &[String]) -> String { tr!("Not in the words: {sections}", sections = names.join(", ")) }
