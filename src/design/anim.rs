//! Object animations on a designed slide (1.1), shaped like PowerPoint's so imported decks keep theirs.
//!
//! **The model.** `Design::anims` is one ordered list of steps for the whole slide, as PowerPoint's animation pane:
//! each `Anim` names its object (`obj`, an `Obj::id`), a `kind` (`Entrance`, `Emphasis`, `Exit`, `Motion`), an
//! `effect`, how it starts (`start`), a `delay_ms` and `dur_ms`, a `dir` for effects that have a side, an `amount`
//! for Spin (degrees, default 360) and Grow (percent, default 150), and a `path` for a Motion step.
//!
//! - Effects are neutral; the kind says which way: `Fade` + `Entrance` is "Fade in", `Fade` + `Exit` is "Fade out",
//!   `Appear` + `Exit` is "Disappear", `Fly` + `Exit` is "Fly out". Entrance/exit effects: Appear, Fade, Fly, Float,
//!   Wipe, Split, Zoom. Emphasis: Spin, Grow, Pulse. Motion: Path. An effect name this version doesn't know loads
//!   as Fade.
//! - `dir` is the side the object comes in from (entrance) or goes out to (exit): `Fly` + `Down` = "Fly in from
//!   bottom"; `Wipe` + `Left` reveals from the left edge. None: Fly/Wipe from the bottom, Float from below.
//! - Motion paths are offsets from where the object is, in % of the slide (PowerPoint's `path="M 0 0 L 0.25 0.1 E"`
//!   is fractions of the slide: `MotionPath::parse_ppt` reads it, times 100). The object ends where the path ends.
//! - Clicks: each `OnClick` step starts a new click group; the `WithPrevious` / `AfterPrevious` steps after it join
//!   its group (`AfterPrevious` waits for everything before it in the group to end). Steps before the first
//!   `OnClick` form group 0, which plays by itself when the slide comes up. A slide takes one Next per `OnClick`.
//! - An object whose first entrance/exit step is an `Entrance` starts hidden. Exits leave objects hidden; Motion,
//!   Spin and Grow leave them where they ended; Pulse comes back.
//! - Old builds (`Obj::build`, 1.0 files) load as `Appear` + `Entrance` steps: the first object of each build
//!   number `OnClick`, the rest `WithPrevious`. They are never written again.
//!
//! `Design::at_step(n)` is the picture after group 0 and `n` clicks (what thumbnails, the operator and the phone
//! show); `Design::play(n)` is what the room animates to get there (`Play::pose` at any time).
use super::{Design, Dir, Obj};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The longest delay or duration of a step.
pub const ANIM_MAX_MS: u32 = 60_000;
/// Most steps one slide holds.
pub const MAX_ANIMS: usize = 300;

/// One animation step of one object.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Anim {
    pub obj: Uuid,
    pub kind: AnimKind,
    pub effect: Effect,
    #[serde(default)]
    pub start: Start,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub delay_ms: u32,
    #[serde(default = "half_second")]
    pub dur_ms: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<Dir>,
    /// Spin: degrees clockwise (default 360). Grow: size in percent (default 150).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<f32>,
    /// Motion: where the object goes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<MotionPath>,
    /// 1.2: only these paragraphs of a text box (first, last; from 0), as PowerPoint's "By paragraph".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub para: Option<(u16, u16)>,
}

fn is_zero(v: &u32) -> bool { *v == 0 }
fn half_second() -> u32 { 500 }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnimKind { #[default] Entrance, Emphasis, Exit, Motion }

/// What a step does (see the module notes for in/out).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    #[default]
    Appear,
    Fly,
    /// Fades while moving a little (PowerPoint's Float In / Float Out).
    Float,
    Wipe,
    /// Opens from the middle out (in), closes to the middle (out).
    Split,
    Zoom,
    Spin,
    Grow,
    Pulse,
    Path,
    /// Also what an unknown effect loads as (so it is last).
    #[serde(other)]
    Fade,
}

impl Effect {
    pub const IN_OUT: [Effect; 7] = [Effect::Appear, Effect::Fade, Effect::Fly, Effect::Float, Effect::Wipe, Effect::Split, Effect::Zoom];
    pub const EMPHASIS: [Effect; 3] = [Effect::Spin, Effect::Grow, Effect::Pulse];

