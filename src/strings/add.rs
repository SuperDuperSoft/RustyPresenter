//! User-facing strings of the Add dialog and the song editor (Plan 3 Tasks 16–17), copied from `web/operator.js`.
//! Declared from `src/ui/dialogs/mod.rs` with a `#[path]` line until a shared `src/strings/mod.rs` exists.

/// `searchList()` (`operator.js:399-410`).
pub const NOTHING_MATCHES: &str = "Nothing matches.";
pub const NOTHING_YET: &str = "Nothing here yet.";

fn plural(n: usize, one: &str, many: &str) -> String { format!("{n} {}", if n == 1 { one } else { many }) }

/// The Song tab's search placeholder (`operator.js:425`).
pub fn search_songs(n: usize) -> String { format!("Search {}…", plural(n, "song", "songs")) }
/// The Media tab's search placeholder (`operator.js:470`).
pub fn search_files(n: usize) -> String { format!("Search {}…", plural(n, "file", "files")) }
/// The Slides tab's search placeholder (`operator.js:497`).
pub const SEARCH_SLIDES: &str = "Search slides…";

/// A song row's subtitle part (`operator.js:421`).
pub fn ccli(n: &str) -> String { format!("CCLI #{n}") }

/// `addScripture()`'s toast (`operator.js:433`).
pub fn bible_added(label: &str) -> String { format!("{label} added") }

/// `addText()` (`operator.js:507`).
pub const GIVE_IT_A_TITLE: &str = "Give it a title.";

/// The Media tab (`operator.js:458-467`).
pub const CONVERTING: &str = "Converting…";
pub const READY: &str = "Ready";
pub fn converting_percent(p: u8) -> String { format!("Converting {p}%") }
pub const NEEDS_CONVERTER: &str = "Needs converter";
pub const FAILED: &str = "Failed";
/// The trash button's tooltip (`operator.js:484`).
pub const DELETE_FROM_LIBRARY: &str = "Delete from library";
pub fn delete_from_library(name: &str) -> String { format!("Delete “{name}” from the library?") }
/// A media row's kind for a video or audio file while libmpv is missing (native spec §5).
pub const PLAYER_MISSING: &str = "The video player is missing — reinstall RustyPresenter";

/// The Slides tab (`operator.js:494-496`).
pub fn slide_count(n: usize) -> String { plural(n, "slide", "slides") }
pub fn delete_deck(title: &str) -> String { format!("Delete “{title}”?") }
/// The Slides tab without PowerPoint or LibreOffice (Plan 4 Task 6c).
pub const NEED_LIBREOFFICE: &str = "To import PowerPoint or Keynote files, install LibreOffice (free). PDF files work without it.";
pub const LIBREOFFICE_FOUND: &str = "LibreOffice found!";
pub const NOT_FOUND_YET: &str = "Not found yet — finish installing, then click Check again.";

/// The question's buttons. The web's `confirm()` has OK and Cancel.
pub const OK: &str = "OK";
pub const CANCEL: &str = "Cancel";

/// The song editor (`operator.js:689-708`).
pub const SONG_SAVED: &str = "Song saved";
/// New: an Order name that matches no section (the core leaves it out without a word).
pub fn not_in_lyrics(names: &[String]) -> String { format!("Not in the lyrics: {}", names.join(", ")) }
