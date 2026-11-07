//! User-facing strings of Settings (Plan 3 Tasks 20-21) built in Rust, copied from `web/operator.js:877-1007`.
//! Declared from `src/ui/dialogs/settings.rs` with a `#[path]` line until a shared `src/strings/mod.rs` exists.
use crate::{tr, trn};

pub fn saved() -> String { tr!("Settings saved") }

/// The 20 fonts of the Look tab (`operator.js:891`).
pub const FONTS: [&str; 20] = [
    "Segoe UI", "Segoe UI Semibold", "Segoe UI Black", "Bahnschrift", "Arial", "Arial Black", "Calibri", "Candara", "Corbel",
    "Franklin Gothic Medium", "Georgia", "Tahoma", "Trebuchet MS", "Verdana", "Gill Sans MT", "Century Gothic", "Montserrat",
    "Open Sans", "Lato", "Roboto",
];

/// The Look preview's sample slide (`operator.js:889`): a hymn's words, so they stay as written.
pub const SAMPLE_TEXT: &str = "Amazing grace how sweet the sound\nThat saved a wretch like me";
pub const SAMPLE_FOOTER: &str = "Amazing Grace · John Newton · Public Domain";

/// Native spec §4 (Product decision 8): the chosen font is not on this computer.
pub fn font_missing(font: &str) -> String { tr!("“{font}” isn't installed on this computer, so slides use Noto Sans.", font = font) }

// ---- Bible (`operator.js:922-973`) ----
pub fn chip(status: &str) -> (&'static str, String) {
    match status {
        "offline" => ("ok", tr!("Ready — offline")),
        "online" => ("ok", tr!("Ready — online")),
        "needs_key" => ("warn", tr!("Needs a free key")),
        _ => ("warn", tr!("No source yet")),
    }
}
pub fn connect_below(labels: &[String]) -> String {
    let or = format!(" {} ", tr!("or"));
    tr!("Connect {sources} below", sources = labels.join(&or))
}
pub fn no_online_source() -> String { tr!("No online source offers this one. Import a licensed file, or paste verses when you add them.") }
pub fn provider_for(translations: &[String]) -> String { tr!("For {bibles}", bibles = translations.join(", ")) }
pub fn key_saved(label: &str, connected: bool) -> String {
    if connected { tr!("{source} connected", source = label) } else { tr!("{source} disconnected", source = label) }
}
pub fn link_copied() -> String { tr!("Link copied — paste it into your browser") }
/// `toast(\`Added ${j.abbreviation} (${j.verses.toLocaleString()} verses)\`)`, with en-US digit groups.
pub fn bible_added(abbr: &str, verses: u64) -> String {
    trn!("Added {bible} ({count} verses)", "Added {bible} ({count} verses)", verses, bible = abbr, count = grouped(verses))
}
pub fn import_failed() -> String { tr!("Import failed.") }
pub fn bible_removed(translation: &str) -> String { tr!("{bible} removed", bible = translation) }

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
pub fn copied() -> String { tr!("Copied") }
/// `mpv` is the `mpv-version` property ("mpv 0.39.0"), which may be empty.
pub fn video_player(mpv: &str) -> String {
    match mpv.strip_prefix("mpv ").unwrap_or(mpv) {
        "" => tr!("Video player: libmpv (LGPL build)"),
        v => tr!("Video player: libmpv {version} (LGPL build)", version = v),
    }
}
pub fn drawing_gpu() -> String { tr!("Video drawing: GPU (OpenGL)") }
pub fn drawing_software() -> String { tr!("Video drawing: Software") }
/// Slint 1.18 has no clipboard-write API; `arboard` failed.
pub fn copy_failed(e: &str) -> String { tr!("Could not copy: {error}", error = e) }

// ---- Backups (`operator.js:984-989`) ----
/// A folder, so not translated.
pub fn backups_folder(data_folder: &str) -> String { format!("{data_folder}\\backups") }
pub fn backup_made() -> String { tr!("Backup made") }
pub fn restore_confirm() -> String { tr!("Restore this backup? Your current songs and services are replaced (a copy of them is kept).") }
pub fn backup_restored() -> String { tr!("Backup restored") }
pub fn backup_counts(songs: usize, services: usize, files: usize) -> String {
    tr!("{songs} songs · {services} services · {files} files", songs = songs, services = services, files = files)
}