    /// The effects a kind offers, in the editor's order.
    pub fn for_kind(kind: AnimKind) -> &'static [Effect] {
        match kind { AnimKind::Entrance | AnimKind::Exit => &Self::IN_OUT, AnimKind::Emphasis => &Self::EMPHASIS, AnimKind::Motion => &[Effect::Path] }
    }

    /// Fly, Float and Wipe come from (or go to) a side.
    pub fn has_dir(self) -> bool { matches!(self, Effect::Fly | Effect::Float | Effect::Wipe) }
}

/// When a step starts.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Start { #[default] OnClick, WithPrevious, AfterPrevious }

/// A motion path: offsets from where the object is, in % of the slide (x across, y down).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MotionPath {
    pub segs: Vec<Seg>,
    /// Starts and ends gently (PowerPoint's smooth start/end); false: even speed.
    #[serde(default = "yes")]
    pub ease: bool,
}

fn yes() -> bool { true }

/// One piece of a motion path.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Seg {
    /// Jump to a point (a path's start, when it is not 0, 0).
    Move { to: (f32, f32) },
    Line { to: (f32, f32) },
    Cubic { c1: (f32, f32), c2: (f32, f32), to: (f32, f32) },
}

impl MotionPath {
    /// A straight line from where the object is to `to`.
    pub fn line(to: (f32, f32)) -> MotionPath { MotionPath { segs: vec![Seg::Line { to }], ease: true } }

    /// Reads a PowerPoint path (`M 0 0 L 0.25 0.1 C … E`, fractions of the slide): M, L, C, Z, E and their
    /// lowercase (relative) forms. None when nothing usable is in it.
    pub fn parse_ppt(s: &str) -> Option<MotionPath> {
        let mut toks: Vec<String> = Vec::new();
        let mut cur = String::new();
        for ch in s.chars() {
            let exponent = matches!(ch, 'e' | 'E') && cur.ends_with(|c: char| c.is_ascii_digit() || c == '.');
            let sign_in_exponent = ch == '-' && cur.ends_with(['e', 'E']);
            if ch.is_ascii_alphabetic() && !exponent {
                if !cur.is_empty() { toks.push(std::mem::take(&mut cur)) }
                toks.push(ch.to_string());
            } else if ch.is_whitespace() || ch == ',' || ch == '-' && !cur.is_empty() && !sign_in_exponent {
                if !cur.is_empty() { toks.push(std::mem::take(&mut cur)) }
                if ch == '-' { cur.push(ch) }
            } else { cur.push(ch) }
        }
        if !cur.is_empty() { toks.push(cur) }
        let (mut segs, mut at, mut first, mut cmd) = (Vec::new(), (0.0f32, 0.0f32), (0.0f32, 0.0f32), 'M');
        let mut i = 0;
        let num = |i: &mut usize| -> Option<f32> { let v = toks.get(*i)?.parse::<f32>().ok().filter(|v| v.is_finite())?; *i += 1; Some(v * 100.0) };
        while i < toks.len() {
            if let Some(c) = toks[i].chars().next().filter(|c| c.is_ascii_alphabetic()) && toks[i].len() == 1 {
                cmd = c;
                i += 1;
                if matches!(c, 'Z' | 'z') { at = first; segs.push(Seg::Line { to: at }); continue }
                if matches!(c, 'E' | 'e') { break }
                continue;
            }
            let rel = cmd.is_ascii_lowercase();
            let off = |p: (f32, f32), at: (f32, f32)| if rel { (p.0 + at.0, p.1 + at.1) } else { p };
            let pt = |i: &mut usize| -> Option<(f32, f32)> { Some((num(i)?, num(i)?)) };
            match cmd.to_ascii_uppercase() {
                'M' => { let Some(p) = pt(&mut i) else { break }; at = off(p, at); first = at; segs.push(Seg::Move { to: at }); cmd = if rel { 'l' } else { 'L' } }
                'L' => { let Some(p) = pt(&mut i) else { break }; at = off(p, at); segs.push(Seg::Line { to: at }) }
                'C' => {
                    let (Some(a), Some(b), Some(c)) = (pt(&mut i), pt(&mut i), pt(&mut i)) else { break };
                    let (c1, c2, to) = (off(a, at), off(b, at), off(c, at));
                    at = to;
                    segs.push(Seg::Cubic { c1, c2, to });
                }
                _ => i += 1,
            }
        }
        if !segs.iter().any(|s| !matches!(s, Seg::Move { .. })) { return None }
        Some(MotionPath { segs, ease: true })
    }

