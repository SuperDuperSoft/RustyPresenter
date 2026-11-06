//! User-facing strings of Settings (Plan 3 Tasks 20-21) built in Rust, copied from `web/operator.js:877-1007`.
//! Declared from `src/ui/dialogs/settings.rs` with a `#[path]` line until a shared `src/strings/mod.rs` exists.

pub const SAVED: &str = "Settings saved";

/// The 20 fonts of the Look tab (`operator.js:891`).
pub const FONTS: [&str; 20] = [
    "Segoe UI", "Segoe UI Semibold", "Segoe UI Black", "Bahnschrift", "Arial", "Arial Black", "Calibri", "Candara", "Corbel",
    "Franklin Gothic Medium", "Georgia", "Tahoma", "Trebuchet MS", "Verdana", "Gill Sans MT", "Century Gothic", "Montserrat",
    "Open Sans", "Lato", "Roboto",
];

/// The Look preview's sample slide (`operator.js:889`).
pub const SAMPLE_TEXT: &str = "Amazing grace how sweet the sound\nThat saved a wretch like me";
pub const SAMPLE_FOOTER: &str = "Amazing Grace · John Newton · Public Domain";

/// Native spec §4 (Product decision 8): the chosen font is not on this computer.
pub fn font_missing(font: &str) -> String { format!("“{font}” isn't installed on this computer, so slides use Noto Sans.") }

// ---- Bible (`operator.js:922-973`) ----
pub fn chip(status: &str) -> (&'static str, &'static str) {
    match status {
        "offline" => ("ok", "Ready — offline"),
        "online" => ("ok", "Ready — online"),
        "needs_key" => ("warn", "Needs a free key"),
        _ => ("warn", "No source yet"),
    }
}
pub fn connect_below(labels: &[String]) -> String { format!("Connect {} below", labels.join(" or ")) }
pub const NO_ONLINE_SOURCE: &str = "No online source offers this one. Import a licensed file, or paste verses when you add them.";
pub fn provider_for(translations: &[String]) -> String { format!("For {}", translations.join(", ")) }
pub fn key_saved(label: &str, connected: bool) -> String {
    if connected { format!("{label} connected") } else { format!("{label} disconnected") }
}
pub const LINK_COPIED: &str = "Link copied — paste it into your browser";
/// `toast(\`Added ${j.abbreviation} (${j.verses.toLocaleString()} verses)\`)`, with en-US digit groups.
pub fn bible_added(abbr: &str, verses: u64) -> String { format!("Added {abbr} ({} verses)", grouped(verses)) }
pub const IMPORT_FAILED: &str = "Import failed.";
pub fn bible_removed(translation: &str) -> String { format!("{translation} removed") }

fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 { out.push(','); }
        out.push(c);
    }
    out
}

// ---- Video ----
pub const COPIED: &str = "Copied";
/// `mpv` is the `mpv-version` property ("mpv 0.39.0"), which may be empty.
pub fn video_player(mpv: &str) -> String {
    match mpv.strip_prefix("mpv ").unwrap_or(mpv) {
        "" => "Video player: libmpv (LGPL build)".into(),
        v => format!("Video player: libmpv {v} (LGPL build)"),
    }
}
pub const DRAWING_GPU: &str = "Video drawing: GPU (OpenGL)";
pub const DRAWING_SOFTWARE: &str = "Video drawing: Software";
/// Slint 1.18 has no clipboard-write API; `arboard` failed.
pub fn copy_failed(e: &str) -> String { format!("Could not copy: {e}") }

// ---- Backups (`operator.js:984-989`) ----
pub fn backups_folder(data_folder: &str) -> String { format!("{data_folder}\\backups") }
pub const BACKUP_MADE: &str = "Backup made";
pub const RESTORE_CONFIRM: &str = "Restore this backup? Your current songs and services are replaced (a copy of them is kept).";
pub const BACKUP_RESTORED: &str = "Backup restored";
pub fn backup_counts(songs: usize, services: usize, files: usize) -> String { format!("{songs} songs · {services} services · {files} files") }

