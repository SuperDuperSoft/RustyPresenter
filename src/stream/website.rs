//! The church's website player (livestreaming design §2, "website embed player"): the code a church pastes into
//! its own website (WordPress, Wix, Squarespace, any page builder's "HTML" or "embed" block) so the live stream
//! plays there too. The app serves no page itself: the church's website is somewhere else, and YouTube and
//! Facebook play the video.
//!
//! - **YouTube:** the channel's live player (`embed/live_stream?channel=UC…`): it plays whatever the channel has
//!   live, so the code never needs changing from week to week. It needs the channel's id ("UC" and 22 letters),
//!   which a signed-in YouTube account fills in by itself.
//! - **Facebook:** Facebook has no player that always shows a Page's live video, so it is the Page's own box
//!   (`plugins/page.php`, timeline): while live, the live video is at the top of it.
use serde::{Deserialize, Serialize};

/// Settings → Livestream → On your website.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Prefs {
    /// The YouTube channel: its id, or its address with the id in it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub youtube: String,
    /// The Facebook Page's address (or its name).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub facebook: String,
}

/// The channel id in `text`: an id as it is ("UCxxxxxxxxxxxxxxxxxxxxxx"), or the one in a channel address
/// (`youtube.com/channel/UC…`). `None` for anything else (a handle such as "@church" has no id in it).
pub fn channel_id(text: &str) -> Option<String> {
    let t = text.trim();
    let is_id = |s: &str| s.len() == 24 && s.starts_with("UC") && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if is_id(t) { return Some(t.to_string()) }
    let after = t.split("/channel/").nth(1)?;
    let id: String = after.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    is_id(&id).then_some(id)
}

/// The Facebook Page's address from what was typed: an address as it is (made `https://`), a bare name as
/// `https://www.facebook.com/<name>`. `None` when empty or not a Facebook address.
pub fn facebook_page(text: &str) -> Option<String> {
    let t = text.trim().trim_end_matches('/');
    if t.is_empty() { return None }
    let lower = t.to_ascii_lowercase();
    let bare = lower.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.").trim_start_matches("m.");
    if let Some(rest) = bare.strip_prefix("facebook.com/").or_else(|| bare.strip_prefix("fb.com/")) {
        let path = &t[t.len() - rest.len()..];
        return (!path.is_empty()).then(|| format!("https://www.facebook.com/{path}"));
    }
    if t.contains('/') || t.contains(':') || t.contains(char::is_whitespace) { return None }
    Some(format!("https://www.facebook.com/{}", t.trim_start_matches('@')))
}

/// A box that keeps the 16:9 shape at any width, with the player filling it.
fn wide_box(src: &str, title: &str, allow: &str) -> String {
    format!("<div style=\"position:relative;width:100%;max-width:960px;aspect-ratio:16/9\">\n  <iframe src=\"{src}\" title=\"{title}\" style=\"position:absolute;inset:0;width:100%;height:100%;border:0\" allow=\"{allow}\" allowfullscreen></iframe>\n</div>")
}

/// The code for the YouTube channel `id`'s live player.
pub fn youtube_code(id: &str) -> String {
    wide_box(&format!("https://www.youtube.com/embed/live_stream?channel={id}"), "Live stream",
        "accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share")
}

/// The code for the Facebook Page at `page` (an address from `facebook_page`).
pub fn facebook_code(page: &str) -> String {
    let href: String = url::form_urlencoded::byte_serialize(page.as_bytes()).collect();
    format!("<iframe src=\"https://www.facebook.com/plugins/page.php?href={href}&tabs=timeline&width=500&height=700&small_header=true&adapt_container_width=true&hide_cover=false&show_facepile=false\" title=\"Live stream on Facebook\" width=\"500\" height=\"700\" style=\"border:none;overflow:hidden;max-width:100%\" scrolling=\"no\" frameborder=\"0\" allowfullscreen=\"true\" allow=\"autoplay; clipboard-write; encrypted-media; picture-in-picture; web-share\"></iframe>")
}

pub mod tr {
    use crate::tr;
    pub fn how() -> String {
        tr!("To play the livestream on your church's website: copy the code, then in your website's editor add an \"HTML\", \"Embed\" or \"Code\" block where the video should go and paste it there. You do this once: the same code plays every week's service.")
    }
    pub fn youtube_how() -> String {
        tr!("YouTube plays whatever your channel has live; between services it shows the next scheduled stream or says nothing is live. YouTube Studio must allow embedding (it does unless it was turned off).")
    }
    pub fn facebook_how() -> String {
        tr!("Facebook shows your Page with its posts; while you are live the live video is at the top. The Page must be public.")
    }
    pub fn no_channel() -> String {
        tr!("Type the channel's id: it starts with UC. In YouTube Studio, choose Customisation → Basic info and copy it from the channel address.")
    }
    pub fn no_page() -> String { tr!("Type the Facebook Page's address, like https://www.facebook.com/yourchurch") }
    pub fn copied() -> String { tr!("Website code copied. Paste it into your website's editor.") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_channel_id_is_found_as_typed_or_in_its_address() {
        let id = "UC_x5XG1OV2P6uZZ5FSM9Ttw";
        assert_eq!(channel_id(id).as_deref(), Some(id));
        assert_eq!(channel_id(&format!(" https://www.youtube.com/channel/{id}/live ")).as_deref(), Some(id));
        assert_eq!(channel_id(&format!("youtube.com/channel/{id}?si=abc")).as_deref(), Some(id));
        for bad in ["", "@ourchurch", "https://www.youtube.com/@ourchurch", "UCshort", "https://youtu.be/abcdefghijk"] {
            assert_eq!(channel_id(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_facebook_page_is_its_address() {
        assert_eq!(facebook_page("ourchurch").as_deref(), Some("https://www.facebook.com/ourchurch"));
        assert_eq!(facebook_page("@OurChurch").as_deref(), Some("https://www.facebook.com/OurChurch"));
        assert_eq!(facebook_page("https://m.facebook.com/OurChurch/").as_deref(), Some("https://www.facebook.com/OurChurch"));
        assert_eq!(facebook_page("facebook.com/profile.php?id=100064").as_deref(), Some("https://www.facebook.com/profile.php?id=100064"));
        for bad in ["", "https://example.org/church", "two words"] { assert_eq!(facebook_page(bad), None, "{bad}") }
    }

    #[test]
    fn the_codes_play_the_live_stream_at_any_width() {
        let y = youtube_code("UC_x5XG1OV2P6uZZ5FSM9Ttw");
        assert!(y.contains("src=\"https://www.youtube.com/embed/live_stream?channel=UC_x5XG1OV2P6uZZ5FSM9Ttw\""), "{y}");
        assert!(y.contains("aspect-ratio:16/9") && y.contains("allowfullscreen"));
        let f = facebook_code("https://www.facebook.com/OurChurch");
        assert!(f.contains("plugins/page.php?href=https%3A%2F%2Fwww.facebook.com%2FOurChurch&tabs=timeline"), "{f}");
    }
}
