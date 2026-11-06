//! FEAT-4 song workflow text (spec §3, §5, §7, §8). Declared from `src/sections/mod.rs` with a `#[path]` line
//! until a shared `src/strings/mod.rs` exists.

pub const VERSE: &str = "Verse";
pub const CHORUS: &str = "Chorus";
pub const PRE_CHORUS: &str = "Pre-chorus";
pub const BRIDGE: &str = "Bridge";
pub const TAG: &str = "Tag";
pub const INTRO: &str = "Intro";
pub const ENDING: &str = "Ending";
pub const INTERLUDE: &str = "Interlude";
pub const OTHER: &str = "Other";

pub const NOTHING_LIVE: &str = "Nothing is live yet.";
pub const KEYS_SONGS_ONLY: &str = "Section keys work on songs.";
pub const SONG_CHANGED: &str = "The song changed — try again.";
pub const LIVE_SONG_CHANGED: &str = "The live song changed — try again.";
pub const SCREEN_CHANGED: &str = "The screen changed — try again.";
pub const NO_LONGER_LIVE: &str = "That song is no longer live.";
pub const NO_SLIDES_NOW: &str = "That song has no slides now. The screen keeps the old words.";
pub const SAVED_TEXT_CLEARED: &str = "Saved. The new words show when you bring the text back.";
pub const LIVE_EDIT_BANNER: &str = "This song is on screen. Nothing changes on screen until you choose Save and show now or Update screen.";

pub const BANNER_WORDS: &str = "You changed the words that are on screen. Update screen shows them now. Clicking a slide also shows the new version.";
pub const BANNER_LOOK: &str = "The live song looks different now. Update screen to show it.";
pub const BANNER_LOST: &str = "The part of the song on screen was removed. Update screen starts the song from the top.";
pub const BANNER_NO_SLIDES: &str = "This song has no words now. The screen keeps the old words.";

pub const CHIP_SHOW_HINT: &str = "Click to show this part";
pub const NOT_IN_LYRICS: &str = "Not in the lyrics — it is skipped";
pub const WRITTEN_ORDER_HINT: &str = "Written order — drag to change";

/// "This song has no Chorus." / "This song has no Verse 4."
pub fn no_section(name: &str, number: Option<u8>) -> String {
    match number {
        Some(n) => format!("This song has no {name} {n}."),
        None => format!("This song has no {name}."),
    }
}

/// "Jumped to Chorus (C)".
pub fn jumped(label: &str, key: char) -> String { format!("Jumped to {label} ({key})") }

/// A section chip's tooltip on the live song: "Verse 2 — press 2".
pub fn chip_key_hint(label: &str, key: char) -> String { format!("{label} — press {key}") }

/// Under the live preview: "Chorus · slide 2 of 3".
pub fn live_section_line(label: &str, page: u16, pages: u16) -> String { format!("{label} · slide {page} of {pages}") }
