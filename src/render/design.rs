//! Draws a designed slide (N26): each object into a picture of its own (fill, words or picture, outline, soft
//! edges), then onto the slide turned and faded, over its blurred shadow. The room, the live monitor, the
//! thumbnails and the editor all draw with this, so the editor shows what the room will.
use super::color::parse_color;
use super::layout::{BUNDLED_FAMILY, CANVAS_W, Fonts, clamp_canvas, max_line_width, text_height};
use super::raster::{Premul, Rendered, blur, blur_reach};
use crate::design::{Design, Fit, HAlign, Obj, ObjKind, Shape, TextStyle, VAlign, strings as s};
use cosmic_text::{Align, Attrs, Buffer, Color, Family, Metrics, Shaping, Style, SwashCache, Weight, Wrap};
use image::RgbaImage;
use slint::{Rgba8Pixel, SharedPixelBuffer};
use tiny_skia::{FillRule, FilterQuality, LineCap, LineJoin, Mask, Paint, PathBuilder, Pixmap, PixmapMut, PixmapPaint, PixmapRef, Rect, Stroke, Transform};

/// Space between a text box's edge and its words, in 1080p pixels.
const TEXT_PAD: f32 = 16.0;
const LINE: f32 = 1.2;
/// The largest picture one object is drawn into, each way.
const MAX_SIDE: u32 = 4096;

/// The 16:9 part of a `canvas` the slide fills (x, y, w, h): all of a 16:9 screen, centred on any other.
pub fn slide_area((w, h): (u32, u32)) -> (f32, f32, f32, f32) {
    let (w, h) = (w as f32, h as f32);
    let aw = w.min(h * 16.0 / 9.0);
    let ah = aw * 9.0 / 16.0;
    ((w - aw) / 2.0, (h - ah) / 2.0, aw, ah)
}

/// Draws `d` on a transparent `canvas`. `theme_font` is used by text boxes with no font of their own; `picture`
/// gives the decoded file of an asset (None: it can't be read, and is left out).
pub fn render_design(fonts: &mut Fonts, d: &Design, theme_font: &str, canvas: (u32, u32), picture: &mut dyn FnMut(&str) -> Option<RgbaImage>) -> Rendered {
    let (w, h) = clamp_canvas(canvas);
    let mut px: SharedPixelBuffer<Rgba8Pixel> = SharedPixelBuffer::new(w, h);
    {
        let mut pix = PixmapMut::from_bytes(px.make_mut_bytes(), w, h).expect("canvas is clamped to a drawable size");
        let area = slide_area((w, h));
        for o in &d.objects { draw_obj(&mut pix, fonts, o, area, theme_font, picture) }
    }
    Rendered { width: w, height: h, rgba_premul: Premul::from_buffer(px), font_px: 0.0, font_fallback: false }
}

/// One picture of a click group as it plays (`Play::layers`): pixels cropped to what is drawn, where they sit on
/// the canvas (x, y in pixels), and the object it is when it animates on its own.
pub struct AnimLayer { pub px: SharedPixelBuffer<Rgba8Pixel>, pub x: u32, pub y: u32, pub obj: Option<usize> }

/// Draws `play`'s layers at `canvas`, each cropped to its pixels (an empty layer is left out).
pub fn render_layers(fonts: &mut Fonts, play: &crate::design::Play, theme_font: &str, canvas: (u32, u32), picture: &mut dyn FnMut(&str) -> Option<RgbaImage>) -> Vec<AnimLayer> {
    play.layers().into_iter().filter_map(|(objs, obj)| {
        let d = Design { objects: objs.iter().map(|i| play.objects[*i].clone()).collect(), ..Design::default() };
        let r = render_design(fonts, &d, theme_font, canvas, picture);
        let (x, y, px) = crop(&r)?;
        Some(AnimLayer { px, x, y, obj })
    }).collect()
}

