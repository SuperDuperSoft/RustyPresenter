//! Turns a laid-out text slide into pixels: lower-third gradient, soft shadows, then the text.
use super::color::parse_color;
use super::layout::{CANVAS_W, Fonts, TextSlide, clamp_canvas, layout, text_height};
use crate::model::Theme;
use cosmic_text::{Buffer, Color, SwashCache};
use slint::{Rgba8Pixel, SharedPixelBuffer};
use tiny_skia::{Color as SkColor, GradientStop, LinearGradient, Paint, PixmapMut, Point, Rect, SpreadMode, Transform};

/// Premultiplied RGBA pixels, drawn straight into the buffer a Slint image is made from, so showing a slide copies
/// nothing (a 2560×1080 slide is 10.6 MB). Reads as a byte slice.
#[derive(Clone)]
pub struct Premul(SharedPixelBuffer<Rgba8Pixel>);

impl Premul {
    /// Transparent pixels.
    pub fn zeroed(width: u32, height: u32) -> Premul { Premul(SharedPixelBuffer::new(width, height)) }

    /// Pixels already premultiplied.
    pub fn from_buffer(b: SharedPixelBuffer<Rgba8Pixel>) -> Premul { Premul(b) }

    /// The pixels for `slint::Image::from_rgba8_premultiplied` (shared, not copied).
    pub fn buffer(&self) -> SharedPixelBuffer<Rgba8Pixel> { self.0.clone() }
}

impl std::ops::Deref for Premul {
    type Target = [u8];
    fn deref(&self) -> &[u8] { self.0.as_bytes() }
}

impl std::fmt::Debug for Premul {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "Premul({}x{})", self.0.width(), self.0.height()) }
}

