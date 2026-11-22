//! Fades driven from Rust, one timer per window, instead of Slint `animate`.
//!
//! Slint 1.18's "something is animating" flag is global: while any `animate` runs, the winit backend redraws
//! **every** window at its refresh rate. A 400 ms room crossfade made the operator and stage redraw too, and they
//! took turns with the room on the one UI thread (`docs/superpowers/specs/2026-11-03-gpu-rendering-design.md`,
//! root cause 2). A `Fader` sets plain properties of its own window from a ~16 ms timer, so only that window
//! redraws. Each value is worked out from the time elapsed, so a fade lasts as long at 60, 75 or 144 Hz, and a
//! late tick catches up instead of slowing the fade down.
use slint::{Timer, TimerMode};
use std::{cell::RefCell, rc::{Rc, Weak}, time::{Duration, Instant}};

/// How often a running fade moves on (about 60 times a second; the window's frame throttle paces the drawing).
pub const TICK: Duration = Duration::from_millis(16);

/// Ease in and out (smoothstep): starts and ends gently, symmetric, so a crossfade's two faces always add up to
/// the same coverage. `t` is clamped to 0–1.
pub fn ease(t: f32) -> f32 {
    let t = if t.is_nan() { 1.0 } else { t.clamp(0.0, 1.0) };
    t * t * (3.0 - 2.0 * t)
}

/// One value moving from `from` to `to` in `dur`, from `start`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ramp { pub from: f32, pub to: f32, pub start: Instant, pub dur: Duration }

impl Ramp {
    /// A value that stays at `v`.
    pub fn still(v: f32, now: Instant) -> Ramp { Ramp { from: v, to: v, start: now, dur: Duration::ZERO } }

    /// The value at `now`, between `from` and `to` (clamped to 0–1).
    pub fn at(&self, now: Instant) -> f32 {
        let v = if self.done(now) {
            self.to
        } else {
            let t = now.saturating_duration_since(self.start).as_secs_f32() / self.dur.as_secs_f32();
            self.from + (self.to - self.from) * ease(t)
        };
        v.clamp(0.0, 1.0)
    }

    pub fn done(&self, now: Instant) -> bool { self.dur.is_zero() || now.saturating_duration_since(self.start) >= self.dur }

    /// From wherever this ramp is at `now` towards `to`. `full` is the time for the whole 0–1 range, so a fade that
    /// starts part way (a new slide arriving mid-fade) moves at the same speed and ends sooner.
    pub fn retarget(&self, to: f32, full: Duration, now: Instant) -> Ramp {
        let from = self.at(now);
        let dur = full.mul_f32((to - from).abs().min(1.0));
        Ramp { from, to, start: now, dur }
    }
}

/// How long after a frame moved the fades on their own timer stays quiet. Past it the window is not drawing
/// (hidden, covered, or drawn in software), and the timer carries the fade to its end.
pub const FED: Duration = Duration::from_millis(50);

/// Whether frames moved things on recently enough (`FED`) that a timer should leave them to it.
pub fn fed(last: Option<Instant>, now: Instant) -> bool { last.is_some_and(|t| now.saturating_duration_since(t) < FED) }

/// When to move a window's fades on after one of its frames, so they step once per refresh of its monitor and not
/// on a timer of their own: a 16 ms timer gives 62 steps a second, which on a 75 Hz screen repeats every sixth
/// frame and on a 120 Hz one every second, and a push or wipe then judders.
///
/// `frame_end` is when the frame was handed to the screen (with vsync, just after a refresh), `period` the
/// monitor's refresh time. The step waits 0.4 of a refresh, which leaves the other windows their turn to draw on
/// the one UI thread and still leaves time to draw before the next refresh. It also keeps 0.9 of a refresh from
/// the step before (`last_step`), so a window whose frames do not wait for the screen cannot run away.
pub fn next_step(frame_end: Instant, last_step: Option<Instant>, period: Duration) -> Instant {
    let after_frame = frame_end + period.mul_f32(0.4);
    last_step.map_or(after_frame, |l| after_frame.max(l + period.mul_f32(0.9)))
}

