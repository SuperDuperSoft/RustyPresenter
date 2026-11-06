//! Background pictures (spec §6.3): a colour, a gradient at its angle, or a decoded picture covering the screen,
//! then the Darken layer. The room, the live monitor and the thumbnails all draw backgrounds with this.
use super::color::parse_color;
use super::layout::clamp_canvas;
use super::raster::{Premul, Rendered};
use crate::model::Background;
use image::RgbaImage;
use slint::{Rgba8Pixel, SharedPixelBuffer};
use tiny_skia::{Color, FilterQuality, GradientStop, LinearGradient, Paint, PixmapMut, PixmapPaint, PixmapRef, Point, Rect, SpreadMode, Transform};

fn sk(c: &str) -> Color {
    let [r, g, b, a] = parse_color(c);
    Color::from_rgba8(r, g, b, a)
}

/// The two ends of a CSS `linear-gradient(<angle>deg, …)` on a `w`×`h` box: 180° runs top → bottom, 90° left →
/// right, 135° top-left → bottom-right.
pub fn gradient_line(angle: u16, (w, h): (f32, f32)) -> (Point, Point) {
    let a = f32::from(angle % 360).to_radians();
    let (dx, dy) = (a.sin(), -a.cos());
    let half = (w * dx.abs() + h * dy.abs()) / 2.0;
    let (cx, cy) = (w / 2.0, h / 2.0);
    (Point::from_xy(cx - dx * half, cy - dy * half), Point::from_xy(cx + dx * half, cy + dy * half))
}

/// `bg` drawn at `canvas` as premultiplied RGBA. `decoded` is the picture of an image background (`None`: black,
/// as a picture that can't be read). A video background draws only its Darken layer: the player is under it.
pub fn background_pixels(bg: &Background, decoded: Option<&RgbaImage>, canvas: (u32, u32)) -> Rendered {
    let (w, h) = clamp_canvas(canvas);
    let mut px: SharedPixelBuffer<Rgba8Pixel> = SharedPixelBuffer::new(w, h);
    {
        let mut pix = PixmapMut::from_bytes(px.make_mut_bytes(), w, h).expect("canvas is clamped to a drawable size");
        let full = Rect::from_xywh(0.0, 0.0, w as f32, h as f32).expect("a non-empty canvas");
        match bg {
            Background::Color { color } => pix.fill(sk(color)),
            Background::Gradient { from, to, angle, .. } => {
                let (start, end) = gradient_line(*angle, (w as f32, h as f32));
                let stops = vec![GradientStop::new(0.0, sk(from)), GradientStop::new(1.0, sk(to))];
                match LinearGradient::new(start, end, stops, SpreadMode::Pad, Transform::identity()) {
                    Some(shader) => pix.fill_rect(full, &Paint { shader, ..Paint::default() }, Transform::identity(), None),
                    None => pix.fill(sk(from)),
                }
            }
            Background::Image { .. } => {
                pix.fill(Color::BLACK);
                if let Some(img) = decoded { cover(&mut pix, img) }
            }
            Background::Video { .. } => {}
        }
        let dim = bg.dim();
        if dim > 0 {
            let paint = Paint { shader: tiny_skia::Shader::SolidColor(Color::from_rgba(0.0, 0.0, 0.0, f32::from(dim) / 100.0).unwrap_or(Color::BLACK)), ..Paint::default() };
            pix.fill_rect(full, &paint, Transform::identity(), None);
        }
    }
    Rendered { width: w, height: h, rgba_premul: Premul::from_buffer(px), font_px: 0.0, font_fallback: false }
}

/// Draws `img` to fill the pixmap, centred and cut at the edges (CSS `object-fit: cover`).
fn cover(pix: &mut PixmapMut, img: &RgbaImage) {
    let (iw, ih) = img.dimensions();
    if iw == 0 || ih == 0 { return }
    let mut data = img.as_raw().clone();
    for p in data.chunks_exact_mut(4) {
        let a = u16::from(p[3]);
        for c in &mut p[..3] { *c = ((u16::from(*c) * a + 127) / 255) as u8 }
    }
    let Some(src) = PixmapRef::from_bytes(&data, iw, ih) else { return };
    let (w, h) = (pix.width() as f32, pix.height() as f32);
    let s = (w / iw as f32).max(h / ih as f32);
    let (x, y) = ((w - iw as f32 * s) / 2.0, (h - ih as f32 * s) / 2.0);
    let paint = PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
    pix.draw_pixmap(0, 0, src, &paint, Transform::from_row(s, 0.0, 0.0, s, x, y), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(r: &Rendered, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * r.width + x) * 4) as usize;
        [r.rgba_premul[i], r.rgba_premul[i + 1], r.rgba_premul[i + 2], r.rgba_premul[i + 3]]
    }

    fn close(a: [u8; 4], b: [u8; 4]) -> bool { a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 3) }

    #[test]
    fn gradient_line_follows_css_angles() {
        let (s, e) = gradient_line(180, (100.0, 50.0));
        assert_eq!((s.x.round(), s.y.round(), e.x.round(), e.y.round()), (50.0, 0.0, 50.0, 50.0));
        let (s, e) = gradient_line(90, (100.0, 50.0));
        assert_eq!((s.x.round(), s.y.round(), e.x.round(), e.y.round()), (0.0, 25.0, 100.0, 25.0));
        assert_eq!(gradient_line(450, (100.0, 50.0)), gradient_line(90, (100.0, 50.0)), "mod 360");
    }

    #[test]
    fn colour_gradient_and_dim() {
        let red = background_pixels(&Background::Color { color: "#ff0000".into() }, None, (8, 8));
        assert_eq!(at(&red, 4, 4), [255, 0, 0, 255]);
        let g = Background::Gradient { from: "#0b1a33".into(), to: "#000000".into(), angle: 180, dim: 0 };
        let r = background_pixels(&g, None, (64, 36));
        assert!(close(at(&r, 0, 0), [0x0b, 0x1a, 0x33, 255]), "{:?}", at(&r, 0, 0));
        assert!(close(at(&r, 0, 35), [0, 0, 0, 255]), "{:?}", at(&r, 0, 35));
        let dimmed = background_pixels(&Background::Color { color: "#ffffff".into() }, None, (4, 4));
        assert_eq!(at(&dimmed, 0, 0), [255, 255, 255, 255], "a colour has no Darken");
        let img = RgbaImage::from_pixel(2, 2, image::Rgba([200, 200, 200, 255]));
        let r = background_pixels(&Background::Image { asset: "a.png".into(), dim: 50 }, Some(&img), (4, 4));
        assert!(close(at(&r, 1, 1), [100, 100, 100, 255]), "{:?}", at(&r, 1, 1));
        let v = background_pixels(&Background::Video { asset: "v.mp4".into(), dim: 40 }, None, (4, 4));
        assert!(close(at(&v, 0, 0), [0, 0, 0, 102]), "only the Darken layer over a video");
    }
}
