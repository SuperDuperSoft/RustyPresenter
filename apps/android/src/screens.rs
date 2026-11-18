//! The screens (`ui/app.slint`, compiled by `build.rs`) and how the Rust side fills them: shared by the phone app
//! (`android`) and the pictures drawn on a computer to check them (`examples/preview.rs`).
use crate::fit;
use slint::{Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};
use std::collections::HashMap;

slint::include_modules!();

pub fn s(x: impl Into<SharedString>) -> SharedString { x.into() }

/// A layout's colour (`#rrggbb`), or `fallback`.
pub fn color(hex: &str, fallback: slint::Color) -> slint::Color {
    crate::protocol::parse_color(hex).map_or(fallback, |[r, g, b, a]| slint::Color::from_argb_u8(a, r, g, b))
}

/// The stage boxes placed by `fit::place`, as the screen draws them; `pics` are the live and next slides' pictures.
pub fn stage_cells(cells: Vec<fit::Placed>, pics: &HashMap<String, SharedPixelBuffer<Rgba8Pixel>>) -> Vec<StageCell> {
    use slint::private_unstable_api::re_exports::{TextHorizontalAlignment as H, TextVerticalAlignment as V};
    let white = slint::Color::from_rgb_u8(255, 255, 255);
    let clear = slint::Color::from_argb_u8(0, 0, 0, 0);
    cells.into_iter().map(|fit::Placed { cell, x, y, w, h, size, caption, rows }| {
        // A chord sheet from an older computer has only its words.
        let kind = if cell.kind == "chords" && cell.lines.is_none() { "text".to_string() } else { cell.kind.clone() };
        let src = if cell.src == "live" { "live" } else { "next" };
        StageCell {
            x, y, w, h,
            kind: s(kind),
            text: s(cell.text.clone()),
            caption: s(cell.caption.clone().unwrap_or_default()),
            color: color(&cell.color, white),
            fill: cell.fill.as_deref().map_or(clear, |f| color(f, clear)),
            size, caption_size: caption.max(1.0), bold: cell.bold,
            halign: match cell.align.as_str() { "left" => H::Left, "right" => H::Right, _ => H::Center },
            valign: match cell.valign.as_str() { "top" => V::Top, "bottom" => V::Bottom, _ => V::Center },
            dim: cell.dim,
            picture: pics.get(src).filter(|_| cell.kind == "picture").map(|p| Image::from_rgba8(p.clone())).unwrap_or_default(),
            shape: s(cell.shape.clone()),
            rows: ModelRc::new(VecModel::from(rows.into_iter().map(|r| ChordRow {
                cols: ModelRc::new(VecModel::from(r.cols.into_iter().map(|(ch, wd)| ChordCol { chord: s(ch), words: s(wd) }).collect::<Vec<_>>())),
                chords: r.chords, words: r.words,
            }).collect::<Vec<_>>())),
            chord_color: color(&cell.chord_color, slint::Color::from_rgb_u8(0xf5, 0xb9, 0x4a)),
        }
    }).collect()
}