// ---- Help (`operator.js:993`, Product decisions 4 and 7) ----
pub fn key_next() -> String { tr!("→  ↓  Space  Page Down") }
pub fn key_back() -> String { tr!("←  ↑  Page Up") }
pub fn key_enter() -> String { tr!("Enter") }
pub fn key_ctrl_b() -> String { tr!("Ctrl + B") }
pub fn key_alt_digits() -> String { tr!("Alt + 1–5") }
pub fn key_ctrl_v() -> String { tr!("Ctrl + V") }
/// The real keys, so not translated.
pub const KEY_SECTIONS: &str = "1–9  C  P  B  T  I  E  L  V";
pub fn do_sections() -> String { tr!("Jump the live song to Verse 1–9, Chorus, Pre-chorus, Bridge, Tag, Intro, Ending, Interlude or the next Verse") }
pub fn do_paste_link() -> String { tr!("Add a YouTube link you copied") }
pub fn key_esc() -> String { tr!("Esc") }
pub fn do_next() -> String { tr!("Next") }
pub fn do_back() -> String { tr!("Back") }
pub fn do_enter() -> String { tr!("Show the selected item") }
pub fn do_bible() -> String { tr!("Bible box") }
pub fn do_clear_all() -> String { tr!("Clear all") }
pub fn do_clear_text() -> String { tr!("Clear text") }
pub fn do_clear_media() -> String { tr!("Clear media & background") }
pub fn do_stop_audio() -> String { tr!("Stop audio") }
pub fn do_logo() -> String { tr!("Logo") }
pub fn do_translation() -> String { tr!("Pick a Bible chip (in the Bible box)") }
pub fn do_esc() -> String { tr!("Leave a text box") }

// ---- About (Task 21, native spec §7) ----
pub fn about_version(version: &str) -> String { format!("RustyPresenter {version}") }
pub fn video_gpu() -> String { tr!("Video: GPU (OpenGL)") }
pub fn video_software() -> String { tr!("Video: software drawing") }
pub fn video_missing() -> String { tr!("Video player missing") }
/// `mpv` is the `mpv-version` property, such as "mpv 0.41.0"; empty if it could not be read, and then nothing is
/// said about the build either.
pub fn mpv_line(mpv: &str, lgpl: bool, api: (u32, u32), path: &str) -> String {
    if mpv.is_empty() { return tr!("libmpv (client API {major}.{minor}) — {path}", major = api.0, minor = api.1, path = path); }
    let version = mpv.strip_prefix("mpv ").unwrap_or(mpv);
    let build = if lgpl { tr!("LGPL build") } else { tr!("not an LGPL build") };
    tr!("libmpv {version} ({build}) · client API {major}.{minor} — {path}", version = version, build = build, major = api.0, minor = api.1, path = path)
}
pub fn ffmpeg_line(ffmpeg: &str) -> String { if ffmpeg.is_empty() { String::new() } else { format!("FFmpeg {ffmpeg}") } }
pub fn pdfium_found() -> String { tr!("found") }
pub fn pdfium_missing() -> String { tr!("missing") }
pub fn libreoffice_snap() -> String { tr!("Snap — not usable") }
pub fn libreoffice_none() -> String { tr!("not installed") }
pub fn pdfium_line(state: &str) -> String { format!("pdfium: {state}") }
pub fn libreoffice_line(state: &str) -> String { format!("LibreOffice: {state}") }
pub fn powerpoint_line(found: bool) -> String { format!("PowerPoint: {}", if found { tr!("found") } else { tr!("not found") }) }
/// Where the LGPL sources of the shipped libmpv/FFmpeg build are offered (native spec §13).
pub const MEDIA_SOURCE_URL: &str = "https://github.com/SuperDuperSoft/RustyPresenter/releases";
pub fn media_licence() -> String {
    tr!("Video playback uses libmpv (LGPL-2.1+) and FFmpeg libraries under the LGPLv2.1 — source: {url}", url = MEDIA_SOURCE_URL)
}

// ---- Language (I18N spec D7) ----
pub fn use_computer_language(name: &str) -> String { tr!("Use the computer's language ({name})", name = name) }
/// A machine-drafted translation no native speaker has checked yet (spec D9).
pub fn needs_review(name: &str) -> String { tr!("{name} (needs review)", name = name) }
pub fn language_sub(english_name: &str, percent: u8) -> String {
    tr!("{english_name} · {percent}% translated", english_name = english_name, percent = percent)
}
pub fn no_font_for_language() -> String { tr!("Your computer may not have a font for this language.") }
/// "Help translate RustyPresenter" copies this.
pub const TRANSLATING_URL: &str = "https://github.com/SuperDuperSoft/RustyPresenter/blob/main/docs/TRANSLATING.md";

#[cfg(test)]
mod tests {
    #[test]
    fn verses_are_grouped_like_to_locale_string() {
        assert_eq!(super::bible_added("HCSB", 31102), "Added HCSB (31,102 verses)");
        assert_eq!(super::bible_added("X", 2), "Added X (2 verses)");
        assert_eq!(super::bible_added("X", 1_234_567), "Added X (1,234,567 verses)");
    }
}

pub fn audio_default() -> String { tr!("This computer's default sound output") }
pub fn audio_saved_gone() -> String { tr!("The saved sound output (not connected)") }
