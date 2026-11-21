//! Drawing on the live slide from the phone, like a sports telestrator: circle a word, underline a line. The same
//! job as the browser remote's (`web/draw.js` in the desktop app), and the same messages to the computer
//! (`POST /remote/draw`): points go as 0–1 of the slide in small batches while the finger moves, so the audience
//! screen shows the line as it is drawn. The computer checks everything again.
//!
//! Ink either stays, or fades like a laser pointer: a few seconds after the finger lifts it fades out and is gone.
//! Nothing here needs a phone: `Pad` keeps the lines and says what to send, `paint` draws them into a picture
//! that lies over the slide.
use serde_json::{Value, json};

/// The same six colours, sizes and highlighter as the computer (`src/live/drawing.rs` there).
pub const PALETTE: [[u8; 3]; 6] = [[0xff, 0xd6, 0x0a], [0xff, 0x3b, 0x30], [0x34, 0xc7, 0x59], [0x0a, 0x84, 0xff], [0xff, 0xff, 0xff], [0x11, 0x11, 0x11]];
/// Pen widths, in pixels of a 1080-line screen. The choice after the last of them is the highlighter.
pub const SIZES: [f32; 3] = [6.0, 12.0, 22.0];
pub const HIGHLIGHTER: f32 = 44.0;
const HIGHLIGHTER_ALPHA: f32 = 110.0 / 255.0;
/// How long fading ink takes to go once its time is up (seconds).
const FADE_OUT: f64 = 0.5;

/// One line on this phone's screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub id: u32,
    pub color: [u8; 3],
    pub width: f32,
    pub highlighter: bool,
    pub fades: bool,
    pub points: Vec<[f32; 2]>,
    /// How many of its points have gone to the computer.
    sent: usize,
    /// The computer has taken its start.
    acked: bool,
    /// When the finger lifted.
    ended: Option<f64>,
}

impl Stroke {
    /// How much of it is left at `now` (1 all, 0 gone): fading ink goes `fade_secs` after the finger lifts.
    pub fn opacity(&self, now: f64, fade_secs: f64) -> f32 {
        let Some(ended) = self.ended.filter(|_| self.fades) else { return 1.0 };
        let over = now - ended - fade_secs;
        if over <= 0.0 { 1.0 } else { (1.0 - over / FADE_OUT).clamp(0.0, 1.0) as f32 }
    }
}

/// The lines drawn on this phone and the tool in hand.
#[derive(Clone, Debug)]
pub struct Pad {
    pub strokes: Vec<Stroke>,
    /// The id of the line under the finger.
    cur: Option<u32>,
    next_id: u32,
    /// The colour (0–5), the size (0–2 a pen, 3 the highlighter), and whether the ink fades.
    pub color: usize,
    pub size: usize,
    pub fades: bool,
}

fn hex(c: [u8; 3]) -> String { format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]) }

fn round4(v: f32) -> f64 { (v as f64 * 10_000.0).round() / 10_000.0 }

impl Pad {
    /// `first_id`: stroke numbers start here (fresh each time the app starts, so they never meet old ones).
    pub fn new(first_id: u32) -> Pad { Pad { strokes: vec![], cur: None, next_id: first_id, color: 0, size: 1, fades: false } }

    pub fn drawing(&self) -> bool { self.cur.is_some() }

    fn open(&mut self) -> Option<&mut Stroke> { let id = self.cur?; self.strokes.iter_mut().find(|s| s.id == id) }

    /// The finger came down at `p` (0–1 of the slide).
    pub fn down(&mut self, p: [f32; 2]) {
        let highlighter = self.size >= SIZES.len();
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.strokes.push(Stroke { id, color: PALETTE[self.color.min(PALETTE.len() - 1)], width: if highlighter { HIGHLIGHTER } else { SIZES[self.size] },
            highlighter, fades: self.fades, points: vec![p], sent: 0, acked: false, ended: None });
        self.cur = Some(id);
    }

    /// The finger moved to `p`; points closer than `step` (0–1 of the slide, each way) to the last add nothing.
    /// True when the line grew.
    pub fn moved(&mut self, p: [f32; 2], step: [f32; 2]) -> bool {
        let Some(s) = self.open() else { return false };
        let last = s.points[s.points.len() - 1];
        if (p[0] - last[0]).abs() < step[0] && (p[1] - last[1]).abs() < step[1] { return false }
        s.points.push(p);
        true
    }

