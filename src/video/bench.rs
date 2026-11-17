//! Settings → Video → "Test which is fastest": each graphics choice this computer has is tried in a child process of
//! the app (`rustypresenter --graphics-bench <choice> --bench-out <file>`, `ui::graphics_test`), because the backend
//! is chosen once per process. Each child opens the real operator, room and stage windows over a data folder of its
//! own, changes lyrics slides over a moving background for a few seconds, and writes what it measured (`Measured`)
//! as JSON: how long a slide change takes to reach the room, hiccups in the room's crossfades and video, freezes of
//! the operator, how long the room takes to draw a frame, and the processor. A child that crashes, hangs (killed after
//! `LIMIT`) or writes nothing "Didn't work". Each choice runs twice (`schedule`, the second round in reverse order so
//! none always goes first) and keeps its better run (`combine`).
//!
//! The results are kept in `<data>/graphics-test.json` (`Record`) with the graphics card's name and driver as each
//! choice reported them. Automatic then uses the winner (`auto_pick`) instead of OpenGL, until the card or driver
//! changes (`driver_changed`), which marks the record stale and offers the test again (`offer`).
use crate::{model::Graphics, tr, trc};
use serde::{Deserialize, Serialize};
use std::{ffi::OsString, io, path::{Path, PathBuf}, time::{Duration, Instant}};

/// The child's flags.
pub const FLAG: &str = "--graphics-bench";
pub const OUT_FLAG: &str = "--bench-out";
/// Put the test's room full screen on a monitor (the room's saved one, or `auto`: the first that is not the main
/// one), as the app does. Only passed while the church's room window is closed.
pub const ROOM_FLAG: &str = "--bench-room";
/// A child still running after this is killed.
pub const LIMIT: Duration = Duration::from_secs(30);

/// What one child measured.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Measured {
    pub graphics: Graphics,
    /// The windows opened and drew.
    pub started: bool,
    /// Slide changes sent, and how many of them were seen on the room.
    pub changes: u32,
    pub seen: u32,
    /// From a slide change command to the room's first frame showing it (ms): the middle one and the slow end (95 %).
    pub delay_ms: f32,
    pub delay_slow_ms: f32,
    /// Room frames that came late, per minute: in a crossfade, or a stall while video plays. `None` where the room's
    /// frames can't be counted (the Software renderer).
    pub hiccups_min: Option<f32>,
    /// Times the operator froze for more than `FREEZE_MS`, and the longest pause of all (ms).
    pub freezes: u32,
    pub freeze_ms: f32,
    /// How long the room takes to draw a frame, slow end (95 %, ms); `None` where it can't be timed.
    pub draw_ms: Option<f32>,
    /// The room's frames a second while measuring (for the log).
    pub fps: f32,
    /// The process's CPU use, % of one core (`None` where it can't be read).
    pub cpu: Option<f32>,
    /// A video played behind the lyrics (libmpv was there).
    pub video: bool,
    /// The room was full screen on its own monitor.
    pub on_monitor: bool,
    /// The graphics card's name and driver, as this choice reports it.
    pub adapter: String,
    /// Why it didn't work, for the log.
    pub error: String,
}

impl Measured {
    pub fn failed(graphics: Graphics, error: impl Into<String>) -> Measured {
        Measured { graphics, error: error.into(), ..Measured::default() }
    }
    pub fn works(&self) -> bool { self.started && self.seen > 0 }
}

/// A pause of the operator's UI thread longer than this is a freeze.
pub const FREEZE_MS: f64 = 50.0;
/// While video plays, a room gap longer than this is a stall.
const STALL_MS: f64 = 100.0;

/// `p` (0–1) of `v`; 0 for none.
pub fn percentile(v: &[f64], p: f64) -> f64 {
    if v.is_empty() { return 0.0 }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    s[((s.len() - 1) as f64 * p).round() as usize]
}

/// The first time in `times` (sorted) at or after `t`.
fn at_or_after(times: &[f64], t: f64) -> Option<f64> { times.get(times.partition_point(|x| *x < t)).copied() }

/// Each slide change's delay (ms): from the command (`sends`) to the room putting the new slide up (`swaps`), then to
/// the first room frame drawn after that (`frames`, when they are counted). A change never seen is left out. All
/// times in ms on one clock, sorted.
pub fn change_delays(sends: &[f64], swaps: &[f64], frames: Option<&[f64]>) -> Vec<f64> {
    sends.iter().filter_map(|s| {
        let swap = at_or_after(swaps, *s)?;
        let shown = match frames { Some(f) => at_or_after(f, swap)?, None => swap };
        Some(shown - s)
    }).collect()
}

/// Room frames that came late: in a crossfade (`fade_ms` after a swap), a gap over 1.8 times the usual crossfade gap
/// (at least 25 ms, so a fast monitor's one missed refresh is not counted); while video plays, any gap over
/// `STALL_MS`.
pub fn hiccups(frames: &[f64], swaps: &[f64], fade_ms: f64, video: bool) -> u32 {
    // Both ends in one crossfade: the gap into its first frame is from before it started, not a late frame.
    let in_fade = |a: f64, b: f64| swaps.iter().any(|s| a >= *s && b <= s + fade_ms);
    let gaps: Vec<(bool, f64)> = frames.windows(2).map(|w| (in_fade(w[0], w[1]), w[1] - w[0])).collect();
    let fade_gaps: Vec<f64> = gaps.iter().filter(|(f, _)| *f).map(|(_, g)| *g).collect();
    let late = (percentile(&fade_gaps, 0.5) * 1.8).max(25.0);
    gaps.iter().filter(|(f, g)| (*f && *g > late) || (video && *g > STALL_MS)).count() as u32
}

