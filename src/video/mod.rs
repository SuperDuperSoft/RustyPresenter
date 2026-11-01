//! Video and audio playback through libmpv (loaded at runtime, never linked).
pub mod ffi;
pub mod player;

pub use player::{Options, Player, PlayerReport, Players, Role, players};
