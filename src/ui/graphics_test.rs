//! The graphics test's child (`rustypresenter --graphics-bench <choice> --bench-out <file>`, `video::bench`): draws
//! with exactly that choice, in three windows the sizes of the operator, room and stage (`ui/bench.slint`), for
//! `MEASURE` after `WARMUP`, and writes what it measured. It never touches the church's data.
use crate::{model::Graphics, ui::{BenchWindow, pacing}, video::{bench::{ChildArgs, Measured, frame_stats}, graphics}};
use slint::{ComponentHandle, PhysicalPosition, PhysicalSize, Rgba8Pixel, SharedPixelBuffer, Timer};
use std::{cell::{Cell, RefCell}, rc::Rc, time::{Duration, Instant}};

pub const WARMUP: Duration = Duration::from_millis(1500);
pub const MEASURE: Duration = Duration::from_secs(5);

/// Each window: its kind (`BenchWindow::kind`), label for `pacing`, size and place. Small enough for a laptop, the
/// room at its own refresh (as the app's room), the others not waiting for it.
const WINDOWS: [(i32, &str, (u32, u32), (i32, i32), bool); 3] = [
    (0, "operator", (1024, 640), (20, 20), false),
    (1, "room", (960, 540), (80, 80), true),
    (2, "stage", (640, 360), (140, 140), false),
];

const LYRICS: [&str; 6] = [
    "Amazing grace, how sweet the sound\nThat saved a wretch like me",
    "I once was lost, but now am found\nWas blind, but now I see",
    "'Twas grace that taught my heart to fear\nAnd grace my fears relieved",
    "How precious did that grace appear\nThe hour I first believed",
    "Through many dangers, toils and snares\nI have already come",
    "'Tis grace hath brought me safe thus far\nAnd grace will lead me home",
];

/// Runs the test for `args` and writes the result. The exit code is always 0: a choice that fails says so in the file
/// (a crash or hang is seen by the parent).
pub fn run(args: ChildArgs) -> i32 {
    let m = match graphics::choose_exact(args.graphics) {
        Ok(_) => measure(args.graphics).unwrap_or_else(|e| Measured::failed(args.graphics, e)),
        Err(e) => Measured::failed(args.graphics, e),
    };
    if !m.error.is_empty() { eprintln!("graphics test {:?}: {}", args.graphics, m.error) }
    if let Err(e) = std::fs::write(&args.out, serde_json::to_vec(&m).unwrap_or_default()) { eprintln!("graphics test: could not write {}: {e}", args.out.display()) }
    0
}

/// A colourful 640×360 background with soft bands, made here (no file needed).
fn background() -> slint::Image {
    let (w, h) = (640u32, 360u32);
    let mut px = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    for (i, p) in px.make_mut_slice().iter_mut().enumerate() {
        let (x, y) = ((i as u32 % w) as f32 / w as f32, (i as u32 / w) as f32 / h as f32);
        let wave = ((x * 9.0 + y * 4.0).sin() * 0.5 + 0.5) * 60.0;
        *p = Rgba8Pixel { r: (30.0 + x * 90.0 + wave) as u8, g: (20.0 + y * 60.0) as u8, b: (90.0 + (1.0 - x) * 110.0 + wave * 0.5) as u8, a: 255 };
    }
    slint::Image::from_rgba8(px)
}

/// The designed slide's picture that flies in: a framed badge.
fn picture() -> slint::Image {
    let (w, h) = (256u32, 128u32);
    let mut px = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    for (i, p) in px.make_mut_slice().iter_mut().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let edge = x < 6 || y < 6 || x >= w - 6 || y >= h - 6;
        *p = if edge { Rgba8Pixel { r: 255, g: 255, b: 255, a: 255 } } else { Rgba8Pixel { r: 245, g: 185, b: (74 + y / 2) as u8, a: 230 } };
    }
    slint::Image::from_rgba8(px)
}