    /// The path as points, curves cut into short lines, starting with its first point.
    pub fn points(&self) -> Vec<(f32, f32)> {
        let mut out = vec![(0.0, 0.0)];
        for s in &self.segs {
            let at = *out.last().unwrap();
            match *s {
                Seg::Move { to } => { if out.len() == 1 { out[0] = to } else { out.push(to) } }
                Seg::Line { to } => out.push(to),
                Seg::Cubic { c1, c2, to } => for k in 1..=16 {
                    let t = k as f32 / 16.0;
                    let u = 1.0 - t;
                    let f = |a: f32, b: f32, c: f32, d: f32| u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
                    out.push((f(at.0, c1.0, c2.0, to.0), f(at.1, c1.1, c2.1, to.1)));
                },
            }
        }
        out
    }

    /// Where the object is `p` (0–1) of the way along, by distance on a 16:9 screen.
    pub fn at(&self, p: f32) -> (f32, f32) {
        let pts = self.points();
        let len = |a: (f32, f32), b: (f32, f32)| ((b.0 - a.0) * 16.0).hypot((b.1 - a.1) * 9.0);
        let total: f32 = pts.windows(2).map(|w| len(w[0], w[1])).sum();
        if total <= 0.0 { return *pts.last().unwrap() }
        let mut want = p.clamp(0.0, 1.0) * total;
        for w in pts.windows(2) {
            let l = len(w[0], w[1]);
            if want <= l && l > 0.0 { let t = want / l; return (w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t) }
            want -= l;
        }
        *pts.last().unwrap()
    }

    /// The end, less the start: how far the object has moved once it is over.
    pub fn moved(&self) -> (f32, f32) { let (a, b) = (self.at(0.0), self.at(1.0)); (b.0 - a.0, b.1 - a.1) }
}

/// Where an object is between steps, relative to how it was designed.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct ObjState { pub shown: bool, pub dx: f32, pub dy: f32, pub scale: f32, pub rot: f32 }

impl Default for ObjState {
    fn default() -> Self { ObjState { shown: true, dx: 0.0, dy: 0.0, scale: 1.0, rot: 0.0 } }
}

/// How an object is drawn at one moment of a click group, relative to its state before the group: moved by
/// (`dx`, `dy`) % of the slide, grown `scale` times and turned `rot` degrees about its middle, at `op` (0–1) of its
/// own opacity, with `clip` (left, right, top, bottom shares of its box) cut off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose { pub op: f32, pub dx: f32, pub dy: f32, pub scale: f32, pub rot: f32, pub clip: [f32; 4] }

impl Pose {
    pub const SHOWN: Pose = Pose { op: 1.0, dx: 0.0, dy: 0.0, scale: 1.0, rot: 0.0, clip: [0.0; 4] };
    pub const HIDDEN: Pose = Pose { op: 0.0, ..Pose::SHOWN };
}

/// One step as it plays: which object (`Play::objects` index), from when in the group.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Track { pub obj: usize, pub begin_ms: u32, pub anim: Anim }

/// One click group to animate: every object as it was before the group (moved, grown and turned already, with
/// whether it was shown), and the group's steps.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Play { pub objects: Vec<Obj>, pub shown: Vec<bool>, pub tracks: Vec<Track> }

impl Play {
    /// How long the group takes.
    pub fn duration_ms(&self) -> u32 { self.tracks.iter().map(|t| t.begin_ms.saturating_add(t.anim.dur_ms)).max().unwrap_or(0) }

    /// Whether object `i` moves, shows or hides in this group.
    pub fn animated(&self, i: usize) -> bool { self.tracks.iter().any(|t| t.obj == i) }

    /// What the room draws while the group plays, bottom to top: runs of objects that stay as they are (drawn
    /// together, `None`) and each object that animates on its own (`Some(index)`).
    pub fn layers(&self) -> Vec<(Vec<usize>, Option<usize>)> {
        let mut out: Vec<(Vec<usize>, Option<usize>)> = vec![];
        for i in 0..self.objects.len() {
            if self.animated(i) { out.push((vec![i], Some(i))); continue }
            if !self.shown.get(i).copied().unwrap_or(false) { continue }
            match out.last_mut() { Some((run, None)) => run.push(i), _ => out.push((vec![i], None)) }
        }
        out
    }

