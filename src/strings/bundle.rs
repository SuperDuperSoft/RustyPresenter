//! What saving a service to a file and opening one says (spec §11).
use crate::{tr, trn};

pub fn not_a_service_file() -> String { tr!("That is not a RustyPresenter service file.") }
pub fn newer_file() -> String { tr!("That service file was made by a newer version of RustyPresenter.") }
pub fn damaged() -> String { tr!("That service file is damaged, so nothing was added.") }
pub fn service_gone() -> String { tr!("That service is gone.") }
pub fn saving() -> String { tr!("Saving the service…") }
pub fn opening() -> String { tr!("Opening the service file…") }
pub fn song_gone() -> String { tr!("A song in this service is no longer in the library.") }
pub fn media_gone() -> String { tr!("A video, picture or sound in this service is no longer in the library.") }
pub fn deck_gone() -> String { tr!("A set of slides in this service is no longer in the library.") }
pub fn background_gone() -> String { tr!("A background in this service is missing its file.") }

pub fn file_gone(name: &str) -> String { tr!("{name} is missing its file.", name = name) }

/// Idea 14 (§10c): the fonts the service was made with that this computer lacks.
pub fn fonts_missing(fonts: &[String]) -> String {
    trn!("This service was made with a font this computer does not have, so its words show in another font: {fonts}. Install it to see them as they were made.",
        "This service was made with fonts this computer does not have, so its words show in another font: {fonts}. Install them to see them as they were made.",
        fonts.len(), fonts = fonts.join(", "))
}

pub fn saved(name: &str) -> String { tr!("Saved “{name}” to a file.", name = name) }

/// The box when a service cannot be saved: what is missing, one line each.
pub fn missing_report(lines: &[String]) -> String {
    const SHOWN: usize = 8;
    let mut text = tr!("This service was not saved, because something it needs is missing:");
    text += "\n\n";
    text += &lines[..lines.len().min(SHOWN)].join("\n");
    if lines.len() > SHOWN { text += "\n"; text += &tr!("…and {n} more.", n = lines.len() - SHOWN) }
    text
}

/// The box after opening a service file.
pub fn opened_report(o: &super::Opened) -> String {
    let mut text = tr!("Opened “{name}”.", name = o.name);
    let mut new = vec![];
    if o.songs_added > 0 { new.push(trn!("{n} song", "{n} songs", o.songs_added)) }
    if o.media_added > 0 { new.push(trn!("{n} media file", "{n} media files", o.media_added)) }
    if o.decks_added > 0 { new.push(trn!("{n} set of slides", "{n} sets of slides", o.decks_added)) }
    if !new.is_empty() { text += "\n\n"; text += &tr!("New in the library: {things}.", things = new.join(", ")) }
    if !o.songs_differ.is_empty() {
        text += "\n\n";
        text += &tr!("These differ from the songs already here, so the service uses a second copy: {titles}", titles = o.songs_differ.join(", "));
    }
    if o.passages_removed > 0 { text += &format!("\n\n{}", crate::scripture::strings::passages_removed(o.passages_removed)) }
    if !o.fonts_missing.is_empty() { text += &format!("\n\n{}", fonts_missing(&o.fonts_missing)) }
    text
}
