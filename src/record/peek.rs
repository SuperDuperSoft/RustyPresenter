//! The small pictures beside the camera buttons while live: what goes out now ("Live") and the camera chosen to go
//! out next ("Next"). The writer shrinks them a few times a second from pictures it already has (`shrink` reads
//! four samples per small pixel, well under a millisecond at 320 wide) and leaves them here; the operator window
//! takes them when it looks. Never a full-size copy.
use std::sync::Arc;

/// How wide the small pictures are, in pixels (twice what the operator window shows, for sharp screens).
pub const WIDTH: u32 = 320;

/// A small RGBA picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Small {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The small pictures now: `None` is no picture (not yet, or the camera is not sending).
#[derive(Clone, Debug, Default)]
pub struct Peek {
    pub live: Option<Arc<Small>>,
    pub next: Option<Arc<Small>>,
}

/// `rgba` (`size` pixels) shrunk to `width` wide, keeping its shape; each pixel the average of four samples around
/// its place. `None` when `rgba` is not that size.
pub fn shrink(rgba: &[u8], (w, h): (u32, u32), width: u32) -> Option<Small> {
    if w == 0 || h == 0 || rgba.len() != w as usize * h as usize * 4 { return None }
    let width = width.clamp(1, w);
    let height = ((u64::from(h) * u64::from(width) / u64::from(w)) as u32).max(1);
    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
    // The four samples sit at a quarter and three quarters of each small pixel's square.
    let at = |i: u32, n: u32, full: u32, quarter: u64| ((u64::from(i) * 4 + quarter) * u64::from(full) / (u64::from(n) * 4)).min(u64::from(full) - 1) as usize;
    for y in 0..height {
        let (y1, y2) = (at(y, height, h, 1), at(y, height, h, 3));
        for x in 0..width {
            let (x1, x2) = (at(x, width, w, 1), at(x, width, w, 3));
            let mut sum = [0u32; 4];
            for (sx, sy) in [(x1, y1), (x2, y1), (x1, y2), (x2, y2)] {
                let p = (sy * w as usize + sx) * 4;
                for (s, v) in sum.iter_mut().zip(&rgba[p..p + 4]) { *s += u32::from(*v) }
            }
            out.extend(sum.map(|s| ((s + 2) / 4) as u8));
        }
    }
    Some(Small { width, height, rgba: out })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_shrunk_keeping_its_shape_and_colours() {
        // 1280×720: the left half red, the right half blue.
        let (w, h) = (1280u32, 720u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..h { for x in 0..w { rgba.extend(if x < w / 2 { [200, 0, 0, 255] } else { [0, 0, 200, 255] }) } }
        let s = shrink(&rgba, (w, h), WIDTH).unwrap();
        assert_eq!((s.width, s.height, s.rgba.len()), (320, 180, 320 * 180 * 4));
        let px = |x: u32, y: u32| { let p = ((y * s.width + x) * 4) as usize; [s.rgba[p], s.rgba[p + 1], s.rgba[p + 2], s.rgba[p + 3]] };
        assert_eq!(px(10, 90), [200, 0, 0, 255]);
        assert_eq!(px(310, 90), [0, 0, 200, 255]);
        // A tall shape stays tall; a picture smaller than the width stays its size.
        assert_eq!(shrink(&[9; 4 * 8 * 16], (8, 16), 4).map(|s| (s.width, s.height)), Some((4, 8)));
        assert_eq!(shrink(&[9; 4 * 8 * 16], (8, 16), WIDTH).map(|s| (s.width, s.height)), Some((8, 16)));
    }

    #[test]
    fn a_picture_of_the_wrong_size_is_no_picture() {
        assert_eq!(shrink(&[0; 12], (2, 2), WIDTH), None);
        assert_eq!(shrink(&[], (0, 0), WIDTH), None);
    }
}
