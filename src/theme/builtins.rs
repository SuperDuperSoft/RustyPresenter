//! N25: the ready-made themes. Each one is a text style, a colour or gradient background (no picture files to ship)
//! and, where it helps, its own layouts. Fonts are the bundled Noto Sans or ones every Windows computer has; any
//! font missing on a computer is drawn in Noto Sans.
use uuid::Uuid;

use crate::theme::backgrounds::Background;
use crate::theme::layouts::{Frame, Layout, LayoutKind, Layouts, default_layout};
use crate::theme::{SavedTheme, TextAlign, Theme, ThemeId, LOWER_THIRD_ID, strings};

/// The fonts the built-in themes use: bundled, or on every Windows computer.
pub const SAFE_FONTS: [&str; 6] = ["Noto Sans", "Segoe UI", "Georgia", "Arial", "Verdana", "Trebuchet MS"];

const fn id(n: u128) -> ThemeId { Uuid::from_u128(0x00000000_0000_4000_8000_000000000000 | n) }

pub const MIDNIGHT_ID: ThemeId = id(0x103);
pub const CLEAN_WHITE_ID: ThemeId = id(0x104);
pub const CLASSIC_ID: ThemeId = id(0x105);
pub const OCEAN_ID: ThemeId = id(0x106);
pub const FOREST_ID: ThemeId = id(0x107);
pub const ROYAL_ID: ThemeId = id(0x108);
pub const BOLD_ID: ThemeId = id(0x109);
pub const SLATE_ID: ThemeId = id(0x10a);
pub const CHRISTMAS_ID: ThemeId = id(0x10b);
pub const EASTER_ID: ThemeId = id(0x10c);
pub const ADVENT_ID: ThemeId = id(0x10d);
pub const YOUTH_ID: ThemeId = id(0x10e);
pub const SUNRISE_ID: ThemeId = id(0x10f);
pub const OUTLINE_ID: ThemeId = id(0x110);

fn gradient(from: &str, to: &str, angle: u16) -> Option<Background> {
    Some(Background::Gradient { from: from.into(), to: to.into(), angle, dim: 0 })
}

fn color(c: &str) -> Option<Background> { Some(Background::Color { color: c.into() }) }

fn style(font: &str, color: &str) -> Theme { Theme { font: font.into(), color: color.into(), ..Theme::default() } }

/// A light theme: dark text with no shadow, and light bands.
fn light(font: &str, text: &str) -> Theme {
    Theme { shadow: false, band_color: "#ffffff".into(), band_opacity: 70, ..style(font, text) }
}

fn layouts(changes: &[(LayoutKind, Layout)]) -> Layouts {
    let mut l = Layouts::default();
    for (k, v) in changes { l.set(*k, v.clone()) }
    l
}

/// The lower-third built-in (FEAT-7's stream screen view uses it).
fn lower_third() -> SavedTheme {
    let style = Theme { size: 64, scripture_size: 52, position: "lower".into(), band: true, band_opacity: 55, ..Theme::default() };
    SavedTheme { id: LOWER_THIRD_ID, name: strings::lower_third_name(), style, background: None }
}

