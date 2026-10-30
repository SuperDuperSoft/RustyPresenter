//! Turns a laid-out text slide into pixels: lower-third gradient, soft shadows, then the text.
use super::color::parse_color;
use super::layout::{CANVAS_W, Fonts, TextSlide, clamp_canvas, layout, text_height};
use crate::model::Theme;
use cosmic_text::{Buffer, Color, SwashCache};
use tiny_skia::{Color as SkColor, GradientStop, LinearGradient, Paint, Pixmap, Point, Rect, SpreadMode, Transform};

/// A rendered text slide: premultiplied RGBA, transparent where there is no text.
#[derive(Clone, Debug)]
pub struct Rendered {
    pub width: u32,
    pub height: u32,
    pub rgba_premul: Vec<u8>,
    /// Body font size in device pixels (theme size × canvas width / 1920, after fitting).
    pub font_px: f32,
    pub font_fallback: bool,
}

/// A CSS-like text shadow: offset and blur radius in logical (1920-wide) pixels.
struct Shadow { dy: f32, blur: f32, alpha: f32 }

const BODY_SHADOWS: [Shadow; 2] = [Shadow { dy: 3.0, blur: 14.0, alpha: 0.85 }, Shadow { dy: 0.0, blur: 3.0, alpha: 0.9 }];
const FOOT_SHADOWS: [Shadow; 1] = [Shadow { dy: 2.0, blur: 8.0, alpha: 0.9 }];
const FOOT_OPACITY: f32 = 0.88;
/// The lower-third gradient covers the bottom 55% of the screen, transparent to 55% black.
const LOWER_SHADE: f32 = 0.55;

pub fn render_text(fonts: &mut Fonts, slide: &TextSlide, theme: &Theme, canvas: (u32, u32)) -> Rendered {
    let canvas = clamp_canvas(canvas);
    let mut l = layout(fonts, slide, theme, canvas);
    let (w, h) = canvas;
    let s = w as f32 / CANVAS_W;
    let mut pix = Pixmap::new(w, h).expect("canvas is clamped to a drawable size");
    if l.lower { draw_lower_shade(&mut pix); }

    let color = parse_color(&theme.color);
    let mut cache = SwashCache::new();

    let body_mask = {
        let text_h = text_height(&l.body);
        let b = l.body_box;
        // Taller than the content box (it may still fit within the padding, as on the web): start at the top.
        let y = if text_h > b.h { b.y } else if l.lower { b.y + b.h - text_h } else { b.y + (b.h - text_h) / 2.0 };
        mask_of(fonts, &mut cache, &mut l.body, (w, h), (b.x, y))
    };
    if theme.shadow { for sh in &BODY_SHADOWS { composite_shadow(&mut pix, &body_mask, sh, s, 1.0); } }
    composite(&mut pix, &body_mask, (0, 0), color, 1.0);

    if let Some(foot) = l.foot.as_mut() {
        let foot_mask = mask_of(fonts, &mut cache, foot, (w, h), (l.body_box.x, l.foot_y));
        if theme.shadow { for sh in &FOOT_SHADOWS { composite_shadow(&mut pix, &foot_mask, sh, s, FOOT_OPACITY); } }
        composite(&mut pix, &foot_mask, (0, 0), color, FOOT_OPACITY);
    }

    Rendered { width: w, height: h, rgba_premul: pix.take(), font_px: l.font_px * s, font_fallback: l.font_fallback }
}

/// Render a buffer's glyph coverage into a canvas-sized alpha mask, with its top-left at `at`.
fn mask_of(fonts: &mut Fonts, cache: &mut SwashCache, buf: &mut Buffer, (w, h): (u32, u32), at: (f32, f32)) -> Vec<u8> {
    let mut mask = vec![0u8; (w * h) as usize];
    let (ox, oy) = (at.0.round() as i32, at.1.round() as i32);
    buf.draw(&mut fonts.sys, cache, Color::rgb(255, 255, 255), |x, y, gw, gh, c| {
        let a = c.a() as u32;
        if a == 0 { return; }
        for py in (y + oy).max(0)..(y + oy + gh as i32).min(h as i32) {
            for px in (x + ox).max(0)..(x + ox + gw as i32).min(w as i32) {
                let m = &mut mask[(py as u32 * w + px as u32) as usize];
                *m = (*m as u32 + a * (255 - *m as u32) / 255) as u8;
            }
        }
    });
    mask
}

fn draw_lower_shade(pix: &mut Pixmap) {
    let h = pix.height() as f32;
    let top = h * (1.0 - LOWER_SHADE);
    let Some(shader) = LinearGradient::new(
        Point::from_xy(0.0, top), Point::from_xy(0.0, h),
        vec![GradientStop::new(0.0, SkColor::from_rgba8(0, 0, 0, 0)), GradientStop::new(1.0, SkColor::from_rgba(0.0, 0.0, 0.0, 0.55).unwrap())],
        SpreadMode::Pad, Transform::identity(),
    ) else { return };
    let paint = Paint { shader, anti_alias: false, ..Paint::default() };
    if let Some(r) = Rect::from_xywh(0.0, top, pix.width() as f32, h - top) { pix.fill_rect(r, &paint, Transform::identity(), None); }
}

fn composite_shadow(pix: &mut Pixmap, mask: &[u8], sh: &Shadow, s: f32, opacity: f32) {
    let (w, h) = (pix.width(), pix.height());
    let mut blurred = mask.to_vec();
    // A CSS blur radius is two standard deviations of a Gaussian.
    gaussian_blur(&mut blurred, w, h, sh.blur * s / 2.0);
    composite(pix, &blurred, (0, (sh.dy * s).round() as i32), [0, 0, 0, 255], sh.alpha * opacity);
}

