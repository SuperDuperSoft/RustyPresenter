//! Song auto-follow (1.3): the lyric slides follow the singers. Pure decision core here: the song's words
//! (`lyrics`), where the singers are (`align`), when to change or blank (`rules`) and the live hookup (`engine`).
//! The audio front end and the acoustic model feed it `Heard` words and `Voice` labels.
pub mod align;
// STUB on v13-ui: v13-audio's `audio` replaces it on merge.
pub mod audio;
pub mod engine;
pub mod lyrics;
pub mod rules;
pub mod strings;

pub use engine::{FollowEvent, FollowState, Follower, Light};
pub use align::{Aligner, Heard, Position};
pub use lyrics::{SlideSpan, SongText, Word, sound_key, syllables};
pub use rules::{FollowAction, Mode, Rules, Settings, Voice};
