//! The camera's pictures before they go to the computer: turned the right way up for how the phone is held, in the
//! NV21 layout Android's own JPEG maker reads, and a small copy for the preview on the phone's screen. Pure, so it
//! is tested on any computer.

/// One picture from the camera: three planes (brightness, then the two colour planes at half size), each with the
/// bytes between rows and between pixels as the camera gives them.
pub struct Yuv<'a> {
    pub w: usize,
    pub h: usize,
    pub y: &'a [u8],
    pub y_row: usize,
    pub u: &'a [u8],
    pub v: &'a [u8],
    pub uv_row: usize,
    pub uv_pixel: usize,
}

/// How far the picture must be turned (clockwise, 0/90/180/270) for it to be upright: from the camera's own
/// mounting (`sensor`, degrees), how the phone is turned (`display`, 0/90/180/270 as Android's `Display.getRotation`
/// counts them, times 90) and which camera it is (the front one is seen as in a mirror).
pub fn rotation(sensor: i32, display: i32, front: bool) -> u32 {
    let r = if front { (sensor + display) % 360 } else { (sensor - display + 360) % 360 };
    (r.rem_euclid(360) / 90 * 90) as u32
}

/// Where the pixel at (`ox`, `oy`) of the turned picture comes from in a `w`×`h` picture.
fn source(ox: usize, oy: usize, w: usize, h: usize, rot: u32) -> (usize, usize) {
    match rot {
        90 => (oy, h - 1 - ox),
        180 => (w - 1 - ox, h - 1 - oy),
        270 => (w - 1 - oy, ox),
        _ => (ox, oy),
    }
}

/// The picture turned by `rot` as NV21 (brightness, then V and U interleaved): its bytes, width and height. Sizes
/// are kept even.
pub fn to_nv21(f: &Yuv, rot: u32) -> (Vec<u8>, usize, usize) {
    let (w, h) = (f.w & !1, f.h & !1);
    let (ow, oh) = if rot == 90 || rot == 270 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; ow * oh * 3 / 2];
    let (luma, chroma) = out.split_at_mut(ow * oh);
    for oy in 0..oh {
        let row = &mut luma[oy * ow..(oy + 1) * ow];
        for (ox, px) in row.iter_mut().enumerate() {
            let (sx, sy) = source(ox, oy, w, h, rot);
            *px = *f.y.get(sy * f.y_row + sx).unwrap_or(&0);
        }
    }
    for cy in 0..oh / 2 {
        for cx in 0..ow / 2 {
            let (sx, sy) = source(cx * 2, cy * 2, w, h, rot);
            let at = (sy / 2) * f.uv_row + (sx / 2) * f.uv_pixel;
            let o = cy * ow + cx * 2;
            chroma[o] = *f.v.get(at).unwrap_or(&128);
            chroma[o + 1] = *f.u.get(at).unwrap_or(&128);
        }
    }
    (out, ow, oh)
}

/// A small copy of an NV21 picture for the screen: `out_w` pixels wide (its shape kept), as RGBA.
pub fn preview_rgba(nv21: &[u8], w: usize, h: usize, out_w: usize) -> (Vec<u8>, usize, usize) {
    let out_w = out_w.clamp(2, w.max(2));
    let out_h = (h * out_w / w.max(1)).max(2);
    let mut out = Vec::with_capacity(out_w * out_h * 4);
    for oy in 0..out_h {
        let sy = oy * h / out_h;
        for ox in 0..out_w {
            let sx = ox * w / out_w;
            let y = f32::from(*nv21.get(sy * w + sx).unwrap_or(&0));
            let c = w * h + (sy / 2) * w + (sx / 2) * 2;
            let v = f32::from(*nv21.get(c).unwrap_or(&128)) - 128.0;
            let u = f32::from(*nv21.get(c + 1).unwrap_or(&128)) - 128.0;
            let px = |x: f32| x.round().clamp(0.0, 255.0) as u8;
            out.extend([px(y + 1.402 * v), px(y - 0.344 * u - 0.714 * v), px(y + 1.772 * u), 255]);
        }
    }
    (out, out_w, out_h)
}