    /// Object `i` at `t_ms` into the group.
    pub fn pose(&self, i: usize, t_ms: f32) -> Pose {
        let mut pose = if self.shown.get(i).copied().unwrap_or(false) { Pose::SHOWN } else { Pose::HIDDEN };
        let Some(o) = self.objects.get(i) else { return pose };
        for tr in self.tracks.iter().filter(|t| t.obj == i) {
            let a = &tr.anim;
            let since = t_ms - tr.begin_ms as f32;
            // Before it starts, an entrance's object stays as it was (hidden), the rest wait too.
            if since < 0.0 { continue }
            let raw = if a.dur_ms == 0 { 1.0 } else { (since / a.dur_ms as f32).clamp(0.0, 1.0) };
            let eased = match &a.path { Some(p) if !p.ease => raw, _ => smooth(raw) };
            step_pose(&mut pose, o, a, raw, eased);
        }
        pose
    }
}

fn smooth(t: f32) -> f32 { t * t * (3.0 - 2.0 * t) }

/// Applies `a` at `p` (eased; `raw` for Appear and Pulse) on top of `pose`.
fn step_pose(pose: &mut Pose, o: &Obj, a: &Anim, raw: f32, p: f32) {
    let r = o.rect;
    match a.kind {
        AnimKind::Entrance | AnimKind::Exit => {
            // How much is there: rising for an entrance, falling for an exit.
            let k = if a.kind == AnimKind::Entrance { p } else { 1.0 - p };
            let dir = a.dir.unwrap_or(Dir::Down);
            *pose = Pose { op: 1.0, ..*pose };
            match a.effect {
                Effect::Appear => pose.op = if a.kind == AnimKind::Entrance { 1.0 } else if raw >= 1.0 { 0.0 } else { 1.0 },
                Effect::Fade => pose.op = k,
                Effect::Fly => {
                    // Off the slide on `dir`'s side, by the object's own distance to that edge.
                    let off = match dir { Dir::Left => (-(r.x + r.w), 0.0), Dir::Right => (100.0 - r.x, 0.0), Dir::Up => (0.0, -(r.y + r.h)), Dir::Down => (0.0, 100.0 - r.y) };
                    pose.dx += off.0 * (1.0 - k);
                    pose.dy += off.1 * (1.0 - k);
                }
                Effect::Float => {
                    let (sx, sy) = dir.step();
                    pose.op = k;
                    pose.dx += sx * 5.0 * (1.0 - k);
                    pose.dy += sy * 10.0 * (1.0 - k);
                }
                Effect::Wipe => {
                    let cut = 1.0 - k;
                    match dir { Dir::Left => pose.clip[1] = cut, Dir::Right => pose.clip[0] = cut, Dir::Up => pose.clip[3] = cut, Dir::Down => pose.clip[2] = cut }
                }
                Effect::Split => { pose.clip[0] = (1.0 - k) / 2.0; pose.clip[1] = (1.0 - k) / 2.0 }
                Effect::Zoom => { pose.op = k; pose.scale *= k.max(0.0) }
                _ => pose.op = k,
            }
        }
        AnimKind::Emphasis => match a.effect {
            Effect::Spin => pose.rot += a.amount.unwrap_or(360.0) * p,
            Effect::Grow => pose.scale *= 1.0 + (a.amount.unwrap_or(150.0) / 100.0 - 1.0) * p,
            _ => pose.scale *= 1.0 + 0.1 * (raw * std::f32::consts::PI).sin(),
        },
        AnimKind::Motion => if let Some(path) = &a.path {
            let (s, at) = (path.at(0.0), path.at(p));
            pose.dx += at.0 - s.0;
            pose.dy += at.1 - s.1;
        },
    }
}

/// What a finished step leaves: shown or hidden, moved, grown, turned.
fn settle(st: &mut ObjState, a: &Anim) {
    match a.kind {
        AnimKind::Entrance => st.shown = true,
        AnimKind::Exit => st.shown = false,
        AnimKind::Emphasis => match a.effect {
            Effect::Spin => st.rot += a.amount.unwrap_or(360.0),
            Effect::Grow => st.scale *= a.amount.unwrap_or(150.0) / 100.0,
            _ => {}
        },
        AnimKind::Motion => if let Some(p) = &a.path { let (x, y) = p.moved(); st.dx += x; st.dy += y },
    }
}