/// A rendered text slide: premultiplied RGBA, transparent where there is no text.
#[derive(Clone, Debug)]
pub struct Rendered {
    pub width: u32,
    pub height: u32,
    pub rgba_premul: Premul,
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
/// Column headings of a parallel slide are the text colour at 70 %, dividers at 25 % (FEAT-5 spec §6).
const HEADING_OPACITY: f32 = 0.7;
const DIVIDER_OPACITY: f32 = 0.25;

pub fn render_text(fonts: &mut Fonts, slide: &TextSlide, theme: &Theme, canvas: (u32, u32)) -> Rendered {
    let canvas = clamp_canvas(canvas);
    let mut l = layout(fonts, slide, theme, canvas);
    let (w, h) = canvas;
    let s = w as f32 / CANVAS_W;
    let mut px: SharedPixelBuffer<Rgba8Pixel> = SharedPixelBuffer::new(w, h);
    let mut pix = PixmapMut::from_bytes(px.make_mut_bytes(), w, h).expect("canvas is clamped to a drawable size");
    // A text band replaces the lower-third shade.
    let band = theme.band && l.columns.is_none();
    if l.lower && !band { draw_lower_shade(&mut pix); }

    let color = parse_color(&theme.color);
    let mut cache = SwashCache::new();

    if let Some(cols) = l.columns.as_mut() {
        for d in &cols.dividers {
            composite(&mut pix, &solid(d, (w, h)), 0, color, DIVIDER_OPACITY);
        }
        for c in &mut cols.columns {
            let heading = coverage_of(fonts, &mut cache, &mut c.heading, (w, h), c.heading_at);
            if theme.shadow { for sh in &FOOT_SHADOWS { composite_shadow(&mut pix, &heading, sh, s, HEADING_OPACITY); } }
            composite(&mut pix, &heading, 0, color, HEADING_OPACITY);
            // Columns start at the top, so verses side by side line up.
            let body = coverage_of(fonts, &mut cache, &mut c.body, (w, h), (c.rect.x, c.rect.y));
            if theme.shadow { for sh in &BODY_SHADOWS { composite_shadow(&mut pix, &body, sh, s, 1.0); } }
            composite(&mut pix, &body, 0, color, 1.0);
        }
    }
    let body = if l.columns.is_some() { Coverage::empty() } else {
        let text_h = text_height(&l.body);
        let b = l.body_box;
        // Taller than the content box (it may still fit within the padding, as on the web): start at the top.
        let y = if text_h > b.h || l.top { b.y } else if l.lower { b.y + b.h - text_h } else { b.y + (b.h - text_h) / 2.0 };
        coverage_of(fonts, &mut cache, &mut l.body, (w, h), (b.x, y))
    };
    if band && body.w > 0 {
        let p = BAND_PAD * s;
        let r = (body.x as f32 - p, body.y as f32 - p, body.w as f32 + 2.0 * p, body.h as f32 + 2.0 * p);
        fill_round_rect(&mut pix, r, BAND_RADIUS * s, parse_color(&theme.band_color), f32::from(theme.band_opacity.min(100)) / 100.0);
    }
    if theme.shadow { for sh in &BODY_SHADOWS { composite_shadow(&mut pix, &body, sh, s, 1.0); } }
    // The outline sits under the text and over its shadow.
    let outline = (f32::from(theme.outline.min(12)) * s).round() as usize;
    if outline > 0 && body.w > 0 {
        let mut ring = body.grown(outline, (w, h));
        dilate(&mut ring.data, ring.w, ring.h, outline);
        composite(&mut pix, &ring, 0, parse_color(&theme.outline_color), 1.0);
    }
    composite(&mut pix, &body, 0, color, 1.0);

    if let Some(foot) = l.foot.as_mut() {
        let foot = coverage_of(fonts, &mut cache, foot, (w, h), (l.body_box.x, l.foot_y));
        if theme.shadow { for sh in &FOOT_SHADOWS { composite_shadow(&mut pix, &foot, sh, s, FOOT_OPACITY); } }
        composite(&mut pix, &foot, 0, color, FOOT_OPACITY);
    }
    if let Some(credit) = l.credit.as_mut() {
        let credit = coverage_of(fonts, &mut cache, credit, (w, h), (l.body_box.x, l.credit_y));
        if theme.shadow { for sh in &FOOT_SHADOWS { composite_shadow(&mut pix, &credit, sh, s, FOOT_OPACITY); } }
        composite(&mut pix, &credit, 0, color, FOOT_OPACITY);
    }

    Rendered { width: w, height: h, rgba_premul: Premul(px), font_px: l.font_px * s, font_fallback: l.font_fallback }
}

/// Text band padding around the lines and its corner radius, in 1080p pixels (FEAT-1 spec §6.2).
const BAND_PAD: f32 = 28.0;
const BAND_RADIUS: f32 = 12.0;

/// A filled rectangle `(x, y, w, h)` with rounded corners, in `color` at `alpha`, over the pixmap.
pub(crate) fn fill_round_rect(pix: &mut PixmapMut, (x, y, w, h): (f32, f32, f32, f32), radius: f32, color: [u8; 4], alpha: f32) {
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    let Some(path) = pb.finish() else { return };
    let a = (f32::from(color[3]) / 255.0 * alpha).clamp(0.0, 1.0);
    let mut paint = Paint::default();
    paint.set_color(SkColor::from_rgba8(color[0], color[1], color[2], (a * 255.0).round() as u8));
    paint.anti_alias = true;
    pix.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
}

/// The text of `buf` with its top-left at `at`, on a transparent `size` picture: `under` draws first (a box), then
/// the slide's body shadow when `shadow`, then the text in `color`. `s` is the canvas scale (width / 1920).
pub(crate) fn text_picture(fonts: &mut Fonts, buf: &mut Buffer, size: (u32, u32), at: (f32, f32), color: [u8; 4], shadow: bool, s: f32,
    under: impl FnOnce(&mut PixmapMut)) -> Premul {
    let (w, h) = clamp_canvas(size);
    let mut px: SharedPixelBuffer<Rgba8Pixel> = SharedPixelBuffer::new(w, h);
    {
        let mut pix = PixmapMut::from_bytes(px.make_mut_bytes(), w, h).expect("a clamped size is drawable");
        under(&mut pix);
        let cov = coverage_of(fonts, &mut SwashCache::new(), buf, (w, h), at);
        if shadow { for sh in &BODY_SHADOWS { composite_shadow(&mut pix, &cov, sh, s, 1.0); } }
        composite(&mut pix, &cov, 0, color, 1.0);
    }
    Premul(px)
}

/// Grows coverage by `r` pixels each way (a square max filter, rows then columns).
fn dilate(data: &mut [f32], w: usize, h: usize, r: usize) {
    if r == 0 || w == 0 || h == 0 { return }
    let mut tmp = vec![0.0f32; data.len()];
    for y in 0..h {
        let row = &data[y * w..(y + 1) * w];
        for x in 0..w {
            tmp[y * w + x] = row[x.saturating_sub(r)..(x + r + 1).min(w)].iter().copied().fold(0.0, f32::max);
        }
    }
    for x in 0..w {
        for y in 0..h {
            data[y * w + x] = (y.saturating_sub(r)..(y + r + 1).min(h)).map(|yy| tmp[yy * w + x]).fold(0.0, f32::max);
        }
    }
}

/// Coverage (0–1) of a box of the canvas; everything outside the box is uncovered. Blurring and
/// compositing only visit this box, not the whole canvas.
#[derive(Clone, Debug, PartialEq)]
struct Coverage {
    /// Top-left corner on the canvas. The box always lies inside the canvas.
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    /// Row-major, `w * h` values.
    data: Vec<f32>,
}

impl Coverage {
    fn empty() -> Self { Self { x: 0, y: 0, w: 0, h: 0, data: Vec::new() } }

