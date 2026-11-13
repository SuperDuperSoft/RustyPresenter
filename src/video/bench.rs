//! Settings → Video → "Test which is fastest": each graphics choice this computer has is tried in a child process of
//! the app (`rustypresenter --graphics-bench <choice> --bench-out <file>`, `ui::graphics_test`), because the backend
//! is chosen once per process. Each child draws a Sunday load for a few seconds and writes what it measured
//! (`Measured`) as JSON. A child that crashes, hangs (killed after `LIMIT`) or writes nothing "Didn't work".
//!
//! The results are kept in `<data>/graphics-test.json` (`Record`) with the graphics card's name and driver as each
//! choice reported them. Automatic then uses the winner (`auto_pick`) instead of OpenGL, until the card or driver
//! changes (`driver_changed`), which marks the record stale and offers the test again (`offer`).
use crate::{model::Graphics, tr};
use serde::{Deserialize, Serialize};
use std::{ffi::OsString, io, path::{Path, PathBuf}, time::{Duration, Instant}};

/// The child's flags.
pub const FLAG: &str = "--graphics-bench";
pub const OUT_FLAG: &str = "--bench-out";
/// A child still running after this is killed.
pub const LIMIT: Duration = Duration::from_secs(15);

/// What one child measured.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Measured {
    pub graphics: Graphics,
    /// The windows opened and drew.
    pub started: bool,
    /// The room window's frames a second.
    pub fps: f32,
    /// The worst 1 % of the room's frame times (ms): the stutters.
    pub low_ms: f32,
    /// The process's CPU use, % of one core (`None` where it can't be read).
    pub cpu: Option<f32>,
    /// The graphics card's name and driver, as this choice reports it.
    pub adapter: String,
    /// Why it didn't work, for the log.
    pub error: String,
}

impl Measured {
    pub fn failed(graphics: Graphics, error: impl Into<String>) -> Measured {
        Measured { graphics, error: error.into(), ..Measured::default() }
    }
    fn works(&self) -> bool { self.started && self.fps > 0.0 }
}

/// Frames a second and the mean of the worst 1 % of frame times (at least one frame; ms), from the times frames
/// ended (ms, in order).
pub fn frame_stats(ends_ms: &[f64]) -> (f32, f32) {
    if ends_ms.len() < 2 { return (0.0, 0.0) }
    let span = ends_ms[ends_ms.len() - 1] - ends_ms[0];
    if span <= 0.0 { return (0.0, 0.0) }
    let mut gaps: Vec<f64> = ends_ms.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.sort_by(|a, b| b.total_cmp(a));
    let worst = (gaps.len() / 100).max(1);
    let low = gaps[..worst].iter().sum::<f64>() / worst as f64;
    (((ends_ms.len() - 1) as f64 / span * 1000.0) as f32, low as f32)
}

/// The word shown beside each choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict { Smooth, Ok, Choppy, Failed }

pub fn verdict(m: &Measured) -> Verdict {
    if !m.works() { return Verdict::Failed }
    if m.fps >= 50.0 && m.low_ms <= 40.0 { Verdict::Smooth } else if m.fps >= 28.0 && m.low_ms <= 80.0 { Verdict::Ok } else { Verdict::Choppy }
}

pub fn word(v: Verdict) -> String {
    match v {
        Verdict::Smooth => tr!("Smooth"),
        Verdict::Ok => tr!("OK"),
        Verdict::Choppy => tr!("Choppy"),
        Verdict::Failed => tr!("Didn't work"),
    }
}

/// How good a run was: mostly frames a second, some for the stutters, less for a busy processor. 0 when it failed.
pub fn score(m: &Measured) -> f32 {
    if !m.works() { return 0.0 }
    let steady = if m.low_ms > 0.0 { 1000.0 / m.low_ms } else { m.fps };
    (0.7 * m.fps.min(240.0) + 0.3 * steady.min(240.0) - m.cpu.unwrap_or(0.0) / 10.0).max(0.1)
}

/// Among runs this close to the best (share of its score), the steadier choice wins.
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