    /// The points not sent yet, as one message: the line's start, or more of it.
    pub fn flush(&mut self) -> Option<Value> {
        let s = self.open()?;
        if s.points.len() <= s.sent { return None }
        let points: Vec<[f64; 2]> = s.points[s.sent..].iter().map(|p| [round4(p[0]), round4(p[1])]).collect();
        let first = s.sent == 0;
        s.sent = s.points.len();
        Some(if first {
            json!({ "op": "start", "stroke": s.id, "color": hex(s.color), "width": s.width, "pen": if s.highlighter { "highlighter" } else { "pen" },
                "ink": if s.fades { "fades" } else { "stays" }, "points": points })
        } else {
            json!({ "op": "more", "stroke": s.id, "points": points })
        })
    }

    /// The finger lifted at `now`: what is left of the line, then that it is done.
    pub fn up(&mut self, now: f64) -> Vec<Value> {
        let mut out: Vec<Value> = self.flush().into_iter().collect();
        if let Some(s) = self.open() {
            if s.sent > 0 { out.push(json!({ "op": "end", "stroke": s.id })) }
            s.ended = Some(now);
        }
        self.cur = None;
        out
    }

    /// Undo takes back the last line that stays (fading ink goes by itself).
    pub fn undo(&mut self, now: f64) -> Vec<Value> {
        let mut out = self.up(now);
        if let Some(i) = self.strokes.iter().rposition(|s| !s.fades) {
            let mine = self.strokes.remove(i);
            if mine.sent > 0 { out.push(json!({ "op": "undo" })) }
        }
        out
    }

    pub fn clear(&mut self, now: f64) -> Vec<Value> {
        let mut out = self.up(now);
        self.strokes.clear();
        out.push(json!({ "op": "clear" }));
        out
    }

    /// The computer took this line's start.
    pub fn taken(&mut self, id: u32) { if let Some(s) = self.strokes.iter_mut().find(|s| s.id == id) { s.acked = true } }

    /// The computer refused this line's start (not allowed to draw, too many lines): it goes.
    pub fn refused(&mut self, id: u32) {
        self.strokes.retain(|s| s.id != id);
        if self.cur == Some(id) { self.cur = None }
    }

    /// The computer says which of this phone's lines are still up (after an undo elsewhere, Clear, a new slide):
    /// the others go. A line it has not heard of yet stays. True when any went.
    pub fn keep_only(&mut self, ids: &[u32]) -> bool {
        let before = self.strokes.len();
        self.strokes.retain(|s| !s.acked || ids.contains(&s.id));
        if self.cur.is_some_and(|id| !self.strokes.iter().any(|s| s.id == id)) { self.cur = None }
        self.strokes.len() != before
    }

    /// Lets go of ink that has faded away. True while some is still fading (the picture must be drawn again).
    pub fn fade(&mut self, now: f64, fade_secs: f64) -> bool {
        self.strokes.retain(|s| s.opacity(now, fade_secs) > 0.0);
        self.strokes.iter().any(|s| s.fades && s.ended.is_some())
    }
}

/// The slide's box in a board `w` × `h`: as big as fits at the audience screen's shape (`aspect`: width / height).
pub fn fit(w: f32, h: f32, aspect: f32) -> (f32, f32) {
    let a = if aspect.is_finite() && aspect > 0.0 { aspect } else { 16.0 / 9.0 };
    let bw = w.min(h * a).max(1.0);
    (bw.floor(), (bw / a).floor().max(1.0))
}

/// A touch at `x`, `y` (0–1 of the slide, may be outside it) kept on the slide; `None` when it is further out than
/// `edge` (a stray touch, not the start of a line).
pub fn on_slide(x: f32, y: f32, edge: [f32; 2]) -> Option<[f32; 2]> {
    if !x.is_finite() || !y.is_finite() || x < -edge[0] || x > 1.0 + edge[0] || y < -edge[1] || y > 1.0 + edge[1] { return None }
    Some([x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)])
}

fn distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx * dx + dy * dy;
    let t = if len <= f32::EPSILON { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len).clamp(0.0, 1.0) };
    let (cx, cy) = (a[0] + t * dx - p[0], a[1] + t * dy - p[1]);
    (cx * cx + cy * cy).sqrt()
}

