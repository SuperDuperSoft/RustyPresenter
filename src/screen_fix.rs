//! N20: a screen's picture fix (colour, corner pin, delay) and its test patterns. The maths lives here, so the GL
//! pass (`video::postpass`) and the tests agree on what each number does.
use serde::{Deserialize, Serialize};

/// Limits of each setting, as the dialog and the saved data clamp them.
pub const GAIN: (f32, f32) = (0.0, 2.0);
pub const GAMMA: (f32, f32) = (0.4, 2.5);
pub const BLACK: (f32, f32) = (0.0, 0.3);
/// How far a corner can move, as a share of the screen's width or height.
pub const CORNER: f32 = 0.3;
pub const MAX_DELAY: u32 = 30;
/// Frames are counted at 60 a second.
pub const FRAME_MS: u64 = 1000 / 60;

/// How one screen's picture is corrected before it leaves the computer. The default changes nothing.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default)]
pub struct ScreenFix {
    /// Each colour's strength: 1 = as is.
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    /// Above 1 brightens the middle tones, below 1 darkens them.
    pub gamma: f32,
    /// Lifts black to this grey (0–0.3), for a projector whose black is not black.
    pub black: f32,
    /// Where each corner goes (top left, top right, bottom right, bottom left), moved by this share of the
    /// screen's width (x) and height (y). Down and right are positive.
    pub corners: [[f32; 2]; 4],
    /// Frames this screen waits before it shows a change, to line it up with a slower screen.
    pub delay: u32,
}

impl Default for ScreenFix {
    fn default() -> Self { Self { red: 1.0, green: 1.0, blue: 1.0, gamma: 1.0, black: 0.0, corners: [[0.0; 2]; 4], delay: 0 } }
}

fn clamp(v: f32, (lo, hi): (f32, f32)) -> f32 { if v.is_finite() { v.clamp(lo, hi) } else { 1.0f32.clamp(lo, hi) } }

impl ScreenFix {
    pub fn is_default(&self) -> bool { *self == Self::default() }

    /// The same fix with every number in range.
    pub fn clamped(mut self) -> Self {
        for c in [&mut self.red, &mut self.green, &mut self.blue] { *c = clamp(*c, GAIN) }
        self.gamma = clamp(self.gamma, GAMMA);
        self.black = if self.black.is_finite() { self.black.clamp(BLACK.0, BLACK.1) } else { 0.0 };
        for c in self.corners.iter_mut().flatten() { *c = if c.is_finite() { c.clamp(-CORNER, CORNER) } else { 0.0 } }
        self.delay = self.delay.min(MAX_DELAY);
        self
    }

    /// Whether the colours are left alone.
    pub fn colour_is_plain(&self) -> bool { self.red == 1.0 && self.green == 1.0 && self.blue == 1.0 && self.gamma == 1.0 && self.black == 0.0 }

    /// Whether the corners are where they belong.
    pub fn corners_are_plain(&self) -> bool { self.corners.iter().flatten().all(|c| *c == 0.0) }

    /// Whether the picture needs the GL pass at all.
    pub fn needs_pass(&self) -> bool { !self.colour_is_plain() || !self.corners_are_plain() }

    /// What a colour (0–1 each) becomes: strength, then gamma, then the black lift. Matches the GL pass.
    pub fn colour(&self, rgb: [f32; 3]) -> [f32; 3] {
        let gains = [self.red, self.green, self.blue];
        std::array::from_fn(|i| {
            let c = (rgb[i] * gains[i]).clamp(0.0, 1.0).powf(1.0 / self.gamma);
            self.black + (1.0 - self.black) * c
        })
    }

    /// The picture's corners on the screen, in 0–1 with y down: top left, top right, bottom right, bottom left.
    pub fn quad(&self) -> [[f32; 2]; 4] {
        const SQUARE: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        std::array::from_fn(|i| [SQUARE[i][0] + self.corners[i][0], SQUARE[i][1] + self.corners[i][1]])
    }

    /// The 3×3 matrix (row major) that takes a point on the screen (0–1, y down) to the point of the picture shown
    /// there: divide x and y by z, and outside 0–1 nothing (black) shows. `None` when the corners fold over.
    pub fn screen_to_picture(&self) -> Option<[f32; 9]> { invert(square_to_quad(self.quad())) }

    /// Where a point of the screen shows from in the picture (`screen_to_picture` applied), if it shows any.
    pub fn source_of(&self, p: [f32; 2]) -> Option<[f32; 2]> {
        let m = self.screen_to_picture()?;
        let z = m[6] * p[0] + m[7] * p[1] + m[8];
        if z.abs() < 1e-9 { return None }
        let s = [(m[0] * p[0] + m[1] * p[1] + m[2]) / z, (m[3] * p[0] + m[4] * p[1] + m[5]) / z];
        (s.iter().all(|v| (-1e-4..=1.0 + 1e-4).contains(v))).then_some(s)
    }

