//! A designed slide's click group playing in the room (1.1): each picture of `Play::layers` is a `Sprite`, moved
//! from Rust every frame (`fade::TICK`, like the transitions) from `Play::pose`. Pure parts here; the timer and
//! the faces are in `room.rs` (`put_animated`).
use crate::design::{Play, Pose};
use crate::render::design::{AnimLayer, slide_area};
use crate::ui::Sprite;
use slint::Image;

/// One layer's picture and place, in shares of the canvas, and the object it is (None: a still run).
#[derive(Clone)]
pub(in crate::ui) struct SpriteImg { pub image: Image, pub x: f32, pub y: f32, pub w: f32, pub h: f32, pub obj: Option<usize> }

/// Layers drawn off the UI thread (pixels are `Send`, an `Image` is not).
pub(in crate::ui) struct Layers { pub canvas: (u32, u32), pub layers: Vec<AnimLayer> }

impl Layers {
    /// The pictures, placed in shares of the canvas.
    pub fn images(self) -> Vec<SpriteImg> {
        let (cw, ch) = (self.canvas.0.max(1) as f32, self.canvas.1.max(1) as f32);
        self.layers.into_iter().map(|l| SpriteImg {
            x: l.x as f32 / cw, y: l.y as f32 / ch, w: l.px.width() as f32 / cw, h: l.px.height() as f32 / ch, obj: l.obj,
            image: Image::from_rgba8_premultiplied(l.px),
        }).collect()
    }
}

/// The slide's 16:9 part of the canvas, in shares of it.
pub(in crate::ui) fn area(canvas: (u32, u32)) -> (f32, f32, f32, f32) {
    let (x, y, w, h) = slide_area(canvas);
    let (cw, ch) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
    (x / cw, y / ch, w / cw, h / ch)
}

/// `img` at `t_ms` into `play`, on a canvas whose slide is at `area` (shares).
pub(in crate::ui) fn sprite_at(img: &SpriteImg, play: &Play, t_ms: f32, (ax, ay, aw, ah): (f32, f32, f32, f32)) -> Sprite {
    let mut s = Sprite { image: img.image.clone(), x: img.x, y: img.y, w: img.w, h: img.h, ox: 0.5, oy: 0.5, scale: 1.0, op: 1.0, cw: 1.0, ch: 1.0, ..Default::default() };
    let Some(i) = img.obj else { return s };
    let Some(o) = play.objects.get(i) else { return s };
    let pose: Pose = play.pose(i, t_ms);
    // The object's box, in shares of the canvas, and where it is now.
    let (bx, by, bw, bh) = (ax + o.rect.x / 100.0 * aw, ay + o.rect.y / 100.0 * ah, o.rect.w / 100.0 * aw, o.rect.h / 100.0 * ah);
    (s.dx, s.dy) = (pose.dx / 100.0 * aw, pose.dy / 100.0 * ah);
    if img.w > 0.0 && img.h > 0.0 { (s.ox, s.oy) = ((bx + bw / 2.0 - img.x) / img.w, (by + bh / 2.0 - img.y) / img.h) }
    (s.scale, s.rot, s.op) = (pose.scale.max(0.0), pose.rot, pose.op.clamp(0.0, 1.0));
    if pose.clip != [0.0; 4] {
        let [l, r, t, b] = pose.clip;
        (s.cx, s.cy) = (bx + s.dx + bw * l, by + s.dy + bh * t);
        (s.cw, s.ch) = ((bw * (1.0 - l - r)).max(0.0), (bh * (1.0 - t - b)).max(0.0));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{Anim, AnimKind, Design, Dir, Effect, Obj, ObjKind, PctRect, Shape, Start};

    #[test]
    fn a_sprite_follows_its_objects_pose_and_a_still_run_stays() {
        let o = Obj::new(ObjKind::Shape { shape: Shape::Rect }, PctRect::new(50.0, 50.0, 20.0, 20.0));
        let mut wipe = Anim::new(o.id, AnimKind::Entrance, Effect::Wipe, Start::OnClick);
        (wipe.dir, wipe.dur_ms) = (Some(Dir::Left), 1000);
        let mut go = Anim::new(o.id, AnimKind::Motion, Effect::Path, Start::WithPrevious);
        (go.path, go.dur_ms) = (Some(crate::design::MotionPath { segs: vec![crate::design::Seg::Line { to: (-10.0, 0.0) }], ease: false }), 1000);
        let play = Design { objects: vec![o], anims: vec![wipe, go], ..Default::default() }.play(1).unwrap();
        let img = |obj| SpriteImg { image: Image::default(), x: 0.5, y: 0.5, w: 0.2, h: 0.2, obj };
        let full = (0.0, 0.0, 1.0, 1.0);
        let still = sprite_at(&img(None), &play, 500.0, full);
        assert_eq!((still.op, still.scale, still.cx, still.cw, still.dx), (1.0, 1.0, 0.0, 1.0, 0.0));
        let s = sprite_at(&img(Some(0)), &play, 500.0, full);
        assert!((s.dx + 0.05).abs() < 1e-4, "half way along: {}", s.dx);
        assert!((s.ox - 0.5).abs() < 1e-4 && (s.oy - 0.5).abs() < 1e-4);
        // Half wiped in from the left: the left half of the box (moved with it) shows.
        assert!((s.cx - 0.45).abs() < 1e-4 && (s.cw - 0.1).abs() < 1e-4 && (s.cy - 0.5).abs() < 1e-4, "{} {} {}", s.cx, s.cw, s.cy);
        // A 4:3 canvas: the slide sits in the middle band.
        assert_eq!(area((1600, 1200)), (0.0, 0.125, 1.0, 0.75));
    }
}
