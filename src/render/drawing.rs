//! Lines drawn from a phone (`live::drawing`), as one transparent picture over the slide: smooth (anti-aliased,
//! round ends and corners) pen lines, and a wide see-through highlighter.
//!
//! There is no cap on how much is drawn, so a `DrawingCanvas` keeps every finished stroke baked into one cached
//! picture and draws only the strokes still being drawn on top of a copy of it: a long drawing costs one copy
//! per update, not every point again. Staying strokes are only ever taken away by undo or clear, which move
//! `Drawing::epoch`; then the cache is made again from scratch, once. Fading ink is drawn fresh each time, at its
//! opacity of the moment.
use super::raster::Premul;
use crate::live::drawing::{Drawing, HIGHLIGHTER_ALPHA, Pen, Stroke, scaled_width, to_screen};
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::time::Instant;
use tiny_skia::{Color, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapMut, Stroke as SkStroke, Transform};

/// Draws one stroke onto `pix` (`size`: its width and height) at `opacity` (fading ink; 1 otherwise).
pub fn stroke(pix: &mut PixmapMut, s: &Stroke, size: (u32, u32), opacity: f32) {
    let Some(first) = s.points.first() else { return };
    if opacity <= 0.0 { return }
    let [r, g, b] = s.color;
    let alpha = if s.pen == Pen::Highlighter { HIGHLIGHTER_ALPHA } else { 255 };
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(r, g, b, (f32::from(alpha) * opacity.min(1.0)).round() as u8));
    paint.anti_alias = true;
    let width = scaled_width(s.width, size.1).max(1.0);
    let pts: Vec<(f32, f32)> = s.points.iter().map(|p| to_screen(*p, size)).collect();
    let (x0, y0) = to_screen(*first, size);
    // A tap is a round dot.
    if pts.len() == 1 {
        if let Some(path) = PathBuilder::from_circle(x0, y0, width / 2.0) { pix.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None) }
        return;
    }
    // Smooth: a curve through the middle of each pair of points, bending at the points themselves.
    let mut pb = PathBuilder::new();
    pb.move_to(x0, y0);
    for w in pts.windows(2).skip(1) {
        let ((cx, cy), (nx, ny)) = (w[0], w[1]);
        pb.quad_to(cx, cy, (cx + nx) / 2.0, (cy + ny) / 2.0);
    }
    let (lx, ly) = pts[pts.len() - 1];
    pb.line_to(lx, ly);
    let Some(path) = pb.finish() else { return };
    // One path per stroke, so a highlighter crossing itself is not darker where it overlaps.
    let sk = SkStroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..SkStroke::default() };
    pix.stroke_path(&path, &paint, &sk, Transform::identity(), None);
}

/// One screen's drawing layer, with its cache of finished staying strokes.
#[derive(Default)]
pub struct DrawingCanvas {
    base: Option<Pixmap>,
    size: (u32, u32),
    epoch: u64,
    /// The staying strokes in `base`: every one up to the serial `upto` (serials grow along the list), and how many.
    upto: u64,
    baked: usize,
}

impl DrawingCanvas {
    pub fn new() -> Self { Self::default() }

    /// How many strokes are baked into the cache (tests).
    pub fn baked(&self) -> usize { self.baked }

    /// `d` at `size` as premultiplied RGBA, or `None` when nothing is drawn.
    pub fn render(&mut self, d: &Drawing, size: (u32, u32)) -> Option<Premul> { self.render_at(d, size, Instant::now()) }

    /// `render`, with fading ink as it is at `now`.
    pub fn render_at(&mut self, d: &Drawing, size: (u32, u32), now: Instant) -> Option<Premul> {
        if d.is_empty() || size.0 == 0 || size.1 == 0 { *self = Self::default(); return None }
        if self.base.is_none() || self.size != size || self.epoch != d.epoch {
            *self = Self { base: Some(Pixmap::new(size.0, size.1)?), size, epoch: d.epoch, upto: 0, baked: 0 };
        }
        let base = self.base.as_mut()?;
        // Bake the finished staying strokes in order; stop at the first one still being drawn, so the order stays
        // right. Fading ink is never baked: it changes every frame while it fades.
        let from = self.upto;
        for s in d.strokes.iter().filter(|s| !s.fades() && s.serial > from) {
            if s.open { break }
            stroke(&mut base.as_mut(), s, size, 1.0);
            self.upto = s.serial;
            self.baked += 1;
        }
        let mut px: SharedPixelBuffer<Rgba8Pixel> = SharedPixelBuffer::new(size.0, size.1);
        {
            let bytes = px.make_mut_bytes();
            bytes.copy_from_slice(base.data());
            let mut pix = PixmapMut::from_bytes(bytes, size.0, size.1)?;
            for s in &d.strokes {
                if s.fades() { stroke(&mut pix, s, size, s.opacity(now, d.fade_after)) }
                else if s.serial > self.upto { stroke(&mut pix, s, size, 1.0) }
            }
        }
        Some(Premul::from_buffer(px))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::drawing::{DrawCmd, PALETTE};
    use std::time::Instant;
    use uuid::Uuid;

    fn at(px: &Premul, (w, _): (u32, u32), (x, y): (u32, u32)) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }
    fn line(d: &mut Drawing, phone: Uuid, id: u32, color: &str, pen: Pen, width: f32, pts: Vec<[f32; 2]>) {
        d.apply(phone, &DrawCmd::Start { stroke: id, color: color.into(), width, pen, points: pts, ink: Default::default() }).unwrap();
    }

