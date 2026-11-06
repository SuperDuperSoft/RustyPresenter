//! Two or three Bibles on one scripture slide (Nov 5 spec §3): equal columns side by side, or rows one above the
//! other, each headed by its Bible's code, all at one text size that the fullest column decides.
use super::layout::{Fonts, Rect, fit_steps, max_line_width, text_height};
use crate::scripture::{Parallel, ParallelLayout};
use cosmic_text::{Align, Attrs, Buffer, Family, Metrics, Shaping, Weight, Wrap};

/// Gap between columns: 4 % of the canvas width (FEAT-5 spec §6).
const GUTTER: f32 = 0.04;
/// Gap between rows, as a share of the canvas height.
const ROW_GAP: f32 = 0.03;
/// Dividers are 2 px wide at 1080p scale.
const DIVIDER_PX: f32 = 2.0;
/// Column headings: 80 % of the reference size, semibold.
const HEADING_SCALE: f32 = 0.8;
/// Lower-third themes have room for two Bibles, stacked.
pub const LOWER_THIRD_MAX: usize = 2;

/// One Bible's place on the slide: its heading, its text and where they go.
pub struct Column {
    pub heading: Buffer,
    pub heading_at: (f32, f32),
    pub body: Buffer,
    /// The text's box, under the heading.
    pub rect: Rect,
}

pub struct Columns {
    /// The main Bible first, then the Also Bibles in order.
    pub columns: Vec<Column>,
    pub dividers: Vec<Rect>,
    /// The shared body size, in 1080p-canvas pixels.
    pub font_px: f32,
}

/// What the parallel layout needs from the single-text layout.
pub struct Frame<'a> {
    pub family: &'a str,
    /// Canvas pixels per 1080p pixel.
    pub scale: f32,
    pub canvas: (f32, f32),
    /// The box the single text would use: the columns share it.
    pub body_box: Rect,
    pub max_px: f32,
    pub line: f32,
    pub reference_px: f32,
    pub lower: bool,
}

/// The texts in column order: the main one, then the Also Bibles (two at most on a lower-third theme).
pub fn texts<'a>(main: &'a str, p: &'a Parallel, lower: bool) -> Vec<(&'a str, &'a str)> {
    let mut out = vec![(p.main_code.as_str(), main)];
    out.extend(p.also.iter().map(|a| (a.code.as_str(), a.text.as_str())));
    if lower { out.truncate(LOWER_THIRD_MAX) }
    out
}