/// A monitor's refresh time from its rate in thousandths of a hertz (winit's `refresh_rate_millihertz`); 60 Hz
/// when it does not say, and never outside 24 to 500 Hz.
pub fn refresh_period(millihertz: Option<u32>) -> Duration {
    let mhz = millihertz.filter(|m| *m > 0).unwrap_or(60_000).clamp(24_000, 500_000);
    Duration::from_secs_f64(1000.0 / f64::from(mhz))
}

/// A fading value: its ramp, what was last written, and where it is written.
struct Channel { ramp: Ramp, shown: f32, set: Box<dyn Fn(f32)> }

struct Inner { timer: Timer, chans: RefCell<Vec<Channel>>, now: Box<dyn Fn() -> Instant>, fed: std::cell::Cell<Option<Instant>> }

/// One fading value of a `Fader`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chan(usize);

/// The fades of one window. Its timer runs only while a fade does.
pub struct Fader { inner: Rc<Inner> }

impl Default for Fader {
    fn default() -> Self { Fader::new() }
}

impl Fader {
    pub fn new() -> Fader { Fader::with_clock(Instant::now) }

    /// A fader reading the time from `now` (tests drive it with a fake clock and `tick`).
    pub fn with_clock(now: impl Fn() -> Instant + 'static) -> Fader {
        Fader { inner: Rc::new(Inner { timer: Timer::default(), chans: RefCell::default(), now: Box::new(now), fed: Default::default() }) }
    }

    /// A new value starting at `initial`, written through `set` (now, and whenever it changes).
    pub fn channel(&self, initial: f32, set: impl Fn(f32) + 'static) -> Chan {
        let initial = initial.clamp(0.0, 1.0);
        set(initial);
        let mut chans = self.inner.chans.borrow_mut();
        chans.push(Channel { ramp: Ramp::still(initial, (self.inner.now)()), shown: initial, set: Box::new(set) });
        Chan(chans.len() - 1)
    }

    /// Fades `ch` to `to`, from where it is now; `full` is how long a whole 0–1 fade takes. Fading to where it is
    /// already heading changes nothing.
    pub fn fade(&self, ch: Chan, to: f32, full: Duration) {
        let to = to.clamp(0.0, 1.0);
        let now = (self.inner.now)();
        {
            let mut chans = self.inner.chans.borrow_mut();
            let Some(c) = chans.get_mut(ch.0) else { return };
            if c.ramp.to == to { return }
            c.ramp = c.ramp.retarget(to, full, now);
        }
        self.tick();
        self.run();
    }

    /// Sets `ch` to `to` at once, stopping its fade.
    pub fn jump(&self, ch: Chan, to: f32) {
        let now = (self.inner.now)();
        if let Some(c) = self.inner.chans.borrow_mut().get_mut(ch.0) { c.ramp = Ramp::still(to.clamp(0.0, 1.0), now) }
        self.tick();
    }

    /// The value last written for `ch`.
    pub fn value(&self, ch: Chan) -> f32 { self.inner.chans.borrow().get(ch.0).map_or(0.0, |c| c.shown) }

    /// Where `ch` is heading (its value once the fade is over).
    pub fn target(&self, ch: Chan) -> f32 { self.inner.chans.borrow().get(ch.0).map_or(0.0, |c| c.ramp.to) }

    /// Whether a fade is still running.
    pub fn running(&self) -> bool {
        let now = (self.inner.now)();
        self.inner.chans.borrow().iter().any(|c| !c.ramp.done(now) || c.shown != c.ramp.to)
    }

    /// Moves every fade on to now and writes the values that changed; stops the timer once all are over.
    pub fn tick(&self) { tick(&self.inner) }

    /// `tick`, from a frame of the window (`next_step`): while frames keep coming the timer leaves the fades to
    /// them, so they move once per frame shown.
    pub fn frame_tick(&self) {
        self.inner.fed.set(Some((self.inner.now)()));
        tick(&self.inner)
    }

    /// The timer's turn: nothing while frames are moving the fades on.
    fn timer_tick(inner: &Inner) { if !fed(inner.fed.get(), (inner.now)()) { tick(inner) } }

    fn run(&self) {
        if self.inner.timer.running() || !self.running() { return }
        let weak: Weak<Inner> = Rc::downgrade(&self.inner);
        self.inner.timer.start(TimerMode::Repeated, TICK, move || if let Some(i) = weak.upgrade() { Fader::timer_tick(&i) });
    }
}

