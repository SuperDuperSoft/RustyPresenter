//! STUB (Task 6 only, branch v13-ui): the sound input lives on v13-audio (Task 5). On merge, take THEIR file
//! whole; these two signatures are all the UI uses.
use super::engine::FollowEvent;
use std::sync::mpsc::Sender;

/// The sound inputs auto-follow can listen to. Stub: none yet.
pub fn input_devices() -> Vec<String> { Vec::new() }

/// Listening to one sound input. Stub: never starts.
pub struct Capture;

impl Capture {
    pub fn start(_device: &str, _tx: Sender<FollowEvent>) -> Result<Capture, String> { Err("not available yet".into()) }
}
