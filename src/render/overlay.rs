//! Overlay drawing (spec §6.4): one slot (a text sign, a picture, a clock) as a picture the size of what it shows,
//! and where it goes on the screen. The room and the live monitor put every slot of the frame over the slide.
use super::color::parse_color;
use super::layout::{BUNDLED_FAMILY, CANVAS_W, Fonts, clamp_canvas, max_line_width, text_height};
use super::raster::{Premul, Rendered, fill_round_rect, text_picture};
use crate::theme::overlay::{Anchor, BoxKind, OverlayLook, OverlaySlot, OverlaySource, Placement};
use cosmic_text::{Align, Attrs, Buffer, Family, Metrics, Shaping, Weight, Wrap};
use image::RgbaImage;
use slint::{Rgba8Pixel, SharedPixelBuffer};
use tiny_skia::{FilterQuality, PixmapMut, PixmapPaint, PixmapRef, Transform};

/// The smallest text size an overlay shrinks to, in 1080p pixels.
pub const MIN_OVERLAY_PX: f32 = 18.0;
/// Padding inside a box, each side, in 1080p pixels.
const BOX_PAD: f32 = 24.0;
const BOX_RADIUS: f32 = 10.0;
/// Room left around unboxed text for its shadow.
const SHADOW_ROOM: f32 = 16.0;
const MAX_LINES: usize = 3;
const LINE: f32 = 1.2;

/// One drawn overlay: its picture and where its top-left corner goes, in canvas pixels.
#[derive(Clone, Debug)]
pub struct Placed {
    pub x: u32,
    pub y: u32,
    pub img: Rendered,
}

/// Text sizes to try, largest first: −4 px above 60 px, then −2 px, down to `MIN_OVERLAY_PX` (a start below it is
/// kept as it is).
pub fn overlay_fit_steps(max: f32) -> Vec<f32> {
    let floor = MIN_OVERLAY_PX.min(max);
    let mut out = vec![max];
    let mut cur = max;
    while cur > floor {
        cur = (cur - if cur > 60.0 { 4.0 } else { 2.0 }).max(floor);
        out.push(cur);
    }
    out
}

/// Where a `w`×`h` box goes on the canvas for `place` (margins already scaled), kept inside the canvas.
fn position(place: &Placement, (w, h): (f32, f32), (cw, ch): (f32, f32), margin: f32, bar: bool) -> (f32, f32) {
    use Anchor::*;
    let col = match place.anchor { TopLeft | Left | BottomLeft => 0, Top | Center | Bottom => 1, TopRight | Right | BottomRight => 2 };
    let row = match place.anchor { TopLeft | Top | TopRight => 0, Left | Center | Right => 1, BottomLeft | Bottom | BottomRight => 2 };
    let x = if bar { 0.0 } else { [margin, (cw - w) / 2.0, cw - w - margin][col] };
    // A bar sits flush on its edge.
    let m = if bar { 0.0 } else { margin };
    let y = [m, (ch - h) / 2.0, ch - h - m][row];
    (x.clamp(0.0, (cw - w).max(0.0)), y.clamp(0.0, (ch - h).max(0.0)))
}

fn align(anchor: Anchor) -> Align {
    match anchor {
        Anchor::TopLeft | Anchor::Left | Anchor::BottomLeft => Align::Left,
        Anchor::TopRight | Anchor::Right | Anchor::BottomRight => Align::Right,
        _ => Align::Center,
    }
}

/// Draws `slot` for a `canvas` (width, height) screen. `text` is a bound source's text (the clock's, from
/// `overlay::bound_text`); `theme_font` is used when the overlay has no font of its own; `picture` is a picture
/// source's decoded file. `None` when there is nothing to draw (a newer kind, no text, no picture).
pub fn draw(fonts: &mut Fonts, slot: &OverlaySlot, text: Option<&str>, theme_font: &str, canvas: (u32, u32), picture: Option<&RgbaImage>) -> Option<Placed> {
    let canvas = clamp_canvas(canvas);
    match &slot.source {
        OverlaySource::Text { text: own, look, .. } => draw_text(fonts, text.unwrap_or(own), look, &slot.place, theme_font, canvas),
        OverlaySource::Clock { look, .. } => draw_text(fonts, text?, look, &slot.place, theme_font, canvas),
        OverlaySource::Image { opacity, .. } => draw_picture(picture?, *opacity, &slot.place, canvas),
        // FEAT-2: the caption on its own line above the time.
        OverlaySource::Timer { caption, look, .. } => {
            let time = text?;
            let shown = if caption.trim().is_empty() { time.to_string() } else { format!("{}\n{time}", caption.trim()) };
            draw_text(fonts, &shown, look, &slot.place, theme_font, canvas)
        }
        OverlaySource::Message { text: own, look, .. } => draw_text(fonts, own, look, &slot.place, theme_font, canvas),
        OverlaySource::Unknown => None,
    }
}