/// The part of `r` that has any pixels, and where it starts. None when it is all clear.
fn crop(r: &Rendered) -> Option<(u32, u32, SharedPixelBuffer<Rgba8Pixel>)> {
    let (w, h) = (r.width as usize, r.height as usize);
    let px: &[u8] = &r.rgba_premul;
    let (mut l, mut t, mut rt, mut b) = (w, h, 0usize, 0usize);
    for y in 0..h {
        let row = &px[y * w * 4..(y + 1) * w * 4];
        let Some(first) = row.chunks_exact(4).position(|p| p[3] != 0) else { continue };
        let last = row.chunks_exact(4).rposition(|p| p[3] != 0).unwrap_or(first);
        (l, rt, t, b) = (l.min(first), rt.max(last + 1), t.min(y), y + 1);
    }
    if l >= rt || t >= b { return None }
    let (cw, ch) = (rt - l, b - t);
    let mut out: SharedPixelBuffer<Rgba8Pixel> = SharedPixelBuffer::new(cw as u32, ch as u32);
    let dst = out.make_mut_bytes();
    for y in 0..ch { dst[y * cw * 4..(y + 1) * cw * 4].copy_from_slice(&px[((t + y) * w + l) * 4..((t + y) * w + rt) * 4]) }
    Some((l as u32, t as u32, out))
}

fn sk_color(c: [u8; 4], alpha: f32) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c[0], c[1], c[2], (f32::from(c[3]) * alpha.clamp(0.0, 1.0)).round() as u8)
}

fn paint(c: &str) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(sk_color(parse_color(c), 1.0));
    p.anti_alias = true;
    p
}

/// A rectangle with rounded corners.
fn round_rect(r: Rect, radius: f32) -> Option<tiny_skia::Path> {
    let rad = radius.min(r.width() / 2.0).min(r.height() / 2.0).max(0.0);
    if rad <= 0.0 { return Some(PathBuilder::from_rect(r)) }
    let (x, y, w, h) = (r.x(), r.y(), r.width(), r.height());
    let mut pb = PathBuilder::new();
    pb.move_to(x + rad, y);
    pb.line_to(x + w - rad, y);
    pb.quad_to(x + w, y, x + w, y + rad);
    pb.line_to(x + w, y + h - rad);
    pb.quad_to(x + w, y + h, x + w - rad, y + h);
    pb.line_to(x + rad, y + h);
    pb.quad_to(x, y + h, x, y + h - rad);
    pb.line_to(x, y + rad);
    pb.quad_to(x, y, x + rad, y);
    pb.close();
    pb.finish()
}

fn shape_path(shape: Shape, r: Rect, radius: f32) -> Option<tiny_skia::Path> {
    let (x, y, w, h) = (r.x(), r.y(), r.width(), r.height());
    match shape {
        Shape::Rect => round_rect(r, radius),
        Shape::Ellipse => PathBuilder::from_oval(r),
        Shape::Triangle => {
            let mut pb = PathBuilder::new();
            pb.move_to(x + w / 2.0, y);
            pb.line_to(x + w, y + h);
            pb.line_to(x, y + h);
            pb.close();
            pb.finish()
        }
        Shape::Line | Shape::Arrow => {
            let mut pb = PathBuilder::new();
            pb.move_to(x, y + h / 2.0);
            pb.line_to(x + w, y + h / 2.0);
            if shape == Shape::Arrow {
                let head = (h / 2.0).min(w / 3.0);
                pb.move_to(x + w - head, y + h / 2.0 - head);
                pb.line_to(x + w, y + h / 2.0);
                pb.line_to(x + w - head, y + h / 2.0 + head);
            }
            pb.finish()
        }
    }
}

