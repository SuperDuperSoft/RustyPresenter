//! Song auto-follow (1.3): the lyric slides follow the singers. Pure decision core here: the song's words
//! (`lyrics`), where the singers are (`align`), when to change or blank (`rules`) and the live hookup (`engine`).
//! The audio front end (`audio`, `voice`) and the acoustic model feed it `Heard` words and `Voice` labels; `log`
//! keeps what it did and why, and `practice` runs it on a recording.
pub mod align;
pub mod audio;
pub mod engine;
pub mod log;
pub mod lyrics;
pub mod practice;
pub mod rules;
pub mod voice;
#[cfg(test)]
pub(crate) mod testing;

pub use audio::{Capture, input_devices};
pub use engine::{FollowEvent, FollowState, Follower, Light};
pub use log::{LogAction, LogEntry};
pub use align::{Aligner, Heard, Position};
pub use lyrics::{SlideSpan, SongText, Word, sound_key, syllables};
pub use rules::{FollowAction, Mode, Rules, Settings, Voice};