/// Paint `color` through `mask` (offset by `off`) over the premultiplied pixmap.
fn composite(pix: &mut Pixmap, mask: &[u8], off: (i32, i32), color: [u8; 4], opacity: f32) {
    let (w, h) = (pix.width() as i32, pix.height() as i32);
    let ca = color[3] as f32 / 255.0 * opacity;
    let data = pix.data_mut();
    for y in 0..h {
        let sy = y - off.1;
        if sy < 0 || sy >= h { continue; }
        for x in 0..w {
            let sx = x - off.0;
            if sx < 0 || sx >= w { continue; }
            let m = mask[(sy * w + sx) as usize];
            if m == 0 { continue; }
            let a = m as f32 / 255.0 * ca;
            let i = ((y * w + x) * 4) as usize;
            for c in 0..3 { data[i + c] = (color[c] as f32 * a + data[i + c] as f32 * (1.0 - a)).round() as u8; }
            data[i + 3] = (255.0 * a + data[i + 3] as f32 * (1.0 - a)).round() as u8;
        }
    }
}

/// Approximate a Gaussian blur of standard deviation `sigma` with three box blurs.
fn gaussian_blur(mask: &mut [u8], w: u32, h: u32, sigma: f32) {
    if sigma < 0.5 { return; }
    let r = (((4.0 * sigma * sigma + 1.0).sqrt() - 1.0) / 2.0).round().max(1.0) as usize;
    let mut tmp = vec![0u8; mask.len()];
    for _ in 0..3 {
        box_pass(mask, &mut tmp, w as usize, h as usize, r, true);
        box_pass(&tmp, mask, w as usize, h as usize, r, false);
    }
}

/// One box-blur pass along rows (`horizontal`) or columns, with a running sum.
fn box_pass(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize, horizontal: bool) {
    let (len, lines) = if horizontal { (w, h) } else { (h, w) };
    let idx = |line: usize, i: usize| if horizontal { line * w + i } else { i * w + line };
    let win = (2 * r + 1) as u32;
    for line in 0..lines {
        let mut sum: u32 = 0;
        for i in 0..=r.min(len - 1) { sum += src[idx(line, i)] as u32; }
        for i in 0..len {
            dst[idx(line, i)] = (sum / win) as u8;
            if i + r + 1 < len { sum += src[idx(line, i + r + 1)] as u32; }
            if i >= r { sum -= src[idx(line, i - r)] as u32; }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::layout::{BUNDLED_FAMILY, TextLayout};
    use cosmic_text::Weight;

    fn slide(text: &str) -> TextSlide<'_> { TextSlide { text, footer: None, reference: None, scripture: false } }

    /// Whether some face in the database has a glyph for every non-space character of `text`.
    fn some_face_covers(fonts: &mut Fonts, text: &str) -> bool {
        let ids: Vec<_> = fonts.sys.db().faces().map(|f| f.id).collect();
        text.chars().filter(|c| !c.is_whitespace()).all(|c| {
            ids.iter().any(|&id| fonts.sys.get_font(id, Weight::NORMAL).is_some_and(|f| f.as_swash().charmap().map(c) != 0))
        })
    }

    /// Glyphs that came out as the missing-glyph box (.notdef, glyph 0).
    fn missing_glyphs(l: &TextLayout) -> usize {
        l.body.layout_runs().flat_map(|r| r.glyphs.iter()).filter(|g| g.glyph_id == 0).count()
    }

    fn inked_pixels(r: &Rendered) -> usize { r.rgba_premul.chunks(4).filter(|p| p[3] > 0).count() }

    /// `text` shapes to real glyphs (no missing-glyph boxes) and draws them. No shadow, so every
    /// inked pixel is a glyph. Skipped, with a note, when no installed font covers the text.
    fn assert_script_draws(fonts: &mut Fonts, text: &str) {
        if !some_face_covers(fonts, text) {
            println!("SKIP: no installed font covers {text:?}; nothing to check on this machine");
            return;
        }
        let t = Theme { shadow: false, ..Theme::default() };
        let l = layout(fonts, &slide(text), &t, (1920, 1080));
        assert_eq!(missing_glyphs(&l), 0, "{text:?}: a covering font is installed but glyphs came out missing");
        let r = render_text(fonts, &slide(text), &t, (1920, 1080));
        assert!(inked_pixels(&r) > 500, "{text:?}: a covering font is installed but almost nothing was drawn");
    }

    #[test]
    fn korean_draws_with_an_installed_font() { assert_script_draws(&mut Fonts::new(), "놀라운 은혜"); }

    #[test]
    fn emoji_draw_with_an_installed_font() { assert_script_draws(&mut Fonts::new(), "🙏🎵"); }

    #[test]
    fn latin_accents_draw_with_bundled_noto_sans() {
        let mut fonts = Fonts::bundled_only();
        assert!(some_face_covers(&mut fonts, "Señor ñáéíóúü"), "Noto Sans covers Latin accents");
        assert_script_draws(&mut fonts, "Señor ñáéíóúü");
    }

    /// The checks above can fail: without a covering font the glyphs come out missing.
    #[test]
    fn missing_glyphs_are_detected() {
        let mut fonts = Fonts::bundled_only();
        assert!(!some_face_covers(&mut fonts, "놀라운"));
        let t = Theme { font: BUNDLED_FAMILY.into(), ..Theme::default() };
        assert!(missing_glyphs(&layout(&mut fonts, &slide("놀라운"), &t, (1920, 1080))) > 0);
    }
}