// ---- Help (`operator.js:993`, Product decisions 4 and 7) ----
pub const KEY_NEXT: &str = "→  ↓  Space  Page Down";
pub const KEY_BACK: &str = "←  ↑  Page Up";
pub const KEY_ENTER: &str = "Enter";
pub const KEY_CTRL_B: &str = "Ctrl + B";
pub const KEY_ALT_DIGITS: &str = "Alt + 1–5";
pub const KEY_CTRL_V: &str = "Ctrl + V";
pub const KEY_SECTIONS: &str = "1–9  C  P  B  T  I  E  L  V";
pub const DO_SECTIONS: &str = "Jump the live song to Verse 1–9, Chorus, Pre-chorus, Bridge, Tag, Intro, Ending, Interlude or the next Verse";
pub const DO_PASTE_LINK: &str = "Add a YouTube link you copied";
pub const KEY_ESC: &str = "Esc";
pub const DO_NEXT: &str = "Next";
pub const DO_BACK: &str = "Back";
pub const DO_ENTER: &str = "Show the selected item";
pub const DO_BIBLE: &str = "Bible box";
pub const DO_CLEAR_ALL: &str = "Clear all";
pub const DO_CLEAR_TEXT: &str = "Clear text";
pub const DO_CLEAR_MEDIA: &str = "Clear media & background";
pub const DO_STOP_AUDIO: &str = "Stop audio";
pub const DO_LOGO: &str = "Logo";
pub const DO_TRANSLATION: &str = "Pick a Bible chip (in the Bible box)";
pub const DO_ESC: &str = "Leave a text box";

// ---- About (Task 21, native spec §7) ----
pub fn about_version(version: &str) -> String { format!("RustyPresenter {version}") }
pub const VIDEO_GPU: &str = "Video: GPU (OpenGL)";
pub const VIDEO_SOFTWARE: &str = "Video: software drawing";
pub const VIDEO_MISSING: &str = "Video player missing";
/// `mpv` is the `mpv-version` property, such as "mpv 0.41.0"; empty if it could not be read, and then nothing is
/// said about the build either.
pub fn mpv_line(mpv: &str, lgpl: bool, api: (u32, u32), path: &str) -> String {
    if mpv.is_empty() { return format!("libmpv (client API {}.{}) — {path}", api.0, api.1); }
    let version = mpv.strip_prefix("mpv ").unwrap_or(mpv);
    let build = if lgpl { "LGPL build" } else { "not an LGPL build" };
    format!("libmpv {version} ({build}) · client API {}.{} — {path}", api.0, api.1)
}
pub fn ffmpeg_line(ffmpeg: &str) -> String { if ffmpeg.is_empty() { String::new() } else { format!("FFmpeg {ffmpeg}") } }
pub const PDFIUM_FOUND: &str = "found";
pub const PDFIUM_MISSING: &str = "missing";
pub const LIBREOFFICE_SNAP: &str = "Snap — not usable";
pub const LIBREOFFICE_NONE: &str = "not installed";
pub fn pdfium_line(state: &str) -> String { format!("pdfium: {state}") }
pub fn libreoffice_line(state: &str) -> String { format!("LibreOffice: {state}") }
pub fn powerpoint_line(found: bool) -> String { format!("PowerPoint: {}", if found { "found" } else { "not found" }) }
/// Where the LGPL sources of the shipped libmpv/FFmpeg build are offered (native spec §13).
pub const MEDIA_SOURCE_URL: &str = "https://github.com/SuperDuperSoft/RustyPresenter/releases";
pub fn media_licence() -> String {
    format!("Video playback uses libmpv (LGPL-2.1+) and FFmpeg libraries under the LGPLv2.1 — source: {MEDIA_SOURCE_URL}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn verses_are_grouped_like_to_locale_string() {
        assert_eq!(super::bible_added("HCSB", 31102), "Added HCSB (31,102 verses)");
        assert_eq!(super::bible_added("X", 2), "Added X (2 verses)");
        assert_eq!(super::bible_added("X", 1_234_567), "Added X (1,234,567 verses)");
    }
}

pub const AUDIO_DEFAULT: &str = "This computer's default sound output";
pub const AUDIO_SAVED_GONE: &str = "The saved sound output (not connected)";