fn tick(inner: &Inner) {
    let now = (inner.now)();
    // Values first, then the writes, so a setter may read the fader.
    let changed: Vec<(usize, f32)> = {
        let mut chans = inner.chans.borrow_mut();
        chans.iter_mut().enumerate().filter_map(|(i, c)| {
            let v = c.ramp.at(now);
            (v != c.shown).then(|| { c.shown = v; (i, v) })
        }).collect()
    };
    let busy = {
        let chans = inner.chans.borrow();
        for (i, v) in &changed { (chans[*i].set)(*v) }
        chans.iter().any(|c| !c.ramp.done(now))
    };
    if !busy { inner.timer.stop() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const FULL: Duration = Duration::from_millis(400);

    /// A fader on a fake clock, and the clock.
    fn faked() -> (Fader, Rc<Cell<Instant>>) {
        let t0 = Instant::now();
        let clock = Rc::new(Cell::new(t0));
        let c = clock.clone();
        (Fader::with_clock(move || c.get()), clock)
    }

    fn later(clock: &Cell<Instant>, ms: u64) { clock.set(clock.get() + Duration::from_millis(ms)) }

    #[test]
    fn ease_is_clamped_symmetric_and_monotonic() {
        assert_eq!((ease(-1.0), ease(0.0), ease(1.0), ease(2.0)), (0.0, 0.0, 1.0, 1.0));
        assert!((ease(0.5) - 0.5).abs() < 1e-6);
        for i in 0..100 {
            let t = i as f32 / 100.0;
            assert!((ease(t) + ease(1.0 - t) - 1.0).abs() < 1e-5, "symmetric at {t}");
            assert!(ease(t + 0.01) >= ease(t));
        }
        assert_eq!(ease(f32::NAN), 1.0);
    }

    #[test]
    fn a_step_follows_each_frame_and_cannot_run_away() {
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        let p = refresh_period(Some(75_000));
        assert!((p.as_secs_f32() * 1000.0 - 13.333).abs() < 0.01);
        assert_eq!(refresh_period(None), refresh_period(Some(60_000)));
        assert_eq!(refresh_period(Some(0)), refresh_period(Some(60_000)));
        assert_eq!(refresh_period(Some(1)), refresh_period(Some(24_000)), "never slower than 24 Hz");
        let near = |a: Instant, b: Instant| a.max(b).duration_since(a.min(b)) < Duration::from_micros(10);
        // The first step: 0.4 of a refresh after the frame.
        assert!(near(next_step(t0, None, ms(10)), t0 + ms(4)));
        // With vsync a frame ends a refresh after the last one, about 0.6 after the step: one step per refresh.
        assert!(near(next_step(t0 + ms(10), Some(t0 + ms(4)), ms(10)), t0 + ms(14)));
        // Without vsync the frame ends at once: the next step still keeps 0.9 of a refresh from the last.
        assert!(near(next_step(t0 + ms(5), Some(t0 + ms(4)), ms(10)), t0 + ms(13)));
    }

    #[test]
    fn frames_keep_the_timer_quiet_until_they_stop() {
        let (f, clock) = faked();
        let seen = Rc::new(Cell::new(0.0f32));
        let s = seen.clone();
        let ch = f.channel(0.0, move |v| s.set(v));
        f.fade(ch, 1.0, FULL);
        later(&clock, 100);
        f.frame_tick();
        let at_frame = seen.get();
        assert!(at_frame > 0.0 && at_frame < 0.5);
        // The timer comes round 10 ms later: the frames have it.
        later(&clock, 10);
        Fader::timer_tick(&f.inner);
        assert_eq!(seen.get(), at_frame, "the timer left it to the frames");
        // No frame for a while (the window is covered): the timer carries on, to the end.
        later(&clock, 90);
        Fader::timer_tick(&f.inner);
        assert!((seen.get() - 0.5).abs() < 1e-3, "half way at 200 ms: {}", seen.get());
        later(&clock, 200);
        Fader::timer_tick(&f.inner);
        assert_eq!(seen.get(), 1.0);
        assert!(!f.running());
        assert!(fed(Some(clock.get()), clock.get()) && !fed(None, clock.get()) && !fed(Some(clock.get()), clock.get() + FED));
    }

    #[test]
    fn a_fade_lasts_its_time_whatever_the_ticks() {
        // The same fade ticked at 144 Hz and at 30 Hz reaches the same values at the same times.
        for every in [7u64, 33] {
            let (f, clock) = faked();
            let seen = Rc::new(Cell::new(0.0f32));
            let s = seen.clone();
            let ch = f.channel(0.0, move |v| s.set(v));
            f.fade(ch, 1.0, FULL);
            let mut t = 0;
            while t + every < 200 { later(&clock, every); t += every; f.tick() }
            later(&clock, 200 - t);
            f.tick();
            assert!((seen.get() - 0.5).abs() < 1e-3, "half way at 200 ms ({every} ms ticks): {}", seen.get());
            later(&clock, 199);
            f.tick();
            assert!(seen.get() < 1.0 && f.running());
            later(&clock, 1);
            f.tick();
            assert_eq!(seen.get(), 1.0);
            assert!(!f.running(), "over after 400 ms");
        }
    }

    #[test]
    fn a_new_target_mid_fade_starts_from_the_current_value() {
        let (f, clock) = faked();
        let ch = f.channel(0.0, |_| {});
        f.fade(ch, 1.0, FULL);
        later(&clock, 200);
        f.tick();
        let mid = f.value(ch);
        assert!((mid - 0.5).abs() < 1e-3);
        // Back to 0 from 0.5: no jump, and it takes half the full time.
        f.fade(ch, 0.0, FULL);
        assert_eq!(f.value(ch), mid, "no jump when the target changes");
        later(&clock, 100);
        f.tick();
        assert!(f.value(ch) < mid && f.value(ch) > 0.0);
        later(&clock, 100);
        f.tick();
        assert_eq!(f.value(ch), 0.0);
        assert!(!f.running());
    }

    #[test]
    fn values_stay_between_0_and_1() {
        let (f, clock) = faked();
        let ch = f.channel(3.0, |_| {});
        assert_eq!(f.value(ch), 1.0);
        f.fade(ch, -2.0, FULL);
        for _ in 0..30 { later(&clock, 16); f.tick(); assert!((0.0..=1.0).contains(&f.value(ch))) }
        assert_eq!(f.value(ch), 0.0);
    }

    #[test]
    fn only_changed_values_are_written() {
        let (f, clock) = faked();
        let writes = Rc::new(Cell::new(0));
        let (wa, wb) = (writes.clone(), writes.clone());
        let a = f.channel(0.0, move |_| wa.set(wa.get() + 1));
        let _b = f.channel(1.0, move |_| wb.set(wb.get() + 1));
        writes.set(0);
        f.fade(a, 1.0, FULL);
        for _ in 0..40 { later(&clock, 16); f.tick() }
        // `b` never moved, so it was never written again; `a` was written once per tick until it got there.
        let n = writes.get();
        assert!((20..=27).contains(&n), "{n} writes");
        f.fade(a, 1.0, FULL);
        f.tick();
        assert_eq!(writes.get(), n, "fading to where it already is writes nothing");
    }

    #[test]
    fn jump_sets_at_once() {
        let (f, _clock) = faked();
        let seen = Rc::new(Cell::new(0.0f32));
        let s = seen.clone();
        let ch = f.channel(0.0, move |v| s.set(v));
        f.fade(ch, 1.0, FULL);
        f.jump(ch, 0.25);
        assert_eq!((seen.get(), f.target(ch)), (0.25, 0.25));
        assert!(!f.running());
    }

    #[test]
    fn the_timer_runs_only_while_fading() {
        crate::ui::ui_test_init();
        let f = Fader::new();
        let seen = Rc::new(Cell::new(0.0f32));
        let s = seen.clone();
        let ch = f.channel(0.0, move |v| s.set(v));
        assert!(!f.inner.timer.running());
        f.fade(ch, 1.0, Duration::from_millis(40));
        assert!(f.inner.timer.running());
        let end = Instant::now() + Duration::from_secs(2);
        while f.inner.timer.running() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(5));
        }
        assert!(!f.inner.timer.running(), "the timer stops once the fade is over");
        assert_eq!(seen.get(), 1.0);
    }
}
