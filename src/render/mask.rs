//! N12 masks: the shape a audience screen's picture is cut to (a smaller rectangle, rounded corners or an oval). Outside
//! the shape the screen stays black. Each audience screen has its own (`Screen::mask`); the room window draws the
//! picture made here over everything else.
use serde::{Deserialize, Serialize};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};

/// The largest inset on each side, in percent of the screen.
pub const MAX_INSET: u8 = 40;
/// The largest corner, in percent of the shorter side.
pub const MAX_RADIUS: u8 = 50;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum MaskShape {
    /// The whole screen shows.
    #[default]
    None,
    /// A smaller rectangle.
    Inset,
    /// A rectangle with rounded corners.
    Rounded,
    /// An oval (a circle on a square screen).
    Ellipse,
}

impl MaskShape {
    pub const ALL: [MaskShape; 4] = [MaskShape::None, MaskShape::Inset, MaskShape::Rounded, MaskShape::Ellipse];

    pub fn index(self) -> usize { Self::ALL.iter().position(|s| *s == self).unwrap_or(0) }

    pub fn at(i: usize) -> MaskShape { Self::ALL.get(i).copied().unwrap_or_default() }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(default)]
pub struct Mask {
    pub shape: MaskShape,
    /// Each side, in percent of the screen (0–40).
    pub inset: u8,
    /// Rounded: the corner, in percent of the shorter side (0–50).
    pub radius: u8,
}

impl Default for Mask {
    fn default() -> Self { Mask { shape: MaskShape::None, inset: 0, radius: 10 } }
}

impl Mask {
    /// Within range.
    pub fn clamped(self) -> Mask { Mask { shape: self.shape, inset: self.inset.min(MAX_INSET), radius: self.radius.min(MAX_RADIUS) } }

    /// Whether anything is cut away.
    pub fn cuts(&self) -> bool { self.shape != MaskShape::None }

    /// The shape's box on a `w`×`h` screen: (x, y, width, height).
    fn area(&self, w: f32, h: f32) -> (f32, f32, f32, f32) {
        let i = f32::from(self.inset.min(MAX_INSET)) / 100.0;
        (w * i, h * i, w * (1.0 - 2.0 * i), h * (1.0 - 2.0 * i))
    }
}

/// The mask as a `w`×`h` picture, premultiplied RGBA: black outside the shape, clear inside. `None` when nothing is
/// cut away (or the size is empty).
pub fn mask_pixels(mask: &Mask, w: u32, h: u32) -> Option<Pixmap> {
    if !mask.cuts() { return None }
    let mut px = Pixmap::new(w, h)?;
    let (fw, fh) = (w as f32, h as f32);
    let (x, y, aw, ah) = mask.area(fw, fh);
    let mut pb = PathBuilder::new();
    pb.push_rect(Rect::from_xywh(0.0, 0.0, fw, fh)?);
    match mask.shape {
        MaskShape::None => return None,
        MaskShape::Inset => { if let Some(r) = Rect::from_xywh(x, y, aw, ah) { pb.push_rect(r) } }
        MaskShape::Ellipse => { if let Some(r) = Rect::from_xywh(x, y, aw, ah) { pb.push_oval(r) } }
        MaskShape::Rounded => rounded(&mut pb, x, y, aw, ah, f32::from(mask.radius.min(MAX_RADIUS)) / 100.0 * aw.min(ah)),
    }
    let path = pb.finish()?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(0, 0, 0, 255);
    paint.anti_alias = true;
    px.fill_path(&path, &paint, FillRule::EvenOdd, Transform::identity(), None);
    Some(px)
}

/// A rounded rectangle, corners of radius `r` (at most half the shorter side).
fn rounded(pb: &mut PathBuilder, x: f32, y: f32, w: f32, h: f32, r: f32) {
    if w <= 0.0 || h <= 0.0 { return }
    let r = r.clamp(0.0, w.min(h) / 2.0);
    // A quarter circle as one cubic: the control points sit this far along the tangents.
    let k = r * 0.552_284_8;
    let (x2, y2) = (x + w, y + h);
    pb.move_to(x + r, y);
    pb.line_to(x2 - r, y);
    pb.cubic_to(x2 - r + k, y, x2, y + r - k, x2, y + r);
    pb.line_to(x2, y2 - r);
    pb.cubic_to(x2, y2 - r + k, x2 - r + k, y2, x2 - r, y2);
    pb.line_to(x + r, y2);
    pb.cubic_to(x + r - k, y2, x, y2 - r + k, x, y2 - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(px: &Pixmap, x: u32, y: u32) -> u8 { px.pixel(x, y).unwrap().alpha() }

    #[test]
    fn no_mask_draws_nothing() {
        assert!(mask_pixels(&Mask::default(), 100, 50).is_none());
        assert!(!Mask::default().cuts());
    }

    #[test]
    fn an_inset_blacks_out_the_edges_only() {
        let px = mask_pixels(&Mask { shape: MaskShape::Inset, inset: 10, radius: 0 }, 200, 100).unwrap();
        assert_eq!(alpha(&px, 5, 50), 255, "left edge is black");
        assert_eq!(alpha(&px, 195, 50), 255, "right edge is black");
        assert_eq!(alpha(&px, 100, 4), 255, "top edge is black");
        assert_eq!(alpha(&px, 100, 50), 0, "the middle shows");
        assert_eq!(alpha(&px, 22, 12), 0, "just inside the corner shows");
    }

    #[test]
    fn rounded_corners_cut_the_corner_but_not_the_edge_middle() {
        let px = mask_pixels(&Mask { shape: MaskShape::Rounded, inset: 0, radius: 30 }, 200, 100).unwrap();
        assert_eq!(alpha(&px, 1, 1), 255, "the corner is black");
        assert_eq!(alpha(&px, 100, 1), 0, "the top middle shows");
        assert_eq!(alpha(&px, 1, 50), 0, "the left middle shows");
        assert_eq!(alpha(&px, 100, 50), 0);
    }

    #[test]
    fn an_ellipse_shows_the_middle_and_hides_the_corners() {
        let px = mask_pixels(&Mask { shape: MaskShape::Ellipse, inset: 0, radius: 0 }, 200, 100).unwrap();
        assert_eq!(alpha(&px, 100, 50), 0);
        assert_eq!(alpha(&px, 100, 2), 0, "touches the top in the middle");
        for (x, y) in [(3, 3), (196, 3), (3, 96), (196, 96)] { assert_eq!(alpha(&px, x, y), 255, "corner {x},{y}") }
    }

    #[test]
    fn values_are_kept_in_range_and_saved_by_name() {
        let m = Mask { shape: MaskShape::Rounded, inset: 90, radius: 200 }.clamped();
        assert_eq!((m.inset, m.radius), (MAX_INSET, MAX_RADIUS));
        let json = serde_json::to_value(Mask { shape: MaskShape::Ellipse, inset: 4, radius: 10 }).unwrap();
        assert_eq!(json, serde_json::json!({ "shape": "ellipse", "inset": 4, "radius": 10 }));
        let old: Mask = serde_json::from_str("{}").unwrap();
        assert_eq!(old, Mask::default(), "screens saved before masks have none");
        assert_eq!(MaskShape::at(MaskShape::Rounded.index()), MaskShape::Rounded);
    }
}
