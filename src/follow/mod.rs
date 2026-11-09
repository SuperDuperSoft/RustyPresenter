//! Song auto-follow (1.3): the lyric slides follow the singers. Pure decision core here: the song's words
//! (`lyrics`), where the singers are (`align`), when to change or blank (`rules`) and the live hookup (`engine`).
//! The audio front end and the acoustic model feed it `Heard` words and `Voice` labels.
pub mod lyrics;

pub use lyrics::{SlideSpan, SongText, Word, sound_key, syllables};