/// Every built-in theme, Lower third first.
pub fn all() -> Vec<SavedTheme> {
    let t = |id, name: String, style: Theme, background| SavedTheme { id, name, style, background };
    let left = |k: LayoutKind| Layout { align: Some(TextAlign::Left), ..default_layout(k) };
    vec![
        lower_third(),
        t(MIDNIGHT_ID, crate::tr!("Midnight"), style("Noto Sans", "#ffffff"), gradient("#0b1a33", "#000000", 180)),
        t(CLEAN_WHITE_ID, crate::tr!("Clean white"), Theme { weight: 600, ..light("Segoe UI", "#1b1b1f") }, color("#f4f2ee")),
        t(CLASSIC_ID, crate::tr!("Classic serif"), Theme { weight: 400, ..style("Georgia", "#f5ecd7") }, gradient("#2b1d14", "#0d0806", 180)),
        t(OCEAN_ID, crate::tr!("Ocean"), style("Segoe UI", "#ffffff"), gradient("#0f4c75", "#081a2b", 135)),
        t(FOREST_ID, crate::tr!("Forest"), style("Noto Sans", "#f1f5e9"), gradient("#1f3b2d", "#0a140f", 180)),
        t(ROYAL_ID, crate::tr!("Royal"), Theme { weight: 700, ..style("Georgia", "#f2d27a") }, gradient("#2a1450", "#0c0618", 180)),
        t(BOLD_ID, crate::tr!("Bold modern"), Theme { weight: 700, uppercase: true, ..style("Arial", "#ffffff") }, color("#111111")),
        t(SLATE_ID, crate::tr!("Slate"), Theme {
            align: TextAlign::Left,
            layouts: layouts(&[(LayoutKind::Title, left(LayoutKind::Title)), (LayoutKind::TitleSubtitle, left(LayoutKind::TitleSubtitle)),
                (LayoutKind::MainPoint, left(LayoutKind::MainPoint)), (LayoutKind::Quote, left(LayoutKind::Quote))]),
            ..style("Segoe UI", "#e8eef3")
        }, gradient("#2f3a45", "#161c22", 90)),
        t(CHRISTMAS_ID, crate::tr!("Christmas red"), style("Georgia", "#ffffff"), gradient("#7a1020", "#24040a", 180)),
        t(EASTER_ID, crate::tr!("Easter morning"), light("Georgia", "#3b2a4d"), gradient("#fff7e3", "#f1d6e6", 180)),
        t(ADVENT_ID, crate::tr!("Advent purple"), style("Georgia", "#ece4f7"), gradient("#3b1f5c", "#110920", 180)),
        t(YOUTH_ID, crate::tr!("Youth night"), Theme { weight: 700, outline: 3, outline_color: "#1a0638".into(), ..style("Trebuchet MS", "#ffffff") },
            gradient("#ff3d7f", "#4b2bd6", 135)),
        t(SUNRISE_ID, crate::tr!("Sunrise"), Theme { weight: 700, ..style("Verdana", "#fffaf0") }, gradient("#f29f4b", "#7c2346", 180)),
        // No background of its own: for words over a video or picture, with a thick outline.
        t(OUTLINE_ID, crate::tr!("Words over video"), Theme {
            weight: 700, outline: 6, layouts: layouts(&[(LayoutKind::Lyrics, Layout { frame: Some(Frame { x: 57, y: 440, w: 886, h: 499 }), position: "lower".into(), ..Layout::default() })]),
            ..style("Noto Sans", "#ffffff")
        }, None),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::layouts::apply;

    #[test]
    fn fifteen_built_in_themes_with_their_own_names_and_ids() {
        let all = all();
        assert_eq!(all.len(), 15);
        assert_eq!(all[0].id, LOWER_THIRD_ID);
        let mut ids: Vec<ThemeId> = all.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 15);
        let mut names: Vec<String> = all.iter().map(|t| t.name.to_lowercase()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 15);
        assert!(!ids.contains(&crate::theme::STANDARD_ID));
    }

    #[test]
    fn built_ins_use_safe_fonts_and_stay_in_range() {
        for t in all() {
            assert!(SAFE_FONTS.contains(&t.style.font.as_str()) || t.id == LOWER_THIRD_ID, "{}: {}", t.name, t.style.font);
            let mut clamped = t.style.clone();
            crate::theme::clamp_style(&mut clamped);
            assert_eq!(clamped, t.style, "{} is already in range", t.name);
        }
    }

    #[test]
    fn a_built_in_can_change_a_layout() {
        let slate = all().into_iter().find(|t| t.id == SLATE_ID).unwrap();
        assert_eq!(apply(&slate.style, LayoutKind::Title).align, TextAlign::Left);
        assert_eq!(apply(&slate.style, LayoutKind::Title).weight, 700, "the rest of the shipped Title stays");
    }
}