    /// The wait before a change shows.
    pub fn delay_ms(&self) -> u64 { u64::from(self.delay.min(MAX_DELAY)) * FRAME_MS }
}

/// The projective map from the unit square onto `q` (Heckbert's square-to-quad), row major.
fn square_to_quad(q: [[f32; 2]; 4]) -> [f64; 9] {
    let p: [[f64; 2]; 4] = q.map(|c| c.map(f64::from));
    let [x0, y0] = p[0];
    let [x1, y1] = p[1];
    let [x2, y2] = p[2];
    let [x3, y3] = p[3];
    let (sx, sy) = (x0 - x1 + x2 - x3, y0 - y1 + y2 - y3);
    if sx.abs() < 1e-12 && sy.abs() < 1e-12 {
        return [x1 - x0, x3 - x0, x0, y1 - y0, y3 - y0, y0, 0.0, 0.0, 1.0];
    }
    let (dx1, dx2, dy1, dy2) = (x1 - x2, x3 - x2, y1 - y2, y3 - y2);
    let den = dx1 * dy2 - dx2 * dy1;
    let g = (sx * dy2 - dx2 * sy) / den;
    let h = (dx1 * sy - sx * dy1) / den;
    [x1 - x0 + g * x1, x3 - x0 + h * x3, x0, y1 - y0 + g * y1, y3 - y0 + h * y3, y0, g, h, 1.0]
}

fn invert(m: [f64; 9]) -> Option<[f32; 9]> {
    let [a, b, c, d, e, f, g, h, i] = m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if !det.is_finite() || det.abs() < 1e-12 { return None }
    let adj = [e * i - f * h, c * h - b * i, b * f - c * e, f * g - d * i, a * i - c * g, c * d - a * f, d * h - e * g, b * g - a * h, a * e - b * d];
    Some(adj.map(|v| (v / det) as f32))
}

/// A test pattern shown on a screen instead of the picture, to line up and colour-match projectors.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TestPattern {
    #[default]
    Off,
    Grid,
    Bars,
    White,
    Black,
}

impl TestPattern {
    pub const ALL: [TestPattern; 5] = [TestPattern::Off, TestPattern::Grid, TestPattern::Bars, TestPattern::White, TestPattern::Black];
    pub fn is_off(&self) -> bool { *self == TestPattern::Off }
}