    #[test]
    fn a_pen_line_is_solid_smooth_and_where_it_was_drawn() {
        let me = Uuid::new_v4();
        let mut d = Drawing::default();
        line(&mut d, me, 1, PALETTE[0], Pen::Pen, 13.0, vec![[0.25, 0.5], [0.75, 0.5]]);
        let size = (640, 360);
        let px = DrawingCanvas::new().render(&d, size).unwrap();
        assert_eq!(at(&px, size, (320, 180)), [0xff, 0xd6, 0x0a, 255], "yellow, solid, on the line");
        assert_eq!(at(&px, size, (320, 100))[3], 0, "clear away from it");
        assert_eq!(at(&px, size, (160, 180))[3], 255, "from x = 0.25");
        // Round ends reach past the end point by half the width (13 px at 1080 → 4.3 px here).
        assert!(at(&px, size, (158, 180))[3] > 0, "round cap");
        // Anti-aliased: some pixel at the edge is part-covered.
        let edge: Vec<u8> = (170..=190).map(|y| at(&px, size, (320, y))[3]).collect();
        assert!(edge.iter().any(|a| *a > 0 && *a < 255), "{edge:?}");
    }

    #[test]
    fn the_highlighter_is_see_through_and_even_where_it_crosses_itself() {
        let me = Uuid::new_v4();
        let mut d = Drawing::default();
        // Across and back (the left part is gone over twice), and a plain line below it.
        line(&mut d, me, 1, PALETTE[0], Pen::Highlighter, 44.0, vec![[0.2, 0.3], [0.8, 0.3], [0.2, 0.3]]);
        line(&mut d, me, 2, PALETTE[0], Pen::Highlighter, 44.0, vec![[0.2, 0.7], [0.8, 0.7]]);
        let size = (400, 400);
        let px = DrawingCanvas::new().render(&d, size).unwrap();
        let once = at(&px, size, (120, 280))[3];
        assert!(once > 60 && once < 150, "see-through: {once}");
        assert_eq!(at(&px, size, (120, 120))[3], once, "no darker where it went over itself");
    }

    #[test]
    fn a_tap_is_a_dot_and_nothing_drawn_is_no_picture() {
        let mut c = DrawingCanvas::new();
        assert!(c.render(&Drawing::default(), (100, 100)).is_none());
        let mut d = Drawing::default();
        line(&mut d, Uuid::new_v4(), 1, PALETTE[1], Pen::Pen, 22.0, vec![[0.5, 0.5]]);
        let px = c.render(&d, (200, 200)).unwrap();
        assert_eq!(at(&px, (200, 200), (100, 100)), [0xff, 0x3b, 0x30, 255]);
    }

    #[test]
    fn finished_strokes_are_baked_and_undo_redraws_from_scratch() {
        let (me, you) = (Uuid::new_v4(), Uuid::new_v4());
        let size = (320, 180);
        let mut d = Drawing::default();
        let mut c = DrawingCanvas::new();
        line(&mut d, me, 1, PALETTE[2], Pen::Pen, 12.0, vec![[0.1, 0.1], [0.4, 0.1]]);
        c.render(&d, size);
        assert_eq!(c.baked(), 0, "still being drawn");
        d.apply(me, &DrawCmd::End { stroke: 1 }).unwrap();
        line(&mut d, you, 1, PALETTE[3], Pen::Pen, 12.0, vec![[0.1, 0.9], [0.4, 0.9]]);
        let px = c.render(&d, size).unwrap();
        assert_eq!(c.baked(), 1);
        assert_eq!(at(&px, size, (80, 18))[1], 0xc7, "green baked");
        assert_eq!(at(&px, size, (80, 162))[2], 0xff, "blue drawn live");
        d.apply(me, &DrawCmd::Undo).unwrap();
        let px = c.render(&d, size).unwrap();
        assert_eq!(at(&px, size, (80, 18))[3], 0, "the undone stroke is gone from the cache too");
        assert_eq!(at(&px, size, (80, 162))[2], 0xff);
        // Same result as drawing it all fresh.
        assert_eq!(&*px, &*DrawingCanvas::new().render(&d, size).unwrap());
    }