fn draw_obj(pix: &mut PixmapMut, fonts: &mut Fonts, o: &Obj, (ax, ay, aw, ah): (f32, f32, f32, f32), theme_font: &str, picture: &mut dyn FnMut(&str) -> Option<RgbaImage>) {
    if o.opacity == 0 || matches!(o.kind, ObjKind::Unknown) { return }
    let sc = aw / CANVAS_W;
    let (bw, bh) = (o.rect.w / 100.0 * aw, o.rect.h / 100.0 * ah);
    if !(bw >= 0.5 && bh >= 0.5) { return }
    let line_w = match &o.kind {
        ObjKind::Path { width, .. } => *width * sc,
        _ => o.look.outline.as_ref().map_or(0.0, |ol| ol.width * sc),
    };
    let pad = (line_w / 2.0 + 2.0).ceil();
    let (lw, lh) = (((bw + 2.0 * pad).ceil() as u32).clamp(1, MAX_SIDE), ((bh + 2.0 * pad).ceil() as u32).clamp(1, MAX_SIDE));
    let Some(mut local) = Pixmap::new(lw, lh) else { return };
    let Some(content) = Rect::from_xywh(pad, pad, bw.min(lw as f32 - 2.0 * pad).max(0.5), bh.min(lh as f32 - 2.0 * pad).max(0.5)) else { return };
    let radius = o.look.radius * sc;
    let fill = o.look.fill.as_deref();
    let stroke = |w: f32| Stroke { width: w.max(0.5), line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
    match &o.kind {
        ObjKind::Text { text, style } => {
            if let (Some(f), Some(path)) = (fill, round_rect(content, radius)) { local.fill_path(&path, &paint(f), FillRule::Winding, Transform::identity(), None) }
            draw_words(&mut local, fonts, text, style, content, sc, theme_font);
            outline_box(&mut local, o, content, radius, line_w);
        }
        ObjKind::Shape { shape } => {
            let Some(path) = shape_path(*shape, content, radius) else { return };
            let line = matches!(shape, Shape::Line | Shape::Arrow);
            if let Some(f) = fill.filter(|_| !line) { local.fill_path(&path, &paint(f), FillRule::Winding, Transform::identity(), None) }
            if let Some(ol) = &o.look.outline { local.stroke_path(&path, &paint(&ol.color), &stroke(line_w), Transform::identity(), None) }
        }
        ObjKind::Picture { asset, fit } => {
            if let Some(f) = fill && let Some(path) = round_rect(content, radius) { local.fill_path(&path, &paint(f), FillRule::Winding, Transform::identity(), None) }
            if let Some(img) = picture(asset) { draw_picture(&mut local, &img, *fit, content, radius) }
            outline_box(&mut local, o, content, radius, line_w);
        }
        ObjKind::Video { thumb, .. } => {
            let frame = thumb.as_deref().and_then(&mut *picture);
            match frame {
                Some(img) => draw_picture(&mut local, &img, Fit::Fill, content, radius),
                None => {
                    if let Some(path) = round_rect(content, radius) { local.fill_path(&path, &paint("#1f2937"), FillRule::Winding, Transform::identity(), None) }
                    let r = content.width().min(content.height()) * 0.18;
                    let (cx, cy) = (content.x() + content.width() / 2.0, content.y() + content.height() / 2.0);
                    let mut pb = PathBuilder::new();
                    pb.move_to(cx - r * 0.8, cy - r);
                    pb.line_to(cx + r, cy);
                    pb.line_to(cx - r * 0.8, cy + r);
                    pb.close();
                    if let Some(p) = pb.finish() { local.fill_path(&p, &paint("#ffffffcc"), FillRule::Winding, Transform::identity(), None) }
                    draw_words(&mut local, fonts, &s::video_label(), &label_style(), content, sc, theme_font);
                }
            }
            outline_box(&mut local, o, content, radius, line_w);
        }
        ObjKind::Path { points, color, .. } => {
            let mut pb = PathBuilder::new();
            for (i, (x, y)) in points.iter().enumerate() {
                let (x, y) = (content.x() + x * content.width(), content.y() + y * content.height());
                if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
            }
            if points.len() == 1 { let (x, y) = (content.x() + points[0].0 * content.width(), content.y() + points[0].1 * content.height()); pb.line_to(x + 0.01, y) }
            if let Some(p) = pb.finish() { local.stroke_path(&p, &paint(color), &stroke(line_w), Transform::identity(), None) }
        }
        ObjKind::Web { url } => {
            if let Some(path) = round_rect(content, radius) { local.fill_path(&path, &paint(fill.unwrap_or("#f3f4f6")), FillRule::Winding, Transform::identity(), None) }
            let words = format!("{}\n{url}", s::web_label());
            draw_words(&mut local, fonts, &words, &TextStyle { color: "#111827".into(), ..label_style() }, content, sc, theme_font);
            outline_box(&mut local, o, content, radius, line_w);
        }
        ObjKind::Unknown => return,
    }
    if o.look.soft > 0.0 { feather(&mut local, content, o.look.soft * sc) }
    let (cx, cy) = (ax + (o.rect.x + o.rect.w / 2.0) / 100.0 * aw, ay + (o.rect.y + o.rect.h / 2.0) / 100.0 * ah);
    let place = Transform::from_translate(cx, cy).pre_rotate(o.rotation).pre_translate(-(lw as f32) / 2.0, -(lh as f32) / 2.0);
    let opacity = f32::from(o.opacity.min(100)) / 100.0;
    if let Some(sh) = &o.look.shadow && sh.opacity > 0 {
        if let Some((shadow, reach)) = shadow_of(&local, &sh.color, sh.blur * sc) {
            let at = place.pre_translate(-(reach as f32), -(reach as f32)).post_translate(sh.dx * sc, sh.dy * sc);
            let paint = PixmapPaint { opacity: opacity * f32::from(sh.opacity.min(100)) / 100.0, quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
            pix.draw_pixmap(0, 0, shadow.as_ref(), &paint, at, None);
        }
    }
    let paint = PixmapPaint { opacity, quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
    pix.draw_pixmap(0, 0, local.as_ref(), &paint, place, None);
}

fn label_style() -> TextStyle { TextStyle { size: 36.0, bold: true, valign: VAlign::Bottom, ..TextStyle::default() } }

fn outline_box(local: &mut Pixmap, o: &Obj, content: Rect, radius: f32, width: f32) {
    let (Some(ol), Some(path)) = (&o.look.outline, round_rect(content, radius)) else { return };
    if width <= 0.0 { return }
    local.stroke_path(&path, &paint(&ol.color), &Stroke { width, line_join: LineJoin::Round, ..Stroke::default() }, Transform::identity(), None);
}

/// Words laid out in `content`, shrunk to fit when the style asks.
fn draw_words(local: &mut Pixmap, fonts: &mut Fonts, text: &str, st: &TextStyle, content: Rect, sc: f32, theme_font: &str) {
    if text.trim().is_empty() { return }
    let pad = TEXT_PAD * sc;
    let (avail_w, avail_h) = ((content.width() - 2.0 * pad).max(1.0), (content.height() - 2.0 * pad).max(1.0));
    let wanted = if st.font.trim().is_empty() { theme_font } else { st.font.as_str() };
    let family = if fonts.has_family(wanted) { wanted.to_string() } else { BUNDLED_FAMILY.to_string() };
    let attrs = Attrs::new().family(Family::Name(&family)).weight(if st.bold { Weight::BOLD } else { Weight::NORMAL })
        .style(if st.italic { Style::Italic } else { Style::Normal });
    let align = match st.align { HAlign::Left => Align::Left, HAlign::Center => Align::Center, HAlign::Right => Align::Right };
    let start = st.size.clamp(crate::design::SIZE_MIN, crate::design::SIZE_MAX) * sc;
    let mut buf = Buffer::new(&mut fonts.sys, Metrics::new(start, start * LINE));
    buf.set_size(Some(avail_w), None);
    buf.set_wrap(Wrap::WordOrGlyph);
    buf.set_text(text, &attrs, Shaping::Advanced, Some(align));
    buf.shape_until_scroll(&mut fonts.sys, false);
    if st.shrink {
        let floor = crate::design::SIZE_MIN * sc;
        let mut px = start;
        while px > floor && (text_height(&buf) > avail_h + 0.5 || max_line_width(&buf) > avail_w + 1.0) {
            px = (px * 0.94).max(floor);
            buf.set_metrics(Metrics::new(px, px * LINE));
            buf.shape_until_scroll(&mut fonts.sys, false);
        }
    }
    let th = text_height(&buf);
    let y = content.y() + pad + match st.valign { VAlign::Top => 0.0, VAlign::Middle => (avail_h - th) / 2.0, VAlign::Bottom => avail_h - th };
    let x = content.x() + pad;
    let [r, g, b, a] = parse_color(&st.color);
    let (w, h) = (local.width() as i32, local.height() as i32);
    let data = local.data_mut();
    let (ox, oy) = (x.round() as i32, y.round() as i32);
    buf.draw(&mut fonts.sys, &mut SwashCache::new(), Color::rgba(255, 255, 255, 255), |gx, gy, gw, gh, c| {
        let cov = u32::from(c.a()) * u32::from(a) / 255;
        if cov == 0 { return }
        for py in (gy + oy).max(0)..(gy + oy + gh as i32).min(h) {
            for px in (gx + ox).max(0)..(gx + ox + gw as i32).min(w) {
                let i = ((py * w + px) * 4) as usize;
                let inv = 255 - cov;
                for (k, ch) in [r, g, b, 255].into_iter().enumerate() {
                    let src = if k == 3 { cov } else { u32::from(ch) * cov / 255 };
                    data[i + k] = (src + u32::from(data[i + k]) * inv / 255).min(255) as u8;
                }
            }
        }
    });
}

/// A picture in `content` by `fit`, cut to the box (with its rounded corners).
fn draw_picture(local: &mut Pixmap, img: &RgbaImage, fit: Fit, content: Rect, radius: f32) {
    let (iw, ih) = img.dimensions();
    if iw == 0 || ih == 0 { return }
    let mut data = img.as_raw().clone();
    for p in data.chunks_exact_mut(4) {
        let a = u16::from(p[3]);
        for c in &mut p[..3] { *c = ((u16::from(*c) * a + 127) / 255) as u8 }
    }
    let Some(src) = PixmapRef::from_bytes(&data, iw, ih) else { return };
    let (sx, sy) = (content.width() / iw as f32, content.height() / ih as f32);
    let (sx, sy) = match fit { Fit::Stretch => (sx, sy), Fit::Fit => (sx.min(sy), sx.min(sy)), Fit::Fill => (sx.max(sy), sx.max(sy)) };
    let (dw, dh) = (iw as f32 * sx, ih as f32 * sy);
    let (dx, dy) = (content.x() + (content.width() - dw) / 2.0, content.y() + (content.height() - dh) / 2.0);
    let mut mask = Mask::new(local.width(), local.height());
    if let (Some(m), Some(path)) = (mask.as_mut(), round_rect(content, radius)) { m.fill_path(&path, FillRule::Winding, true, Transform::identity()) }
    let paint = PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
    local.draw_pixmap(0, 0, src, &paint, Transform::from_row(sx, 0.0, 0.0, sy, dx, dy), mask.as_ref());
}

/// Fades the picture out towards the box's edges over `soft` pixels.
fn feather(local: &mut Pixmap, content: Rect, soft: f32) {
    let soft = soft.min(content.width() / 2.0).min(content.height() / 2.0);
    if soft < 0.5 { return }
    let w = local.width() as usize;
    let (l, t, r, b) = (content.x(), content.y(), content.x() + content.width(), content.y() + content.height());
    for (i, px) in local.data_mut().chunks_exact_mut(4).enumerate() {
        if px[3] == 0 { continue }
        let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
        let d = (x - l).min(r - x).min(y - t).min(b - y);
        let f = (d / soft).clamp(0.0, 1.0);
        if f >= 1.0 { continue }
        let f = f * f * (3.0 - 2.0 * f);
        for c in px.iter_mut() { *c = (f32::from(*c) * f).round() as u8 }
    }
}

/// The object's shadow: its coverage blurred by `blur` (a CSS-like radius) in `color`, grown on every side by the
/// returned reach.
fn shadow_of(local: &Pixmap, color: &str, blur_px: f32) -> Option<(Pixmap, usize)> {
    let sigma = blur_px / 2.0;
    let reach = blur_reach(sigma);
    let (w, h) = (local.width() as usize, local.height() as usize);
    let (sw, sh) = (w + 2 * reach, h + 2 * reach);
    let mut cov = vec![0.0f32; sw * sh];
    for (i, px) in local.data().chunks_exact(4).enumerate() {
        cov[(i / w + reach) * sw + i % w + reach] = f32::from(px[3]) / 255.0;
    }
    blur(&mut cov, sw, sh, sigma);
    let mut out = Pixmap::new(sw as u32, sh as u32)?;
    let [r, g, b, a] = parse_color(color);
    for (m, px) in cov.iter().zip(out.data_mut().chunks_exact_mut(4)) {
        let al = (m.clamp(0.0, 1.0) * f32::from(a)).round();
        for (k, ch) in [r, g, b].into_iter().enumerate() { px[k] = (f32::from(ch) * al / 255.0).round() as u8 }
        px[3] = al as u8;
    }
    Some((out, reach))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{Look, ObjShadow, Outline, PctRect};

    fn at(r: &Rendered, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * r.width + x) * 4) as usize;
        [r.rgba_premul[i], r.rgba_premul[i + 1], r.rgba_premul[i + 2], r.rgba_premul[i + 3]]
    }

    fn shape(rect: PctRect, fill: &str) -> Obj {
        let mut o = Obj::new(ObjKind::Shape { shape: Shape::Rect }, rect);
        o.look.fill = Some(fill.into());
        o
    }

    fn draw(d: &Design, canvas: (u32, u32)) -> Rendered { render_design(&mut Fonts::bundled_only(), d, "Noto Sans", canvas, &mut |_| None) }

    #[test]
    fn a_rectangle_fills_its_place_and_nothing_else() {
        let d = Design { objects: vec![shape(PctRect::new(25.0, 25.0, 50.0, 50.0), "#ff0000")], ..Default::default() };
        let r = draw(&d, (160, 90));
        assert_eq!(at(&r, 80, 45), [255, 0, 0, 255]);
        assert_eq!(at(&r, 10, 10), [0, 0, 0, 0], "transparent around it: the background shows");
    }

    #[test]
    fn opacity_and_turning() {
        let mut o = shape(PctRect::new(40.0, 0.0, 20.0, 100.0), "#ffffff");
        o.opacity = 50;
        let r = draw(&Design { objects: vec![o.clone()], ..Default::default() }, (160, 90));
        assert!((120..=135).contains(&at(&r, 80, 45)[3]), "{:?}", at(&r, 80, 45));
        o.opacity = 100;
        o.rotation = 90.0;
        o.rect = PctRect::new(40.0, 40.0, 20.0, 20.0);
        let r = draw(&Design { objects: vec![o], ..Default::default() }, (160, 90));
        // 32 × 18 px turned upright: 18 wide, 32 tall, about the middle.
        assert_eq!(at(&r, 80, 31)[3], 255);
        assert_eq!(at(&r, 68, 45)[3], 0);
    }

    #[test]
    fn text_draws_inside_its_box_in_its_colour() {
        let o = Obj::new(ObjKind::Text { text: "HELLO".into(), style: TextStyle { color: "#00ff00".into(), size: 200.0, ..TextStyle::default() } }, PctRect::new(10.0, 10.0, 80.0, 40.0));
        let r = draw(&Design { objects: vec![o], ..Default::default() }, (320, 180));
        let lit: Vec<(u32, u32)> = (0..180).flat_map(|y| (0..320).map(move |x| (x, y))).filter(|&(x, y)| at(&r, x, y)[3] > 128).collect();
        assert!(!lit.is_empty());
        assert!(lit.iter().all(|&(x, y)| (32..=288).contains(&x) && (18..=90).contains(&y)), "shrunk to fit its box");
        let &(x, y) = lit.iter().find(|&&(x, y)| at(&r, x, y)[3] == 255).unwrap();
        assert_eq!(at(&r, x, y), [0, 255, 0, 255]);
    }

    #[test]
    fn a_shadow_falls_down_and_right_and_soft_edges_fade() {
        let mut o = shape(PctRect::new(30.0, 30.0, 40.0, 40.0), "#ffffff");
        o.look.shadow = Some(ObjShadow { blur: 0.0, dx: 120.0, dy: 120.0, opacity: 100, color: "#000000".into() });
        let r = draw(&Design { objects: vec![o.clone()], ..Default::default() }, (192, 108));
        let below = at(&r, 120, 80);
        assert!(below[3] > 200 && below[0] < 20, "the black shadow past the corner: {below:?}");
        o.look = Look { fill: Some("#ffffff".into()), soft: 200.0, ..Look::default() };
        let r = draw(&Design { objects: vec![o], ..Default::default() }, (192, 108));
        assert!(at(&r, 59, 34)[3] < at(&r, 96, 54)[3], "fainter at the edge than in the middle");
    }

    #[test]
    fn pictures_fill_their_box_and_missing_ones_are_left_out() {
        let o = Obj::new(ObjKind::Picture { asset: "p.png".into(), fit: Fit::Fill }, PctRect::new(0.0, 0.0, 50.0, 100.0));
        let d = Design { objects: vec![o], ..Default::default() };
        let blue = RgbaImage::from_pixel(10, 10, image::Rgba([0, 0, 255, 255]));
        let r = render_design(&mut Fonts::bundled_only(), &d, "", (160, 90), &mut |a| (a == "p.png").then(|| blue.clone()));
        assert_eq!(at(&r, 5, 85), [0, 0, 255, 255]);
        assert_eq!(at(&r, 100, 45)[3], 0);
        assert_eq!(at(&draw(&d, (160, 90)), 5, 85)[3], 0, "a picture that can't be read draws nothing");
    }

    #[test]
    fn a_4_by_3_screen_shows_the_slide_letterboxed() {
        assert_eq!(slide_area((1024, 768)), (0.0, 96.0, 1024.0, 576.0));
        let d = Design { objects: vec![shape(PctRect::new(0.0, 0.0, 100.0, 100.0), "#ffffff")], ..Default::default() };
        let r = draw(&d, (64, 48));
        assert_eq!(at(&r, 32, 2)[3], 0);
        assert_eq!(at(&r, 32, 24)[3], 255);
    }

    #[test]
    fn lines_paths_and_outlines_draw() {
        let mut line = Obj::new(ObjKind::Shape { shape: Shape::Line }, PctRect::new(10.0, 45.0, 80.0, 10.0));
        line.look.outline = Some(Outline { color: "#ffffff".into(), width: 40.0 });
        let path = Obj::new(ObjKind::Path { points: vec![(0.0, 0.0), (1.0, 1.0)], width: 40.0, color: "#ff0000".into() }, PctRect::new(0.0, 0.0, 20.0, 20.0));
        let r = draw(&Design { objects: vec![line, path], ..Default::default() }, (160, 90));
        assert_eq!(at(&r, 80, 45)[3], 255);
        assert!(at(&r, 16, 9)[0] > 200);
    }

    #[test]
    fn a_play_is_drawn_as_cropped_layers() {
        use crate::design::{Anim, AnimKind, Effect, Start};
        let (a, b) = (shape(PctRect::new(0.0, 0.0, 50.0, 50.0), "#ff0000"), shape(PctRect::new(50.0, 50.0, 25.0, 25.0), "#00ff00"));
        let d = Design { anims: vec![Anim::new(b.id, AnimKind::Entrance, Effect::Fade, Start::OnClick)], objects: vec![a, b], ..Default::default() };
        let layers = render_layers(&mut Fonts::bundled_only(), &d.play(1).unwrap(), "Noto Sans", (160, 90), &mut |_| None);
        let at = |l: &AnimLayer| (l.x, l.y, l.px.width(), l.px.height(), l.obj);
        assert_eq!(layers.iter().map(at).collect::<Vec<_>>(), [(0, 0, 80, 45, None), (80, 45, 40, 23, Some(1))]);
    }
}
