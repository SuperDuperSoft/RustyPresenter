//! What Preflight says (spec §13). Each line names the thing and where to fix it.
pub const ALL_CLEAR: &str = "All set. Every song, file and screen for this service was found.";
pub const EMPTY_SERVICE: &str = "This service has no items yet.";
pub const NO_ROOM: &str = "No room screen is chosen. Choose one in Screens.";
pub const ROOM_GONE: &str = "The room screen is not connected. Check its cable, or choose another in Screens.";
pub const STAGE_GONE: &str = "The stage screen is not connected. The service can run without it.";
pub const AUDIO_GONE: &str = "The sound output chosen in Settings is not connected. Plug it in, or choose another in Settings → Video.";
/// On the live column, when the output goes away during the service (spec §6.4).
pub const SOUND_GONE: &str = "The sound output is not connected, so the video or audio was paused. Plug it in, or choose another in Settings → Video, then press play.";
pub const LOGO_MISSING: &str = "The logo's file is missing. Choose it again in Settings.";
pub const BACKGROUND_MISSING: &str = "The default background's file is missing. Choose it again in Settings.";
pub const SONG_MISSING: &str = "the song is no longer in the library";
pub const SONG_EMPTY: &str = "the song has no words";
pub const NO_VERSES: &str = "there is no verse text";
pub const MEDIA_GONE: &str = "it is no longer in the library";
pub const FILE_MISSING: &str = "its file is missing";
pub const DECK_GONE: &str = "these slides are no longer in the library";
pub const ITEM_BACKGROUND_MISSING: &str = "its background file is missing";

pub fn cant_play(reason: &str) -> String { format!("it can't be played ({reason})") }
pub fn slides_missing(missing: usize, all: usize) -> String { format!("{missing} of {all} slides are missing") }
pub fn item(number: usize, title: &str, problem: &str) -> String { format!("{number}. {title}: {problem}") }
pub fn more(count: usize) -> String { format!("…and {count} more") }
pub fn found(count: usize) -> String { if count == 1 { "1 thing to fix before the service:".into() } else { format!("{count} things to fix before the service:") } }

/// An online Bible's passage is not in the capped cache, so it would not show without internet (Nov 5 spec §5).
pub fn not_offline(abbr: &str, reference: &str) -> String { format!("{abbr} isn't ready offline for {reference}. Open this service once with internet.") }