    /// The same coverage in a box grown by `pad` on every side, cut to the `canvas`.
    fn grown(&self, pad: usize, (cw, ch): (u32, u32)) -> Self {
        if self.w == 0 || self.h == 0 { return Self::empty(); }
        let (x0, y0) = (self.x.saturating_sub(pad), self.y.saturating_sub(pad));
        let (x1, y1) = ((self.x + self.w + pad).min(cw as usize), (self.y + self.h + pad).min(ch as usize));
        let (w, h) = (x1 - x0, y1 - y0);
        let mut data = vec![0.0; w * h];
        for row in 0..self.h {
            let at = (self.y - y0 + row) * w + (self.x - x0);
            data[at..at + self.w].copy_from_slice(&self.data[row * self.w..(row + 1) * self.w]);
        }
        Self { x: x0, y: y0, w, h, data }
    }
}

/// Render a buffer's glyph coverage, with its top-left at `at`, cut to the canvas and to the
/// smallest box holding every covered pixel.
fn coverage_of(fonts: &mut Fonts, cache: &mut SwashCache, buf: &mut Buffer, (w, h): (u32, u32), at: (f32, f32)) -> Coverage {
    let mut mask = vec![0u8; (w * h) as usize];
    let (ox, oy) = (at.0.round() as i32, at.1.round() as i32);
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    buf.draw(&mut fonts.sys, cache, Color::rgb(255, 255, 255), |x, y, gw, gh, c| {
        let a = c.a() as u32;
        if a == 0 { return; }
        let (px0, px1) = ((x + ox).max(0), (x + ox + gw as i32).min(w as i32));
        let (py0, py1) = ((y + oy).max(0), (y + oy + gh as i32).min(h as i32));
        if px0 >= px1 || py0 >= py1 { return; }
        (x0, y0, x1, y1) = (x0.min(px0), y0.min(py0), x1.max(px1), y1.max(py1));
        for py in py0..py1 {
            for px in px0..px1 {
                let m = &mut mask[(py as u32 * w + px as u32) as usize];
                *m = (*m as u32 + a * (255 - *m as u32) / 255) as u8;
            }
        }
    });
    if x0 >= x1 { return Coverage::empty(); }
    let (x, y, cw, ch) = (x0 as usize, y0 as usize, (x1 - x0) as usize, (y1 - y0) as usize);
    let mut data = Vec::with_capacity(cw * ch);
    for row in y..y + ch {
        data.extend(mask[row * w as usize + x..row * w as usize + x + cw].iter().map(|&m| m as f32 / 255.0));
    }
    Coverage { x, y, w: cw, h: ch, data }
}

/// Full coverage of a rectangle, cut to the canvas.
fn solid(r: &super::layout::Rect, (cw, ch): (u32, u32)) -> Coverage {
    let (x0, y0) = (r.x.round().max(0.0) as usize, r.y.round().max(0.0) as usize);
    let (x1, y1) = (((r.x + r.w).round().max(0.0) as usize).min(cw as usize), ((r.y + r.h).round().max(0.0) as usize).min(ch as usize));
    if x1 <= x0 || y1 <= y0 { return Coverage::empty() }
    Coverage { x: x0, y: y0, w: x1 - x0, h: y1 - y0, data: vec![1.0; (x1 - x0) * (y1 - y0)] }
}

fn draw_lower_shade(pix: &mut PixmapMut) {
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

fn composite_shadow(pix: &mut PixmapMut, cov: &Coverage, sh: &Shadow, s: f32, opacity: f32) {
    // A CSS blur radius is two standard deviations of a Gaussian.
    let sigma = sh.blur * s / 2.0;
    // Grown by exactly how far the blur reaches, the box holds the whole shadow.
    let mut shadow = cov.grown(blur_reach(sigma), (pix.width(), pix.height()));
    blur(&mut shadow.data, shadow.w, shadow.h, sigma);
    composite(pix, &shadow, (sh.dy * s).round() as i32, [0, 0, 0, 255], sh.alpha * opacity);
}

/// Paint `color` through `cov`, moved down by `dy` pixels, over the premultiplied pixmap.
fn composite(pix: &mut PixmapMut, cov: &Coverage, dy: i32, color: [u8; 4], opacity: f32) {
    let (w, h) = (pix.width() as usize, pix.height() as i32);
    let ca = color[3] as f32 / 255.0 * opacity;
    let rgb = [color[0] as f32, color[1] as f32, color[2] as f32];
    let data = pix.data_mut();
    for row in 0..cov.h {
        let y = cov.y as i32 + row as i32 + dy;
        if y < 0 || y >= h { continue; }
        let src = &cov.data[row * cov.w..(row + 1) * cov.w];
        let dst = &mut data[(y as usize * w + cov.x) * 4..(y as usize * w + cov.x + cov.w) * 4];
        for (&m, p) in src.iter().zip(dst.chunks_exact_mut(4)) {
            if m <= 0.0 { continue; }
            let a = m.min(1.0) * ca;
            for c in 0..3 { p[c] = (rgb[c] * a + p[c] as f32 * (1.0 - a)).round() as u8; }
            p[3] = (255.0 * a + p[3] as f32 * (1.0 - a)).round() as u8;
        }
    }
}

/// Below this σ the blur is an exact (sampled) Gaussian kernel; above it, three box blurs, whose
/// cost does not grow with σ.
const BOX_BLUR_MIN_SIGMA: f32 = 2.0;

/// How far, in pixels, `blur` spreads coverage.
fn blur_reach(sigma: f32) -> usize {
    if !(sigma > 0.0) { 0 } else if sigma < BOX_BLUR_MIN_SIGMA { kernel_radius(sigma) } else { box_radii(sigma).iter().sum() }
}

fn kernel_radius(sigma: f32) -> usize { (3.0 * sigma).ceil() as usize }

/// Blur coverage values with a Gaussian of standard deviation `sigma`. Outside the slice counts as
/// uncovered (as past the canvas edge).
fn blur(data: &mut [f32], w: usize, h: usize, sigma: f32) {
    if !(sigma > 0.0) || w == 0 || h == 0 { return; }
    let mut tmp = vec![0.0f32; data.len()];
    if sigma < BOX_BLUR_MIN_SIGMA {
        let k = gaussian_kernel(sigma);
        convolve_rows(data, &mut tmp, w, &k);
        convolve_cols(&tmp, data, w, h, &k);
    } else {
        for r in box_radii(sigma) {
            if r == 0 { continue; }
            box_rows(data, &mut tmp, w, r);
            box_cols(&tmp, data, w, h, r);
        }
    }
}

/// Normalized Gaussian weights for offsets −radius..=radius.
fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    let r = kernel_radius(sigma) as i32;
    let k: Vec<f32> = (-r..=r).map(|x| (-(x * x) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let total: f32 = k.iter().sum();
    k.into_iter().map(|v| v / total).collect()
}

fn convolve_rows(src: &[f32], dst: &mut [f32], w: usize, k: &[f32]) {
    let r = k.len() / 2;
    for (s, d) in src.chunks_exact(w).zip(dst.chunks_exact_mut(w)) {
        for (x, out) in d.iter_mut().enumerate() {
            let (lo, hi) = (x.saturating_sub(r), (x + r + 1).min(w));
            *out = s[lo..hi].iter().zip(&k[lo + r - x..hi + r - x]).map(|(a, b)| a * b).sum();
        }
    }
}

fn convolve_cols(src: &[f32], dst: &mut [f32], w: usize, h: usize, k: &[f32]) {
    let r = k.len() / 2;
    dst.fill(0.0);
    for y in 0..h {
        let out = &mut dst[y * w..(y + 1) * w];
        for sy in y.saturating_sub(r)..(y + r + 1).min(h) {
            let weight = k[sy + r - y];
            for (o, s) in out.iter_mut().zip(&src[sy * w..(sy + 1) * w]) { *o += weight * s; }
        }
    }
}

/// Three box radii whose combined variance, Σ r(r+1)/3, is as close to σ² as boxes allow
/// (radii at most 2 apart, so the result stays bell-shaped).
fn box_radii(sigma: f32) -> [usize; 3] {
    let want = 3.0 * sigma * sigma;
    let n = (2.0 * sigma).ceil() as usize + 1;
    let mut best = ([0; 3], f32::INFINITY);
    for r1 in 0..=n {
        for r2 in r1..=(r1 + 2).min(n) {
            for r3 in r2..=(r1 + 2).min(n) {
                let err = ([r1, r2, r3].iter().map(|r| r * (r + 1)).sum::<usize>() as f32 - want).abs();
                if err < best.1 { best = ([r1, r2, r3], err); }
            }
        }
    }
    best.0
}

/// One box blur of radius `r` along each row, with a running sum.
fn box_rows(src: &[f32], dst: &mut [f32], w: usize, r: usize) {
    let inv = 1.0 / (2 * r + 1) as f32;
    for (s, d) in src.chunks_exact(w).zip(dst.chunks_exact_mut(w)) {
        let mut sum: f32 = s[..=r.min(w - 1)].iter().sum();
        for i in 0..w {
            d[i] = sum * inv;
            if i + r + 1 < w { sum += s[i + r + 1]; }
            if i >= r { sum -= s[i - r]; }
        }
    }
}

/// One box blur of radius `r` down each column, a whole row at a time.
fn box_cols(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let inv = 1.0 / (2 * r + 1) as f32;
    let row = |y: usize| &src[y * w..(y + 1) * w];
    let mut sum = vec![0.0f32; w];
    for y in 0..=r.min(h - 1) { for (a, b) in sum.iter_mut().zip(row(y)) { *a += b; } }
    for y in 0..h {
        for (d, a) in dst[y * w..(y + 1) * w].iter_mut().zip(&sum) { *d = a * inv; }
        if y + r + 1 < h { for (a, b) in sum.iter_mut().zip(row(y + r + 1)) { *a += b; } }
        if y >= r { for (a, b) in sum.iter_mut().zip(row(y - r)) { *a -= b; } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::layout::{BUNDLED_FAMILY, TextLayout};
    use tiny_skia::Pixmap;
    use cosmic_text::Weight;

    fn slide(text: &str) -> TextSlide<'_> { TextSlide { text, footer: None, reference: None, scripture: false, size: 0, passage: &[], parallel: None } }

    /// Blur a one-pixel vertical line and return the horizontal profile of the middle row
    /// (the 1-D kernel), with its total and variance.
    fn line_profile(sigma: f32) -> (Vec<f32>, f64, f64) {
        let (w, h) = (161, 161);
        let mut d = vec![0.0f32; w * h];
        for y in 0..h { d[y * w + w / 2] = 1.0; }
        blur(&mut d, w, h, sigma);
        let row = d[(h / 2) * w..(h / 2 + 1) * w].to_vec();
        let total: f64 = row.iter().map(|&v| v as f64).sum();
        let var = row.iter().enumerate().map(|(x, &v)| (x as f64 - (w / 2) as f64).powi(2) * v as f64).sum::<f64>() / total;
        (row, total, var)
    }

    /// CSS blurs with a true Gaussian (σ = blur radius / 2). Small shadows (the 3 px body
    /// shadow: σ 1.5 at 1080p, 0.75 on a 960-wide preview) used to come out as σ ≈ 1.41 whatever
    /// was asked, and the 14 px one as σ ≈ 7.5 instead of 7.
    #[test]
    fn blur_spread_matches_sigma() {
        for sigma in [0.75f32, 1.0, 1.5, 2.5, 3.5, 7.0, 14.0] {
            let (_, total, var) = line_profile(sigma);
            let want = (sigma * sigma) as f64;
            assert!((total - 1.0).abs() < 0.01, "σ {sigma}: blur must keep the ink, total {total:.4}");
            assert!((var - want).abs() <= want * 0.05, "σ {sigma}: variance {var:.3}, want {want:.3} ± 5%");
        }
    }

    /// Shadows are blurred and composited only around the text. That must give the same pixels as
    /// blurring the whole canvas, including where the text touches the canvas edges.
    #[test]
    fn shadow_around_the_text_matches_a_whole_canvas_shadow() {
        let canvas = (200u32, 120u32);
        let block = |x: usize, y: usize, w: usize, h: usize| Coverage { x, y, w, h, data: (0..w * h).map(|i| ((i * 37) % 11) as f32 / 10.0).collect() };
        for cov in [block(80, 50, 30, 12), block(0, 0, 25, 9), block(150, 100, 50, 20), block(0, 111, 200, 9)] {
            for sh in [Shadow { dy: 3.0, blur: 14.0, alpha: 0.85 }, Shadow { dy: 0.0, blur: 3.0, alpha: 0.9 }, Shadow { dy: -4.0, blur: 1.0, alpha: 1.0 }] {
                let mut got = Pixmap::new(canvas.0, canvas.1).unwrap();
                composite_shadow(&mut got.as_mut(), &cov, &sh, 1.0, 0.88);

                // The same shadow, blurred and composited over the whole canvas.
                let mut want = Pixmap::new(canvas.0, canvas.1).unwrap();
                let mut whole = cov.grown(400, canvas);
                assert_eq!((whole.w, whole.h), (200, 120));
                blur(&mut whole.data, whole.w, whole.h, sh.blur / 2.0);
                composite(&mut want.as_mut(), &whole, sh.dy as i32, [0, 0, 0, 255], sh.alpha * 0.88);

                let worst = got.data().iter().zip(want.data()).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
                assert!(worst <= 1, "block at ({}, {}), blur {}: off by {worst}", cov.x, cov.y, sh.blur);
                assert!(got.data().iter().any(|&v| v > 0));
            }
        }
    }

    #[test]
    fn empty_coverage_draws_nothing() {
        let mut pix = Pixmap::new(64, 64).unwrap();
        composite_shadow(&mut pix.as_mut(), &Coverage::empty(), &BODY_SHADOWS[0], 1.0, 1.0);
        composite(&mut pix.as_mut(), &Coverage::empty(), 0, [255, 255, 255, 255], 1.0);
        assert!(pix.data().iter().all(|&v| v == 0));
    }

    #[test]
    fn blur_is_symmetric_and_peaks_in_the_middle() {
        for sigma in [0.75f32, 7.0] {
            let (row, _, _) = line_profile(sigma);
            let c = row.len() / 2;
            for k in 1..c { assert!((row[c - k] - row[c + k]).abs() < 1e-4, "σ {sigma}: asymmetric at ±{k}"); }
            assert!(row[c] >= row[c + 1] && row[c + 1] >= row[c + 2], "σ {sigma}: not peaked");
        }
    }

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

