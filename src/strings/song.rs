//! FEAT-4 song workflow text (spec §3, §5, §7, §8). Declared from `src/sections/mod.rs` with a `#[path]` line
//! until a shared `src/strings/mod.rs` exists.
use crate::tr;

pub fn verse() -> String { tr!("Verse") }
pub fn chorus() -> String { tr!("Chorus") }
pub fn pre_chorus() -> String { tr!("Pre-chorus") }
pub fn bridge() -> String { tr!("Bridge") }
pub fn tag() -> String { tr!("Tag") }
pub fn intro() -> String { tr!("Intro") }
pub fn ending() -> String { tr!("Ending") }
pub fn interlude() -> String { tr!("Interlude") }
pub fn other() -> String { tr!("Other") }

pub fn nothing_live() -> String { tr!("Nothing is live yet.") }
pub fn keys_songs_only() -> String { tr!("Section keys work on songs.") }
pub fn song_changed() -> String { tr!("The song changed — try again.") }
pub fn live_song_changed() -> String { tr!("The live song changed — try again.") }
pub fn screen_changed() -> String { tr!("The screen changed — try again.") }
pub fn no_longer_live() -> String { tr!("That song is no longer live.") }
pub fn no_slides_now() -> String { tr!("That song has no slides now. The screen keeps the old words.") }
pub fn saved_text_cleared() -> String { tr!("Saved. The new words show when you bring the text back.") }
pub fn live_edit_banner() -> String { tr!("This song is on screen. Nothing changes on screen until you choose Save and show now or Update screen.") }

pub fn banner_words() -> String { tr!("You changed the words that are on screen. Update screen shows them now. Clicking a slide also shows the new version.") }
pub fn banner_look() -> String { tr!("The live song looks different now. Update screen to show it.") }
pub fn banner_lost() -> String { tr!("The part of the song on screen was removed. Update screen starts the song from the top.") }
pub fn banner_no_slides() -> String { tr!("This song has no words now. The screen keeps the old words.") }

pub fn chip_show_hint() -> String { tr!("Click to show this part") }
pub fn not_in_lyrics() -> String { tr!("Not in the lyrics — it is skipped") }
pub fn written_order_hint() -> String { tr!("Written order — drag to change") }

/// "This song has no Chorus." / "This song has no Verse 4."
pub fn no_section(name: &str, number: Option<u8>) -> String {
    match number {
        Some(n) => tr!("This song has no {section} {number}.", section = name, number = n),
        None => tr!("This song has no {section}.", section = name),
    }
}

/// "Jumped to Chorus (C)".
pub fn jumped(label: &str, key: char) -> String { tr!("Jumped to {section} ({key})", section = label, key = key) }

/// A section chip's tooltip on the live song: "Verse 2 — press 2".
pub fn chip_key_hint(label: &str, key: char) -> String { tr!("{section} — press {key}", section = label, key = key) }

/// Under the live preview: "Chorus · slide 2 of 3".
pub fn live_section_line(label: &str, page: u16, pages: u16) -> String {
    tr!("{section} · slide {page} of {pages}", section = label, page = page, pages = pages)
}