/// CPU time this process has used.
pub fn cpu_time() -> Option<Duration> {
    #[cfg(unix)]
    {
        let mut u: libc::rusage = unsafe { std::mem::zeroed() };
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) } != 0 { return None }
        let t = |v: libc::timeval| Duration::from_secs(v.tv_sec as u64) + Duration::from_micros(v.tv_usec as u64);
        Some(t(u.ru_utime) + t(u.ru_stime))
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{Foundation::FILETIME, System::Threading::{GetCurrentProcess, GetProcessTimes}};
        let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut c, mut e, mut k, mut u) = (z, z, z, z);
        if GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) == 0 { return None }
        let t = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
        Some(Duration::from_nanos((t(k) + t(u)) * 100))
    }
    #[cfg(not(any(unix, windows)))]
    None
}

/// Frame end times of each window while measuring.
#[derive(Default)]
struct Frames { on: Cell<bool>, ends: RefCell<[Vec<Instant>; 3]> }

fn measure(g: Graphics) -> Result<Measured, String> {
    let frames = Rc::new(Frames::default());
    let (bg, pic) = (background(), picture());
    let lyrics: Vec<slint::SharedString> = LYRICS.iter().map(|l| (*l).into()).collect();
    let lyrics = slint::ModelRc::new(slint::VecModel::from(lyrics));
    let mut open = vec![];
    for (kind, label, (w, h), (x, y), vsync) in WINDOWS {
        let win = BenchWindow::new().map_err(|e| format!("could not open the {label} window: {e}"))?;
        win.set_kind(kind);
        win.set_bg(bg.clone());
        win.set_picture(pic.clone());
        win.set_lyrics(lyrics.clone());
        let f = frames.clone();
        win.on_frame(move |_| {
            if f.on.get() { f.ends.borrow_mut()[kind as usize].push(Instant::now()) }
            1.0
        });
        // The swap interval as the app sets it; also hears the card's name.
        pacing::attach(win.window(), label, vsync);
        win.window().set_size(PhysicalSize::new(w, h));
        win.window().set_position(PhysicalPosition::new(x, y));
        win.show().map_err(|e| format!("could not show the {label} window: {e}"))?;
        open.push(win);
    }
    let cpu_at: Rc<Cell<Option<(Instant, Option<Duration>)>>> = Rc::default();
    let result: Rc<RefCell<Option<(f64, Option<Duration>)>>> = Rc::default();
    let (f, at) = (frames.clone(), cpu_at.clone());
    let start = Timer::default();
    start.start(slint::TimerMode::SingleShot, WARMUP, move || { at.set(Some((Instant::now(), cpu_time()))); f.on.set(true) });
    let (f, at, res) = (frames.clone(), cpu_at.clone(), result.clone());
    let stop = Timer::default();
    stop.start(slint::TimerMode::SingleShot, WARMUP + MEASURE, move || {
        f.on.set(false);
        if let Some((t0, c0)) = at.get() {
            let cpu = c0.zip(cpu_time()).map(|(a, b)| b.saturating_sub(a));
            *res.borrow_mut() = Some((t0.elapsed().as_secs_f64(), cpu));
        }
        let _ = slint::quit_event_loop();
    });
    slint::run_event_loop_until_quit().map_err(|e| format!("could not run the windows: {e}"))?;
    for w in &open { let _ = w.hide(); }
    let Some((secs, cpu)) = *result.borrow() else { return Err("the test did not finish".into()) };
    let ends = frames.ends.borrow();
    let ms = |v: &[Instant]| v.first().map(|t0| v.iter().map(|t| (*t - *t0).as_secs_f64() * 1000.0).collect::<Vec<_>>()).unwrap_or_default();
    // The room is what the church sees.
    let (fps, low_ms) = frame_stats(&ms(&ends[1]));
    let adapter = if g == Graphics::Software { "Software".to_string() } else { graphics::adapter() };
    Ok(Measured {
        graphics: g,
        started: ends[1].len() >= 2,
        fps,
        low_ms,
        cpu: cpu.map(|c| (c.as_secs_f64() / secs * 100.0) as f32),
        adapter,
        error: String::new(),
    })
}
