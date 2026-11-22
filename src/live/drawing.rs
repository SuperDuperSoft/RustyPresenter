//! Drawing on the live slide from a phone or tablet, like a sports telestrator: circle a word, underline a line.
//! Strokes live beside the frame, not in it: drawing never moves the live version, so a phone's Next or a
//! macro is never made stale by someone drawing. Coordinates are 0–1 of the slide (any screen size matches);
//! widths are in pixels of a 1080-line screen.
//!
//! No limits a person drawing would meet unless Settings → "Limit phone drawing" is on (`actions`). Three ceilings
//! are always on, far above any real drawing, so a stuck touch screen or a script can never fill the computer's
//! memory: `MAX_POINTS_PER_REQUEST`, `MAX_POINTS_PER_STROKE` and `MAX_POINTS`. Malformed input is always refused.
//!
//! Ink either stays or fades, like a laser pointer: a fading stroke stays a while after the finger lifts
//! (Settings → "Fading ink disappears after", `Drawing::fade_after`), fades out over `FADE_OUT`, then is removed.
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub mod actions;

/// For serde: a field that is on unless the file says otherwise.
pub fn yes() -> bool { true }

/// The six pen colours, high-visibility yellow first (the default).
pub const PALETTE: [&str; 6] = ["#ffd60a", "#ff3b30", "#34c759", "#0a84ff", "#ffffff", "#111111"];
/// The phone's three pen sizes and its highlighter, in 1080p pixels.
pub const PEN_SIZES: [f32; 3] = [6.0, 12.0, 22.0];
pub const HIGHLIGHTER_WIDTH: f32 = 44.0;
/// A width outside this is malformed, not a choice.
pub const WIDTH_RANGE: std::ops::RangeInclusive<f32> = 1.0..=100.0;
/// How see-through the highlighter is (0–255).
pub const HIGHLIGHTER_ALPHA: u8 = 110;
/// The most points one command may carry, one line may hold, and the whole slide may hold. A finger gives about a
/// hundred points a second, so these are minutes of drawing without lifting, and hours in all.
pub const MAX_POINTS_PER_REQUEST: usize = 5_000;
pub const MAX_POINTS_PER_STROKE: usize = 20_000;
pub const MAX_POINTS: usize = 200_000;
/// A fading line whose phone sent nothing for this long with the finger still down (it dropped off the Wi-Fi,
/// locked, or closed the page) counts as lifted, so it fades like any other.
pub const ABANDONED_AFTER: Duration = Duration::from_secs(3);
/// How long fading ink takes to go, once its time is up.
pub const FADE_OUT: Duration = Duration::from_millis(500);
/// How often a screen redraws while ink is fading out (about 30 a second).
pub const FADE_FRAME: Duration = Duration::from_millis(33);
/// Settings → "Fading ink disappears after: N seconds": the default and the range.
pub const FADE_SECS_DEFAULT: u32 = 3;
pub const FADE_SECS_RANGE: std::ops::RangeInclusive<u32> = 1..=30;

/// Stays until it is undone or cleared, or fades on its own.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Ink { #[default] Stays, Fades }

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Pen { Pen, Highlighter }

/// One line drawn by one phone. `open` while the finger is still down: only an open stroke gets more points.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    /// Unique for the life of the app, so a renderer can tell strokes apart.
    pub serial: u64,
    pub phone: Uuid,
    /// The phone's own number for it.
    pub id: u32,
    pub color: [u8; 3],
    pub width: f32,
    pub pen: Pen,
    pub points: Vec<[f32; 2]>,
    pub open: bool,
    pub ink: Ink,
    /// When the finger lifted (fading ink counts its time from here).
    pub ended: Option<Instant>,
    /// When its newest points came in.
    pub last_point: Instant,
}

impl Stroke {
    pub fn fades(&self) -> bool { self.ink == Ink::Fades }

    /// How much of a fading stroke is left at `now` (1 = all of it, 0 = gone); staying ink is always 1.
    pub fn opacity(&self, now: Instant, after: Duration) -> f32 {
        let Some(ended) = self.ended.filter(|_| self.fades()) else { return 1.0 };
        let t = now.saturating_duration_since(ended);
        if t <= after { return 1.0 }
        (1.0 - (t - after).as_secs_f32() / FADE_OUT.as_secs_f32()).clamp(0.0, 1.0)
    }