fn draw_text(fonts: &mut Fonts, text: &str, look: &OverlayLook, place: &Placement, theme_font: &str, canvas: (u32, u32)) -> Option<Placed> {
    let text = text.trim();
    if text.is_empty() { return None }
    let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
    let s = cw / CANVAS_W;
    let boxed = look.box_kind != BoxKind::None;
    let bar = look.box_kind == BoxKind::Bar;
    let pad = if boxed { BOX_PAD * s } else { 0.0 };
    let room = if look.shadow && !boxed { SHADOW_ROOM * s } else { 0.0 };
    let box_w = if bar { cw } else { cw * f32::from(place.width.clamp(5, 100)) / 100.0 };
    let avail = (box_w - 2.0 * pad).max(1.0);
    let wanted = if look.font.trim().is_empty() { theme_font } else { look.font.as_str() };
    let family = if fonts.has_family(wanted) { wanted.to_string() } else { BUNDLED_FAMILY.to_string() };
    let attrs = Attrs::new().family(Family::Name(&family)).weight(Weight(look.weight.min(700)));
    let start = (look.size.max(1) as f32).min(400.0);
    let mut buf = Buffer::new(&mut fonts.sys, Metrics::new(start * s, start * s * LINE));
    buf.set_size(Some(avail), None);
    buf.set_wrap(Wrap::WordOrGlyph);
    buf.set_text(text, &attrs, Shaping::Advanced, Some(align(place.anchor)));
    for px in overlay_fit_steps(start) {
        buf.set_metrics(Metrics::new(px * s, px * s * LINE));
        buf.shape_until_scroll(&mut fonts.sys, false);
        if buf.layout_runs().count() <= MAX_LINES && max_line_width(&buf) <= avail + 1.0 { break }
    }
    let (tw, th) = (max_line_width(&buf).ceil(), text_height(&buf).ceil());
    // Laid out across `avail`, the lines sit inside it by their alignment: shift them so the picture holds only the text.
    let shift = match align(place.anchor) { Align::Left => 0.0, Align::Right => avail - tw, _ => (avail - tw) / 2.0 };
    let content_w = if bar { cw } else { (tw + 2.0 * pad).min(cw) };
    let content_h = (th + 2.0 * pad).min(ch);
    let (w, h) = (content_w + 2.0 * room, content_h + 2.0 * room);
    let margin = f32::from(place.margin) * 10.0 * s;
    let (x, y) = position(place, (content_w, content_h), (cw, ch), margin, bar);
    let (x, y) = ((x - room).max(0.0), (y - room).max(0.0));
    let size = ((w.round() as u32).clamp(1, canvas.0), (h.round() as u32).clamp(1, canvas.1));
    let text_x = if bar { (cw - tw) / 2.0 - shift } else { room + pad - shift };
    let color = parse_color(&look.color);
    let box_color = parse_color(&look.box_color);
    let box_alpha = f32::from(look.box_opacity.min(100)) / 100.0;
    let img = text_picture(fonts, &mut buf, size, (text_x, room + pad), color, look.shadow, s, |pix| {
        if !boxed { return }
        let radius = if bar { 0.0 } else { BOX_RADIUS * s };
        fill_round_rect(pix, (room, room, content_w, content_h), radius, box_color, box_alpha);
    });
    Some(Placed { x: x.round() as u32, y: y.round() as u32, img: Rendered { width: size.0, height: size.1, rgba_premul: img, font_px: 0.0, font_fallback: family != wanted } })
}