/// `o` as it is drawn in state `st`.
pub fn posed(o: &Obj, st: &ObjState) -> Obj {
    let mut o = o.clone();
    let r = o.rect;
    let (w, h) = (r.w * st.scale, r.h * st.scale);
    o.rect = super::PctRect::new(r.x + st.dx + (r.w - w) / 2.0, r.y + st.dy + (r.h - h) / 2.0, w, h);
    o.rotation = ((o.rotation + st.rot + 180.0).rem_euclid(360.0)) - 180.0;
    if let super::ObjKind::Text { style, paras, .. } = &mut o.kind && st.scale != 1.0 {
        style.size = (style.size * st.scale).clamp(super::SIZE_MIN, super::SIZE_MAX);
        for r in paras.iter_mut().flat_map(|p| p.runs.iter_mut()) { if let Some(s) = &mut r.size { *s = (*s * st.scale).clamp(super::SIZE_MIN, super::SIZE_MAX) } }
    }
    o
}

impl Design {
    /// The click groups: group 0 (steps before the first OnClick, maybe none) then one per OnClick, each as
    /// (anims index, start in ms from the group's start).
    pub fn groups(&self) -> Vec<Vec<(usize, u32)>> {
        let mut out: Vec<Vec<(usize, u32)>> = vec![vec![]];
        let (mut par, mut end) = (0u32, 0u32);
        for (i, a) in self.anims.iter().enumerate() {
            match a.start {
                Start::OnClick => { out.push(vec![]); par = 0; end = 0 }
                Start::WithPrevious => {}
                Start::AfterPrevious => par = end,
            }
            let begin = par.saturating_add(a.delay_ms);
            end = end.max(begin.saturating_add(a.dur_ms));
            out.last_mut().unwrap().push((i, begin));
        }
        out
    }

    /// Every object's state before any step: hidden when its first entrance/exit is an entrance.
    fn first_states(&self) -> Vec<ObjState> {
        self.objects.iter().map(|o| {
            let first = self.anims.iter().find(|a| a.obj == o.id && matches!(a.kind, AnimKind::Entrance | AnimKind::Exit));
            ObjState { shown: !first.is_some_and(|a| a.kind == AnimKind::Entrance), ..Default::default() }
        }).collect()
    }

    /// Every object's state once group 0 and `clicks` click groups are over.
    pub fn states_after(&self, clicks: usize) -> Vec<ObjState> {
        let mut st = self.first_states();
        for g in self.groups().iter().take(clicks + 1) {
            for (i, _) in g {
                let a = &self.anims[*i];
                if let Some(o) = self.objects.iter().position(|o| o.id == a.obj) { settle(&mut st[o], a) }
            }
        }
        st
    }

    /// Group `group` to animate (0: the steps that play as the slide comes up). None when it has no steps.
    pub fn play(&self, group: u16) -> Option<Play> {
        if self.anims.iter().any(|a| a.para.is_some()) { return self.split_paras().play(group) }
        let groups = self.groups();
        let g = groups.get(usize::from(group)).filter(|g| !g.is_empty())?;
        let st = if group == 0 { self.first_states() } else { self.states_after(usize::from(group) - 1) };
        let objects = self.objects.iter().zip(&st).map(|(o, s)| posed(o, s)).collect();
        let tracks = g.iter().filter_map(|(i, begin)| {
            let a = &self.anims[*i];
            Some(Track { obj: self.objects.iter().position(|o| o.id == a.obj)?, begin_ms: *begin, anim: a.clone() })
        }).collect();
        Some(Play { objects, shown: st.iter().map(|s| s.shown).collect(), tracks })
    }

    /// The slide with each text box that has paragraph steps cut into pieces: the box (its fill, outline and the
    /// paragraphs no step names) and, right above it, one copy per paragraph range drawing only those paragraphs
    /// (all laid out as in the whole box). The steps then point at the pieces, so paragraphs come and go as objects.
    pub fn split_paras(&self) -> Design {
        let mut d = self.clone();
        let mut objects = Vec::with_capacity(d.objects.len());
        for o in std::mem::take(&mut d.objects) {
            let ranges: Vec<(u16, u16)> = d.anims.iter().filter(|a| a.obj == o.id).filter_map(|a| a.para).fold(vec![], |mut v, r| { if !v.contains(&r) { v.push(r) } v });
            let super::ObjKind::Text { text, style, paras } = &o.kind else { objects.push(o); continue };
            if ranges.is_empty() { objects.push(o); continue }
            let all = super::rich::paragraphs(text, paras);
            let inside = |i: usize, r: &(u16, u16)| (usize::from(r.0)..=usize::from(r.1)).contains(&i);
            let only = |keep: &dyn Fn(usize) -> bool| -> Vec<super::Para> {
                all.iter().enumerate().map(|(i, p)| super::Para { hidden: p.hidden || !keep(i), ..p.clone() }).collect()
            };
            let mut base = o.clone();
            base.kind = super::ObjKind::Text { text: text.clone(), style: style.clone(), paras: only(&|i| !ranges.iter().any(|r| inside(i, r))) };
            objects.push(base);
            for (k, r) in ranges.iter().enumerate() {
                let id = Uuid::from_u128(o.id.as_u128() ^ (((k as u128) + 1) << 96 | 0x9e37_79b9));
                let piece = super::Obj {
                    id,
                    kind: super::ObjKind::Text { text: text.clone(), style: style.clone(), paras: only(&|i| inside(i, r)) },
                    look: super::Look { fill: None, outline: None, ..o.look.clone() },
                    ..o.clone()
                };
                objects.push(piece);
                for a in d.anims.iter_mut().filter(|a| a.obj == o.id && a.para == Some(*r)) { a.obj = id; a.para = None }
            }
        }
        d.objects = objects;
        d
    }