    fn close(&mut self, now: Instant) {
        self.open = false;
        self.ended = Some(now);
    }

    /// Fading ink still open long after its last point: the phone went away mid-line.
    fn abandoned(&self, now: Instant) -> bool { self.open && self.fades() && now.saturating_duration_since(self.last_point) > ABANDONED_AFTER }
}

/// Every stroke on the live slide, oldest first. `rev` moves on with every change; `epoch` only when staying
/// strokes are taken away (undo, clear), so a renderer knows when what it baked is no longer right (fading ink
/// is never baked). `fade_after`: how long fading ink stays (the App keeps it in step with the settings).
#[derive(Clone, Debug, PartialEq)]
pub struct Drawing { pub strokes: Vec<Arc<Stroke>>, pub rev: u64, pub epoch: u64, pub fade_after: Duration, next_serial: u64 }

impl Default for Drawing {
    fn default() -> Self {
        Self { strokes: vec![], rev: 0, epoch: 0, fade_after: Duration::from_secs(FADE_SECS_DEFAULT.into()), next_serial: 0 }
    }
}

/// What a phone sends (`POST /remote/draw`), as `{"op": …}`.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DrawCmd {
    /// A new stroke with its first points.
    Start { stroke: u32, color: String, width: f32, pen: Pen, points: Vec<[f32; 2]>, #[serde(default)] ink: Ink },
    /// More points for the phone's open stroke.
    More { stroke: u32, points: Vec<[f32; 2]> },
    /// The finger lifted.
    End { stroke: u32 },
    /// Take away this phone's last staying stroke (fading ink goes on its own).
    Undo,
    /// Take away this one stroke of the phone's (a pinch that began as a line).
    Cancel { stroke: u32 },
    /// Take everything away.
    Clear,
}