fn draw_picture(picture: &RgbaImage, opacity: u8, place: &Placement, canvas: (u32, u32)) -> Option<Placed> {
    let (iw, ih) = picture.dimensions();
    if iw == 0 || ih == 0 { return None }
    let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
    let s = cw / CANVAS_W;
    let mut w = cw * f32::from(place.width.clamp(5, 100)) / 100.0;
    let mut h = w * ih as f32 / iw as f32;
    if h > ch { w *= ch / h; h = ch }
    let size = ((w.round() as u32).max(1), (h.round() as u32).max(1));
    let (x, y) = position(place, (size.0 as f32, size.1 as f32), (cw, ch), f32::from(place.margin) * 10.0 * s, false);
    let mut data = picture.as_raw().clone();
    for p in data.chunks_exact_mut(4) {
        let a = u16::from(p[3]);
        for c in &mut p[..3] { *c = ((u16::from(*c) * a + 127) / 255) as u8 }
    }
    let src = PixmapRef::from_bytes(&data, iw, ih)?;
    let mut px: SharedPixelBuffer<Rgba8Pixel> = SharedPixelBuffer::new(size.0, size.1);
    {
        let mut pix = PixmapMut::from_bytes(px.make_mut_bytes(), size.0, size.1)?;
        let paint = PixmapPaint { opacity: f32::from(opacity.min(100)) / 100.0, quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
        pix.draw_pixmap(0, 0, src, &paint, Transform::from_scale(size.0 as f32 / iw as f32, size.1 as f32 / ih as f32), None);
    }
    Some(Placed { x: x.round() as u32, y: y.round() as u32, img: Rendered { width: size.0, height: size.1, rgba_premul: Premul::from_buffer(px), font_px: 0.0, font_fallback: false } })
}

/// Puts each drawn overlay over a transparent `canvas`-sized picture, in order (later on top).
pub fn compose(placed: &[Placed], canvas: (u32, u32)) -> Rendered {
    let (w, h) = clamp_canvas(canvas);
    let mut px: SharedPixelBuffer<Rgba8Pixel> = SharedPixelBuffer::new(w, h);
    {
        let dst = px.make_mut_bytes();
        for p in placed {
            for row in 0..p.img.height {
                let y = p.y + row;
                if y >= h { break }
                for col in 0..p.img.width {
                    let x = p.x + col;
                    if x >= w { break }
                    let si = ((row * p.img.width + col) * 4) as usize;
                    let a = u32::from(p.img.rgba_premul[si + 3]);
                    if a == 0 { continue }
                    let di = ((y * w + x) * 4) as usize;
                    for c in 0..4 {
                        let s = u32::from(p.img.rgba_premul[si + c]);
                        let d = u32::from(dst[di + c]);
                        dst[di + c] = (s + (d * (255 - a) + 127) / 255).min(255) as u8;
                    }
                }
            }
        }
    }
    Rendered { width: w, height: h, rgba_premul: Premul::from_buffer(px), font_px: 0.0, font_fallback: false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::overlay::{ImageSource, saved_key};

    fn slot(source: OverlaySource, place: Placement) -> OverlaySlot {
        OverlaySlot { key: saved_key(uuid::Uuid::nil()), z: 0, inst: 1, name: "o".into(), source, place }
    }

    fn sign(text: &str, box_kind: BoxKind) -> OverlaySource {
        OverlaySource::Text { text: text.into(), styles: vec![], look: OverlayLook { box_kind, ..OverlayLook::default() } }
    }

    fn covered(p: &Placed) -> bool { p.img.rgba_premul.chunks_exact(4).any(|px| px[3] > 0) }

    #[test]
    fn fit_steps_stop_at_18_and_keep_small_starts() {
        assert_eq!(overlay_fit_steps(20.0), [20.0, 18.0]);
        assert_eq!(overlay_fit_steps(16.0), [16.0]);
        assert_eq!(overlay_fit_steps(64.0)[..3], [64.0, 60.0, 58.0]);
    }

    #[test]
    fn text_bar_bottom_spans_the_width_flush_on_the_edge() {
        let mut f = Fonts::bundled_only();
        let place = Placement { anchor: Anchor::Bottom, width: 40, margin: 4 };
        let p = draw(&mut f, &slot(sign("Wi-Fi: GraceGuest", BoxKind::Bar), place), None, "Noto Sans", (1920, 1080), None).unwrap();
        assert_eq!((p.x, p.img.width), (0, 1920));
        assert_eq!(p.y + p.img.height, 1080);
        assert!(covered(&p));
    }

    #[test]
    fn overlay_draw_is_bounding_box_only() {
        let mut f = Fonts::bundled_only();
        let place = Placement { anchor: Anchor::TopLeft, width: 60, margin: 4 };
        let p = draw(&mut f, &slot(sign("Hi", BoxKind::Behind), place), None, "", (1920, 1080), None).unwrap();
        assert!(p.img.width < 400 && p.img.height < 200, "{}x{}", p.img.width, p.img.height);
        assert_eq!((p.x, p.y), (40, 40), "the margin from the corner");
    }

    #[test]
    fn picture_top_right_scaled_to_width() {
        let pic = RgbaImage::from_pixel(100, 50, image::Rgba([255, 0, 0, 255]));
        let place = Placement { anchor: Anchor::TopRight, width: 10, margin: 4 };
        let src = OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 50 };
        let p = draw(&mut Fonts::bundled_only(), &slot(src, place), None, "", (1920, 1080), Some(&pic)).unwrap();
        assert_eq!((p.img.width, p.img.height), (192, 96));
        assert_eq!((p.x, p.y), (1920 - 192 - 40, 40));
        assert!((126..=129).contains(&p.img.rgba_premul[3]), "half see-through");
        assert!(draw(&mut Fonts::bundled_only(), &slot(OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 100 }, place), None, "", (1920, 1080), None).is_none(), "no picture");
    }

    #[test]
    fn overlay_never_leaves_canvas() {
        let mut f = Fonts::bundled_only();
        let tall = RgbaImage::from_pixel(1, 4000, image::Rgba([0, 0, 255, 255]));
        let wide = Placement { anchor: Anchor::BottomRight, width: 100, margin: 20 };
        for canvas in [(1024, 768), (0, 0), (64, 36)] {
            let c = clamp_canvas(canvas);
            for src in [OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 100 }, sign(&"long words ".repeat(40), BoxKind::Behind)] {
                if let Some(p) = draw(&mut f, &slot(src, wide), None, "", canvas, Some(&tall)) {
                    assert!(p.x + p.img.width <= c.0 && p.y + p.img.height <= c.1, "{canvas:?}");
                }
            }
        }
    }

    #[test]
    fn clock_draws_its_bound_text_only() {
        let mut f = Fonts::bundled_only();
        let clock = OverlaySource::Clock { look: OverlayLook { box_kind: BoxKind::Behind, ..OverlayLook::default() }, seconds: false, h24: false };
        let place = Placement { anchor: Anchor::BottomLeft, width: 20, margin: 4 };
        assert!(draw(&mut f, &slot(clock.clone(), place), None, "", (1920, 1080), None).is_none());
        let p = draw(&mut f, &slot(clock, place), Some("9:05 AM"), "", (1920, 1080), None).unwrap();
        assert_eq!(p.x, 40);
        assert_eq!(p.y + p.img.height, 1080 - 40);
    }

    #[test]
    fn on_4x3_keeps_corner() {
        let mut f = Fonts::bundled_only();
        let pic = RgbaImage::from_pixel(10, 10, image::Rgba([0, 255, 0, 255]));
        let place = Placement { anchor: Anchor::BottomRight, width: 12, margin: 4 };
        let src = OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 100 };
        let p = draw(&mut f, &slot(src, place), None, "", (1024, 768), Some(&pic)).unwrap();
        let m = (40.0f32 * 1024.0 / 1920.0).round() as u32;
        assert_eq!((p.x + p.img.width + m, p.y + p.img.height + m), (1024, 768));
    }

    #[test]
    fn compose_puts_later_overlays_on_top() {
        let red = RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]));
        let blue = RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 255, 255]));
        let full = Placement { anchor: Anchor::TopLeft, width: 100, margin: 0 };
        let src = OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 100 };
        let mut f = Fonts::bundled_only();
        let a = draw(&mut f, &slot(src.clone(), full), None, "", (8, 8), Some(&red)).unwrap();
        let b = draw(&mut f, &slot(src, full), None, "", (8, 8), Some(&blue)).unwrap();
        let c = compose(&[a, b], (8, 8));
        assert_eq!(&c.rgba_premul[0..4], &[0, 0, 255, 255]);
    }
}
