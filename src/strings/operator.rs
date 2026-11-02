//! User-facing strings the native operator builds in Rust (Plan 3's string file, FEAT-4 §7 convention).
//! The text is copied from `web/operator.js`; tests compare against it.
//! Declared from `src/ui/operator/mod.rs` with a `#[path]` line until a shared `src/strings/mod.rs` exists.

/// `KIND_NAME` in `operator.js:40`, keyed by the kind `slides::build_item` returns.
pub fn kind_name(kind: &str) -> &'static str {
    match kind {
        "song" => "Song",
        "scripture" => "Scripture",
        "video" => "Video",
        "image" => "Picture",
        "audio" => "Audio",
        "deck" => "Slides",
        "text" => "Announcement",
        _ => "",
    }
}

pub fn converting(kind: &str, percent: u8) -> String { format!("{kind} · converting {percent}%") }
pub fn needs_converter(kind: &str) -> String { format!("{kind} · may not play — needs converter") }
pub fn conversion_failed(kind: &str) -> String { format!("{kind} · conversion failed") }
pub const LOOPS: &str = " · loops";
/// Native spec §5: a video or audio item when libmpv could not be loaded.
pub fn player_missing(kind: &str) -> String { format!("{kind} · The video player is missing — reinstall RustyPresenter") }
pub fn slide_count(kind: &str, n: usize) -> String { format!("{kind} · {n} slide{}", if n == 1 { "" } else { "s" }) }

pub fn items(n: usize) -> String { format!("{n} item{}", if n > 1 { "s" } else { "" }) }
pub fn phones(n: usize) -> String { format!("{n} phone{}", if n > 1 { "s" } else { "" }) }

pub const LOGO: &str = "Logo";
pub const CLEAR: &str = "Clear";
pub fn audio_title(title: &str) -> String { format!("♪ {title}") }
pub const SLIDE_PICTURE: &str = "Slide picture";
pub const VIDEO: &str = "Video";
pub const PICTURE: &str = "Picture";
pub const AUDIO: &str = "Audio";
pub const END_OF_SERVICE: &str = "End of service";
pub const START_OF_SERVICE: &str = "Start of service";
pub const NOTHING: &str = "—";
pub const SERVICE: &str = "Service";

/// The core call never finished (its task panicked or the runtime is shutting down).
pub const ACTION_STOPPED: &str = "That did not finish. Try again.";

/// The title a deck from pictures gets when its name prompt is cancelled or left blank (`operator.js:591`).
pub const DEFAULT_DECK_TITLE: &str = "Slides";
