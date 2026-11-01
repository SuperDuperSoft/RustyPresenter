//! Video and audio playback through libmpv (loaded at runtime, never linked).
pub mod ffi;
pub mod player;
pub mod surface;

pub use player::{Options, Player, PlayerReport, Players, Role, players};
pub use surface::{DrawMode, VideoSurface, choose_backend, drawing};
