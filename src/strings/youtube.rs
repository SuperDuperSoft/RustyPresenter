//! User-facing strings of YouTube downloads: the Add dialog's YouTube panel, the job card and the errors.
//! Declared from `src/youtube/mod.rs` with a `#[path]` line until a shared `src/strings/mod.rs` exists.

/// The panel, while it works.
pub const LOOKING_UP: &str = "Looking up the video…";
pub fn getting_ready(percent: u8) -> String { format!("Getting YouTube downloads ready — first time only ({percent}%)…") }
pub fn updating(percent: u8) -> String { format!("Updating YouTube downloads ({percent}%)…") }
pub fn joiner_ready(percent: u8) -> String { format!("Getting the video joiner ready — first time only ({percent}%)…") }
pub const NOT_A_LINK: &str = "Paste a link to a YouTube video. It starts with https://www.youtube.com or https://youtu.be.";
pub fn playlist(count: usize) -> String {
    format!("This link is a playlist of {count} videos. Only this first video will be downloaded.")
}
/// The panel's line under the title: the length and the channel.
pub fn detail(seconds: Option<f64>, channel: &str) -> String {
    let len = seconds.filter(|s| *s > 0.0).map(length);
    match (len, channel.is_empty()) {
        (Some(l), false) => format!("{l} · {channel}"),
        (Some(l), true) => l,
        (None, _) => channel.to_string(),
    }
}
/// `12:34` or `1:02:03`.
pub fn length(seconds: f64) -> String {
    let s = seconds.round() as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}
/// The Quality choices, in `Quality` order.
pub const QUALITIES: [&str; 4] = ["Best (up to 1080p)", "Smaller (720p)", "Smallest (480p)", "Sound only"];
pub fn started(title: &str) -> String { format!("Downloading “{title}”. It will be in Video & media when it's done.") }

/// The job card.
pub fn downloading(title: &str) -> String { format!("Downloading “{title}”") }
pub fn downloading_left(title: &str, seconds: u64) -> String {
    let left = match seconds {
        0..60 => "less than a minute left".to_string(),
        60..120 => "about a minute left".to_string(),
        s => format!("about {} minutes left", (s + 30) / 60),
    };
    format!("Downloading “{title}” — {left}")
}
pub fn finishing(title: &str) -> String { format!("Finishing “{title}”…") }
pub fn adding(title: &str) -> String { format!("Adding “{title}” to the library…") }
pub fn downloaded(title: &str) -> String { format!("Downloaded “{title}”") }
pub const IN_SERVICE: &str = "It's in Video & media and in the service.";
pub const IN_LIBRARY: &str = "It's in Video & media.";
pub const LOW_QUALITY: &str = "Saved at lower quality (360p) because the video joiner couldn't be set up.";

/// What went wrong, in plain words.
pub const OFFLINE: &str = "Can't reach YouTube. Check the internet connection and try again.";
pub const PRIVATE: &str = "This video is private, so only its owner can download it.";
pub const SIGN_IN: &str = "YouTube only shows this video to signed-in viewers (for example, it is age-restricted), so it can't be downloaded here.";
pub const MEMBERS: &str = "This video is only for the channel's members, so it can't be downloaded here.";
pub const BOT_CHECK: &str = "YouTube wants a sign-in to check this isn't a robot. Wait a while and try again.";
pub const LIVE: &str = "This live stream hasn't finished yet. Try again after it ends.";
pub const UPCOMING: &str = "This live stream or premiere hasn't started yet.";
pub const PROCESSING: &str = "This live stream just ended and YouTube is still preparing it. Try again in a while.";
pub const NOT_A_VIDEO: &str = "That link isn't a YouTube video. Copy the link from the video's page and try again.";
pub const UNAVAILABLE: &str = "This video isn't available. It may have been removed, or blocked in this country.";
pub const EMPTY_PLAYLIST: &str = "This playlist has no videos that can be downloaded.";
pub const DISK_FULL: &str = "The computer's disk is full. Free up some space and try again.";
pub const BROKEN: &str = "YouTube changed something and the downloader can't read this video yet. Try again later.";
pub const SETUP_OFFLINE: &str = "Couldn't get YouTube downloads ready. Check the internet connection and try again.";
pub const SETUP_CHECKSUM: &str = "The YouTube downloader that arrived didn't match its official fingerprint, so it wasn't used. Try again later.";
pub const SETUP_NO_BUILD: &str = "YouTube downloads don't work on this kind of computer yet.";
pub fn setup_failed(detail: &str) -> String { format!("Couldn't get YouTube downloads ready ({detail}). Try again later.") }
pub fn other(detail: &str) -> String { format!("The download didn't work: {detail}") }
pub const CANCELLED: &str = "Cancelled.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_and_time_left() {
        assert_eq!(length(5.0), "0:05");
        assert_eq!(length(754.4), "12:34");
        assert_eq!(length(3723.0), "1:02:03");
        assert_eq!(detail(Some(754.0), "Blender"), "12:34 · Blender");
        assert_eq!(detail(None, "Blender"), "Blender");
        assert_eq!(detail(Some(0.0), ""), "");
        assert_eq!(downloading_left("A", 20), "Downloading “A” — less than a minute left");
        assert_eq!(downloading_left("A", 100), "Downloading “A” — about a minute left");
        assert_eq!(downloading_left("A", 200), "Downloading “A” — about 3 minutes left");
    }
}
