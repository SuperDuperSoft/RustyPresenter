//! User-facing strings of YouTube downloads: the Add dialog's YouTube panel, the job card and the errors.
//! Declared from `src/youtube/mod.rs` with a `#[path]` line until a shared `src/strings/mod.rs` exists.
use crate::{tr, trn};

/// The panel, while it works.
pub fn looking_up() -> String { tr!("Looking up the video…") }
pub fn getting_ready(percent: u8) -> String { tr!("Getting YouTube downloads ready — first time only ({percent}%)…", percent = percent) }
pub fn updating(percent: u8) -> String { tr!("Updating YouTube downloads ({percent}%)…", percent = percent) }
pub fn joiner_ready(percent: u8) -> String { tr!("Getting the video joiner ready — first time only ({percent}%)…", percent = percent) }
pub fn not_a_link() -> String { tr!("Paste a link to a YouTube video. It starts with https://www.youtube.com or https://youtu.be.") }
pub fn playlist(count: usize) -> String {
    trn!("This link is a playlist of {n} videos. Only this first video will be downloaded.", "This link is a playlist of {n} videos. Only this first video will be downloaded.", count)
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
pub fn qualities() -> [String; 4] { [tr!("Best (up to 1080p)"), tr!("Smaller (720p)"), tr!("Smallest (480p)"), tr!("Sound only")] }
pub fn started(title: &str) -> String { tr!("Downloading “{title}”. It will be in Video & media when it's done.", title = title) }

/// The job card.
pub fn downloading(title: &str) -> String { tr!("Downloading “{title}”", title = title) }
pub fn downloading_left(title: &str, seconds: u64) -> String {
    let left = match seconds {
        0..60 => tr!("less than a minute left"),
        60..120 => tr!("about a minute left"),
        s => trn!("about {n} minute left", "about {n} minutes left", (s + 30) / 60),
    };
    tr!("Downloading “{title}” — {left}", title = title, left = left)
}
pub fn finishing(title: &str) -> String { tr!("Finishing “{title}”…", title = title) }
pub fn adding(title: &str) -> String { tr!("Adding “{title}” to the library…", title = title) }
pub fn downloaded(title: &str) -> String { tr!("Downloaded “{title}”", title = title) }
pub fn in_service() -> String { tr!("It's in Video & media and in the service.") }
pub fn in_library() -> String { tr!("It's in Video & media.") }
pub fn low_quality() -> String { tr!("Saved at lower quality (360p) because the video joiner couldn't be set up.") }

/// What went wrong, in plain words.
pub fn offline() -> String { tr!("Can't reach YouTube. Check the internet connection and try again.") }
pub fn private() -> String { tr!("This video is private, so only its owner can download it.") }
pub fn sign_in() -> String { tr!("YouTube only shows this video to signed-in viewers (for example, it is age-restricted), so it can't be downloaded here.") }
pub fn members() -> String { tr!("This video is only for the channel's members, so it can't be downloaded here.") }
pub fn bot_check() -> String { tr!("YouTube wants a sign-in to check this isn't a robot. Wait a while and try again.") }
pub fn live() -> String { tr!("This live stream hasn't finished yet. Try again after it ends.") }
pub fn upcoming() -> String { tr!("This live stream or premiere hasn't started yet.") }
pub fn processing() -> String { tr!("This live stream just ended and YouTube is still preparing it. Try again in a while.") }
pub fn not_a_video() -> String { tr!("That link isn't a YouTube video. Copy the link from the video's page and try again.") }
pub fn unavailable() -> String { tr!("This video isn't available. It may have been removed, or blocked in this country.") }
pub fn empty_playlist() -> String { tr!("This playlist has no videos that can be downloaded.") }
pub fn disk_full() -> String { tr!("The computer's disk is full. Free up some space and try again.") }
pub fn broken() -> String { tr!("YouTube changed something and the downloader can't read this video yet. Try again later.") }
pub fn setup_offline() -> String { tr!("Couldn't get YouTube downloads ready. Check the internet connection and try again.") }
pub fn setup_checksum() -> String { tr!("The YouTube downloader that arrived didn't match its official fingerprint, so it wasn't used. Try again later.") }
pub fn setup_no_build() -> String { tr!("YouTube downloads don't work on this kind of computer yet.") }
pub fn setup_failed(detail: &str) -> String { tr!("Couldn't get YouTube downloads ready ({detail}). Try again later.", detail = detail) }
pub fn other(detail: &str) -> String { tr!("The download didn't work: {detail}", detail = detail) }
pub fn cancelled() -> String { tr!("Cancelled.") }

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