/// The lines as a picture `w` × `h` to lay over the slide (RGBA, colour already multiplied by its see-through).
/// A line never darkens itself where it crosses its own path; the highlighter lets the slide show through.
pub fn paint(strokes: &[Stroke], w: u32, h: u32, now: f64, fade_secs: f64) -> Vec<u8> {
    let (wu, hu) = (w as usize, h as usize);
    let mut out = vec![0u8; wu * hu * 4];
    if wu == 0 || hu == 0 { return out }
    let mut cover = vec![0f32; wu * hu];
    for s in strokes {
        let alpha = s.opacity(now, fade_secs) * if s.highlighter { HIGHLIGHTER_ALPHA } else { 1.0 };
        if alpha <= 0.0 || s.points.is_empty() { continue }
        let r = (s.width * h as f32 / 1080.0 / 2.0).max(0.75);
        let px = |p: [f32; 2]| [p[0] * w as f32, p[1] * h as f32];
        // The box the line touches, so only that part is gone over.
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
        let mut seg = |a: [f32; 2], b: [f32; 2], cover: &mut [f32]| {
            let lo = |v: f32| (v - r - 1.0).floor().max(0.0) as usize;
            let (sx0, sy0) = (lo(a[0].min(b[0])), lo(a[1].min(b[1])));
            let (sx1, sy1) = (((a[0].max(b[0]) + r + 1.0).ceil() as usize).min(wu), ((a[1].max(b[1]) + r + 1.0).ceil() as usize).min(hu));
            for y in sy0..sy1 {
                for x in sx0..sx1 {
                    let c = (r + 0.5 - distance([x as f32 + 0.5, y as f32 + 0.5], a, b)).clamp(0.0, 1.0);
                    let at = &mut cover[y * wu + x];
                    if c > *at { *at = c }
                }
            }
            if sx0 < sx1 && sy0 < sy1 { x0 = x0.min(sx0); y0 = y0.min(sy0); x1 = x1.max(sx1); y1 = y1.max(sy1) }
        };
        let first = px(s.points[0]);
        if s.points.len() == 1 { seg(first, first, &mut cover) }
        for pair in s.points.windows(2) { seg(px(pair[0]), px(pair[1]), &mut cover) }
        if x0 >= x1 || y0 >= y1 { continue }
        for y in y0..y1 {
            for x in x0..x1 {
                let c = std::mem::take(&mut cover[y * wu + x]);
                if c <= 0.0 { continue }
                let a = c * alpha;
                let o = &mut out[(y * wu + x) * 4..(y * wu + x) * 4 + 4];
                for i in 0..3 { o[i] = (s.color[i] as f32 * a + o[i] as f32 * (1.0 - a)).round() as u8 }
                o[3] = (255.0 * a + o[3] as f32 * (1.0 - a)).round() as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_goes_to_the_computer_as_it_is_drawn() {
        let mut p = Pad::new(7);
        assert!(p.flush().is_none() && !p.drawing());
        p.color = 1;
        p.size = 0;
        p.down([0.1, 0.2]);
        assert!(p.drawing());
        assert!(!p.moved([0.1001, 0.2001], [0.01, 0.01]), "too close to add anything");
        assert!(p.moved([0.3, 0.2], [0.01, 0.01]));
        let start = p.flush().unwrap();
        assert_eq!(start, json!({ "op": "start", "stroke": 7, "color": "#ff3b30", "width": 6.0, "pen": "pen", "ink": "stays", "points": [[0.1, 0.2], [0.3, 0.2]] }));
        assert!(p.flush().is_none(), "nothing new");
        p.moved([0.5, 0.25], [0.01, 0.01]);
        assert_eq!(p.flush().unwrap(), json!({ "op": "more", "stroke": 7, "points": [[0.5, 0.25]] }));
        p.moved([0.7, 0.3], [0.01, 0.01]);
        let end = p.up(10.0);
        assert_eq!(end, [json!({ "op": "more", "stroke": 7, "points": [[0.7, 0.3]] }), json!({ "op": "end", "stroke": 7 })]);
        assert!(!p.drawing() && p.strokes.len() == 1);
        // The next line has the next number; the highlighter fades.
        p.size = 3;
        p.fades = true;
        p.down([0.5, 0.5]);
        let both = p.up(11.0);
        assert_eq!(both[0]["stroke"], 8);
        assert_eq!((both[0]["pen"].as_str(), both[0]["ink"].as_str(), both[0]["width"].as_f64()), (Some("highlighter"), Some("fades"), Some(44.0)));
        assert_eq!(both[1], json!({ "op": "end", "stroke": 8 }));
    }

    #[test]
    fn undo_clear_and_what_the_computer_took_away() {
        let mut p = Pad::new(1);
        for (i, fades) in [false, true, false].into_iter().enumerate() {
            p.fades = fades;
            p.down([0.1 * i as f32, 0.1]);
            p.up(i as f64);
        }
        for id in 1..=3 { p.taken(id) }
        // Undo takes the last line that stays, not the fading one.
        assert_eq!(p.undo(5.0), [json!({ "op": "undo" })]);
        assert_eq!(p.strokes.iter().map(|s| s.id).collect::<Vec<_>>(), [1, 2]);
        // A new slide on the computer: only line 2 is still up there. A line it has not heard of yet stays here.
        p.fades = false;
        p.down([0.9, 0.9]);
        assert!(p.keep_only(&[2]));
        assert_eq!(p.strokes.iter().map(|s| s.id).collect::<Vec<_>>(), [2, 4]);
        assert!(!p.keep_only(&[2]));
        // Refused by the computer: gone, and the finger draws nothing more.
        p.refused(4);
        assert!(!p.drawing() && !p.moved([0.5, 0.5], [0.0, 0.0]));
        assert_eq!(p.clear(6.0), [json!({ "op": "clear" })]);
        assert!(p.strokes.is_empty());
        // Undo with nothing that stays sends nothing.
        assert!(p.undo(7.0).is_empty());
    }

    #[test]
    fn fading_ink_goes_by_itself() {
        let mut p = Pad::new(1);
        p.fades = true;
        p.down([0.5, 0.5]);
        assert!(!p.fade(100.0, 3.0), "under the finger it does not fade");
        p.up(10.0);
        assert_eq!(p.strokes[0].opacity(12.9, 3.0), 1.0);
        assert!((p.strokes[0].opacity(13.25, 3.0) - 0.5).abs() < 0.01);
        assert!(p.fade(13.25, 3.0) && p.strokes.len() == 1);
        assert!(!p.fade(13.6, 3.0) && p.strokes.is_empty());
    }

    #[test]
    fn the_slide_fits_the_board_and_touches_are_kept_on_it() {
        assert_eq!(fit(400.0, 800.0, 16.0 / 9.0), (400.0, 225.0));
        assert_eq!(fit(800.0, 300.0, 16.0 / 9.0), (533.0, 300.0));
        assert_eq!(fit(400.0, 800.0, 4.0 / 3.0), (400.0, 300.0));
        assert_eq!(fit(400.0, 800.0, f32::NAN), (400.0, 225.0));
        assert_eq!(on_slide(0.5, 0.5, [0.05, 0.05]), Some([0.5, 0.5]));
        assert_eq!(on_slide(-0.03, 1.04, [0.05, 0.05]), Some([0.0, 1.0]), "just off the edge starts on it");
        assert_eq!(on_slide(-0.2, 0.5, [0.05, 0.05]), None);
        assert_eq!(on_slide(f32::NAN, 0.5, [0.05, 0.05]), None);
    }

    #[test]
    fn the_picture_has_the_lines_and_nothing_else() {
        let mut p = Pad::new(1);
        p.color = 1;
        p.size = 2;
        p.down([0.25, 0.5]);
        p.moved([0.75, 0.5], [0.0, 0.0]);
        p.up(0.0);
        let (w, h) = (200u32, 100u32);
        let px = paint(&p.strokes, w, h, 1.0, 3.0);
        let at = |x: u32, y: u32| { let i = ((y * w + x) * 4) as usize; [px[i], px[i + 1], px[i + 2], px[i + 3]] };
        assert_eq!(at(100, 50), [0xff, 0x3b, 0x30, 255], "on the line: its colour, solid");
        assert_eq!(at(100, 10), [0, 0, 0, 0], "off the line: nothing, the slide shows");
        assert_eq!(at(10, 50), [0, 0, 0, 0]);
        // A 22 px pen on a 1080-line screen is about 2 px on 100 lines.
        assert_eq!((at(100, 49)[3], at(100, 50)[3]), (255, 255));
        assert_eq!((at(100, 46)[3], at(100, 53)[3]), (0, 0));
        // The highlighter lets the slide show through, the same where it crosses itself.
        let mut p = Pad::new(1);
        p.size = 3;
        p.down([0.2, 0.5]);
        p.moved([0.8, 0.5], [0.0, 0.0]);
        p.moved([0.2, 0.5], [0.0, 0.0]);
        p.up(0.0);
        let px = paint(&p.strokes, w, h, 1.0, 3.0);
        assert_eq!(px[((50 * w + 100) * 4 + 3) as usize], 110);
        // A dot is a dot.
        let mut p = Pad::new(1);
        p.down([0.5, 0.5]);
        p.up(0.0);
        let px = paint(&p.strokes, w, h, 1.0, 3.0);
        assert!(px[((50 * w + 100) * 4 + 3) as usize] > 0);
        assert!(paint(&p.strokes, 0, 0, 1.0, 3.0).is_empty());
    }
}
