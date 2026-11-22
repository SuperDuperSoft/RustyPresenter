//! Light mode, for a computer that cannot keep up: slide changes cut instead of fading, and the live preview's
//! video is refreshed once a second instead of four times. A crossfade is the heaviest thing the audience screen
//! does with still pictures (two full-screen layers blended sixty times a second), and a fade that stutters looks
//! worse than a clean cut.
//!
//! Settings → General → Screens and graphics → Light mode is Automatic, On or Off. Automatic watches the audience
//! screen's own fades: the room says when one starts (`fade_started`), every frame it then draws is counted
//! (`frame`, from `perf::observe`), and a fade that drew fewer than `POOR_FPS` frames a second was a poor one.
//! Poor fades again and again (`Judge`) turn light mode on for the rest of the session, and the operator is told
//! once (`take_notice`). A window that draws in software cannot report its frames, and software drawing is the
//! slowest of all, so there Automatic is on from the start.
//!
//! "Slides are drawn at" (`set_draw_height`, `fit`) is here too: the size the audience screens' slides are drawn
//! with, when that is to be smaller than the screen.
//!
//! All of it lives on the UI thread, where every window draws and the room plans its fades.
use crate::model::LightMode;
use std::{cell::RefCell, collections::VecDeque, time::{Duration, Instant}};

/// A fade that drew fewer frames a second than this was a poor one (`perf::SLOW_FPS`).
pub const POOR_FPS: f32 = super::perf::SLOW_FPS;
/// Fades shorter than this are too short to judge, and longer ones (a slow change of look) are not the everyday
/// slide change this is about.
pub const SHORTEST: Duration = Duration::from_millis(200);
pub const LONGEST: Duration = Duration::from_secs(2);
/// The fades looked at, and how many poor ones of them mean the computer cannot keep up.
pub const LAST: usize = 5;
pub const TOO_MANY: usize = 3;
/// How often the live preview asks for the video's picture, usually and in light mode.
pub const GRAB_EVERY: Duration = Duration::from_millis(250);
pub const GRAB_EVERY_LIGHT: Duration = Duration::from_secs(1);

/// Whether the audience screen keeps up with its fades. One stutter (a program starting, a big picture loading) is
/// nothing; `TOO_MANY` poor fades in the last `LAST` is a computer that cannot do it, and that is not unsaid: a
/// screen that went back to fading would stutter again.
#[derive(Debug, Default)]
pub struct Judge { recent: VecDeque<bool>, struggling: bool }

impl Judge {
    /// A fade of `dur` drew `frames` frames. True the moment the computer is found not to keep up.
    pub fn fade(&mut self, frames: u32, dur: Duration) -> bool {
        // No frame at all: the window was not drawing (hidden, or it cannot report its frames).
        if frames == 0 || dur < SHORTEST || dur > LONGEST { return false }
        self.recent.push_back((frames as f32) < POOR_FPS * dur.as_secs_f32());
        if self.recent.len() > LAST { self.recent.pop_front(); }
        if self.struggling || self.recent.iter().filter(|p| **p).count() < TOO_MANY { return false }
        self.struggling = true;
        true
    }

    pub fn struggling(&self) -> bool { self.struggling }
}

/// The fade being counted.
#[derive(Clone, Copy, Debug)]
struct Watch { start: Instant, dur: Duration, frames: u32 }

#[derive(Default)]
struct State { mode: LightMode, judge: Judge, watch: Option<Watch>, notice: bool, draw_height: u32 }

impl State {
    /// A watched fade that is over at `now` goes to the judge.
    fn settle(&mut self, now: Instant) {
        let Some(w) = self.watch.filter(|w| now.saturating_duration_since(w.start) >= w.dur) else { return };
        self.watch = None;
        if self.judge.fade(w.frames, w.dur) && self.mode == LightMode::Auto { self.notice = true }
    }
}

thread_local! { static STATE: RefCell<State> = RefCell::default(); }

