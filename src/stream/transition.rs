//! How the stream changes from one scene to the next (design `docs/superpowers/specs/2026-11-17-scene-extras-design.md`):
//! a cut, a fade, a slide (the new scene pushes the old one out to the left) or a stinger (a short video played over
//! the change, the scene changing at its middle). Each of the church's scenes can have its own; the others use the
//! default from Settings. `Running` is one change under way, drawn by the recorder's writer frame by frame.
use super::compose::{blend, over};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// How long a fade or a slide takes.
pub const LENGTH: Duration = super::compose::FADE;
/// A stinger whose first picture has not come by then is left out: the scene changes at once.
const STINGER_WAIT: Duration = Duration::from_millis(1500);
/// A stinger never runs longer than this.
const STINGER_MOST: Duration = Duration::from_secs(10);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Transition {
    /// At once.
    Cut,
    /// The new scene comes in from the right, pushing the old one out.
    Slide,
    /// A short video file played over the change.
    Stinger { #[serde(default)] path: String },
    /// The old scene fades into the new one (also anything a newer RustyPresenter made).
    #[default]
    #[serde(other)]
    Fade,
}

impl Transition {
    pub fn is_fade(&self) -> bool { *self == Transition::Fade }

    /// The choices offered, in order.
    pub fn choices() -> [Transition; 4] { [Transition::Cut, Transition::Fade, Transition::Slide, Transition::Stinger { path: String::new() }] }

    pub fn index(&self) -> usize { match self { Transition::Cut => 0, Transition::Fade => 1, Transition::Slide => 2, Transition::Stinger { .. } => 3 } }

    /// Choice `i`, keeping a stinger's video when it stays a stinger.
    pub fn changed_to(&self, i: usize) -> Transition {
        match (i, self) {
            (3, Transition::Stinger { .. }) => self.clone(),
            (i, _) => Transition::choices().into_iter().nth(i).unwrap_or_default(),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Transition::Cut => crate::tr!("Cut (at once)"),
            Transition::Fade => crate::tr!("Fade"),
            Transition::Slide => crate::tr!("Slide"),
            Transition::Stinger { .. } => crate::tr!("Stinger video"),
        }
    }

    /// Its stinger's video, if any.
    pub fn stinger(&self) -> Option<&str> {
        match self { Transition::Stinger { path } if !path.trim().is_empty() => Some(path.trim()), _ => None }
    }
}

/// A stinger playing: its pictures (premultiplied RGBA of the stream's size, one for each frame at the stream's
/// frame rate) as they come, and the frame at which the scene changes.
pub struct Playing {
    pub frames: std::sync::mpsc::Receiver<Vec<u8>>,
    /// The scene changes after this many frames.
    pub middle: u32,
    pub fps: u32,
}

/// A stinger playing over both pictures, the wide one and the tall one (when there is one).
pub struct Stinger { pub wide: Playing, pub tall: Option<Playing> }

/// One picture's share of a stinger: the frames taken so far and the newest.
struct Track { playing: Playing, shown: u32, last: Option<Vec<u8>>, ended: bool }

impl Track {
    fn new(playing: Playing) -> Track { Track { playing, shown: 0, last: None, ended: false } }

    /// Takes the frames due `t` after the start.
    fn catch_up(&mut self, t: Duration) {
        let due = (t.as_secs_f64() * f64::from(self.playing.fps.max(1))).floor() as u32 + 1;
        while self.shown < due && !self.ended {
            match self.playing.frames.try_recv() {
                Ok(f) => { self.last = Some(f); self.shown += 1 }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.ended = true,
            }
        }
    }

    fn past_middle(&self) -> bool { self.shown > self.playing.middle || self.ended }
}

enum Kind { Fade, Slide, Stinger { wide: Track, tall: Option<Track>, first: Option<Instant> } }

/// A change under way, from the picture that was going out (and its tall one).
pub struct Running {
    kind: Kind,
    from: Vec<u8>,
    from_tall: Option<Vec<u8>>,
    at: Instant,
}

/// One picture: its size, the new scene's picture, and where the frame goes.
pub struct Frame<'a> { pub size: (u32, u32), pub next: &'a [u8], pub out: &'a mut [u8] }

impl Running {
    /// Starts `t` from the pictures going out now. `None` for a cut. A stinger with no video (or none playing:
    /// `stinger`) fades instead.
    pub fn start(t: &Transition, from: &[u8], from_tall: Option<&[u8]>, now: Instant, stinger: Option<Stinger>) -> Option<Running> {
        let kind = match t {
            Transition::Cut => return None,
            Transition::Fade => Kind::Fade,
            Transition::Slide => Kind::Slide,
            Transition::Stinger { .. } => match stinger {
                Some(s) => Kind::Stinger { wide: Track::new(s.wide), tall: s.tall.map(Track::new), first: None },
                None => Kind::Fade,
            },
        };
        Some(Running { kind, from: from.to_vec(), from_tall: from_tall.map(<[u8]>::to_vec), at: now })
    }

    /// Draws the change at `now` into each picture's `out`. False once it is over (`out` is then left alone: the
    /// caller puts the new scene's picture out).
    pub fn frame(&mut self, wide: Frame, tall: Option<Frame>, now: Instant) -> bool {
        let t = now.saturating_duration_since(self.at);
        match &mut self.kind {
            Kind::Fade | Kind::Slide if t >= LENGTH => false,
            Kind::Fade => {
                let k = t.as_secs_f32() / LENGTH.as_secs_f32();
                blend(&self.from, wide.next, k, wide.out);
                if let (Some(f), Some(tl)) = (&self.from_tall, tall) { blend(f, tl.next, k, tl.out) }
                true
            }
            Kind::Slide => {
                let k = t.as_secs_f32() / LENGTH.as_secs_f32();
                // Slow at both ends.
                let k = k * k * (3.0 - 2.0 * k);
                slide(&self.from, wide.next, wide.size, k, wide.out);
                if let (Some(f), Some(tl)) = (&self.from_tall, tall) { slide(f, tl.next, tl.size, k, tl.out) }
                true
            }
            Kind::Stinger { wide: w, tall: tt, first } => {
                if t >= STINGER_MOST { return false }
                // Its time runs from its first picture: until then the old scene stays.
                let since = first.map(|f| now.saturating_duration_since(f)).unwrap_or_default();
                w.catch_up(since);
                if let Some(tt) = tt.as_mut() { tt.catch_up(since) }
                if first.is_none() {
                    if w.last.is_none() {
                        if t >= STINGER_WAIT || w.ended { return false }
                        wide.out.copy_from_slice(&self.from);
                        if let (Some(f), Some(tl)) = (&self.from_tall, tall) { tl.out.copy_from_slice(f) }
                        return true;
                    }
                    *first = Some(now);
                }
                if w.ended { return false }
                let past = w.past_middle();
                wide.out.copy_from_slice(if past { wide.next } else { &self.from });
                if let Some(f) = &w.last && f.len() == wide.out.len() { over(wide.out, f) }
                if let (Some(from), Some(tl)) = (&self.from_tall, tall) {
                    tl.out.copy_from_slice(if past { tl.next } else { from });
                    if let Some(f) = tt.as_ref().and_then(|t| t.last.as_ref()) && f.len() == tl.out.len() { over(tl.out, f) }
                }
                true
            }
        }
    }
}

/// `from` pushed `k` (0 to 1) of the way out to the left by `to` coming in from the right.
fn slide(from: &[u8], to: &[u8], (w, h): (u32, u32), k: f32, out: &mut [u8]) {
    let shift = ((k.clamp(0.0, 1.0) * w as f32).round() as usize).min(w as usize);
    let row = w as usize * 4;
    let cut = shift * 4;
    for y in 0..h as usize {
        let r = y * row..(y + 1) * row;
        let (o, f, t) = (&mut out[r.clone()], &from[r.clone()], &to[r]);
        o[..row - cut].copy_from_slice(&f[cut..]);
        o[row - cut..].copy_from_slice(&t[..cut]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 4] = [200, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 200, 255];

    fn run(t: &Transition, stinger: Option<Stinger>, at_ms: &[u64]) -> Vec<Option<Vec<u8>>> {
        let (from, next) = (RED.repeat(4), BLUE.repeat(4));
        let t0 = Instant::now();
        let mut r = Running::start(t, &from, None, t0, stinger);
        at_ms.iter().map(|ms| {
            let r = r.as_mut()?;
            let mut out = vec![0u8; 16];
            r.frame(Frame { size: (4, 1), next: &next, out: &mut out }, None, t0 + Duration::from_millis(*ms)).then_some(out)
        }).collect()
    }

    #[test]
    fn a_cut_is_at_once_and_a_fade_takes_half_a_second() {
        assert!(Running::start(&Transition::Cut, &[0; 4], None, Instant::now(), None).is_none());
        let f = run(&Transition::Fade, None, &[250, 600]);
        assert_eq!(f[0].as_ref().map(|o| o[..4].to_vec()), Some(vec![100, 0, 100, 255]));
        assert_eq!(f[1], None, "over");
    }

    #[test]
    fn a_slide_pushes_the_old_scene_out_to_the_left() {
        let f = run(&Transition::Slide, None, &[0, 250, 600]);
        assert_eq!(f[0].as_deref(), Some(&RED.repeat(4)[..]), "the old scene at first");
        let half = f[1].clone().unwrap();
        assert_eq!((&half[..4], &half[12..]), (&RED[..], &BLUE[..]), "half way: the old on the left, the new on the right");
        assert_eq!(f[2], None);
    }

    fn stinger(frames: Vec<Vec<u8>>, middle: u32) -> Stinger {
        let (tx, rx) = std::sync::mpsc::sync_channel(frames.len().max(1));
        for f in frames { tx.send(f).unwrap() }
        Stinger { wide: Playing { frames: rx, middle, fps: 10 }, tall: None }
    }

    #[test]
    fn a_stinger_plays_over_the_change_and_the_scene_changes_at_its_middle() {
        // Four frames at 10 a second: clear but for a white first pixel; the scene changes after the second.
        let mut f = vec![0u8; 16];
        f[..4].copy_from_slice(&[255; 4]);
        let s = stinger(vec![f.clone(); 4], 2);
        let out = run(&Transition::Stinger { path: "x.webm".into() }, Some(s), &[0, 150, 250, 450]);
        let o0 = out[0].clone().unwrap();
        assert_eq!((&o0[..4], &o0[4..8]), (&[255u8; 4][..], &RED[..]), "the stinger over the old scene");
        let o2 = out[2].clone().unwrap();
        assert_eq!(&o2[4..8], &BLUE[..], "past its middle: the new scene under it");
        assert_eq!(out[3], None, "its frames have run out: over");
    }

    #[test]
    fn a_stinger_that_never_starts_does_not_hold_the_stream() {
        let (_tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(1);
        let s = Stinger { wide: Playing { frames: rx, middle: 5, fps: 30 }, tall: None };
        let out = run(&Transition::Stinger { path: "x.webm".into() }, Some(s), &[100, 2000]);
        assert_eq!(out[0].as_deref(), Some(&RED.repeat(4)[..]), "the old scene while waiting");
        assert_eq!(out[1], None);
        let f = run(&Transition::Stinger { path: String::new() }, None, &[250]);
        assert_eq!(f[0].as_ref().map(|o| o[..4].to_vec()), Some(vec![100, 0, 100, 255]), "no video: a fade");
    }

    #[test]
    fn transitions_are_kept_and_newer_ones_fade() {
        let t = Transition::Stinger { path: "C:/s.webm".into() };
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "stinger", "path": "C:/s.webm" }));
        assert_eq!(serde_json::from_value::<Transition>(v).unwrap(), t);
        assert_eq!(serde_json::from_value::<Transition>(serde_json::json!({ "kind": "wipe" })).unwrap(), Transition::Fade);
        for (i, c) in Transition::choices().iter().enumerate() { assert_eq!(c.index(), i) }
        assert_eq!(t.changed_to(3), t);
        assert_eq!(t.changed_to(2), Transition::Slide);
    }
}
