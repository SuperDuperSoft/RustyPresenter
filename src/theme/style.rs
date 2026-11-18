//! The text style of a theme: `model::Theme` extended with weight, alignment, outline and a text band
//! (spec §4.1). Old data loads unchanged, and the defaults draw exactly what today's `model::Theme` draws.
use serde::{Deserialize, Serialize};

use crate::theme::layouts::{Frame, Layouts, clamp_layout};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(default)]
pub struct Theme {
    pub font: String,
    /// Largest text size in pixels on a 1080p screen. Text shrinks to fit.
    pub size: u32,
    pub color: String,
    pub shadow: bool,
    /// "center", "lower" or "top"
    pub position: String,
    pub uppercase: bool,
    pub scripture_size: u32,
    /// 400, 600 or 700. Lyrics and announcements only; scripture always draws at 500.
    pub weight: u16,
    /// Lyrics and announcements only; scripture stays left-aligned.
    pub align: TextAlign,
    /// Outline width in logical px: 0 off, 3 thin, 6 thick (at most 12).
    pub outline: u8,
    pub outline_color: String,
    /// A see-through band behind the text block.
    pub band: bool,
    pub band_color: String,
    /// 0–100
    pub band_opacity: u8,
    /// N25: the layouts this theme changed (`theme::layouts`). Empty on a drawn style (`layouts::apply`).
    #[serde(skip_serializing_if = "Layouts::is_empty")]
    pub layouts: Layouts,
    /// The fields below are set by a layout on the drawn style. Italic text.
    #[serde(skip_serializing_if = "is_false")]
    pub italic: bool,
    /// Each line starts with a bullet.
    #[serde(skip_serializing_if = "is_false")]
    pub bullets: bool,
    /// The lines after the first at this percent of the text size; 0 is the same size.
    #[serde(skip_serializing_if = "is_zero")]
    pub subtitle: u8,
    /// The text box; None is the whole screen inside its margins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<Frame>,
}

fn is_false(b: &bool) -> bool { !*b }
fn is_zero(n: &u8) -> bool { *n == 0 }

impl Default for Theme {
    fn default() -> Self {
        Self {
            font: "Segoe UI".into(),
            size: 84,
            color: "#ffffff".into(),
            shadow: true,
            position: "center".into(),
            uppercase: false,
            scripture_size: 64,
            weight: 600,
            align: TextAlign::Center,
            outline: 0,
            outline_color: "#000000".into(),
            band: false,
            band_color: "#000000".into(),
            band_opacity: 55,
            layouts: Layouts::default(),
            italic: false,
            bullets: false,
            subtitle: 0,
            frame: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    Left,
    #[default]
    Center,
    Right,
}

/// Brings a hand-edited or sent style back into range: sizes 24–200, weight snapped to 400/600/700,
/// outline at most 12, band opacity at most 100, and an unknown position becomes "center".
pub fn clamp_style(t: &mut Theme) {
    t.size = t.size.clamp(24, 200);
    t.scripture_size = t.scripture_size.clamp(24, 200);
    t.weight = match t.weight {
        w if w < 500 => 400,
        w if w < 650 => 600,
        _ => 700,
    };
    t.outline = t.outline.min(12);
    t.band_opacity = t.band_opacity.min(100);
    if !matches!(t.position.as_str(), "center" | "lower" | "top") {
        t.position = "center".into();
    }
    t.subtitle = t.subtitle.min(100);
    t.frame = t.frame.map(|f| f.clamped());
    for l in t.layouts.0.values_mut() {
        clamp_layout(l);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_theme_json_loads_with_todays_defaults() {
        let t: Theme = serde_json::from_str(r##"{"font":"Segoe UI","size":84,"color":"#ffffff","shadow":true,"position":"center","uppercase":false,"scripture_size":64}"##).unwrap();
        assert_eq!(t, Theme::default());
        assert_eq!((t.weight, t.align, t.outline, t.band, t.band_opacity), (600, TextAlign::Center, 0, false, 55));
        assert_eq!((t.outline_color.as_str(), t.band_color.as_str()), ("#000000", "#000000"));
    }

    #[test]
    fn theme_round_trips() {
        let t = Theme { align: TextAlign::Right, band: true, ..Theme::default() };
        assert_eq!(serde_json::from_value::<Theme>(serde_json::to_value(&t).unwrap()).unwrap(), t);
    }

    #[test]
    fn clamp_style_snaps_weight_and_limits() {
        let mut t = Theme { size: 500, scripture_size: 1, weight: 550, outline: 40, band_opacity: 200, position: "middle".into(), ..Theme::default() };
        clamp_style(&mut t);
        assert_eq!((t.size, t.scripture_size, t.weight, t.outline, t.band_opacity, t.position.as_str()), (200, 24, 600, 12, 100, "center"));
    }
}