    /// No limits: 200 000 points still render, and with the finished strokes baked an update while drawing costs
    /// a copy of the cache plus the stroke under the finger.
    #[test]
    fn a_very_large_drawing_still_renders_quickly() {
        let me = Uuid::new_v4();
        let size = (1920, 1080);
        let mut d = Drawing::default();
        // Under the slide's ceiling (`MAX_POINTS`), with room to keep drawing.
        for id in 0..150u32 {
            let y = 0.05 + 0.9 * id as f32 / 150.0;
            let pts: Vec<[f32; 2]> = (0..1000).map(|i| { let t = i as f32 / 1000.0; [0.05 + 0.9 * t, y + 0.02 * (t * 40.0).sin()] }).collect();
            line(&mut d, me, id, PALETTE[id as usize % 6], if id % 3 == 0 { Pen::Highlighter } else { Pen::Pen }, 12.0, pts);
        }
        d.apply(me, &DrawCmd::End { stroke: 149 }).unwrap();
        assert_eq!(d.points(), 150_000);
        let mut c = DrawingCanvas::new();
        let t = Instant::now();
        assert!(c.render(&d, size).is_some());
        let full = t.elapsed();
        assert_eq!(c.baked(), 150);
        // Keep drawing: a new stroke grows batch by batch.
        line(&mut d, me, 500, PALETTE[0], Pen::Pen, 12.0, vec![[0.5, 0.5]]);
        let t = Instant::now();
        for i in 0..20 {
            d.apply(me, &DrawCmd::More { stroke: 500, points: vec![[0.5 + i as f32 / 100.0, 0.5]] }).unwrap();
            assert!(c.render(&d, size).is_some());
        }
        let each = t.elapsed() / 20;
        eprintln!("200k points: first render {full:?}, then {each:?} per update");
        // Debug builds are far slower than the app; these bounds hold there with room to spare.
        assert!(full.as_secs_f32() < 60.0, "{full:?}");
        assert!(each.as_secs_f32() < 0.5, "{each:?}");
        assert!(each * 4 < full, "an update must not redraw every point: {each:?} vs {full:?}");
    }

    #[test]
    fn fading_ink_fades_on_screen_and_staying_ink_does_not() {
        use crate::live::drawing::Ink;
        use std::time::Duration;
        let me = Uuid::new_v4();
        let size = (320, 180);
        let mut d = Drawing::default();
        let t0 = Instant::now();
        d.apply_at(me, &DrawCmd::Start { stroke: 1, color: PALETTE[4].into(), width: 30.0, pen: Pen::Pen, points: vec![[0.1, 0.2], [0.9, 0.2]], ink: Ink::Stays }, t0).unwrap();
        d.apply_at(me, &DrawCmd::Start { stroke: 2, color: PALETTE[4].into(), width: 30.0, pen: Pen::Pen, points: vec![[0.1, 0.8], [0.9, 0.8]], ink: Ink::Fades }, t0).unwrap();
        d.apply_at(me, &DrawCmd::End { stroke: 2 }, t0).unwrap();
        let mut c = DrawingCanvas::new();
        let alpha = |c: &mut DrawingCanvas, d: &Drawing, ms: u64| {
            let px = c.render_at(d, size, t0 + Duration::from_millis(ms)).unwrap();
            (at(&px, size, (160, 36))[3], at(&px, size, (160, 144))[3])
        };
        assert_eq!(alpha(&mut c, &d, 1_000), (255, 255), "both solid at first");
        let (stay, fade) = alpha(&mut c, &d, 3_250);
        assert_eq!(stay, 255);
        assert!((120..=135).contains(&fade), "half way through its fade: {fade}");
        assert_eq!(alpha(&mut c, &d, 3_600), (255, 0), "gone from the screen");
        assert_eq!(c.baked(), 1, "only the staying line is baked");
        assert!(d.expire(t0 + Duration::from_millis(3_600)));
        assert_eq!(alpha(&mut c, &d, 3_600), (255, 0));
        assert_eq!(c.baked(), 1, "removing faded ink leaves the cache alone");
    }
}
