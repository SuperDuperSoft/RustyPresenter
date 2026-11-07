//! What Preflight says (spec §13). Each line names the thing and where to fix it.
use crate::{tr, trn};

pub fn all_clear() -> String { tr!("All set. Every song, file and screen for this service was found.") }
pub fn empty_service() -> String { tr!("This service has no items yet.") }
pub fn no_room() -> String { tr!("No room screen is chosen. Choose one in Screens.") }
pub fn room_gone() -> String { tr!("The room screen is not connected. Check its cable, or choose another in Screens.") }
pub fn stage_gone() -> String { tr!("The stage screen is not connected. The service can run without it.") }
pub fn audio_gone() -> String { tr!("The sound output chosen in Settings is not connected. Plug it in, or choose another in Settings → Video.") }
/// On the live column, when the output goes away during the service (spec §6.4).
pub fn sound_gone() -> String { tr!("The sound output is not connected, so the video or audio was paused. Plug it in, or choose another in Settings → Video, then press play.") }
pub fn logo_missing() -> String { tr!("The logo's file is missing. Choose it again in Settings.") }
pub fn background_missing() -> String { tr!("The default background's file is missing. Choose it again in Settings.") }
pub fn song_missing() -> String { tr!("the song is no longer in the library") }
pub fn song_empty() -> String { tr!("the song has no words") }
pub fn no_verses() -> String { tr!("there is no verse text") }
pub fn media_gone() -> String { tr!("it is no longer in the library") }
pub fn file_missing() -> String { tr!("its file is missing") }
pub fn deck_gone() -> String { tr!("these slides are no longer in the library") }
pub fn item_background_missing() -> String { tr!("its background file is missing") }

pub fn cant_play(reason: &str) -> String { tr!("it can't be played ({reason})", reason = reason) }
pub fn slides_missing(missing: usize, all: usize) -> String {
    trn!("{missing} of {n} slides are missing", "{missing} of {n} slides are missing", all, missing = missing)
}
pub fn item(number: usize, title: &str, problem: &str) -> String {
    tr!("{number}. {title}: {problem}", number = number, title = title, problem = problem)
}
pub fn more(count: usize) -> String { tr!("…and {n} more", n = count) }
pub fn found(count: usize) -> String { trn!("{n} thing to fix before the service:", "{n} things to fix before the service:", count) }

/// An online Bible's passage is not in the capped cache, so it would not show without internet (Nov 5 spec §5).
pub fn not_offline(abbr: &str, reference: &str) -> String {
    tr!("{bible} isn't ready offline for {passage}. Open this service once with internet.", bible = abbr, passage = reference)
}