/// The pattern as straight RGBA pixels at `size`; `None` for Off.
pub fn pattern_pixels(p: TestPattern, (w, h): (u32, u32)) -> Option<Vec<u8>> {
    let (w, h) = (w.max(1), h.max(1));
    let solid = |c: [u8; 3]| c.iter().copied().chain([255]).cycle().take((w * h * 4) as usize).collect::<Vec<u8>>();
    match p {
        TestPattern::Off => None,
        TestPattern::White => Some(solid([255; 3])),
        TestPattern::Black => Some(solid([0; 3])),
        TestPattern::Bars => {
            const BARS: [[u8; 3]; 8] = [[255, 255, 255], [255, 255, 0], [0, 255, 255], [0, 255, 0], [255, 0, 255], [255, 0, 0], [0, 0, 255], [0, 0, 0]];
            let row: Vec<u8> = (0..w).flat_map(|x| { let c = BARS[(x * 8 / w) as usize]; [c[0], c[1], c[2], 255] }).collect();
            Some(row.iter().copied().cycle().take((w * h * 4) as usize).collect())
        }
        TestPattern::Grid => {
            // 16 × 9 squares, a border, a cross through the middle and a circle: straight lines show a crooked picture.
            let line = (h / 270).max(2);
            let mut px = solid([0; 3]);
            let (cx, cy, r) = (w as f32 / 2.0, h as f32 / 2.0, h as f32 * 0.4);
            for y in 0..h {
                for x in 0..w {
                    let on_grid = (x * 16 / w) != ((x + line) * 16 / w).min(16) || (y * 9 / h) != ((y + line) * 9 / h).min(9) || x < line || y < line;
                    let middle = x.abs_diff(w / 2) < line || y.abs_diff(h / 2) < line;
                    let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
                    let circle = (d - r).abs() < line as f32;
                    let c: [u8; 3] = if middle || circle { [255, 210, 0] } else if on_grid { [255; 3] } else { continue };
                    let i = ((y * w + x) * 4) as usize;
                    px[i..i + 3].copy_from_slice(&c);
                }
            }
            Some(px)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 2], b: [f32; 2]) -> bool { (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4 }

    #[test]
    fn the_default_changes_nothing() {
        let f = ScreenFix::default();
        assert!(!f.needs_pass());
        assert_eq!(f.colour([0.2, 0.5, 0.9]), [0.2, 0.5, 0.9]);
        assert!(close(f.source_of([0.25, 0.75]).unwrap(), [0.25, 0.75]));
        assert_eq!(f.delay_ms(), 0);
    }

    #[test]
    fn colour_strength_gamma_and_black() {
        let f = ScreenFix { red: 0.5, gamma: 2.0, black: 0.1, ..Default::default() };
        let [r, g, b] = f.colour([1.0, 0.25, 0.0]);
        assert!((r - (0.1 + 0.9 * 0.5f32.sqrt())).abs() < 1e-5, "half the red, then gamma");
        assert!((g - (0.1 + 0.9 * 0.5)).abs() < 1e-5, "gamma 2 lifts a quarter to a half");
        assert!((b - 0.1).abs() < 1e-6, "black becomes the black level");
        assert_eq!(ScreenFix { green: 2.0, ..Default::default() }.colour([0.0, 0.8, 0.0])[1], 1.0, "never past full");
    }

    #[test]
    fn a_moved_corner_moves_that_corner_of_the_picture() {
        // The top left pulled in to (0.1, 0.05): the picture's top left corner shows there, and the screen's own
        // corner shows nothing.
        let f = ScreenFix { corners: [[0.1, 0.05], [0.0; 2], [0.0; 2], [0.0; 2]], ..Default::default() };
        assert!(f.needs_pass());
        assert!(close(f.source_of([0.1, 0.05]).unwrap(), [0.0, 0.0]));
        assert!(close(f.source_of([1.0, 1.0]).unwrap(), [1.0, 1.0]), "the other corners stay");
        assert!(close(f.source_of([1.0, 0.0]).unwrap(), [1.0, 0.0]));
        assert_eq!(f.source_of([0.01, 0.01]), None, "black outside the picture");
    }

    #[test]
    fn keystone_is_a_true_perspective() {
        // A projector tilted up: the top is wider. Squared up by pulling both top corners in.
        let f = ScreenFix { corners: [[0.1, 0.0], [-0.1, 0.0], [0.0; 2], [0.0; 2]], ..Default::default() };
        let mid_top = f.source_of([0.5, 0.0]).unwrap();
        assert!(close(mid_top, [0.5, 0.0]));
        let s = f.source_of([0.5, 0.5]).unwrap();
        assert!((s[0] - 0.5).abs() < 1e-4);
        assert!(s[1] > 0.52, "perspective: the narrow top half is squeezed, so the middle of the screen shows from lower down ({})", s[1]);
    }

    #[test]
    fn folded_corners_show_nothing_rather_than_fail() {
        let f = ScreenFix { corners: [[1.2, 1.2], [0.0; 2], [-1.2, -1.2], [0.0; 2]], ..Default::default() };
        let _ = f.screen_to_picture(); // never panics
        assert!(ScreenFix { corners: [[0.3, 0.3], [-0.3, 0.3], [-0.3, -0.3], [0.3, -0.3]], ..Default::default() }.screen_to_picture().is_some());
    }

    #[test]
    fn numbers_out_of_range_are_brought_back() {
        let f = ScreenFix { red: 9.0, gamma: 0.0, black: -1.0, corners: [[2.0, f32::NAN], [0.0; 2], [0.0; 2], [0.0; 2]], delay: 500, ..Default::default() }.clamped();
        assert_eq!((f.red, f.gamma, f.black, f.corners[0], f.delay), (2.0, 0.4, 0.0, [0.3, 0.0], 30));
        assert_eq!(f.delay_ms(), 30 * 16);
    }

    #[test]
    fn saved_without_a_fix_reads_as_none() {
        let f: ScreenFix = serde_json::from_str("{}").unwrap();
        assert!(f.is_default());
        let f: ScreenFix = serde_json::from_str(r#"{"red":0.9}"#).unwrap();
        assert_eq!((f.red, f.green), (0.9, 1.0));
    }

    #[test]
    fn patterns() {
        assert!(pattern_pixels(TestPattern::Off, (16, 9)).is_none());
        let white = pattern_pixels(TestPattern::White, (4, 2)).unwrap();
        assert_eq!(white.len(), 32);
        assert!(white.iter().all(|b| *b == 255));
        let bars = pattern_pixels(TestPattern::Bars, (80, 2)).unwrap();
        assert_eq!(&bars[0..4], &[255, 255, 255, 255], "white first");
        assert_eq!(&bars[(15 * 4)..(15 * 4 + 4)], &[255, 255, 0, 255], "then yellow");
        assert_eq!(&bars[(79 * 4)..(79 * 4 + 4)], &[0, 0, 0, 255], "black last");
        let grid = pattern_pixels(TestPattern::Grid, (320, 180)).unwrap();
        assert_eq!(&grid[0..3], &[255, 255, 255], "a border");
        let middle = ((90 * 320 + 160) * 4) as usize;
        assert_eq!(&grid[middle..middle + 3], &[255, 210, 0], "a cross in the middle");
        let inside = ((5 * 320 + 5) * 4) as usize;
        assert_eq!(&grid[inside..inside + 3], &[0, 0, 0]);
        assert_eq!(serde_json::to_string(&TestPattern::Bars).unwrap(), "\"bars\"");
    }
}