    /// Old builds (`Obj::build`) as Appear steps, when the slide has no steps of its own; the builds are cleared.
    pub(super) fn upgrade_builds(&mut self) {
        let mut built: Vec<(u16, usize)> = self.objects.iter().enumerate().filter(|(_, o)| o.build > 0).map(|(i, o)| (o.build, i)).collect();
        if self.anims.is_empty() {
            built.sort();
            let mut last = 0;
            for (b, i) in built {
                let start = if b == last { Start::WithPrevious } else { Start::OnClick };
                last = b;
                self.anims.push(Anim::new(self.objects[i].id, AnimKind::Entrance, Effect::Appear, start));
            }
        }
        for o in &mut self.objects { o.build = 0 }
    }

    /// The click (1, 2…) on which `obj` first comes in; 0 when it is there from the start.
    pub fn click_of(&self, obj: Uuid) -> u16 {
        self.groups().iter().position(|g| g.iter().any(|(i, _)| self.anims[*i].obj == obj && self.anims[*i].kind == AnimKind::Entrance)).unwrap_or(0) as u16
    }

    /// Swaps click groups `click` and `click + 1` (1-based), keeping each group's steps together.
    pub fn swap_clicks(&mut self, click: u16) {
        let g = self.groups();
        let (Some(a), Some(b)) = (g.get(usize::from(click)), g.get(usize::from(click) + 1)) else { return };
        if click == 0 || a.is_empty() || b.is_empty() { return }
        let (a0, b0, b1) = (a[0].0, b[0].0, b[b.len() - 1].0 + 1);
        let moved: Vec<Anim> = self.anims[b0..b1].iter().chain(&self.anims[a0..b0]).cloned().collect();
        self.anims.splice(a0..b1, moved);
    }

    /// Drops steps of objects no longer on the slide; numbers in range.
    pub(super) fn sanitise_anims(&mut self) {
        let ids: Vec<Uuid> = self.objects.iter().map(|o| o.id).collect();
        self.anims.retain(|a| ids.contains(&a.obj));
        self.anims.truncate(MAX_ANIMS);
        for a in &mut self.anims {
            a.delay_ms = a.delay_ms.min(ANIM_MAX_MS);
            a.dur_ms = a.dur_ms.min(ANIM_MAX_MS);
            if let Some(v) = a.amount { a.amount = Some(if v.is_finite() { v.clamp(-3600.0, 3600.0) } else { 100.0 }) }
            if let Some(p) = &mut a.path {
                let ok = |v: &(f32, f32)| v.0.is_finite() && v.1.is_finite() && v.0.abs() <= 1000.0 && v.1.abs() <= 1000.0;
                p.segs.retain(|s| match s { Seg::Move { to } | Seg::Line { to } => ok(to), Seg::Cubic { c1, c2, to } => ok(c1) && ok(c2) && ok(to) });
                p.segs.truncate(500);
            }
            if a.kind == AnimKind::Motion && a.path.as_ref().is_none_or(|p| p.segs.is_empty()) { a.path = Some(MotionPath::line((0.0, 0.0))) }
        }
    }
}

impl Anim {
    /// A step with half a second's run, no delay, no side.
    pub fn new(obj: Uuid, kind: AnimKind, effect: Effect, start: Start) -> Anim {
        let dur_ms = if effect == Effect::Appear { 0 } else { 500 };
        Anim { obj, kind, effect, start, delay_ms: 0, dur_ms, dir: None, amount: None, path: None, para: None }
    }
}

#[cfg(test)]
#[path = "anim_tests.rs"]
mod tests;
