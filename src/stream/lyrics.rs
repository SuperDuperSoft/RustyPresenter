//! The words over the camera (a lower third), for the livestream's Camera + words scene: the live text slide (a
//! song, a verse) drawn the way the built-in Lower third theme draws it (at the bottom, on a soft band, in the
//! slide's own font and colour) on a clear picture. Pictures, videos and a cleared slide have no words picture.
//! `stream::pictures` draws it and the recorder lays it over the camera (`stream::compose`).
use crate::live::SlideLayer;
use crate::model::Theme;
use crate::render::cache::RenderKey;
use std::sync::Arc;

/// What is live now, as the words picture to draw (`None`: nothing with words).
pub type Source = Arc<dyn Fn() -> Option<RenderKey> + Send + Sync>;

/// The lower-third look of `base` (the slide's own theme): its font and colours, at the bottom on a band.
pub fn lower_third(base: &Theme) -> Theme {
    Theme { size: 64, scripture_size: 52, position: "lower".into(), band: true, band_opacity: 55, ..base.clone() }
}

/// The words picture for live slide `slide` (`theme`: the item's own), at `canvas`. `None` for a slide with no
/// words of its own to put over a camera (a picture, a designed slide).
pub fn key_for(slide: &SlideLayer, theme: &Theme, canvas: (u32, u32)) -> Option<RenderKey> { key_for_at(slide, theme, canvas, false) }

/// `key_for`, with the words at the top of the picture when `top` (Settings: the scene layout).
pub fn key_for_at(slide: &SlideLayer, theme: &Theme, canvas: (u32, u32), top: bool) -> Option<RenderKey> {
    let SlideLayer::Text { text, footer, reference, scripture, size, passage, parallel, theme: own } = slide else { return None };
    if text.trim().is_empty() { return None }
    let base = own.clone().unwrap_or_else(|| theme.clone());
    // The slide's own largest size is for the room; over a camera the lower third's size is the one.
    let _ = size;
    Some(RenderKey { text: text.clone(), footer: footer.clone(), reference: reference.clone(), scripture: *scripture, size: 0, passage: passage.iter().cloned().collect(),
        parallel: parallel.clone(), theme: Theme { position: if top { "top".into() } else { "lower".into() }, ..lower_third(&base) }, canvas, design: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str) -> SlideLayer {
        SlideLayer::Text { text: t.into(), footer: None, reference: None, scripture: false, size: 120, passage: vec![], parallel: None, theme: None }
    }

    #[test]
    fn the_words_take_the_lower_third_look_in_the_slides_own_font() {
        let theme = Theme { font: "Georgia".into(), color: "#ffee00".into(), size: 90, ..Theme::default() };
        let key = key_for(&text("Amazing grace"), &theme, (1280, 720)).unwrap();
        assert_eq!((key.theme.position.as_str(), key.theme.band, key.theme.size, key.size), ("lower", true, 64, 0));
        assert_eq!((key.theme.font.as_str(), key.theme.color.as_str()), ("Georgia", "#ffee00"));
        assert_eq!(key.canvas, (1280, 720));
        assert_eq!(key_for(&text("  "), &theme, (1280, 720)), None, "no words, nothing over the camera");
        // The words at the top instead, on the same band.
        let top = key_for_at(&text("Amazing grace"), &theme, (1280, 720), true).unwrap();
        assert_eq!((top.theme.position.as_str(), top.theme.band), ("top", true));
    }
}
