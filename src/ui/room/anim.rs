//! A designed slide's click group playing in the room (1.1): each picture of `Play::layers` is a `Sprite`, moved
//! from Rust every frame (`fade::TICK`, like the transitions) from `Play::pose`. Pure parts here; the timer and
//! the faces are in `room.rs` (`put_animated`). `Animator` is the frame timer the room, the operator's monitor
//! share: it runs only while a group plays.
use crate::design::{Play, Pose};
use crate::render::design::{AnimLayer, slide_area};
use crate::ui::Sprite;
use slint::{Image, Model, Timer, TimerMode, VecModel};
use std::{cell::RefCell, rc::Rc, time::Instant};

/// One layer's picture and place, in shares of the canvas, and the object it is (None: a still run).
#[derive(Clone)]
pub struct SpriteImg { pub image: Image, pub x: f32, pub y: f32, pub w: f32, pub h: f32, pub obj: Option<usize> }

/// Layers drawn off the UI thread (pixels are `Send`, an `Image` is not).
pub struct Layers { pub canvas: (u32, u32), pub layers: Vec<AnimLayer> }

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
pub(crate) fn area(canvas: (u32, u32)) -> (f32, f32, f32, f32) {
    let (x, y, w, h) = slide_area(canvas);
    let (cw, ch) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
    (x / cw, y / ch, w / cw, h / ch)
}

/// `img` at `t_ms` into `play`, on a canvas whose slide is at `area` (shares).
pub(crate) fn sprite_at(img: &SpriteImg, play: &Play, t_ms: f32, (ax, ay, aw, ah): (f32, f32, f32, f32)) -> Sprite {
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

/// A click group playing: its pictures, the slide's place, and `lead_ms`, the slide's own transition before it
/// (group 0 of a new slide): the pictures, as they are when the group starts, come in over it, then it plays.
pub(in crate::ui) struct Running { pub imgs: Vec<SpriteImg>, pub play: Play, pub area: (f32, f32, f32, f32), pub lead_ms: f32 }

impl Running {
    /// How long it all takes.
    pub fn total_ms(&self) -> f32 { self.lead_ms + self.play.duration_ms() as f32 }

    /// The pictures `t_ms` after it started and how far the lead is (0–1); None once it is over.
    pub fn at(&self, t_ms: f32) -> Option<(Vec<Sprite>, f32)> {
        if t_ms >= self.total_ms() { return None }
        let k = if self.lead_ms > 0.0 { crate::ui::fade::ease(t_ms / self.lead_ms) } else { 1.0 };
        let t = (t_ms - self.lead_ms).max(0.0);
        let rows = self.imgs.iter().map(|img| { let mut s = sprite_at(img, &self.play, t, self.area); s.op *= k; s }).collect();
        Some((rows, k))
    }
}

/// Plays a `Running` on the shared frame timer (`fade::TICK`), only while it runs: each frame hands `frame` the
/// pictures and how far the lead is, then `None` once it is over.
#[derive(Default)]
pub(in crate::ui) struct Animator {
    run: Rc<RefCell<Option<(Running, Instant)>>>,
    timer: Rc<Timer>,
    /// One step of what plays, for the timer and for the window's frames (`frame_tick`).
    step: RefCell<Option<Rc<dyn Fn()>>>,
    /// When a frame last moved it on (`fade::fed`).
    fed: Rc<std::cell::Cell<Option<Instant>>>,
}

impl Animator {
    /// Starts `run` (stopping what played), drawing its first frame now.
    pub fn start(&self, run: Running, frame: impl Fn(Option<(Vec<Sprite>, f32)>) + 'static) {
        *self.run.borrow_mut() = Some((run, Instant::now()));
        let (cell, timer) = (self.run.clone(), Rc::downgrade(&self.timer));
        let tick = move || {
            let rows = cell.borrow().as_ref().map(|(r, at)| r.at(at.elapsed().as_secs_f32() * 1000.0));
            match rows {
                None => {}
                Some(Some(rows)) => frame(Some(rows)),
                Some(None) => {
                    cell.borrow_mut().take();
                    if let Some(t) = timer.upgrade() { t.stop() }
                    frame(None)
                }
            }
        };
        tick();
        if self.run.borrow().is_none() {
            self.step.borrow_mut().take();
            return self.timer.stop()
        }
        let tick: Rc<dyn Fn()> = Rc::new(tick);
        *self.step.borrow_mut() = Some(tick.clone());
        self.fed.set(None);
        let fed = self.fed.clone();
        // While the window's frames move it on, the timer leaves it to them.
        self.timer.start(TimerMode::Repeated, crate::ui::fade::TICK, move || if !crate::ui::fade::fed(fed.get(), Instant::now()) { tick() });
    }

    /// One step from a frame of the window (`fade::next_step`), so the pictures move once per frame shown.
    pub fn frame_tick(&self) {
        if !self.running() { return }
        // Taken out first: the step may start or stop a run.
        let step = self.step.borrow().clone();
        if let Some(step) = step {
            self.fed.set(Some(Instant::now()));
            step()
        }
    }

    /// Stops it where it is; false when nothing played. `frame` is not called.
    pub fn stop(&self) -> bool {
        self.timer.stop();
        self.step.borrow_mut().take();
        self.run.borrow_mut().take().is_some()
    }

    pub fn running(&self) -> bool { self.run.borrow().is_some() }
}

/// Writes the sprites' rows: changed in place when the count is the same, so the window keeps its items.
pub(in crate::ui) fn set_rows(model: &VecModel<Sprite>, rows: Vec<Sprite>) {
    if model.row_count() == rows.len() { for (i, r) in rows.into_iter().enumerate() { model.set_row_data(i, r) } } else { model.set_vec(rows) }
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

    #[test]
    fn a_lead_brings_the_starting_pictures_in_then_the_group_plays() {
        let o = Obj::new(ObjKind::Shape { shape: Shape::Rect }, PctRect::new(50.0, 50.0, 20.0, 20.0));
        let mut fade = Anim::new(o.id, AnimKind::Entrance, Effect::Fade, Start::WithPrevious);
        fade.dur_ms = 1000;
        let play = Design { objects: vec![o], anims: vec![fade], ..Default::default() }.play(0).unwrap();
        let img = |obj| SpriteImg { image: Image::default(), x: 0.5, y: 0.5, w: 0.2, h: 0.2, obj };
        let run = Running { imgs: vec![img(None), img(Some(0))], play, area: (0.0, 0.0, 1.0, 1.0), lead_ms: 400.0 };
        assert_eq!(run.total_ms(), 1400.0);
        let (rows, k) = run.at(200.0).unwrap();
        assert!((k - 0.5).abs() < 1e-4 && (rows[0].op - 0.5).abs() < 1e-4, "the still part comes in with the transition");
        assert_eq!(rows[1].op, 0.0, "the fading one waits, hidden");
        let (rows, k) = run.at(900.0).unwrap();
        assert_eq!((k, rows[0].op), (1.0, 1.0));
        assert!(rows[1].op > 0.3 && rows[1].op < 0.7, "half way in: {}", rows[1].op);
        assert!(run.at(1400.0).is_none());
        // No lead: it plays at once.
        let run = Running { lead_ms: 0.0, ..run };
        assert_eq!(run.at(0.0).unwrap().1, 1.0);
    }
}