/// Freezes over `FREEZE_MS` and the longest gap, from the gaps between a 1 ms timer's ticks on the UI thread.
pub fn freezes(gaps_ms: &[f64]) -> (u32, f32) {
    (gaps_ms.iter().filter(|g| **g > FREEZE_MS).count() as u32, gaps_ms.iter().copied().fold(0.0, f64::max) as f32)
}

/// Frames a second from the times frames ended (ms, in order).
pub fn fps(ends_ms: &[f64]) -> f32 {
    if ends_ms.len() < 2 { return 0.0 }
    let span = ends_ms[ends_ms.len() - 1] - ends_ms[0];
    if span <= 0.0 { 0.0 } else { ((ends_ms.len() - 1) as f64 / span * 1000.0) as f32 }
}

/// The word shown beside each choice, worst first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict { Failed, Choppy, Ok, Smooth }

/// Smooth: slides show within 150 ms, almost no hiccups, no pause over 100 ms. OK: within 300 ms, a hiccup every few
/// seconds at most, no pause over 250 ms. A run whose room frames can't be counted is at best OK.
pub fn verdict(m: &Measured) -> Verdict {
    if !m.works() { return Verdict::Failed }
    let h = m.hiccups_min;
    if m.delay_slow_ms <= 150.0 && h.is_some_and(|h| h <= 6.0) && m.freeze_ms <= 100.0 { return Verdict::Smooth }
    if m.delay_slow_ms <= 300.0 && h.is_none_or(|h| h <= 30.0) && m.freeze_ms <= 250.0 { return Verdict::Ok }
    Verdict::Choppy
}

pub fn word(v: Verdict) -> String {
    match v {
        Verdict::Smooth => tr!("Smooth"),
        Verdict::Ok => trc!("graphics test result", "OK"),
        Verdict::Choppy => tr!("Choppy"),
        Verdict::Failed => tr!("Didn't work"),
    }
}

/// How good a run was, out of 100: points off for slow slide changes, hiccups, pauses, slow room frames and a busy
/// processor. 0 when it failed.
pub fn score(m: &Measured) -> f32 {
    if !m.works() { return 0.0 }
    let off = (m.delay_slow_ms - 60.0).max(0.0) / 4.0
        + m.hiccups_min.unwrap_or(0.0) * 1.5
        + (m.freeze_ms - 30.0).max(0.0) / 8.0
        + (m.draw_ms.unwrap_or(0.0) - 6.0).max(0.0)
        + m.cpu.unwrap_or(0.0) / 10.0;
    (100.0 - off).max(1.0)
}

/// Among runs this close to the best (share of its score), the preferred choice wins.
const CLOSE: f32 = 0.05;

/// Which of near-equal choices to prefer: OpenGL draws video under the scene; Software never wins a near tie.
fn preference(g: Graphics) -> u8 {
    match g {
        Graphics::OpenGl | Graphics::Auto => 0,
        Graphics::Direct3D | Graphics::Metal => 1,
        Graphics::Vulkan => 2,
        Graphics::Software => 3,
    }
}

/// The best choice: the highest verdict, then the score; near ties go to `preference`. Software, whose room frames
/// can't be counted, wins only when no graphics-card choice is at least OK. `None` when nothing worked.
pub fn best(results: &[Measured]) -> Option<Graphics> {
    let card_ok = results.iter().any(|m| m.graphics != Graphics::Software && verdict(m) >= Verdict::Ok);
    let fair: Vec<&Measured> = results.iter().filter(|m| !(card_ok && m.graphics == Graphics::Software)).collect();
    let top = fair.iter().map(|m| verdict(m)).max().filter(|v| *v > Verdict::Failed)?;
    let tier: Vec<&Measured> = fair.into_iter().filter(|m| verdict(m) == top).collect();
    let high = tier.iter().map(|m| score(m)).fold(0.0, f32::max);
    tier.into_iter().filter(|m| score(m) >= high * (1.0 - CLOSE)).min_by_key(|m| preference(m.graphics)).map(|m| m.graphics)
}

/// The better of two runs of one choice: the higher verdict, then the higher score.
pub fn better(a: Measured, b: Measured) -> Measured {
    if (verdict(&b), score(&b)) > (verdict(&a), score(&a)) { b } else { a }
}

/// The order the children run in: every choice, then again in reverse (Software, the fallback, only once).
pub fn schedule(choices: &[Graphics]) -> Vec<Graphics> {
    let mut v = choices.to_vec();
    v.extend(choices.iter().rev().filter(|g| **g != Graphics::Software));
    v
}

/// One result per choice, in `choices`' order: the better of its runs.
pub fn combine(choices: &[Graphics], runs: Vec<Measured>) -> Vec<Measured> {
    choices.iter().filter_map(|g| runs.iter().filter(|m| m.graphics == *g).cloned().reduce(better)).collect()
}