/// The setting (after every read of the settings).
pub fn set_mode(mode: LightMode) { STATE.with_borrow_mut(|s| s.mode = mode) }

/// Whether light mode is on now.
pub fn on() -> bool {
    STATE.with_borrow(|s| match s.mode {
        LightMode::On => true,
        LightMode::Off => false,
        LightMode::Auto => s.judge.struggling() || software(),
    })
}

/// The windows are drawn in software (no usable graphics card, or safe mode). False before a renderer is chosen.
fn software() -> bool { crate::video::graphics::picked().is_some_and(|c| c.graphics == crate::model::Graphics::Software) }

/// The audience screen starts a crossfade of `dur` now.
pub fn fade_started(dur: Duration) { fade_started_at(dur, Instant::now()) }

fn fade_started_at(dur: Duration, now: Instant) {
    STATE.with_borrow_mut(|s| {
        s.settle(now);
        // A fade cut short by the next slide says nothing: it is dropped, and the new one counted.
        s.watch = (dur >= SHORTEST && dur <= LONGEST).then_some(Watch { start: now, dur, frames: 0 });
    })
}

/// The audience screen drew a frame.
pub fn frame(now: Instant) {
    STATE.with_borrow_mut(|s| {
        match &mut s.watch {
            Some(w) if now.saturating_duration_since(w.start) < w.dur => w.frames += 1,
            Some(_) => s.settle(now),
            None => {}
        }
    })
}

/// Once a second, from the operator: a fade that ended on a still picture is judged, and true is returned once
/// when Automatic has just turned light mode on (to tell the operator).
pub fn take_notice() -> bool { take_notice_at(Instant::now()) }

fn take_notice_at(now: Instant) -> bool {
    STATE.with_borrow_mut(|s| {
        s.settle(now);
        std::mem::take(&mut s.notice)
    })
}

/// How often the live preview asks the room's player for its picture.
pub fn grab_every() -> Duration { if on() { GRAB_EVERY_LIGHT } else { GRAB_EVERY } }

/// "Slides are drawn at": the most rows of pixels (`Settings::draw_height`; 0: the screen's own).
pub fn set_draw_height(rows: u32) { STATE.with_borrow_mut(|s| s.draw_height = rows) }

/// The size slides are drawn at for a screen of `size`: the screen's, or smaller with the same shape.
pub fn fit(size: (u32, u32)) -> (u32, u32) { fit_to(size, STATE.with_borrow(|s| s.draw_height)) }