/// The best choice: the highest verdict, then the score; near ties go to `preference`. `None` when nothing worked.
pub fn best(results: &[Measured]) -> Option<Graphics> {
    let rank = |m: &Measured| match verdict(m) { Verdict::Smooth => 3, Verdict::Ok => 2, Verdict::Choppy => 1, Verdict::Failed => 0 };
    let top = results.iter().map(rank).max().filter(|r| *r > 0)?;
    let tier: Vec<&Measured> = results.iter().filter(|m| rank(m) == top).collect();
    let high = tier.iter().map(|m| score(m)).fold(0.0, f32::max);
    tier.into_iter().filter(|m| score(m) >= high * (1.0 - CLOSE)).min_by_key(|m| preference(m.graphics)).map(|m| m.graphics)
}

/// One line of the results list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row { pub graphics: Graphics, pub name: String, pub bar: f32, pub word: String, pub best: bool, pub works: bool }

/// The results list: each choice with a bar (share of the best score), its word and a star on the best.
pub fn rows(results: &[Measured]) -> Vec<Row> {
    let winner = best(results);
    let high = results.iter().map(score).fold(0.0, f32::max);
    results.iter().map(|m| Row {
        graphics: m.graphics,
        name: super::graphics::name(m.graphics),
        bar: if high > 0.0 { score(m) / high } else { 0.0 },
        word: word(verdict(m)),
        best: Some(m.graphics) == winner,
        works: m.works(),
    }).collect()
}

/// The choices to test: every one this build offers but Automatic (which is OpenGL until a test picks).
pub fn choices(available: &[Graphics]) -> Vec<Graphics> {
    available.iter().copied().filter(|g| *g != Graphics::Auto).collect()
}

// ---- The child's command line ----

/// `--graphics-bench <choice> --bench-out <file>`.
#[derive(Clone, Debug, PartialEq)]
pub struct ChildArgs { pub graphics: Graphics, pub out: PathBuf }

/// The child's arguments, when this process is one. A choice that can't be read still runs (and fails) as asked,
/// so the parent hears back.
pub fn child_args(args: impl IntoIterator<Item = OsString>) -> Option<ChildArgs> {
    let (mut graphics, mut out, mut asked) = (None, None, false);
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        if a == FLAG { asked = true; graphics = it.next().and_then(|g| serde_json::from_value(serde_json::Value::String(g.to_string_lossy().into())).ok()) }
        else if a == OUT_FLAG { out = it.next().map(PathBuf::from) }
    }
    if !asked { return None }
    Some(ChildArgs { graphics: graphics.unwrap_or(Graphics::Software), out: out? })
}