/// Why a run didn't work, in plain words.
pub fn reason(m: &Measured) -> String {
    let e = m.error.as_str();
    if e == "took too long" { tr!("It stopped responding.") }
    else if e.starts_with("could not start") { tr!("It could not start.") }
    else if e == "stopped without a result" { tr!("It closed before it finished. It may have crashed.") }
    else if m.started && m.seen == 0 { tr!("Slides never reached the audience screen.") }
    else { tr!("This graphics card or its driver can't draw with it.") }
}

/// The line under each choice: what was measured, or why it didn't work.
pub fn details(m: &Measured) -> String {
    if !m.works() { return reason(m) }
    let mut parts = vec![tr!("Slide change {ms} ms", ms = m.delay_slow_ms.round() as i64)];
    if let Some(h) = m.hiccups_min { parts.push(tr!("{n} hiccups a minute", n = h.round() as i64)) }
    parts.push(tr!("longest pause {ms} ms", ms = m.freeze_ms.round() as i64));
    if let Some(c) = m.cpu { parts.push(tr!("processor {pct} %", pct = c.round() as i64)) }
    parts.join(" · ")
}

/// One line of the results list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row { pub graphics: Graphics, pub name: String, pub bar: f32, pub word: String, pub details: String, pub best: bool, pub works: bool }

/// The results list: each choice with a bar (share of the best score), its word, what was measured, and a star on
/// the best.
pub fn rows(results: &[Measured]) -> Vec<Row> {
    let winner = best(results);
    let high = results.iter().map(score).fold(0.0, f32::max);
    results.iter().map(|m| Row {
        graphics: m.graphics,
        name: super::graphics::name(m.graphics),
        bar: if high > 0.0 { score(m) / high } else { 0.0 },
        word: word(verdict(m)),
        details: details(m),
        best: Some(m.graphics) == winner,
        works: m.works(),
    }).collect()
}

/// The choices to test: every one this build offers but Automatic (which is OpenGL until a test picks).
pub fn choices(available: &[Graphics]) -> Vec<Graphics> {
    available.iter().copied().filter(|g| *g != Graphics::Auto).collect()
}

// ---- The child's command line ----

/// `--graphics-bench <choice> --bench-out <file> [--bench-room <monitor|auto>]`.
#[derive(Clone, Debug, PartialEq)]
pub struct ChildArgs { pub graphics: Graphics, pub out: PathBuf, pub room: Option<String> }

/// The child's arguments, when this process is one. A choice that can't be read still runs (and fails) as asked,
/// so the parent hears back.
pub fn child_args(args: impl IntoIterator<Item = OsString>) -> Option<ChildArgs> {
    let (mut graphics, mut out, mut room, mut asked) = (None, None, None, false);
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        if a == FLAG { asked = true; graphics = it.next().and_then(|g| serde_json::from_value(serde_json::Value::String(g.to_string_lossy().into())).ok()) }
        else if a == OUT_FLAG { out = it.next().map(PathBuf::from) }
        else if a == ROOM_FLAG { room = it.next().map(|m| m.to_string_lossy().into_owned()) }
    }
    if !asked { return None }
    Some(ChildArgs { graphics: graphics.unwrap_or(Graphics::Software), out: out?, room })
}

