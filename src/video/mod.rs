//! Video and audio playback through libmpv (loaded at runtime, never linked).
pub mod ffi;
pub mod graphics;
pub mod inputs;
pub mod player;
pub mod postpass;
pub mod surface;

pub use player::{FrameGrabber, Options, Player, PlayerReport, Players, Role, players};
pub use graphics::{choose_backend, drawing};
pub use surface::{DrawMode, VideoSurface};

/// Seekable 16-bit mono PCM for playback tests, with no external tools required.
#[cfg(test)]
pub(crate) fn silent_wav(path: &std::path::Path, millis: u32) {
    let rate = 8000u32;
    let pcm_bytes = rate * millis / 1000 * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + pcm_bytes).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&pcm_bytes.to_le_bytes());
    bytes.resize(bytes.len() + pcm_bytes as usize, 0);
    std::fs::write(path, bytes).unwrap();
}
