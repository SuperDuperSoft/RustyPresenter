//! User-facing strings the native operator builds in Rust (Plan 3's string file, FEAT-4 §7 convention).
//! The text is copied from `web/operator.js`; tests compare against it.
//! Declared from `src/ui/operator/mod.rs` with a `#[path]` line until a shared `src/strings/mod.rs` exists.
use crate::{tr, trn};

/// `KIND_NAME` in `operator.js:40`, keyed by the kind `slides::build_item` returns.
pub fn kind_name(kind: &str) -> String {
    match kind {
        "song" => tr!("Song"),
        "scripture" => tr!("Scripture"),
        "video" => tr!("Video"),
        "image" => tr!("Picture"),
        "audio" => tr!("Audio"),
        "deck" => tr!("Slides"),
        "text" => tr!("Announcement"),
        _ => String::new(),
    }
}

pub fn cant_be_played(kind: &str) -> String { tr!("{kind} · can't be played", kind = kind) }
pub fn loops() -> String { tr!(" · loops") }
/// Native spec §5: a video or audio item when libmpv could not be loaded.
pub fn player_missing(kind: &str) -> String { tr!("{kind} · The video player is missing — reinstall RustyPresenter", kind = kind) }
pub fn slide_count(kind: &str, n: usize) -> String { trn!("{kind} · {n} slide", "{kind} · {n} slides", n, kind = kind) }

pub fn items(n: usize) -> String { trn!("{n} item", "{n} items", n) }
pub fn phones(n: usize) -> String { trn!("{n} phone", "{n} phones", n) }

pub fn logo() -> String { tr!("Logo") }
pub fn clear() -> String { tr!("Clear") }
pub fn audio_title(title: &str) -> String { format!("♪ {title}") }
pub fn slide_picture() -> String { tr!("Slide picture") }
pub fn video() -> String { tr!("Video") }
pub fn picture() -> String { tr!("Picture") }
pub fn audio() -> String { tr!("Audio") }
pub fn end_of_service() -> String { tr!("End of service") }
pub fn start_of_service() -> String { tr!("Start of service") }
pub const NOTHING: &str = "—";
pub fn service() -> String { tr!("Service") }
/// The slides header's passage text size when it follows Settings.
pub fn text_size_auto() -> String { tr!("Auto") }
/// A+ on a passage whose text already had to shrink to fit.
pub fn text_as_big_as_fits() -> String { tr!("The text is already as big as fits. Pick fewer verses per slide (Edit) to make it bigger.") }
/// The slides column's title with nothing selected (`operator.js:162`).
pub fn pick_an_item() -> String { tr!("Pick an item on the left") }
pub fn welcome() -> String { tr!("Welcome") }

/// The core call never finished (its task panicked or the runtime is shutting down). The web's generic
/// failure text (`operator.js:48`).
pub fn action_stopped() -> String { tr!("Something went wrong.") }

/// The room pill's tooltip (`operator.js:104`).
pub fn room_showing() -> String { tr!("Room screen is showing") }
pub fn room_starting() -> String { tr!("Room screen is starting…") }
pub fn room_off() -> String { tr!("Room screen is not open — click Screens") }

pub fn open_failed(e: impl std::fmt::Display) -> String { tr!("Could not open the operator window: {error}", error = e) }

/// The title a deck from pictures gets when its name prompt is cancelled or left blank (`operator.js:591`).
pub fn default_deck_title() -> String { tr!("Slides") }

/// The confirm for several pictures dropped at once, and its two buttons (the native wording of OK/Cancel, `operator.js:610`).
pub fn pictures_to_deck(n: usize) -> String {
    trn!("Make {n} pictures into one set of slides?", "Make {n} pictures into one set of slides?", n)
}
pub fn one_slide_set() -> String { tr!("One slide set") }
pub fn separate_pictures() -> String { tr!("Separate pictures") }

/// File picker filter names. The web `accept` lists have none, so these are new.
pub fn filter_media() -> String { tr!("Videos, pictures and music") }
pub fn filter_powerpoint() -> String { tr!("Presentations") }
pub fn filter_lyrics() -> String { tr!("SongSelect lyric files") }
pub fn filter_pictures() -> String { tr!("Pictures") }
pub fn filter_pictures_and_videos() -> String { tr!("Pictures and videos") }
pub fn filter_bibles() -> String { tr!("Bible files") }
pub fn filter_services() -> String { tr!("Service files") }

