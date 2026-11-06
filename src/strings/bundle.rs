//! What saving a service to a file and opening one says (spec §11).

pub const NOT_A_SERVICE_FILE: &str = "That is not a RustyPresenter service file.";
pub const NEWER_FILE: &str = "That service file was made by a newer version of RustyPresenter.";
pub const DAMAGED: &str = "That service file is damaged, so nothing was added.";
pub const SERVICE_GONE: &str = "That service is gone.";
pub const SAVING: &str = "Saving the service…";
pub const OPENING: &str = "Opening the service file…";
pub const SONG_GONE: &str = "A song in this service is no longer in the library.";
pub const MEDIA_GONE: &str = "A video, picture or sound in this service is no longer in the library.";
pub const DECK_GONE: &str = "A set of slides in this service is no longer in the library.";
pub const BACKGROUND_GONE: &str = "A background in this service is missing its file.";

pub fn file_gone(name: &str) -> String { format!("{name} is missing its file.") }

pub fn saved(name: &str) -> String { format!("Saved “{name}” to a file.") }

/// The box when a service cannot be saved: what is missing, one line each.
pub fn missing_report(lines: &[String]) -> String {
    const SHOWN: usize = 8;
    let mut text = format!("This service was not saved, because something it needs is missing:\n\n{}", lines[..lines.len().min(SHOWN)].join("\n"));
    if lines.len() > SHOWN { text += &format!("\n…and {} more.", lines.len() - SHOWN) }
    text
}

fn plural(n: usize, one: &str, many: &str) -> String { format!("{n} {}", if n == 1 { one } else { many }) }

/// The box after opening a service file.
pub fn opened_report(o: &super::Opened) -> String {
    let mut text = format!("Opened “{}”.", o.name);
    let mut new = vec![];
    if o.songs_added > 0 { new.push(plural(o.songs_added, "song", "songs")) }
    if o.media_added > 0 { new.push(plural(o.media_added, "media file", "media files")) }
    if o.decks_added > 0 { new.push(plural(o.decks_added, "set of slides", "sets of slides")) }
    if !new.is_empty() { text += &format!("\n\nNew in the library: {}.", new.join(", ")) }
    if !o.songs_differ.is_empty() {
        text += &format!("\n\nThese differ from the songs already here, so the service uses a second copy: {}", o.songs_differ.join(", "));
    }
    if o.passages_removed > 0 { text += &format!("\n\n{}", crate::scripture::strings::passages_removed(o.passages_removed)) }
    text
}
