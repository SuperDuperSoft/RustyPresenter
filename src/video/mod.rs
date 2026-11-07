//! Video and audio playback through libmpv (loaded at runtime, never linked).
pub mod ffi;
pub mod graphics;
pub mod player;
pub mod surface;

pub use player::{FrameGrabber, Options, Player, PlayerReport, Players, Role, players};
pub use graphics::{choose_backend, drawing};
pub use surface::{DrawMode, VideoSurface};
