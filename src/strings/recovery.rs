//! User-facing strings for crash restore and single instance (Plan 4 Task 11).

/// The notice after the supervisor restarted a crashed app and the frame came back.
pub const RESTARTED: &str = "RustyPresenter restarted after a problem.";
/// The notice on the second restart in a row: the frame came back without its video and audio.
pub const RESTARTED_NO_MEDIA: &str = "RustyPresenter restarted after a problem. Video/audio stopped — click it again to replay.";

/// Another program holds RustyPresenter's port.
pub fn port_taken(port: u16) -> String {
    format!("Port {port} is in use by another program, so RustyPresenter cannot start.")
}