/// A choice as the child's argument (`vulkan`, the saved setting's spelling).
pub fn arg(g: Graphics) -> String {
    serde_json::to_value(g).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
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

/// This executable, with the child's flags.
pub struct Exe(pub PathBuf);

impl Launch for Exe {
    type C = std::process::Child;
    fn launch(&self, g: Graphics, out: &Path) -> io::Result<std::process::Child> {
        use std::process::{Command, Stdio};
        Command::new(&self.0).arg(FLAG).arg(arg(g)).arg(OUT_FLAG).arg(out)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
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

/// Runs every choice one after another; `progress(i, g)` before each.
pub fn run_all(l: &impl Launch, choices: &[Graphics], dir: &Path, limit: Duration, poll: Duration, mut progress: impl FnMut(usize, Graphics)) -> Vec<Measured> {
    choices.iter().enumerate().map(|(i, g)| {
        progress(i, *g);
        run_one(l, *g, &dir.join(format!("graphics-test-{}.json", arg(*g))), limit, poll)
    }).collect()
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
        Offer::DriverChanged => tr!("The graphics card or its driver changed. Want me to find the best graphics for this PC again? About 30 seconds"),
        _ => tr!("Want me to find the best graphics for this PC? About 30 seconds"),
    }
}

pub fn button() -> String { tr!("Test which is fastest") }
pub fn not_now() -> String { tr!("Not now") }
pub fn nothing_worked() -> String { tr!("None of the graphics choices worked in the test. RustyPresenter keeps the current setting.") }
pub fn done_text(winner: Graphics) -> String {
    tr!("{mode} is the best for this PC. Automatic will use it from the next start.", mode = super::graphics::name(winner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use Graphics::*;
    use std::{cell::Cell, rc::Rc};

    fn ran(graphics: Graphics, fps: f32, low_ms: f32, cpu: Option<f32>) -> Measured {
        Measured { graphics, started: true, fps, low_ms, cpu, adapter: "Card 1.0".into(), error: String::new() }
    }

    #[test]
    fn frame_stats_give_the_rate_and_the_worst_stutters() {
        let even: Vec<f64> = (0..=60).map(|i| i as f64 * 1000.0 / 60.0).collect();
        let (fps, low) = frame_stats(&even);
        assert!((fps - 60.0).abs() < 0.01, "{fps}");
        assert!((low - 16.67).abs() < 0.01, "{low}");
        // One 200 ms hitch among 100 frames is the worst 1 %.
        let mut t = 0.0;
        let mut hitchy = vec![0.0];
        for i in 0..100 { t += if i == 50 { 200.0 } else { 10.0 }; hitchy.push(t) }
        assert_eq!(frame_stats(&hitchy).1, 200.0);
        assert_eq!(frame_stats(&[]), (0.0, 0.0));
        assert_eq!(frame_stats(&[5.0]), (0.0, 0.0));
    }

    #[test]
    fn each_run_gets_a_plain_word() {
        assert_eq!(verdict(&ran(OpenGl, 60.0, 20.0, None)), Verdict::Smooth);
        assert_eq!(verdict(&ran(OpenGl, 60.0, 60.0, None)), Verdict::Ok, "stutters");
        assert_eq!(verdict(&ran(OpenGl, 35.0, 30.0, None)), Verdict::Ok);
        assert_eq!(verdict(&ran(OpenGl, 15.0, 90.0, None)), Verdict::Choppy);
        assert_eq!(verdict(&Measured::failed(Vulkan, "crashed")), Verdict::Failed);
        assert_eq!(verdict(&Measured { started: true, ..Measured::failed(Vulkan, "") }), Verdict::Failed, "no frames");
        assert_eq!(word(Verdict::Failed), "Didn't work");
        assert_eq!(word(Verdict::Smooth), "Smooth");
        assert_eq!(word(Verdict::Choppy), "Choppy");
        assert_eq!(word(Verdict::Ok), "OK");
    }

    #[test]
    fn the_score_wants_frames_steadiness_and_an_idle_processor() {
        assert!(score(&ran(Vulkan, 60.0, 17.0, None)) > score(&ran(OpenGl, 45.0, 17.0, None)));
        assert!(score(&ran(Vulkan, 60.0, 17.0, None)) > score(&ran(OpenGl, 60.0, 50.0, None)));
        assert!(score(&ran(Vulkan, 60.0, 17.0, Some(10.0))) > score(&ran(OpenGl, 60.0, 17.0, Some(90.0))));
        assert_eq!(score(&Measured::failed(OpenGl, "x")), 0.0);
    }

    #[test]
    fn the_best_is_the_smoothest_and_near_ties_keep_opengl() {
        assert_eq!(best(&[ran(OpenGl, 40.0, 30.0, None), ran(Vulkan, 60.0, 17.0, None), ran(Software, 20.0, 60.0, None)]), Some(Vulkan));
        assert_eq!(best(&[ran(Vulkan, 60.0, 17.0, None), ran(OpenGl, 59.0, 17.0, None)]), Some(OpenGl), "within 5 %");
        assert_eq!(best(&[ran(Software, 60.0, 17.0, None), ran(Vulkan, 59.0, 17.0, None)]), Some(Vulkan), "Software never wins a tie");
        assert_eq!(best(&[Measured::failed(OpenGl, "x"), ran(Software, 12.0, 120.0, None)]), Some(Software), "the only one that worked");
        assert_eq!(best(&[Measured::failed(OpenGl, "x"), Measured::failed(Vulkan, "y")]), None);
        // A Smooth run beats a choppy one with more frames but bad stutters.
        assert_eq!(best(&[ran(Vulkan, 90.0, 120.0, None), ran(Direct3D, 55.0, 25.0, None)]), Some(Direct3D));
    }

    #[test]
    fn the_list_shows_bars_words_and_a_star() {
        let r = rows(&[ran(OpenGl, 30.0, 40.0, None), ran(Vulkan, 60.0, 17.0, None), Measured::failed(Software, "x")]);
        assert_eq!(r.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["OpenGL", "Vulkan", "Software"]);
        assert_eq!(r.iter().map(|r| r.best).collect::<Vec<_>>(), [false, true, false]);
        assert_eq!(r[1].bar, 1.0);
        assert!(r[0].bar > 0.0 && r[0].bar < 1.0);
        assert_eq!((r[2].bar, r[2].word.as_str(), r[2].works), (0.0, "Didn't work", false));
        assert_eq!(r[1].word, "Smooth");
    }

    #[test]
    fn automatic_is_not_tested_twice() {
        assert_eq!(choices(&[Auto, OpenGl, Vulkan, Software]), [OpenGl, Vulkan, Software]);
    }

    #[test]
    fn the_child_reads_its_flags() {
        let a = |v: &[&str]| child_args(v.iter().map(OsString::from));
        assert_eq!(a(&["--graphics-bench", "vulkan", "--bench-out", "/tmp/x.json"]), Some(ChildArgs { graphics: Vulkan, out: "/tmp/x.json".into() }));
        assert_eq!(a(&["--bench-out", "o", "--graphics-bench", "opengl"]), Some(ChildArgs { graphics: OpenGl, out: "o".into() }));
        assert_eq!(a(&["--app", "--restore"]), None);
        assert_eq!(a(&["--graphics-bench", "vulkan"]), None, "nowhere to write");
        assert_eq!(arg(Direct3D), "direct3d");
        for g in crate::video::graphics::ALL { assert_eq!(a(&[FLAG, &arg(g), OUT_FLAG, "o"]).unwrap().graphics, g) }
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
        let json = serde_json::to_string(&ran(OpenGl, 58.0, 20.0, Some(30.0))).unwrap();
        let f = fake(Some(3), Some(&json));
        let m = run_one(&f, Vulkan, &d.join("o.json"), Duration::from_secs(5), MS);
        assert_eq!(m, ran(Vulkan, 58.0, 20.0, Some(30.0)), "named for what was asked");
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
    fn every_choice_runs_one_at_a_time_in_order() {
        let d = dir();
        let json = serde_json::to_string(&ran(OpenGl, 60.0, 17.0, None)).unwrap();
        let f = fake(Some(1), Some(&json));
        let mut seen = vec![];
        let all = run_all(&f, &[OpenGl, Vulkan, Software], &d, Duration::from_secs(5), MS, |i, g| seen.push((i, g)));
        assert_eq!(seen, [(0, OpenGl), (1, Vulkan), (2, Software)]);
        assert_eq!(all.iter().map(|m| m.graphics).collect::<Vec<_>>(), [OpenGl, Vulkan, Software]);
        assert_eq!(f.launched.get(), 3);
        assert_eq!(testing(Vulkan, 1, 3), "Testing Vulkan… (2 of 3)");
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
        let m = run_one(&Sh(r#"printf '{"started":true,"fps":60,"low_ms":17}' > "$1""#), OpenGl, &d.join("a.json"), Duration::from_secs(10), MS);
        assert_eq!((m.started, m.fps), (true, 60.0));
        let t = Instant::now();
        let m = run_one(&Sh("sleep 30"), OpenGl, &d.join("b.json"), Duration::from_millis(200), MS);
        assert_eq!(m.error, "took too long");
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn automatic_uses_the_winner_until_the_driver_changes() {
        assert_eq!(auto_pick(None), None);
        let r = record(vec![ran(OpenGl, 40.0, 30.0, None), ran(Vulkan, 60.0, 17.0, None)]);
        assert_eq!(r.winner, Some(Vulkan));
        assert_eq!(auto_pick(Some(&r)), Some(Vulkan));
        assert_eq!(auto_pick(Some(&Record { stale: true, ..r.clone() })), None);
        assert_eq!(auto_pick(Some(&record(vec![Measured::failed(OpenGl, "x")]))), None);
    }

    #[test]
    fn a_new_card_or_driver_is_noticed_only_with_the_same_choice() {
        let r = record(vec![ran(OpenGl, 40.0, 30.0, None), Measured { adapter: "Vk card 2".into(), ..ran(Vulkan, 60.0, 17.0, None) }]);
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
        let r = record(vec![ran(OpenGl, 60.0, 17.0, None)]);
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
        let r = record(vec![ran(Vulkan, 60.0, 17.0, None)]);
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