/// A choice as the child's argument (`vulkan`, the saved setting's spelling).
pub fn arg(g: Graphics) -> String {
    serde_json::to_value(g).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// What `--bench-room` is given: the room's saved monitor, or `auto` when none is saved. `None` (a plain window)
/// while the church's room window is open, so the test never covers what the room shows.
pub fn room_arg(room_open: bool, saved: Option<&str>) -> Option<String> {
    if room_open { return None }
    Some(saved.filter(|s| !s.is_empty()).unwrap_or("auto").to_string())
}

// ---- Running the children ----

/// A running child (a real process, or a fake in tests).
pub trait Child {
    /// It has ended.
    fn done(&mut self) -> bool;
    fn kill(&mut self);
}

impl Child for std::process::Child {
    fn done(&mut self) -> bool { !matches!(self.try_wait(), Ok(None)) }
    fn kill(&mut self) { let _ = std::process::Child::kill(self); let _ = self.wait(); }
}

/// Starts one child for `g`, writing to `out`.
pub trait Launch {
    type C: Child;
    fn launch(&self, g: Graphics, out: &Path) -> io::Result<Self::C>;
}

/// This executable, with the child's flags (and `--bench-room` when `room` is set).
pub struct Exe { pub exe: PathBuf, pub room: Option<String> }

impl Launch for Exe {
    type C = std::process::Child;
    fn launch(&self, g: Graphics, out: &Path) -> io::Result<std::process::Child> {
        use std::process::{Command, Stdio};
        let mut c = Command::new(&self.exe);
        c.arg(FLAG).arg(arg(g)).arg(OUT_FLAG).arg(out);
        if let Some(r) = &self.room { c.arg(ROOM_FLAG).arg(r); }
        c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
    }
}

/// Runs one child for `g` and reads what it wrote; a child that runs past `limit` is killed.
pub fn run_one(l: &impl Launch, g: Graphics, out: &Path, limit: Duration, poll: Duration) -> Measured {
    let _ = std::fs::remove_file(out);
    let mut child = match l.launch(g, out) {
        Ok(c) => c,
        Err(e) => return Measured::failed(g, format!("could not start: {e}")),
    };
    let start = Instant::now();
    while !child.done() {
        if start.elapsed() >= limit {
            child.kill();
            return Measured::failed(g, "took too long");
        }
        std::thread::sleep(poll);
    }
    let read = std::fs::read(out).ok().and_then(|b| serde_json::from_slice::<Measured>(&b).ok());
    let _ = std::fs::remove_file(out);
    match read {
        Some(m) => Measured { graphics: g, ..m },
        None => Measured::failed(g, "stopped without a result"),
    }
}

/// Runs every choice twice (`schedule`), one child at a time, and keeps each one's better run (`combine`).
/// `progress(i, n, g)` before each child.
pub fn run_all(l: &impl Launch, choices: &[Graphics], dir: &Path, limit: Duration, poll: Duration, mut progress: impl FnMut(usize, usize, Graphics)) -> Vec<Measured> {
    let order = schedule(choices);
    let runs = order.iter().enumerate().map(|(i, g)| {
        progress(i, order.len(), *g);
        let m = run_one(l, *g, &dir.join(format!("graphics-test-{}.json", arg(*g))), limit, poll);
        eprintln!("Graphics test {}: {}", arg(*g), log_line(&m));
        m
    }).collect();
    combine(choices, runs)
}

/// One run for the log.
pub fn log_line(m: &Measured) -> String {
    if !m.works() { return format!("didn't work ({})", m.error) }
    let opt = |v: Option<f32>| v.map_or("-".to_string(), |v| format!("{v:.1}"));
    format!(
        "{:?}, score {:.0}; slide change {:.0}/{:.0} ms (seen {}/{}), hiccups/min {}, freezes {} (longest {:.0} ms), room draw {} ms, {:.0} fps, cpu {}%, video {}, own monitor {}; {}",
        verdict(m), score(m), m.delay_ms, m.delay_slow_ms, m.seen, m.changes, opt(m.hiccups_min), m.freezes, m.freeze_ms, opt(m.draw_ms), m.fps, opt(m.cpu), m.video, m.on_monitor, m.adapter,
    )
}

pub fn testing(g: Graphics, i: usize, n: usize) -> String {
    tr!("Testing {mode}… ({at} of {count})", mode = super::graphics::name(g), at = i + 1, count = n)
}

// ---- What is kept ----

/// `<data>/graphics-test.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Record {
    pub results: Vec<Measured>,
    pub winner: Option<Graphics>,
    /// The card or driver changed since: Automatic is OpenGL again until the next test.
    pub stale: bool,
    /// The test was offered at a start (taken or not).
    pub offered: bool,
}

pub fn file(root: &Path) -> PathBuf { root.join("graphics-test.json") }

/// The record; `None` when there is none or it can't be read.
pub fn load(root: &Path) -> Option<Record> { serde_json::from_slice(&std::fs::read(file(root)).ok()?).ok() }

pub fn save(root: &Path, r: &Record) {
    let json = serde_json::to_vec_pretty(r).unwrap_or_default();
    if let Err(e) = std::fs::write(file(root), json) { eprintln!("Could not write {}: {e}", file(root).display()) }
}

/// A finished test's record.
pub fn record(results: Vec<Measured>) -> Record {
    Record { winner: best(&results), results, stale: false, offered: true }
}

/// What Automatic uses instead of OpenGL: the last test's winner, while the card and driver are the same.
pub fn auto_pick(r: Option<&Record>) -> Option<Graphics> {
    let r = r?;
    if r.stale { return None }
    r.winner.filter(|w| *w != Graphics::Auto)
}

/// The card or driver this start reports (`adapter`, drawing with `using`) differs from what the test saw with the
/// same choice. A choice the test didn't run, or an empty report, says nothing.
pub fn driver_changed(r: &Record, using: Graphics, adapter: &str) -> bool {
    let using = if using == Graphics::Auto { Graphics::OpenGl } else { using };
    if adapter.is_empty() { return false }
    r.results.iter().find(|m| m.graphics == using && m.started && !m.adapter.is_empty()).is_some_and(|m| m.adapter != adapter)
}

/// Why the test is offered at this start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer { No, First, DriverChanged }

pub fn offer(r: Option<&Record>, using: Graphics, adapter: &str) -> Offer {
    match r {
        None => Offer::First,
        Some(r) if !r.offered => Offer::First,
        Some(r) if !r.stale && driver_changed(r, using, adapter) => Offer::DriverChanged,
        _ => Offer::No,
    }
}

/// Keeps that the test was offered (and, after a card or driver change, that the old winner is stale).
pub fn mark_offered(root: &Path, why: Offer) {
    let mut r = load(root).unwrap_or_default();
    r.offered = true;
    if why == Offer::DriverChanged { r.stale = true }
    save(root, &r);
}

pub fn offer_text(why: Offer) -> String {
    match why {
        Offer::DriverChanged => tr!("The graphics card or its driver changed. Want me to find the best graphics for this computer again? About a minute"),
        _ => tr!("Want me to find the best graphics for this computer? About a minute"),
    }
}

