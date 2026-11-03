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
/// The slides column's title with nothing selected (`operator.js:162`).
pub const PICK_AN_ITEM: &str = "Pick an item on the left";
pub const WELCOME: &str = "Welcome";

/// The core call never finished (its task panicked or the runtime is shutting down). The web's generic
/// failure text (`operator.js:48`).
pub const ACTION_STOPPED: &str = "Something went wrong.";

/// The room pill's tooltip (`operator.js:104`).
pub const ROOM_SHOWING: &str = "Room screen is showing";
pub const ROOM_STARTING: &str = "Room screen is starting…";
pub const ROOM_OFF: &str = "Room screen is not open — click Screens";

pub fn open_failed(e: impl std::fmt::Display) -> String { format!("Could not open the operator window: {e}") }

/// The title a deck from pictures gets when its name prompt is cancelled or left blank (`operator.js:591`).
pub const DEFAULT_DECK_TITLE: &str = "Slides";

/// The confirm for several pictures dropped at once, and its two buttons (the native wording of OK/Cancel, `operator.js:610`).
pub fn pictures_to_deck(n: usize) -> String { format!("Make {n} pictures into one set of slides?") }
pub const ONE_SLIDE_SET: &str = "One slide set";
pub const SEPARATE_PICTURES: &str = "Separate pictures";

/// File picker filter names. The web `accept` lists have none, so these are new.
pub const FILTER_MEDIA: &str = "Videos, pictures and music";
pub const FILTER_POWERPOINT: &str = "PowerPoint";
pub const FILTER_LYRICS: &str = "SongSelect lyric files";
pub const FILTER_PICTURES: &str = "Pictures";
pub const FILTER_PICTURES_AND_VIDEOS: &str = "Pictures and videos";
pub const FILTER_BIBLES: &str = "Bible files";

/// The toast after the Bible box adds a passage (`operator.js:336`).
pub fn bible_added(label: &str) -> String { format!("{label} added — press Enter to show it") }

/// Adding files (`uploadFiles`, `imagesToDeck` and `addItem` in `operator.js:369-375, 536-598`).
pub const ADDED_TO_SERVICE: &str = "Added to the service";
pub fn uploading(name: &str) -> String { format!("Uploading {name}") }
pub fn uploading_pictures(n: usize) -> String { format!("Uploading {n} picture{}", if n > 1 { "s" } else { "" }) }
pub fn media_added(name: &str) -> String { format!("{name} added") }
pub fn media_added_converting(name: &str) -> String { format!("{name} added — converting so it plays smoothly…") }
pub fn song_imported(title: &str) -> String { format!("Imported “{title}”") }
pub fn song_duplicate(title: &str) -> String { format!("“{title}” was already in the library — added a second copy.") }
pub fn deck_ready(title: &str) -> String { format!("“{title}” is ready") }
pub fn file_failed(name: &str, err: &str) -> String { format!("{name}: {err}") }
pub const NAME_THESE_SLIDES: &str = "Name these slides";