/// Lays out `texts` (code, text) in `f.body_box`. Lower-third themes always stack.
pub fn columns(fonts: &mut Fonts, f: &Frame, texts: &[(&str, &str)], layout: ParallelLayout) -> Columns {
    let s = f.scale;
    let n = texts.len().max(1) as f32;
    let stacked = f.lower || layout == ParallelLayout::Stacked;
    let b = f.body_box;
    let boxes: Vec<Rect> = (0..texts.len()).map(|i| {
        let i = i as f32;
        if stacked {
            let gap = ROW_GAP * f.canvas.1;
            let h = ((b.h - gap * (n - 1.0)) / n).max(0.0);
            Rect { x: b.x, y: b.y + i * (h + gap), w: b.w, h }
        } else {
            let gap = GUTTER * f.canvas.0;
            let w = ((b.w - gap * (n - 1.0)) / n).max(0.0);
            Rect { x: b.x + i * (w + gap), y: b.y, w, h: b.h }
        }
    }).collect();
    let dividers = boxes.windows(2).map(|w| {
        let (a, c) = (w[0], w[1]);
        if stacked {
            Rect { x: a.x, y: (a.y + a.h + c.y) / 2.0 - DIVIDER_PX * s / 2.0, w: a.w, h: DIVIDER_PX * s }
        } else {
            Rect { x: (a.x + a.w + c.x) / 2.0 - DIVIDER_PX * s / 2.0, y: a.y, w: DIVIDER_PX * s, h: a.h }
        }
    }).collect();

    let heading_px = f.reference_px * HEADING_SCALE * s;
    let heading_attrs = Attrs::new().family(Family::Name(f.family)).weight(Weight(600));
    let body_attrs = Attrs::new().family(Family::Name(f.family)).weight(Weight(500));
    let mut columns: Vec<Column> = texts.iter().zip(&boxes).map(|((code, text), r)| {
        let mut heading = Buffer::new(&mut fonts.sys, Metrics::new(heading_px, heading_px * 1.25));
        heading.set_size(Some(r.w), None);
        heading.set_wrap(Wrap::None);
        heading.set_text(code, &heading_attrs, Shaping::Advanced, Some(Align::Left));
        heading.shape_until_scroll(&mut fonts.sys, false);
        let hh = text_height(&heading) + 6.0 * s;
        let rect = Rect { x: r.x, y: r.y + hh, w: r.w, h: (r.h - hh).max(0.0) };
        let mut body = Buffer::new(&mut fonts.sys, Metrics::new(f.max_px * s, f.max_px * s * f.line));
        body.set_size(Some(rect.w), None);
        body.set_wrap(Wrap::WordOrGlyph);
        body.set_text(text, &body_attrs, Shaping::Advanced, Some(Align::Left));
        Column { heading, heading_at: (r.x, r.y), body, rect }
    }).collect();

    let mut font_px = super::layout::MIN_PX;
    for px in fit_steps(f.max_px) {
        font_px = px;
        let mut all = true;
        for c in &mut columns {
            c.body.set_metrics(Metrics::new(px * s, px * s * f.line));
            c.body.shape_until_scroll(&mut fonts.sys, false);
            if text_height(&c.body) > c.rect.h + 1.0 || max_line_width(&c.body) > c.rect.w + 1.0 { all = false }
        }
        if all { break }
    }
    Columns { columns, dividers, font_px }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripture::ParallelText;

    fn frame(lower: bool) -> Frame<'static> {
        Frame {
            family: super::super::layout::BUNDLED_FAMILY, scale: 1.0, canvas: (1920.0, 1080.0),
            body_box: Rect { x: 110.0, y: 66.0, w: 1700.0, h: 860.0 }, max_px: 64.0, line: 1.32, reference_px: 44.0, lower,
        }
    }

    fn three() -> Parallel {
        Parallel {
            main_code: "KJV".into(),
            also: vec![ParallelText { code: "WEB".into(), text: "For God so loved the world".into() }, ParallelText { code: "RVR".into(), text: "Porque de tal manera amó Dios al mundo".into() }],
            layout: ParallelLayout::SideBySide,
        }
    }

    #[test]
    fn three_columns_equal_width_with_two_dividers() {
        let p = three();
        let c = columns(&mut Fonts::bundled_only(), &frame(false), &texts("For God so loved the world", &p, false), ParallelLayout::SideBySide);
        assert_eq!((c.columns.len(), c.dividers.len()), (3, 2));
        let w: Vec<f32> = c.columns.iter().map(|c| c.rect.w).collect();
        assert!(w.iter().all(|x| (x - w[0]).abs() < 1.0), "{w:?}");
        assert!(c.columns[0].rect.x < c.columns[1].rect.x && c.columns[1].rect.x < c.columns[2].rect.x);
        assert!(c.dividers.iter().all(|d| d.h > d.w), "vertical dividers");
    }

    #[test]
    fn stacked_makes_rows() {
        let p = three();
        let c = columns(&mut Fonts::bundled_only(), &frame(false), &texts("a", &p, false), ParallelLayout::Stacked);
        assert!(c.columns[0].rect.y < c.columns[1].rect.y && c.columns[1].rect.y < c.columns[2].rect.y);
        assert!(c.dividers.iter().all(|d| d.w > d.h), "horizontal dividers");
    }

    #[test]
    fn one_size_for_all_columns_and_nothing_overflows() {
        let mut p = three();
        p.also[1].text = "Porque de tal manera amó Dios al mundo, que ha dado a su Hijo unigénito. ".repeat(8);
        let c = columns(&mut Fonts::bundled_only(), &frame(false), &texts("For God so loved the world", &p, false), ParallelLayout::SideBySide);
        assert!(c.font_px < 64.0, "the long column shrinks everything: {}", c.font_px);
        let px: Vec<f32> = c.columns.iter().map(|c| c.body.metrics().font_size).collect();
        assert!(px.iter().all(|x| (x - px[0]).abs() < 0.01));
        assert!(c.columns.iter().all(|c| text_height(&c.body) <= c.rect.h + 1.0 && c.rect.y + c.rect.h <= 66.0 + 860.0 + 0.5));
    }

    #[test]
    fn lower_third_draws_two_of_three_stacked() {
        let p = three();
        let t = texts("a", &p, true);
        assert_eq!(t.len(), 2);
        let c = columns(&mut Fonts::bundled_only(), &frame(true), &t, ParallelLayout::SideBySide);
        assert!(c.columns[0].rect.y < c.columns[1].rect.y);
    }
}