/// `size` made at most `rows` high, keeping its shape (0: as it is). Never made bigger.
pub fn fit_to(size: (u32, u32), rows: u32) -> (u32, u32) {
    if rows == 0 || size.1 <= rows || size.0 == 0 { return size }
    let w = (u64::from(size.0) * u64::from(rows) + u64::from(size.1) / 2) / u64::from(size.1);
    (w.max(1) as u32, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FADE: Duration = Duration::from_millis(400);

    #[test]
    fn poor_fades_again_and_again_mean_struggling() {
        let mut j = Judge::default();
        // 24 frames in 400 ms is 60 a second; 12 is 30, still fine.
        for frames in [24, 12, 24, 9, 24, 24] { assert!(!j.fade(frames, FADE), "{frames} frames") }
        // 6 frames in 400 ms is 15 a second.
        assert!(!j.fade(6, FADE), "one stutter is nothing");
        assert!(!j.fade(24, FADE));
        assert!(!j.fade(5, FADE));
        assert!(j.fade(7, FADE), "three poor fades of the last five");
        assert!(j.struggling());
        assert!(!j.fade(3, FADE), "said once");
        for _ in 0..10 { j.fade(24, FADE); }
        assert!(j.struggling(), "it stays: going back to fades would stutter again");
    }

    #[test]
    fn fades_that_say_nothing_are_left_out() {
        let mut j = Judge::default();
        for _ in 0..10 {
            assert!(!j.fade(0, FADE), "no frame at all: the window was not drawing");
            assert!(!j.fade(1, Duration::from_millis(100)), "too short to judge");
            assert!(!j.fade(40, Duration::from_secs(10)), "a slow change of look");
        }
        assert!(!j.struggling());
        // A longer fade is judged by its own length: 30 frames in 2 s is 15 a second.
        assert!(!j.fade(30, LONGEST) && !j.fade(30, LONGEST) && j.fade(30, LONGEST));
    }

    /// One fade from `t`, drawing a frame every `every` ms; the time it ended.
    fn one_fade(t: Instant, every: u64) -> Instant {
        fade_started_at(FADE, t);
        let mut ms = every;
        while ms < 400 { frame(t + Duration::from_millis(ms)); ms += every }
        t + FADE
    }

    #[test]
    fn automatic_turns_on_when_the_room_cannot_keep_up_and_says_so_once() {
        let mut t = Instant::now();
        set_mode(LightMode::Auto);
        assert!(!on(), "off to begin with");
        assert_eq!(grab_every(), GRAB_EVERY);
        // Smooth fades, each followed by a still picture: the once-a-second look settles them.
        for _ in 0..4 { t = one_fade(t, 16) + Duration::from_secs(3); assert!(!take_notice_at(t)) }
        assert!(!on());
        // Three fades at ten frames a second.
        for i in 0..3 {
            t = one_fade(t, 100) + Duration::from_secs(1);
            assert_eq!(take_notice_at(t), i == 2, "fade {i}");
        }
        assert!(on());
        assert_eq!(grab_every(), GRAB_EVERY_LIGHT);
        assert!(!take_notice_at(t + Duration::from_secs(1)), "told once");
        // The setting still decides.
        set_mode(LightMode::Off);
        assert!(!on());
        set_mode(LightMode::On);
        assert!(on());
    }

    #[test]
    fn on_and_off_by_hand() {
        set_mode(LightMode::On);
        assert!(on(), "on with nothing measured");
        set_mode(LightMode::Off);
        let mut t = Instant::now();
        for _ in 0..5 { t = one_fade(t, 100) + Duration::from_secs(1); assert!(!take_notice_at(t), "Off is not told about it") }
        assert!(!on(), "Off stays off however the fades go");
    }

    #[test]
    fn a_fade_cut_short_is_not_counted() {
        set_mode(LightMode::Auto);
        let t = Instant::now();
        // Next pressed five times in half a second: each fade is dropped by the one after it.
        for i in 0..5u64 { fade_started_at(FADE, t + Duration::from_millis(i * 100)); frame(t + Duration::from_millis(i * 100 + 50)) }
        assert!(STATE.with_borrow(|s| s.judge.recent.is_empty()), "none of the first four was judged");
        // The last one runs to its end. A frame drawn after that settles it, without waiting for the operator's look.
        for ms in (466..800).step_by(16) { frame(t + Duration::from_millis(ms)) }
        frame(t + Duration::from_millis(805));
        assert!(STATE.with_borrow(|s| s.watch.is_none() && s.judge.recent == [false]), "judged, and a good one");
        assert!(!take_notice_at(t + Duration::from_secs(2)));
        assert!(!on());
    }

    #[test]
    fn slides_drawn_smaller_keep_their_shape() {
        assert_eq!(fit_to((3840, 2160), 0), (3840, 2160), "0: the screen's own size");
        assert_eq!(fit_to((3840, 2160), 1080), (1920, 1080));
        assert_eq!(fit_to((1920, 1080), 720), (1280, 720));
        assert_eq!(fit_to((1280, 720), 1080), (1280, 720), "never made bigger");
        assert_eq!(fit_to((1920, 1200), 1080), (1728, 1080), "16:10 stays 16:10");
        assert_eq!(fit_to((1080, 1920), 1080), (608, 1080), "a screen on its side");
        assert_eq!(fit_to((0, 0), 720), (0, 0));
        set_draw_height(720);
        assert_eq!(fit((1920, 1080)), (1280, 720));
        set_draw_height(0);
        assert_eq!(fit((1920, 1080)), (1920, 1080));
    }
}