pub fn button() -> String { tr!("Test which is fastest") }
pub fn not_now() -> String { tr!("Not now") }
pub fn nothing_worked() -> String { tr!("None of the graphics choices worked in the test. RustyPresenter keeps the current setting.") }
pub fn done_text(winner: Graphics) -> String {
    tr!("{mode} is the best for this computer. Automatic will use it from the next start.", mode = super::graphics::name(winner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use Graphics::*;
    use std::{cell::Cell, rc::Rc};

    /// A run that worked: its slow-end slide change, hiccups a minute, longest pause and processor.
    fn ran(graphics: Graphics, delay: f32, hiccups: Option<f32>, pause: f32, cpu: Option<f32>) -> Measured {
        Measured {
            graphics, started: true, changes: 10, seen: 10, delay_ms: delay * 0.7, delay_slow_ms: delay, hiccups_min: hiccups,
            freezes: 0, freeze_ms: pause, draw_ms: Some(4.0), fps: 60.0, cpu, adapter: "Card 1.0".into(), ..Measured::default()
        }
    }
    /// A smooth run with this slide change time.
    fn smooth(g: Graphics, delay: f32) -> Measured { ran(g, delay, Some(0.0), 20.0, None) }

    #[test]
    fn a_change_is_timed_from_the_command_to_the_first_frame_showing_it() {
        // Sent at 0 and 100; put up at 30 and 160; frames at 10, 45, 170.
        assert_eq!(change_delays(&[0.0, 100.0], &[30.0, 160.0], Some(&[10.0, 45.0, 170.0])), [45.0, 70.0]);
        assert_eq!(change_delays(&[0.0, 100.0], &[30.0, 160.0], None), [30.0, 60.0], "no frames counted: until it was put up");
        assert_eq!(change_delays(&[0.0, 200.0], &[30.0], Some(&[40.0])), [40.0], "a change never put up is left out");
        assert_eq!(change_delays(&[0.0], &[30.0], Some(&[10.0])), Vec::<f64>::new(), "no frame after it yet");
    }

    #[test]
    fn hiccups_are_late_frames_in_a_crossfade_or_video_stalls() {
        // A 400 ms crossfade at 60 Hz from 0, one frame 50 ms late at 200, then idle for a second (not a hiccup).
        let mut f: Vec<f64> = (0..=24).map(|i| i as f64 * 16.7).filter(|t| (*t - 200.4).abs() > 30.0).collect();
        f.push(1500.0);
        assert_eq!(hiccups(&f, &[0.0], 400.0, false), 1);
        let even: Vec<f64> = (0..=24).map(|i| i as f64 * 16.7).collect();
        assert_eq!(hiccups(&even, &[0.0], 400.0, false), 0);
        // A 144 Hz monitor missing one refresh (14 ms) is not counted.
        let fast: Vec<f64> = (0..=56).map(|i| i as f64 * 6.9).filter(|t| (*t - 200.1).abs() > 1.0).collect();
        assert_eq!(hiccups(&fast, &[0.0], 400.0, false), 0);
        // Video at 25 fps: a 300 ms stall outside a crossfade counts only with video.
        let video = [1000.0, 1040.0, 1080.0, 1380.0, 1420.0];
        assert_eq!((hiccups(&video, &[0.0], 400.0, true), hiccups(&video, &[0.0], 400.0, false)), (1, 0));
        // Video frames every 40 ms, then a crossfade at 60 Hz from 1000: the gap into its first frame is not late.
        let mut f: Vec<f64> = (0..25).map(|i| i as f64 * 40.0).collect();
        f.extend((0..=24).map(|i| 1005.0 + i as f64 * 16.7));
        assert_eq!(hiccups(&f, &[1000.0], 400.0, true), 0, "found on a real PC: every change counted once");
        f.extend([1450.0, 1466.7]);
        assert_eq!(hiccups(&f, &[1000.0, 1440.0], 400.0, true), 0, "the next crossfade's first gap too");
    }

    #[test]
    fn freezes_are_ui_pauses_over_50_ms() {
        assert_eq!(freezes(&[1.0, 16.0, 51.0, 3.0, 300.0]), (2, 300.0));
        assert_eq!(freezes(&[]), (0, 0.0));
        assert_eq!(fps(&(0..=60).map(|i| i as f64 * 1000.0 / 60.0).collect::<Vec<_>>()).round(), 60.0);
        assert_eq!(fps(&[5.0]), 0.0);
        assert_eq!(percentile(&[5.0, 1.0, 3.0], 0.5), 3.0);
    }

    #[test]
    fn each_run_gets_a_plain_word() {
        assert_eq!(verdict(&smooth(OpenGl, 80.0)), Verdict::Smooth);
        assert_eq!(verdict(&smooth(OpenGl, 200.0)), Verdict::Ok, "slow slide changes");
        assert_eq!(verdict(&ran(OpenGl, 80.0, Some(20.0), 20.0, None)), Verdict::Ok, "some hiccups");
        assert_eq!(verdict(&ran(OpenGl, 80.0, Some(0.0), 180.0, None)), Verdict::Ok, "a pause");
        assert_eq!(verdict(&ran(OpenGl, 80.0, None, 20.0, None)), Verdict::Ok, "frames not counted: at best OK");
        assert_eq!(verdict(&smooth(OpenGl, 400.0)), Verdict::Choppy);
        assert_eq!(verdict(&ran(OpenGl, 80.0, Some(60.0), 20.0, None)), Verdict::Choppy);
        assert_eq!(verdict(&ran(OpenGl, 80.0, Some(0.0), 600.0, None)), Verdict::Choppy);
        assert_eq!(verdict(&Measured::failed(Vulkan, "crashed")), Verdict::Failed);
        assert_eq!(verdict(&Measured { seen: 0, ..smooth(Vulkan, 80.0) }), Verdict::Failed, "no slide reached the room");
        assert_eq!(word(Verdict::Failed), "Didn't work");
        assert_eq!(word(Verdict::Smooth), "Smooth");
        assert_eq!(word(Verdict::Choppy), "Choppy");
        assert_eq!(word(Verdict::Ok), "OK");
    }

    #[test]
    fn the_score_wants_quick_changes_no_hiccups_or_pauses_and_an_idle_processor() {
        assert!(score(&smooth(Vulkan, 60.0)) > score(&smooth(OpenGl, 140.0)));
        assert!(score(&ran(Vulkan, 60.0, Some(0.0), 20.0, None)) > score(&ran(OpenGl, 60.0, Some(5.0), 20.0, None)));
        assert!(score(&ran(Vulkan, 60.0, Some(0.0), 20.0, None)) > score(&ran(OpenGl, 60.0, Some(0.0), 90.0, None)));
        assert!(score(&ran(Vulkan, 60.0, Some(0.0), 20.0, Some(10.0))) > score(&ran(OpenGl, 60.0, Some(0.0), 20.0, Some(90.0))));
        assert!(score(&smooth(Vulkan, 60.0)) > score(&Measured { draw_ms: Some(14.0), ..smooth(OpenGl, 60.0) }), "slow room frames");
        assert_eq!(score(&Measured::failed(OpenGl, "x")), 0.0);
        assert!(score(&ran(OpenGl, 5000.0, Some(500.0), 5000.0, Some(400.0))) >= 1.0, "a working run never scores 0");
    }

    #[test]
    fn the_best_is_the_smoothest_and_near_ties_keep_opengl() {
        assert_eq!(best(&[smooth(OpenGl, 200.0), smooth(Vulkan, 60.0), Measured::failed(Direct3D, "x")]), Some(Vulkan));
        assert_eq!(best(&[smooth(Vulkan, 60.0), smooth(OpenGl, 66.0)]), Some(OpenGl), "within 5 %");
        assert_eq!(best(&[Measured::failed(OpenGl, "x"), smooth(Vulkan, 400.0)]), Some(Vulkan), "the only one that worked");
        assert_eq!(best(&[Measured::failed(OpenGl, "x"), Measured::failed(Vulkan, "y")]), None);
        // A Smooth run beats a higher-scoring one that pauses for a long time.
        assert_eq!(best(&[ran(Vulkan, 60.0, Some(0.0), 140.0, None), smooth(Direct3D, 100.0)]), Some(Direct3D));
    }

    #[test]
    fn software_wins_only_when_no_graphics_card_choice_is_ok() {
        let sw = ran(Software, 60.0, None, 20.0, None);
        assert_eq!(best(&[smooth(OpenGl, 250.0), sw.clone()]), Some(OpenGl));
        assert_eq!(best(&[ran(OpenGl, 500.0, Some(80.0), 600.0, None), sw.clone()]), Some(Software));
        assert_eq!(best(&[Measured::failed(OpenGl, "x"), sw]), Some(Software));
    }

    #[test]
    fn each_choice_runs_twice_in_turned_round_order_and_keeps_its_better_run() {
        assert_eq!(schedule(&[OpenGl, Vulkan, Direct3D, Software]), [OpenGl, Vulkan, Direct3D, Software, Direct3D, Vulkan, OpenGl]);
        let runs = vec![smooth(OpenGl, 200.0), Measured::failed(Vulkan, "took too long"), smooth(Vulkan, 70.0), smooth(OpenGl, 80.0), smooth(Software, 90.0)];
        let got = combine(&[OpenGl, Vulkan, Software], runs);
        assert_eq!(got.iter().map(|m| (m.graphics, m.delay_slow_ms)).collect::<Vec<_>>(), [(OpenGl, 80.0), (Vulkan, 70.0), (Software, 90.0)]);
        assert_eq!(better(Measured::failed(OpenGl, "x"), smooth(OpenGl, 400.0)).delay_slow_ms, 400.0, "a run that worked beats one that didn't");
        assert_eq!(combine(&[OpenGl, Metal], vec![smooth(OpenGl, 80.0)]).len(), 1, "a choice with no run is left out");
    }

    #[test]
    fn the_list_shows_bars_words_details_and_a_star() {
        let r = rows(&[smooth(OpenGl, 200.0), ran(Vulkan, 60.0, Some(2.0), 20.0, Some(31.6)), Measured::failed(Software, "took too long")]);
        assert_eq!(r.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["OpenGL", "Vulkan", "Software"]);
        assert_eq!(r.iter().map(|r| r.best).collect::<Vec<_>>(), [false, true, false]);
        assert_eq!(r[1].bar, 1.0);
        assert!(r[0].bar > 0.0 && r[0].bar < 1.0);
        assert_eq!((r[2].bar, r[2].word.as_str(), r[2].works), (0.0, "Didn't work", false));
        assert_eq!(r[1].word, "Smooth");
        assert_eq!(r[1].details, "Slide change 60 ms · 2 hiccups a minute · longest pause 20 ms · processor 32 %");
        assert_eq!(r[2].details, "It stopped responding.");
        assert_eq!(details(&ran(Software, 60.0, None, 20.0, None)), "Slide change 60 ms · longest pause 20 ms", "nothing unmeasured shown");
    }

    #[test]
    fn a_failure_says_why_in_plain_words() {
        assert_eq!(reason(&Measured::failed(Vulkan, "could not start: no file")), "It could not start.");
        assert_eq!(reason(&Measured::failed(Vulkan, "stopped without a result")), "It closed before it finished. It may have crashed.");
        assert_eq!(reason(&Measured { started: true, ..Measured::failed(Vulkan, "") }), "Slides never reached the audience screen.");
        assert_eq!(reason(&Measured::failed(Vulkan, "No Vulkan adapter")), "This graphics card or its driver can't draw with it.");
    }

    #[test]
    fn automatic_is_not_tested_twice() {
        assert_eq!(choices(&[Auto, OpenGl, Vulkan, Software]), [OpenGl, Vulkan, Software]);
    }

    #[test]
    fn the_child_reads_its_flags() {
        let a = |v: &[&str]| child_args(v.iter().map(OsString::from));
        assert_eq!(a(&["--graphics-bench", "vulkan", "--bench-out", "/tmp/x.json"]), Some(ChildArgs { graphics: Vulkan, out: "/tmp/x.json".into(), room: None }));
        assert_eq!(a(&["--bench-out", "o", "--graphics-bench", "opengl", "--bench-room", "auto"]), Some(ChildArgs { graphics: OpenGl, out: "o".into(), room: Some("auto".into()) }));
        assert_eq!(a(&["--app", "--restore"]), None);
        assert_eq!(a(&["--graphics-bench", "vulkan"]), None, "nowhere to write");
        assert_eq!(arg(Direct3D), "direct3d");
        for g in crate::video::graphics::ALL { assert_eq!(a(&[FLAG, &arg(g), OUT_FLAG, "o"]).unwrap().graphics, g) }
    }

    #[test]
    fn the_test_uses_the_projector_only_while_the_room_is_closed() {
        assert_eq!(room_arg(true, Some("DELL 1")), None);
        assert_eq!(room_arg(false, Some("DELL 1")), Some("DELL 1".into()));
        assert_eq!(room_arg(false, None), Some("auto".into()));
        assert_eq!(room_arg(false, Some("")), Some("auto".into()));
    }

    /// A child that ends after `polls` looks (writing `json` first), or never (`None`).
    struct Fake { polls: Option<u32>, json: Option<String>, killed: Rc<Cell<bool>>, launched: Rc<Cell<u32>> }
    struct FakeChild { left: Option<u32>, out: PathBuf, json: Option<String>, killed: Rc<Cell<bool>> }

    impl Child for FakeChild {
        fn done(&mut self) -> bool {
            match &mut self.left {
                Some(0) => { if let Some(j) = &self.json { std::fs::write(&self.out, j).unwrap() } true }
                Some(n) => { *n -= 1; false }
                None => false,
            }
        }
        fn kill(&mut self) { self.killed.set(true) }
    }

    impl Launch for Fake {
        type C = FakeChild;
        fn launch(&self, _g: Graphics, out: &Path) -> io::Result<FakeChild> {
            self.launched.set(self.launched.get() + 1);
            Ok(FakeChild { left: self.polls, out: out.into(), json: self.json.clone(), killed: self.killed.clone() })
        }
    }

    fn fake(polls: Option<u32>, json: Option<&str>) -> Fake {
        Fake { polls, json: json.map(str::to_string), killed: Rc::default(), launched: Rc::default() }
    }

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("rp-bench-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn a_child_that_writes_its_result_is_read() {
        let d = dir();
        let json = serde_json::to_string(&smooth(OpenGl, 58.0)).unwrap();
        let f = fake(Some(3), Some(&json));
        let m = run_one(&f, Vulkan, &d.join("o.json"), Duration::from_secs(5), MS);
        assert_eq!(m, smooth(Vulkan, 58.0), "named for what was asked");
        assert!(!f.killed.get());
        assert!(!d.join("o.json").exists(), "cleaned up");
    }

    #[test]
    fn a_child_that_hangs_is_killed_and_did_not_work() {
        let d = dir();
        let f = fake(None, None);
        let m = run_one(&f, Vulkan, &d.join("o.json"), Duration::from_millis(30), MS);
        assert!(f.killed.get());
        assert_eq!(verdict(&m), Verdict::Failed);
        assert_eq!(m.error, "took too long");
    }

    #[test]
    fn a_child_that_crashes_or_writes_rubbish_did_not_work() {
        let d = dir();
        assert_eq!(verdict(&run_one(&fake(Some(0), None), OpenGl, &d.join("a.json"), Duration::from_secs(5), MS)), Verdict::Failed);
        let m = run_one(&fake(Some(0), Some("not json")), OpenGl, &d.join("b.json"), Duration::from_secs(5), MS);
        assert_eq!((m.graphics, m.error.as_str()), (OpenGl, "stopped without a result"));
        struct NoStart;
        impl Launch for NoStart {
            type C = FakeChild;
            fn launch(&self, _: Graphics, _: &Path) -> io::Result<FakeChild> { Err(io::Error::other("no")) }
        }
        assert_eq!(verdict(&run_one(&NoStart, OpenGl, &d.join("c.json"), Duration::from_secs(5), MS)), Verdict::Failed);
    }

    #[test]
    fn every_choice_runs_one_at_a_time_twice_and_keeps_the_better() {
        let d = dir();
        let json = serde_json::to_string(&smooth(OpenGl, 60.0)).unwrap();
        let f = fake(Some(1), Some(&json));
        let mut seen = vec![];
        let all = run_all(&f, &[OpenGl, Vulkan, Software], &d, Duration::from_secs(5), MS, |i, n, g| seen.push((i, n, g)));
        assert_eq!(seen, [(0, 5, OpenGl), (1, 5, Vulkan), (2, 5, Software), (3, 5, Vulkan), (4, 5, OpenGl)]);
        assert_eq!(all.iter().map(|m| m.graphics).collect::<Vec<_>>(), [OpenGl, Vulkan, Software]);
        assert_eq!(f.launched.get(), 5);
        assert_eq!(testing(Vulkan, 1, 5), "Testing Vulkan… (2 of 5)");
    }

    #[test]
    fn a_real_child_process_is_waited_for_or_killed() {
        if !cfg!(unix) { return }
        // `sh` stands in for the app: one writes a result, one hangs.
        struct Sh(&'static str);
        impl Launch for Sh {
            type C = std::process::Child;
            fn launch(&self, _: Graphics, out: &Path) -> io::Result<std::process::Child> {
                std::process::Command::new("sh").arg("-c").arg(self.0).arg("sh").arg(out).spawn()
            }
        }
        let d = dir();
        let m = run_one(&Sh(r#"printf '{"started":true,"seen":3,"delay_slow_ms":70}' > "$1""#), OpenGl, &d.join("a.json"), Duration::from_secs(10), MS);
        assert_eq!((m.started, m.seen, m.delay_slow_ms), (true, 3, 70.0));
        let t = Instant::now();
        let m = run_one(&Sh("sleep 30"), OpenGl, &d.join("b.json"), Duration::from_millis(200), MS);
        assert_eq!(m.error, "took too long");
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn automatic_uses_the_winner_until_the_driver_changes() {
        assert_eq!(auto_pick(None), None);
        let r = record(vec![smooth(OpenGl, 200.0), smooth(Vulkan, 60.0)]);
        assert_eq!(r.winner, Some(Vulkan));
        assert_eq!(auto_pick(Some(&r)), Some(Vulkan));
        assert_eq!(auto_pick(Some(&Record { stale: true, ..r.clone() })), None);
        assert_eq!(auto_pick(Some(&record(vec![Measured::failed(OpenGl, "x")]))), None);
    }

    #[test]
    fn an_old_record_still_reads() {
        // Written by the first version of the test (fps and worst 1 %, no slide changes).
        let old = r#"{"results":[{"graphics":"vulkan","started":true,"fps":60.0,"low_ms":17.0,"cpu":null,"adapter":"Vk card","error":""}],"winner":"vulkan","stale":false,"offered":true}"#;
        let d = dir();
        std::fs::write(file(&d), old).unwrap();
        let r = load(&d).expect("reads");
        assert_eq!(auto_pick(Some(&r)), Some(Vulkan));
        assert!(driver_changed(&r, Vulkan, "Other card"));
    }

    #[test]
    fn a_new_card_or_driver_is_noticed_only_with_the_same_choice() {
        let r = record(vec![smooth(OpenGl, 200.0), Measured { adapter: "Vk card 2".into(), ..smooth(Vulkan, 60.0) }]);
        assert!(!driver_changed(&r, Vulkan, "Vk card 2"));
        assert!(driver_changed(&r, Vulkan, "Vk card 3"));
        assert!(driver_changed(&r, Auto, "Other GL"), "Automatic without a winner is OpenGL");
        assert!(!driver_changed(&r, OpenGl, "Card 1.0"));
        assert!(!driver_changed(&r, Software, "Software"), "not tested");
        assert!(!driver_changed(&r, Vulkan, ""), "nothing heard");
    }

    #[test]
    fn the_test_is_offered_once_and_again_after_a_driver_change() {
        assert_eq!(offer(None, Auto, ""), Offer::First);
        let declined = Record { offered: true, ..Record::default() };
        assert_eq!(offer(Some(&declined), Auto, "Any"), Offer::No);
        let r = record(vec![smooth(OpenGl, 60.0)]);
        assert_eq!(offer(Some(&r), OpenGl, "Card 1.0"), Offer::No);
        assert_eq!(offer(Some(&r), OpenGl, "Card 2.0"), Offer::DriverChanged);
        assert_eq!(offer(Some(&Record { stale: true, ..r }), OpenGl, "Card 2.0"), Offer::No, "asked already");
    }

    #[test]
    fn the_record_is_kept_in_the_data_folder() {
        let d = dir();
        assert_eq!(load(&d), None);
        mark_offered(&d, Offer::First);
        assert_eq!(load(&d), Some(Record { offered: true, ..Record::default() }));
        let r = record(vec![smooth(Vulkan, 60.0)]);
        save(&d, &r);
        assert_eq!(load(&d), Some(r));
        mark_offered(&d, Offer::DriverChanged);
        let after = load(&d).unwrap();
        assert!(after.stale);
        assert_eq!(auto_pick(Some(&after)), None);
        std::fs::write(file(&d), "nope").unwrap();
        assert_eq!(load(&d), None);
    }
}
