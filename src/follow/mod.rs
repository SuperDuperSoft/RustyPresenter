//! Song auto-follow (1.3): the lyric slides follow the singers. Pure decision core here: the song's words
//! (`lyrics`), where the singers are (`align`), when to change or blank (`rules`) and the live hookup (`engine`).
//! The audio front end (`audio`, `voice`) and the acoustic model feed it `Heard` words and `Voice` labels; `log`
//! keeps what it did and why, and `practice` runs it on a recording. The ear (`ear`, `model`, `fetch`, `g2p`) is
//! a CTC speech model run in Candle on a model downloaded on demand.
pub mod align;
pub mod audio;
pub mod ear;
pub mod engine;
pub mod fetch;
pub mod g2p;
pub mod log;
pub mod lyrics;
#[cfg(feature = "follow-model")]
pub mod model;
pub mod practice;
pub mod rules;
pub mod voice;
#[cfg(test)]
pub(crate) mod testing;

pub use audio::{Capture, input_devices};
pub use ear::{Choice, Lexicon, room_ear};
pub use engine::{FollowEvent, FollowState, Follower, Light};
pub use log::{LogAction, LogEntry};
pub use align::{Aligner, Heard, Position};
pub use lyrics::{SlideSpan, SongText, Word, sound_key, syllables};
pub use rules::{FollowAction, Mode, Rules, Settings, Voice};
