//! User-facing strings the native dialogs build in Rust. The text is copied from `web/operator.js` unless a line says
//! otherwise; tests compare against it. Declared from `src/ui/dialogs/mod.rs` with a `#[path]` line, as `operator.rs` is.

// Add dialog.
pub fn ccli_number(n: &str) -> String { format!("CCLI #{n}") }
pub const MEDIA_READY: &str = "Ready";
pub fn media_converting(percent: u8) -> String { format!("Converting {percent}%") }
pub const MEDIA_NEEDS_CONVERTER: &str = "Needs converter";
pub const MEDIA_FAILED: &str = "Failed";

// Song editor. The core drops order names that match no section, so this warning is new (Plan 3 Task 17).
pub fn not_in_lyrics(names: &[String]) -> String { format!("Not in the lyrics: {}", names.join(", ")) }

// Background picker.
pub const BG_THEME_DEFAULT: &str = "Theme default";
pub const BG_PICK_ANY_COLOR: &str = "Pick any color";
pub const BG_UPLOAD: &str = "+ Upload…";
pub fn bg_video(name: &str) -> String { format!("▶ {name}") }

// Screens.
pub const SCREEN_SHOWING: &str = "Showing";
pub const SCREEN_STARTING: &str = "Starting…";
pub const SCREEN_OFF: &str = "Off";

// Settings → Backups.
pub fn backup_contents(songs: usize, services: usize, files: usize) -> String {
    format!("{songs} songs · {services} services · {files} files")
}

// Settings → Bible.
pub const BIBLE_OFFLINE: &str = "Ready — offline";
pub const BIBLE_ONLINE: &str = "Ready — online";
pub const BIBLE_NEEDS_KEY: &str = "Needs a free key";
pub const BIBLE_NO_SOURCE: &str = "No source yet";
pub const BIBLE_NO_SOURCE_HINT: &str = "No online source offers this one. Import a licensed file, or paste verses when you add them.";
pub fn bible_connect(providers: &[&str]) -> String { format!("Connect {} below", providers.join(" or ")) }

// Settings → Help. The rows are the web's, with ↓, ↑ and Alt + 1–6 added (Plan 3 product decisions 4 and 7). Keys in a
// row are separated by two spaces, as the web writes them. `settings::tests` checks this against `keys::route`.
pub const HELP_KEYS: &[(&str, &str)] = &[
    ("→  ↓  Space  Page Down", "Next"),
    ("←  ↑  Page Up", "Back"),
    ("Enter", "Show the selected item"),
    ("Ctrl + B", "Bible box"),
    ("F1", "Clear all"),
    ("F2", "Clear text"),
    ("F3", "Clear media & background"),
    ("F4", "Stop audio"),
    ("F5", "Logo"),
    ("Esc", "Leave a text box"),
    ("Alt + 1–6", "Bible translation (in the Bible box)"),
];
pub const HELP_TIP: &str = "Tip: clicking any slide puts it on the screen right away. The red outline shows what the room sees.";