/// `#rrggbb` from the palette (any case), as RGB.
pub fn palette_color(c: &str) -> Option<[u8; 3]> {
    let c = c.trim().to_ascii_lowercase();
    if !PALETTE.contains(&c.as_str()) { return None }
    let n = u32::from_str_radix(&c[1..], 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

fn check_points(points: &[[f32; 2]]) -> Result<(), String> {
    let ok = points.iter().flatten().all(|v| v.is_finite() && (0.0..=1.0).contains(v));
    if ok { Ok(()) } else { Err(crate::tr!("A point is off the slide.")) }
}

/// Adds `points` to `to`, leaving out a point equal to the one before it (it would draw nothing).
fn extend(to: &mut Vec<[f32; 2]>, points: &[[f32; 2]]) {
    for p in points {
        if to.last() != Some(p) { to.push(*p) }
    }
}

impl Drawing {
    pub fn is_empty(&self) -> bool { self.strokes.is_empty() }

    /// Every point of every stroke.
    pub fn points(&self) -> usize { self.strokes.iter().map(|s| s.points.len()).sum() }

    /// Runs one phone command. `Ok(true)` when the drawing changed. A refused command changes nothing.
    pub fn apply(&mut self, phone: Uuid, cmd: &DrawCmd) -> Result<bool, String> { self.apply_at(phone, cmd, Instant::now()) }

    /// `apply`, at `now`.
    pub fn apply_at(&mut self, phone: Uuid, cmd: &DrawCmd, now: Instant) -> Result<bool, String> {
        match cmd {
            DrawCmd::Start { stroke, color, width, pen, points, ink } => {
                let color = palette_color(color).ok_or_else(|| crate::tr!("That colour isn't one of the pen colours."))?;
                if !width.is_finite() || !WIDTH_RANGE.contains(width) { return Err(crate::tr!("That pen size isn't right.")) }
                if points.is_empty() { return Err(crate::tr!("A line needs at least one point.")) }
                check_points(points)?;
                if points.len() > MAX_POINTS_PER_REQUEST || self.points() + points.len() > MAX_POINTS { return Err(actions::too_many_lines()) }
                for s in self.strokes.iter_mut().filter(|s| s.phone == phone && s.open) { Arc::make_mut(s).close(now) }
                let mut pts = Vec::with_capacity(points.len());
                extend(&mut pts, points);
                self.next_serial += 1;
                self.strokes.push(Arc::new(Stroke { serial: self.next_serial, phone, id: *stroke, color, width: *width, pen: *pen, points: pts, open: true, ink: *ink, ended: None, last_point: now }));
            }
            DrawCmd::More { stroke, points } => {
                check_points(points)?;
                let on_slide = self.points();
                let s = self.strokes.iter_mut().rev().find(|s| s.phone == phone && s.id == *stroke && s.open)
                    .ok_or_else(|| crate::tr!("That line was cleared."))?;
                if points.is_empty() { return Ok(false) }
                if points.len() > MAX_POINTS_PER_REQUEST || s.points.len() + points.len() > MAX_POINTS_PER_STROKE || on_slide + points.len() > MAX_POINTS {
                    return Err(actions::too_many_lines());
                }
                let s = Arc::make_mut(s);
                extend(&mut s.points, points);
                s.last_point = now;
            }
            DrawCmd::End { stroke } => {
                let Some(s) = self.strokes.iter_mut().find(|s| s.phone == phone && s.id == *stroke && s.open) else { return Ok(false) };
                Arc::make_mut(s).close(now);
            }
            DrawCmd::Undo => {
                let Some(i) = self.strokes.iter().rposition(|s| s.phone == phone && !s.fades()) else { return Ok(false) };
                self.strokes.remove(i);
                self.epoch += 1;
            }
            DrawCmd::Cancel { stroke } => {
                let Some(i) = self.strokes.iter().rposition(|s| s.phone == phone && s.id == *stroke) else { return Ok(false) };
                if !self.strokes.remove(i).fades() { self.epoch += 1 }
            }
            DrawCmd::Clear => return Ok(self.clear()),
        }
        self.rev += 1;
        Ok(true)
    }

    /// Takes every stroke away. False when there were none.
    pub fn clear(&mut self) -> bool {
        if self.strokes.is_empty() { return false }
        self.strokes.clear();
        self.epoch += 1;
        self.rev += 1;
        true
    }

    /// Removes fading ink whose time is up, and lifts the finger of a fading line whose phone went away mid-line.
    /// Its time counts from its last point, but never so far back that it would vanish at once: it still fades
    /// out like any other. True when anything changed. Staying strokes are never touched, so what a renderer
    /// baked stays right (no `epoch` move).
    pub fn expire(&mut self, now: Instant) -> bool {
        let after = self.fade_after;
        let mut changed = false;
        for s in self.strokes.iter_mut().filter(|s| s.abandoned(now)) {
            let s = Arc::make_mut(s);
            s.close(now.checked_sub(after).map_or(s.last_point, |earliest| earliest.max(s.last_point)));
            changed = true;
        }
        let before = self.strokes.len();
        self.strokes.retain(|s| s.opacity(now, after) > 0.0);
        if self.strokes.len() == before && !changed { return false }
        self.rev += 1;
        true
    }

    /// `expire` would change something at `now`.
    pub fn needs_expire(&self, now: Instant) -> bool {
        self.strokes.iter().any(|s| s.abandoned(now) || s.opacity(now, self.fade_after) <= 0.0)
    }

    /// When a screen should draw again by itself because ink is fading: soon while some is fading out, else when
    /// the next fading stroke's time is up. `None` when nothing fades.
    pub fn next_change(&self, now: Instant) -> Option<Duration> {
        self.strokes.iter().filter_map(|s| {
            let ended = s.ended.filter(|_| s.fades())?;
            let t = now.saturating_duration_since(ended);
            if t >= self.fade_after + FADE_OUT { return None } // gone from the screen already
            Some(if t < self.fade_after { self.fade_after - t } else { FADE_FRAME })
        }).min()
    }

    /// Is any fading ink on the slide (finished or not)?
    pub fn has_fading(&self) -> bool { self.strokes.iter().any(|s| s.fades()) }

    /// The numbers of `phone`'s strokes, oldest first (the phone keeps only these on its own screen).
    pub fn ids_of(&self, phone: Uuid) -> Vec<u32> { self.strokes.iter().filter(|s| s.phone == phone).map(|s| s.id).collect() }
}

/// Clears `d` in place (copying only when a screen still holds the old one). False when it was empty.
pub fn clear_shared(d: &mut Arc<Drawing>) -> bool {
    if d.is_empty() { return false }
    Arc::make_mut(d).clear()
}

/// Where a 0–1 point lands on a `w`×`h` screen.
pub fn to_screen([x, y]: [f32; 2], (w, h): (u32, u32)) -> (f32, f32) { (x * w as f32, y * h as f32) }

/// A 1080p width on a screen `h` pixels tall.
pub fn scaled_width(width: f32, h: u32) -> f32 { width * h as f32 / 1080.0 }

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cmd(v: serde_json::Value) -> Result<DrawCmd, serde_json::Error> { serde_json::from_value(v) }
    fn start(id: u32, points: Vec<[f32; 2]>) -> DrawCmd { DrawCmd::Start { stroke: id, color: "#ffd60a".into(), width: 12.0, pen: Pen::Pen, points, ink: Ink::Stays } }

    #[test]
    fn a_stroke_grows_while_open_and_stops_when_ended() {
        let (mut d, me) = (Drawing::default(), Uuid::new_v4());
        assert_eq!(d.apply(me, &start(1, vec![[0.1, 0.1]])), Ok(true));
        assert_eq!(d.apply(me, &DrawCmd::More { stroke: 1, points: vec![[0.2, 0.2], [0.2, 0.2], [0.3, 0.25]] }), Ok(true));
        assert_eq!(d.strokes[0].points, vec![[0.1, 0.1], [0.2, 0.2], [0.3, 0.25]], "a repeated point is left out");
        assert_eq!(d.strokes[0].color, [0xff, 0xd6, 0x0a]);
        assert_eq!(d.apply(me, &DrawCmd::End { stroke: 1 }), Ok(true));
        assert!(d.apply(me, &DrawCmd::More { stroke: 1, points: vec![[0.5, 0.5]] }).is_err(), "an ended stroke takes no more");
        assert_eq!(d.rev, 3);
    }

    #[test]
    fn malformed_commands_are_refused_and_change_nothing() {
        let (mut d, me) = (Drawing::default(), Uuid::new_v4());
        let bad = [
            DrawCmd::Start { stroke: 1, color: "#123456".into(), width: 12.0, pen: Pen::Pen, points: vec![[0.5, 0.5]], ink: Ink::Stays },
            DrawCmd::Start { stroke: 1, color: "#FFD60A".into(), width: 0.0, pen: Pen::Pen, points: vec![[0.5, 0.5]], ink: Ink::Stays },
            DrawCmd::Start { stroke: 1, color: "#ffd60a".into(), width: 500.0, pen: Pen::Pen, points: vec![[0.5, 0.5]], ink: Ink::Stays },
            DrawCmd::Start { stroke: 1, color: "#ffd60a".into(), width: f32::NAN, pen: Pen::Pen, points: vec![[0.5, 0.5]], ink: Ink::Stays },
            DrawCmd::Start { stroke: 1, color: "#ffd60a".into(), width: 12.0, pen: Pen::Pen, points: vec![], ink: Ink::Stays },
            start(1, vec![[1.01, 0.5]]),
            start(1, vec![[0.5, -0.001]]),
            start(1, vec![[f32::NAN, 0.5]]),
            start(1, vec![[0.5, f32::INFINITY]]),
            DrawCmd::More { stroke: 9, points: vec![[0.5, 0.5]] },
        ];
        for c in &bad { assert!(d.apply(me, c).is_err(), "{c:?}") }
        assert_eq!(d, Drawing::default());
        // Upper-case palette colours and the edges of the slide are fine.
        assert!(d.apply(me, &DrawCmd::Start { stroke: 1, color: "#FFD60A".into(), width: 12.0, pen: Pen::Pen, points: vec![[0.0, 1.0]], ink: Ink::Stays }).is_ok());
        let fresh = d.clone();
        assert!(d.apply(me, &DrawCmd::More { stroke: 1, points: vec![[0.4, 0.4], [2.0, 0.4]] }).is_err());
        assert_eq!(d, fresh, "one bad point refuses the whole batch");
    }

    #[test]
    fn bad_json_unknown_kinds_and_colours_are_refused() {
        assert!(cmd(json!({ "op": "start", "stroke": 1, "color": "#ffd60a", "width": 12, "pen": "crayon", "points": [[0.1, 0.1]] })).is_err());
        assert!(cmd(json!({ "op": "explode" })).is_err());
        assert!(cmd(json!({ "op": "more", "stroke": 1, "points": [[0.1]] })).is_err(), "a point has two numbers");
        assert!(cmd(json!({ "op": "more", "stroke": 1, "points": [["a", 0.1]] })).is_err());
        assert!(cmd(json!({ "op": "more", "stroke": -1, "points": [] })).is_err());
        let ok = cmd(json!({ "op": "start", "stroke": 7, "color": "#0a84ff", "width": 44, "pen": "highlighter", "points": [[0.25, 0.5]] })).unwrap();
        assert_eq!(ok, DrawCmd::Start { stroke: 7, color: "#0a84ff".into(), width: 44.0, pen: Pen::Highlighter, points: vec![[0.25, 0.5]], ink: Ink::Stays });
        assert_eq!(cmd(json!({ "op": "undo" })).unwrap(), DrawCmd::Undo);
        assert_eq!(palette_color("#ff0000"), None);
        assert!(PALETTE.iter().all(|c| palette_color(c).is_some()));
    }

    #[test]
    fn undo_takes_only_this_phones_last_stroke() {
        let (mut d, me, you) = (Drawing::default(), Uuid::new_v4(), Uuid::new_v4());
        d.apply(me, &start(1, vec![[0.1, 0.1]])).unwrap();
        d.apply(you, &start(1, vec![[0.2, 0.2]])).unwrap();
        d.apply(me, &start(2, vec![[0.3, 0.3]])).unwrap();
        d.apply(you, &start(2, vec![[0.4, 0.4]])).unwrap();
        let epoch = d.epoch;
        assert_eq!(d.apply(me, &DrawCmd::Undo), Ok(true));
        assert_eq!((d.ids_of(me), d.ids_of(you)), (vec![1], vec![1, 2]));
        assert!(d.epoch > epoch, "a renderer must draw again from scratch");
        d.apply(me, &DrawCmd::Undo).unwrap();
        assert_eq!(d.apply(me, &DrawCmd::Undo), Ok(false), "nothing left of mine");
        assert_eq!(d.ids_of(you), vec![1, 2]);
        assert_eq!(d.apply(you, &DrawCmd::Clear), Ok(true));
        assert!(d.is_empty());
        assert_eq!(d.apply(you, &DrawCmd::Clear), Ok(false));
    }

    #[test]
    fn starting_a_stroke_ends_the_phones_last_one_only() {
        let (mut d, me, you) = (Drawing::default(), Uuid::new_v4(), Uuid::new_v4());
        d.apply(me, &start(1, vec![[0.1, 0.1]])).unwrap();
        d.apply(you, &start(1, vec![[0.2, 0.2]])).unwrap();
        d.apply(me, &start(2, vec![[0.3, 0.3]])).unwrap();
        assert_eq!(d.strokes.iter().map(|s| s.open).collect::<Vec<_>>(), [false, true, true]);
    }

    /// Many distinct points along a line.
    fn line(n: usize, y: f32) -> Vec<[f32; 2]> { (0..n).map(|i| [i as f32 / n as f32, y]).collect() }

    #[test]
    fn a_long_drawing_is_kept_as_sent() {
        let (mut d, me) = (Drawing::default(), Uuid::new_v4());
        for id in 0..150u32 { d.apply(me, &start(id, line(1000, id as f32 / 150.0))).unwrap(); }
        assert_eq!(d.points(), 150_000, "every point kept as sent");
    }

    /// B9: ceilings no real drawing reaches, always on.
    #[test]
    fn the_hard_ceilings_refuse_and_change_nothing() {
        let (mut d, me) = (Drawing::default(), Uuid::new_v4());
        // One command with too many points.
        assert_eq!(d.apply(me, &start(1, line(MAX_POINTS_PER_REQUEST + 1, 0.5))), Err(actions::too_many_lines()));
        assert!(d.is_empty() && d.rev == 0);
        // One line that would grow past its ceiling.
        d.apply(me, &start(1, line(MAX_POINTS_PER_REQUEST, 0.1))).unwrap();
        for part in 1..MAX_POINTS_PER_STROKE / MAX_POINTS_PER_REQUEST {
            d.apply(me, &DrawCmd::More { stroke: 1, points: line(MAX_POINTS_PER_REQUEST, 0.1 + part as f32 / 100.0) }).unwrap();
        }
        assert_eq!(d.strokes[0].points.len(), MAX_POINTS_PER_STROKE);
        let (rev, before) = (d.rev, d.strokes[0].points.len());
        assert_eq!(d.apply(me, &DrawCmd::More { stroke: 1, points: vec![[0.9, 0.9]] }), Err(actions::too_many_lines()));
        assert_eq!((d.rev, d.strokes[0].points.len()), (rev, before), "a refused command changes nothing");
        // The whole slide.
        let mut id = 2;
        while d.points() + MAX_POINTS_PER_REQUEST <= MAX_POINTS { d.apply(me, &start(id, line(MAX_POINTS_PER_REQUEST, 0.7))).unwrap(); id += 1; }
        assert_eq!(d.points(), MAX_POINTS);
        assert_eq!(d.apply(me, &start(id, vec![[0.5, 0.5]])), Err(actions::too_many_lines()));
        assert_eq!(d.apply(me, &DrawCmd::More { stroke: id - 1, points: vec![[0.6, 0.6]] }), Err(actions::too_many_lines()));
        // Taking lines away is never refused, and makes room again.
        assert_eq!(d.apply(me, &DrawCmd::Undo), Ok(true));
        assert_eq!(d.apply(me, &start(id, vec![[0.5, 0.5]])), Ok(true));
    }

    #[test]
    fn normalised_points_map_to_any_screen() {
        assert_eq!(to_screen([0.5, 0.5], (1920, 1080)), (960.0, 540.0));
        assert_eq!(to_screen([0.25, 1.0], (1280, 720)), (320.0, 720.0));
        assert_eq!(to_screen([1.0, 0.0], (3840, 2160)), (3840.0, 0.0));
        assert_eq!(scaled_width(12.0, 1080), 12.0);
        assert_eq!(scaled_width(12.0, 720), 8.0);
        assert_eq!(scaled_width(12.0, 2160), 24.0);
    }

    fn fading(id: u32) -> DrawCmd { DrawCmd::Start { stroke: id, color: "#ff3b30".into(), width: 12.0, pen: Pen::Pen, points: vec![[0.5, 0.5]], ink: Ink::Fades } }

    #[test]
    fn fading_ink_stays_its_time_fades_smoothly_then_goes() {
        let (mut d, me) = (Drawing::default(), Uuid::new_v4());
        assert_eq!(d.fade_after, Duration::from_secs(3), "3 s by default");
        let t0 = Instant::now();
        d.apply_at(me, &start(1, vec![[0.1, 0.1]]), t0).unwrap();
        d.apply_at(me, &DrawCmd::End { stroke: 1 }, t0).unwrap();
        d.apply_at(me, &fading(2), t0).unwrap();
        let at = |d: &Drawing, ms: u64| d.strokes.iter().map(|s| s.opacity(t0 + Duration::from_millis(ms), d.fade_after)).collect::<Vec<_>>();
        assert_eq!(at(&d, 60_000), [1.0, 1.0], "the clock starts when the finger lifts");
        assert_eq!(d.next_change(t0 + Duration::from_secs(60)), None);
        d.apply_at(me, &DrawCmd::End { stroke: 2 }, t0).unwrap();
        assert_eq!(at(&d, 2_900), [1.0, 1.0]);
        assert_eq!(d.next_change(t0 + Duration::from_millis(2_900)), Some(Duration::from_millis(100)), "wake when it starts to fade");
        let half = at(&d, 3_250)[1];
        assert!((half - 0.5).abs() < 0.01, "half way through a 0.5 s fade: {half}");
        assert_eq!(d.next_change(t0 + Duration::from_millis(3_250)), Some(FADE_FRAME), "smooth: about 30 frames a second");
        let (rev, epoch) = (d.rev, d.epoch);
        assert!(!d.expire(t0 + Duration::from_millis(3_400)), "not gone yet");
        assert!(d.expire(t0 + Duration::from_millis(3_500)));
        assert_eq!(d.strokes.len(), 1, "the staying stroke is untouched");
        assert!(!d.strokes[0].fades());
        assert!(d.rev > rev && d.epoch == epoch, "what a screen baked is still right");
        assert!(!d.has_fading());
    }

    /// B12: the phone dropped off with the finger down, so `End` never came.
    #[test]
    fn fading_ink_whose_phone_went_away_mid_line_still_fades() {
        let (mut d, me) = (Drawing::default(), Uuid::new_v4());
        let t0 = Instant::now();
        d.apply_at(me, &fading(1), t0).unwrap();
        d.apply_at(me, &DrawCmd::More { stroke: 1, points: vec![[0.6, 0.6]] }, t0 + Duration::from_secs(1)).unwrap();
        let last = t0 + Duration::from_secs(1);
        assert!(!d.needs_expire(last + Duration::from_secs(2)) && !d.expire(last + Duration::from_secs(2)), "still drawing, as far as anyone knows");
        assert!(d.strokes[0].open);
        // Four seconds with nothing new: the finger counts as lifted, and the line starts to fade out now (its
        // three seconds are already up) instead of vanishing at once.
        let noticed = last + Duration::from_secs(4);
        assert!(d.needs_expire(noticed));
        assert!(d.expire(noticed));
        assert!(!d.strokes[0].open && d.strokes[0].ended == Some(noticed - d.fade_after));
        assert_eq!(d.strokes[0].opacity(noticed, d.fade_after), 1.0);
        assert!(d.apply_at(me, &DrawCmd::More { stroke: 1, points: vec![[0.7, 0.7]] }, noticed).is_err(), "a closed line takes no more");
        assert!(d.strokes[0].opacity(noticed + FADE_OUT / 2, d.fade_after) < 1.0, "fading out");
        // Then it goes like any other.
        assert!(d.expire(noticed + FADE_OUT));
        assert!(d.is_empty());
        // With a long fade time, the time still counts from its last point.
        d.fade_after = Duration::from_secs(10);
        d.apply_at(me, &fading(5), t0).unwrap();
        assert!(d.expire(t0 + Duration::from_secs(4)));
        assert_eq!(d.strokes[0].ended, Some(t0));
        assert!(d.expire(t0 + Duration::from_secs(10) + FADE_OUT));
        d.fade_after = Duration::from_secs(3);
        // Staying ink is never closed this way: a finger may rest as long as it likes.
        d.apply_at(me, &start(2, vec![[0.1, 0.1]]), t0).unwrap();
        assert!(!d.expire(t0 + Duration::from_secs(600)));
        assert!(d.strokes[0].open);
    }

    #[test]
    fn the_fade_time_is_whatever_the_setting_says() {
        let (mut d, me) = (Drawing::default(), Uuid::new_v4());
        d.fade_after = Duration::from_secs(10);
        let t0 = Instant::now();
        d.apply_at(me, &fading(1), t0).unwrap();
        d.apply_at(me, &DrawCmd::End { stroke: 1 }, t0).unwrap();
        assert!(!d.expire(t0 + Duration::from_secs(5)), "3 s would have gone by now");
        assert!(d.expire(t0 + Duration::from_millis(10_500)));
    }

    #[test]
    fn undo_skips_fading_ink_and_a_phone_can_mix_both() {
        let (mut d, me) = (Drawing::default(), Uuid::new_v4());
        d.apply(me, &start(1, vec![[0.1, 0.1]])).unwrap();
        d.apply(me, &fading(2)).unwrap();
        d.apply(me, &start(3, vec![[0.3, 0.3]])).unwrap();
        d.apply(me, &fading(4)).unwrap();
        assert_eq!(d.ids_of(me), [1, 2, 3, 4]);
        assert!(!d.strokes[1].open && d.strokes[1].ended.is_some(), "starting a new line ends the last one, fading or not");
        d.apply(me, &DrawCmd::Undo).unwrap();
        assert_eq!(d.ids_of(me), [1, 2, 4], "the last staying line, not the fading ones");
        d.apply(me, &DrawCmd::Undo).unwrap();
        assert_eq!(d.apply(me, &DrawCmd::Undo), Ok(false), "only fading ink left: nothing to undo");
        assert_eq!(d.ids_of(me), [2, 4]);
        // A pinch takes back exactly the line it began.
        assert_eq!(d.apply(me, &DrawCmd::Cancel { stroke: 2 }), Ok(true));
        assert_eq!(d.ids_of(me), [4]);
        assert_eq!(d.apply(me, &DrawCmd::Cancel { stroke: 99 }), Ok(false));
        let ink: DrawCmd = serde_json::from_value(json!({ "op": "start", "stroke": 1, "color": "#ffd60a", "width": 12, "pen": "pen", "points": [[0.1, 0.1]], "ink": "fades" })).unwrap();
        assert!(matches!(ink, DrawCmd::Start { ink: Ink::Fades, .. }));
        assert!(serde_json::from_value::<DrawCmd>(json!({ "op": "start", "stroke": 1, "color": "#ffd60a", "width": 12, "pen": "pen", "points": [[0.1, 0.1]], "ink": "sparkles" })).is_err());
    }
}