/// The toast after the Bible box adds a passage (`operator.js:336`).
pub fn bible_added(label: &str) -> String { tr!("{passage} added — press Enter to show it", passage = label) }

/// Adding files (`uploadFiles`, `imagesToDeck` and `addItem` in `operator.js:369-375, 536-598`).
pub fn added_to_service() -> String { tr!("Added to the service") }
pub fn uploading(name: &str) -> String { tr!("Uploading {name}", name = name) }
pub fn uploading_pictures(n: usize) -> String { trn!("Uploading {n} picture", "Uploading {n} pictures", n) }
pub fn media_added(name: &str) -> String { tr!("{name} added", name = name) }
pub fn song_imported(title: &str) -> String { tr!("Imported “{title}”", title = title) }
pub fn song_in_library(title: &str) -> String { tr!("“{title}” is already in the library.", title = title) }
pub fn song_updated(title: &str) -> String { tr!("Updated “{title}”", title = title) }
pub fn update_existing() -> String { tr!("Update existing") }
pub fn keep_both() -> String { tr!("Keep both") }
pub fn deck_ready(title: &str) -> String { tr!("“{title}” is ready", title = title) }
pub fn editable_slides() -> String { tr!("Editable slides") }
pub fn as_pictures() -> String { tr!("Pictures") }
pub fn file_failed(name: &str, err: &str) -> String { format!("{name}: {err}") }
pub fn name_these_slides() -> String { tr!("Name these slides") }

pub fn stage_message_ask() -> String { tr!("Message for the stage screen. Leave it empty to take it down.") }

// The Bible picker (Bible library spec §7.2).
pub fn picker_favourites() -> String { tr!("Favourites") }
pub fn picker_recent() -> String { tr!("Recent") }
/// Heads installed Bibles whose language is not known (an imported file without one).
pub fn picker_other() -> String { tr!("Other Bibles") }
/// Added to a chip when an imported file and another Bible would read alike ("RV1909 · spa · imported").
pub fn bible_imported() -> String { tr!("imported") }
pub fn picker_no_match() -> String { tr!("No Bible you have matches. Find more Bibles below.") }
pub fn favourite_added(label: &str) -> String { tr!("{bible} added to favourites", bible = label) }
pub fn favourite_removed(label: &str) -> String { tr!("{bible} removed from favourites", bible = label) }
/// The media library (`media_bin.rs`).
pub fn all_media() -> String { tr!("All media") }
pub fn media_video() -> String { tr!("Video") }
pub fn media_picture() -> String { tr!("Picture") }
pub fn media_audio() -> String { tr!("Audio") }
pub fn media_failed() -> String { tr!("Failed") }
pub fn converting_short(percent: u8) -> String { tr!("Converting {percent}%", percent = percent) }
pub fn bin_hint_none() -> String { tr!("Click a thumbnail to choose it.") }
pub fn bin_hint_use() -> String { tr!("Double-click a thumbnail to use it as the selected item’s background.") }
pub fn bin_hint_audio() -> String { tr!("Add it to the service to play it.") }
pub fn bin_hint_not_ready() -> String { tr!("This file isn’t ready to play yet.") }
pub fn pick_item_first() -> String { tr!("Pick a service item first, then choose its background.") }
pub fn background_set(name: &str) -> String { tr!("Background set to {name}", name = name) }
pub fn move_to_folder() -> String { tr!("Folder name (leave empty for none)") }
pub fn new_folder() -> String { tr!("Name for the new folder") }
pub fn delete_folder(name: &str) -> String { tr!("Delete the folder “{name}”? Its files stay in the library.", name = name) }
pub fn delete_folder_button() -> String { tr!("Delete folder") }
pub fn cancel_action() -> String { tr!("Cancel") }

/// N29: what the stage-only bar names when a slide has no label of its own.
pub fn stage_only_slide(n: usize) -> String { tr!("slide {n}", n = n) }
/// N29: the stage-only slide went up.
pub fn on_stage_only() -> String { tr!("On the stage screen only. The room didn't change.") }
