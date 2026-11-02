//! The background picker's list of choices (`backgroundPicker`, `operator.js:711-733`).
use super::strings as tr;
use crate::model::{Background, MediaFile, MediaKind, MediaStatus};

pub const SWATCHES: [&str; 6] = ["#000000", "#0b1a33", "#1d2b1f", "#2b1530", "#3a2412", "#ffffff"];
/// The custom colour box's value when the current background is not a colour (`operator.js:719`).
const CUSTOM_START: &str = "#123456";

/// What clicking a choice does.
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    /// Use this background. `None` is "Theme default".
    Set(Option<Background>),
    /// Open the colour grid.
    Custom,
    /// Pick files and add them to the media library.
    Upload,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub pick: Pick,
    pub label: String,
    pub tooltip: String,
}

fn choice(pick: Pick, label: impl Into<String>, tooltip: impl Into<String>) -> Choice {
    Choice { pick, label: label.into(), tooltip: tooltip.into() }
}

/// In the web's order: "Theme default" when allowed, the swatches, the custom colour, every ready picture or video,
/// then Upload.
pub fn choices(media: &[MediaFile], allow_default: bool) -> Vec<Choice> {
    let mut out = Vec::new();
    if allow_default {
        out.push(choice(Pick::Set(None), tr::BG_THEME_DEFAULT, ""));
    }
    out.extend(SWATCHES.iter().map(|c| choice(Pick::Set(Some(Background::Color { color: (*c).into() })), "", "")));
    out.push(choice(Pick::Custom, "", tr::BG_PICK_ANY_COLOR));
    for m in media.iter().filter(|m| m.status == MediaStatus::Ready) {
        let asset = m.asset.clone();
        match m.kind {
            MediaKind::Image => out.push(choice(Pick::Set(Some(Background::Image { asset })), m.name.clone(), m.name.clone())),
            MediaKind::Video => out.push(choice(Pick::Set(Some(Background::Video { asset })), tr::bg_video(&m.name), m.name.clone())),
            MediaKind::Audio => {}
        }
    }
    out.push(choice(Pick::Upload, tr::BG_UPLOAD, ""));
    out
}

/// The choice to outline as current. A colour with no swatch outlines nothing, as on the web.
pub fn selected(choices: &[Choice], value: &Option<Background>) -> Option<usize> {
    choices.iter().position(|c| matches!(&c.pick, Pick::Set(v) if v == value))
}

/// The colour the custom box starts from.
pub fn custom_start(value: &Option<Background>) -> &str {
    match value {
        Some(Background::Color { color }) => color,
        _ => CUSTOM_START,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(name: &str, kind: MediaKind, status: MediaStatus) -> MediaFile {
        MediaFile { id: Default::default(), name: name.into(), kind, asset: format!("{name}.bin"), status, original_asset: None }
    }

    fn library() -> Vec<MediaFile> {
        vec![
            media("Sunrise", MediaKind::Image, MediaStatus::Ready),
            media("Clouds", MediaKind::Video, MediaStatus::Ready),
            media("Hymn", MediaKind::Audio, MediaStatus::Ready),
            media("Busy", MediaKind::Video, MediaStatus::Converting { percent: 5 }),
            media("Odd", MediaKind::Video, MediaStatus::Unconverted { reason: "x".into() }),
            media("Broken", MediaKind::Image, MediaStatus::Failed { reason: "x".into() }),
        ]
    }

    #[test]
    fn background_choices_order_and_ready_only() {
        let list = choices(&library(), true);
        assert_eq!(list.len(), 1 + 6 + 1 + 2 + 1);
        assert_eq!((list[0].pick.clone(), list[0].label.as_str()), (Pick::Set(None), "Theme default"));
        for (i, color) in SWATCHES.iter().enumerate() {
            assert_eq!(list[i + 1].pick, Pick::Set(Some(Background::Color { color: (*color).into() })));
        }
        assert_eq!((list[7].pick.clone(), list[7].tooltip.as_str()), (Pick::Custom, "Pick any color"));
        assert_eq!(list[8].pick, Pick::Set(Some(Background::Image { asset: "Sunrise.bin".into() })));
        assert_eq!((list[8].label.as_str(), list[8].tooltip.as_str()), ("Sunrise", "Sunrise"));
        assert_eq!(list[9].pick, Pick::Set(Some(Background::Video { asset: "Clouds.bin".into() })));
        assert_eq!((list[9].label.as_str(), list[9].tooltip.as_str()), ("▶ Clouds", "Clouds"));
        assert_eq!((list[10].pick.clone(), list[10].label.as_str()), (Pick::Upload, "+ Upload…"));
    }

    #[test]
    fn settings_picker_has_no_theme_default() {
        let list = choices(&[], false);
        assert_eq!(list.len(), 6 + 1 + 1);
        assert!(!list.iter().any(|c| c.pick == Pick::Set(None)));
    }

    #[test]
    fn selected_choice_matches_current_value() {
        let list = choices(&library(), true);
        assert_eq!(selected(&list, &None), Some(0));
        assert_eq!(selected(&list, &Some(Background::Color { color: "#2b1530".into() })), Some(4));
        assert_eq!(selected(&list, &Some(Background::Video { asset: "Clouds.bin".into() })), Some(9));
    }

    #[test]
    fn same_asset_as_a_picture_and_a_video_are_different_choices() {
        let list = choices(&library(), true);
        assert_eq!(selected(&list, &Some(Background::Video { asset: "Sunrise.bin".into() })), None);
    }

    #[test]
    fn nothing_selected_for_a_custom_colour_a_gone_file_or_no_default() {
        let list = choices(&library(), true);
        assert_eq!(selected(&list, &Some(Background::Color { color: "#abcdef".into() })), None);
        assert_eq!(selected(&list, &Some(Background::Image { asset: "gone".into() })), None);
        assert_eq!(selected(&choices(&[], false), &None), None);
    }

    #[test]
    fn custom_box_starts_from_the_current_colour() {
        assert_eq!(custom_start(&Some(Background::Color { color: "#abcdef".into() })), "#abcdef");
        assert_eq!(custom_start(&None), "#123456");
        assert_eq!(custom_start(&Some(Background::Image { asset: "a".into() })), "#123456");
    }
}