/// The camera size to ask for: of the sizes the camera can give, the biggest that is no wider than the computer's
/// stream (`want`), preferring its shape (16:9); the smallest when all are bigger.
pub fn choose_size(sizes: &[(u32, u32)], want: (u32, u32)) -> Option<(u32, u32)> {
    let shape = |(w, h): (u32, u32)| (u64::from(w) * u64::from(want.1)).abs_diff(u64::from(h) * u64::from(want.0)) * 100 <= u64::from(h) * u64::from(want.0) * 2;
    let fits = |s: &&(u32, u32)| s.0 <= want.0.max(640) && s.1 <= want.1.max(360);
    sizes.iter().filter(|s| shape(**s)).filter(fits).max_by_key(|s| s.0 * s.1)
        .or_else(|| sizes.iter().filter(fits).max_by_key(|s| s.0 * s.1))
        .or_else(|| sizes.iter().min_by_key(|s| s.0 * s.1)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4×2 picture whose brightness is its pixel number and whose colour planes say which 2×2 block they are.
    fn frame(y: &[u8], u: &[u8], v: &[u8]) -> Yuv<'static> {
        let leak = |b: &[u8]| -> &'static [u8] { Box::leak(b.to_vec().into_boxed_slice()) };
        Yuv { w: 4, h: 2, y: leak(y), y_row: 4, u: leak(u), v: leak(v), uv_row: 2, uv_pixel: 1 }
    }

    #[test]
    fn the_picture_is_turned_upright() {
        let f = frame(&[0, 1, 2, 3, 4, 5, 6, 7], &[10, 11], &[20, 21]);
        let (same, w, h) = to_nv21(&f, 0);
        assert_eq!((w, h), (4, 2));
        assert_eq!(same, [0, 1, 2, 3, 4, 5, 6, 7, 20, 10, 21, 11], "V before U");
        let (cw, w, h) = to_nv21(&f, 90);
        assert_eq!((w, h), (2, 4));
        // Turned clockwise: the bottom-left pixel comes to the top left.
        assert_eq!(&cw[..8], &[4, 0, 5, 1, 6, 2, 7, 3]);
        let (half, _, _) = to_nv21(&f, 180);
        assert_eq!(&half[..8], &[7, 6, 5, 4, 3, 2, 1, 0]);
        assert_eq!(&half[8..], &[21, 11, 20, 10]);
        let (ccw, w, h) = to_nv21(&f, 270);
        assert_eq!((w, h), (2, 4));
        assert_eq!(&ccw[..8], &[3, 7, 2, 6, 1, 5, 0, 4]);
    }

    #[test]
    fn colour_planes_two_bytes_apart_are_read_too() {
        // Most phones give U and V interleaved: every other byte, rows of 4.
        let f = Yuv { w: 4, h: 2, y: &[9; 8], y_row: 4, u: &[10, 0, 11, 0], v: &[20, 0, 21, 0], uv_row: 4, uv_pixel: 2 };
        assert_eq!(&to_nv21(&f, 0).0[8..], &[20, 10, 21, 11]);
    }

    #[test]
    fn how_far_to_turn() {
        // Most back cameras sit at 90°: upright held, a quarter turn; held sideways (turned left), none.
        assert_eq!(rotation(90, 0, false), 90);
        assert_eq!(rotation(90, 90, false), 0);
        assert_eq!(rotation(90, 270, false), 180);
        // Front cameras at 270°.
        assert_eq!(rotation(270, 0, true), 270);
        assert_eq!(rotation(270, 90, true), 0);
    }

    #[test]
    fn the_preview_keeps_the_shape_and_the_colours() {
        let (w, h) = (8, 4);
        let mut nv21 = vec![200u8; w * h];
        nv21.extend(std::iter::repeat_n([128u8, 128], w * h / 4).flatten());
        let (px, pw, ph) = preview_rgba(&nv21, w, h, 4);
        assert_eq!((pw, ph, px.len()), (4, 2, 32));
        assert_eq!(&px[..4], &[200, 200, 200, 255], "grey stays grey");
    }

    #[test]
    fn the_camera_size_is_the_streams_or_the_nearest_below() {
        let sizes = [(4032, 3024), (1920, 1080), (1280, 720), (640, 480), (3840, 2160)];
        assert_eq!(choose_size(&sizes, (1920, 1080)), Some((1920, 1080)));
        assert_eq!(choose_size(&sizes, (1280, 720)), Some((1280, 720)));
        assert_eq!(choose_size(&[(1440, 1080), (640, 480)], (1920, 1080)), Some((1440, 1080)), "another shape when none fits");
        assert_eq!(choose_size(&[(4000, 3000)], (1920, 1080)), Some((4000, 3000)), "the smallest when all are bigger");
        assert_eq!(choose_size(&[], (1920, 1080)), None);
    }
}
