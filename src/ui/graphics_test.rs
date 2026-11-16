//! The graphics test's child (`rustypresenter --graphics-bench <choice> --bench-out <file> [--bench-room <monitor>]`,
//! `video::bench`): with exactly that graphics choice, opens the app's real operator, room and stage windows over a
//! data folder of its own (never the church's), shows five songs' lyrics over a moving background (a looping video
//! when the video player is there, else a picture), changes slide every `INTERVAL` from another thread as the phone
//! remote does, and writes what it measured. The room goes full screen on its monitor when `--bench-room` says so
//! (the two-screen setup that shows lags), else it is a plain window.
use crate::{
    app::App,
    live::{Command, Envelope},
    model::{Graphics, ItemKind, ServiceItem, Song},
    platform::monitors::{self, MonitorId},
    render::Renderer,
    store::Store,
    theme::backgrounds::Background,
    ui::{operator::window::{Operator, OperatorScreens}, ops::Ops, pacing, room::Room, screens::NativeScreens, stage::Stage},
    video::{self, bench::{self, ChildArgs, Measured}, graphics},
};
use slint::{PhysicalPosition, PhysicalSize, RenderingState, Timer, TimerMode};
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}},
    time::{Duration, Instant},
};
use uuid::Uuid;

/// From the first slide until measuring starts (windows settle, the video starts, caches fill).
pub const WARMUP: Duration = Duration::from_millis(2500);
/// Measured stretch: a slide change every `INTERVAL`.
pub const CHANGES: u32 = 12;
pub const INTERVAL: Duration = Duration::from_millis(600);
/// After the last change, until the windows close (its crossfade ends).
const SETTLE: Duration = Duration::from_millis(800);
/// The room's crossfade (`ui::room::FADE`).
const FADE_MS: f64 = 400.0;

/// Runs the test for `args` and writes the result. The exit code is always 0: a choice that fails says so in the file
/// (a crash or hang is seen by the parent).
pub fn run(args: ChildArgs) -> i32 {
    let m = match graphics::choose_exact(args.graphics) {
        Ok(chosen) => measure(args.graphics, chosen.draw, args.room.as_deref()).unwrap_or_else(|e| Measured::failed(args.graphics, e)),
        Err(e) => Measured::failed(args.graphics, e),
    };
    eprintln!("graphics test {:?}: {}", args.graphics, bench::log_line(&m));
    if let Err(e) = std::fs::write(&args.out, serde_json::to_vec(&m).unwrap_or_default()) { eprintln!("graphics test: could not write {}: {e}", args.out.display()) }
    // Threads still waiting on the app (screens, video) end with the process.
    std::process::exit(0)
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

/// Lyrics of ten sections (ten slides), four lines each.
fn lyrics(n: usize) -> String {
    (1..=10).map(|s| format!(
        "Verse {s}\nSong {n}, verse {s}: amazing grace, how sweet the sound\nThat saved a wretch like me, I once was lost\nBut now am found, was blind but now I see\nThrough many dangers, toils and snares we have come"
    )).collect::<Vec<_>>().join("\n\n")
}

/// A looping 2 s background clip, 25 frames a second, every frame different (mpv plays an animated GIF as video).
fn write_clip(path: &Path) -> Result<(), String> {
    let (w, h, n) = (480u32, 270u32, 50u32);
    let frames = (0..n).map(|i| {
        let mut img = image::RgbaImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            let wave = (((x as f32 / w as f32) * 6.0 + i as f32 * 0.25).sin() * 0.5 + 0.5) * 70.0;
            *p = image::Rgba([(30.0 + wave) as u8, (20 + y * 60 / h) as u8, (120.0 + wave * 0.6) as u8, 255]);
        }
        image::Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(40, 1))
    });
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = image::codecs::gif::GifEncoder::new_with_speed(file, 30);
    enc.encode_frames(frames).map_err(|e| e.to_string())
}

/// A full-HD picture background, for when there is no video player.
fn write_picture(path: &Path) -> Result<(), String> {
    let (w, h) = (1920u32, 1080u32);
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let wave = (((x as f32 / w as f32) * 9.0 + (y as f32 / h as f32) * 4.0).sin() * 0.5 + 0.5) * 60.0;
        image::Rgb([(30.0 + wave) as u8, (20 + y * 60 / h) as u8, (90.0 + wave * 0.5) as u8])
    });
    img.save(path).map_err(|e| e.to_string())
}

/// Times (on one clock) that the UI thread hears while measuring.
#[derive(Default)]
struct Heard {
    on: Cell<bool>,
    /// Room frames ended, and how long each took to draw (ms), when the renderer says.
    frames: RefCell<Vec<Instant>>,
    draw_ms: RefCell<Vec<f64>>,
    began: Cell<Option<Instant>>,
    counted: Cell<bool>,
    /// The room putting a new slide up.
    swaps: RefCell<Vec<Instant>>,
    /// Gaps between the 1 ms timer's ticks.
    gaps: RefCell<Vec<f64>>,
    last_tick: Cell<Option<Instant>>,
}

impl Heard {
    fn room(&self, state: &RenderingState) {
        match state {
            RenderingState::RenderingSetup => self.counted.set(true),
            RenderingState::BeforeRendering => self.began.set(Some(Instant::now())),
            RenderingState::AfterRendering => {
                let now = Instant::now();
                let began = self.began.take();
                if !self.on.get() { return }
                self.frames.borrow_mut().push(now);
                if let Some(b) = began { self.draw_ms.borrow_mut().push((now - b).as_secs_f64() * 1000.0) }
            }
            _ => {}
        }
    }
}

/// What the UI thread keeps open while the test runs.
struct Open { _op: Rc<Operator>, _room: Rc<Room>, _stage: Rc<Stage>, _ticks: Timer }

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
    static SETUP: RefCell<Option<Box<dyn FnOnce(Vec<MonitorId>, Option<MonitorId>)>>> = const { RefCell::new(None) };
    static ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The monitor for the room: the saved one (or the first that is not the main one, for `auto`), never the main one.
fn room_monitor(want: Option<&str>, mons: &[MonitorId], main: Option<&MonitorId>) -> Option<MonitorId> {
    let saved = want.filter(|w| *w != "auto");
    monitors::pick(mons, saved, main).filter(|m| Some(m) != main)
}

fn measure(g: Graphics, draw: video::DrawMode, room_want: Option<&str>) -> Result<Measured, String> {
    // A data folder of its own, five ten-slide songs over the moving background.
    let root: PathBuf = std::env::temp_dir().join(format!("rp-graphics-test-{}", Uuid::new_v4()));
    let store = Store::open(root.clone()).map_err(|e| e.to_string())?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let mpv = video::ffi::Mpv::load().ok();
    let video = mpv.is_some() && write_clip(&app.store.asset_path("graphics-test.gif")).is_ok();
    let background = if video {
        Background::Video { asset: "graphics-test.gif".into(), dim: 20 }
    } else {
        write_picture(&app.store.asset_path("graphics-test.png"))?;
        Background::Image { asset: "graphics-test.png".into(), dim: 20 }
    };
    let first = {
        let mut data = app.data.lock().unwrap();
        let mut first = None;
        for n in 1..=5 {
            let song = Song { id: Uuid::new_v4(), title: format!("Test song {n}"), lyrics: lyrics(n), ..Default::default() };
            let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: Some(background.clone()), notes: String::new(), theme: None };
            first.get_or_insert(item.id);
            data.songs.push(song);
            data.services[0].items.push(item);
        }
        first.ok_or("no song")?
    };
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let renderer = Renderer::new();
    let screens = NativeScreens::new(&app, renderer.clone(), mpv.clone());
    app.set_screens(screens.clone());

    let heard = Rc::new(Heard::default());
    let on_monitor = Rc::new(Cell::new(false));
    let (app2, handle, h, on_mon) = (app.clone(), rt.handle().clone(), heard.clone(), on_monitor.clone());
    let room_want = room_want.map(str::to_string);
    let setup = move |mons: Vec<MonitorId>, main: Option<MonitorId>| {
        let open = || -> Result<Open, String> {
            // The operator and stage report through `pacing` (it also hears the graphics card); in OpenGL mode so
            // does the room's video surface.
            let hp = h.clone();
            pacing::set_probe(move |label, state, _| if label == "room" { hp.room(state) });
            let op = Operator::open(app2.clone(), Ops::new(app2.clone(), handle), Rc::new(screens.clone()) as Rc<dyn OperatorScreens>)?;
            op.window().set_position(PhysicalPosition::new(20, 20));
            op.window().set_size(PhysicalSize::new(1280, 760));
            let mon = room_want.as_deref().and_then(|w| room_monitor(Some(w), &mons, main.as_ref()));
            on_mon.set(mon.is_some());
            let room = Room::open(app2.clone(), renderer.clone(), mpv.clone(), mon.clone())?;
            if mon.is_none() {
                room.window().set_position(PhysicalPosition::new(120, 80));
                room.window().set_size(PhysicalSize::new(1280, 720));
            }
            let hs = h.clone();
            room.set_swap_probe(move || if hs.on.get() { hs.swaps.borrow_mut().push(Instant::now()) });
            if draw != video::DrawMode::OpenGl {
                // Its own notifier (the Software renderer has none: its frames can't be counted).
                let hr = h.clone();
                if room.window().set_rendering_notifier(move |state, api| {
                    if matches!(state, RenderingState::RenderingSetup) { graphics::heard(api) }
                    hr.room(&state)
                }).is_err() { h.counted.set(false) }
            }
            let stage = Stage::open(app2.clone(), None)?;
            stage.window().set_position(PhysicalPosition::new(60, 440));
            stage.window().set_size(PhysicalSize::new(640, 360));
            let (ticks, ht) = (Timer::default(), h.clone());
            ticks.start(TimerMode::Repeated, Duration::from_millis(1), move || {
                let now = Instant::now();
                if let Some(last) = ht.last_tick.replace(Some(now)) && ht.on.get() { ht.gaps.borrow_mut().push((now - last).as_secs_f64() * 1000.0) }
            });
            Ok(Open { _op: op, _room: room, _stage: stage, _ticks: ticks })
        };
        match open() {
            Ok(o) => OPEN.with(|s| *s.borrow_mut() = Some(o)),
            Err(e) => {
                ERROR.with(|s| *s.borrow_mut() = Some(e));
                let _ = slint::quit_event_loop();
            }
        }
    };
    SETUP.with(|s| *s.borrow_mut() = Some(Box::new(setup)));
    slint::winit_030::invoke_from_active_event_loop(|el| {
        let (mons, main) = monitors::on_event_loop(el);
        if let Some(f) = SETUP.with(|s| s.borrow_mut().take()) { f(mons, main) }
    }).map_err(|e| format!("could not open the windows: {e}"))?;

    // The driver: the first slide, warm up, then Next every INTERVAL from a plain thread.
    let sends: Arc<Mutex<Vec<Instant>>> = Arc::default();
    let measuring = Arc::new(AtomicBool::new(false));
    let from: Arc<Mutex<Option<(Instant, Option<Duration>)>>> = Arc::default();
    let shown = Arc::new(AtomicBool::new(true));
    {
        let (app, sends, measuring, from, shown) = (app.clone(), sends.clone(), measuring.clone(), from.clone(), shown.clone());
        std::thread::spawn(move || {
            let env = |base, cmd| Envelope { id: Uuid::new_v4().to_string(), base, cmd };
            std::thread::sleep(Duration::from_millis(800));
            if !app.command(env(None, Command::Show { item_id: first, slide: 0 })).ok { shown.store(false, Ordering::Release) }
            std::thread::sleep(WARMUP);
            *from.lock().unwrap() = Some((Instant::now(), cpu_time()));
            measuring.store(true, Ordering::Release);
            let t0 = Instant::now();
            for i in 0..CHANGES {
                if let Some(d) = (t0 + INTERVAL * i).checked_duration_since(Instant::now()) { std::thread::sleep(d) }
                let base = app.live.lock().unwrap().version;
                let sent = Instant::now();
                if app.command(env(Some(base), Command::Next)).ok { sends.lock().unwrap().push(sent) }
            }
            std::thread::sleep(INTERVAL + SETTLE);
            let _ = slint::invoke_from_event_loop(|| { let _ = slint::quit_event_loop(); });
        });
    }
    // Measuring starts on the UI thread as soon as the driver says.
    let (m2, h2) = (measuring.clone(), heard.clone());
    let start = Timer::default();
    start.start(TimerMode::Repeated, Duration::from_millis(2), move || if m2.load(Ordering::Acquire) { h2.on.set(true) });
    slint::run_event_loop_until_quit().map_err(|e| format!("could not run the windows: {e}"))?;
    let end = (Instant::now(), cpu_time());
    heard.on.set(false);
    drop(start);
    if let Some(e) = ERROR.with(|s| s.borrow_mut().take()) { return Err(e) }
    OPEN.with(|s| if let Some(o) = s.borrow().as_ref() { let _ = o._op.window().hide(); });

    let Some((t0, cpu0)) = *from.lock().unwrap() else { return Err("the test did not finish".into()) };
    if !shown.load(Ordering::Acquire) { return Err("the first slide could not be shown".into()) }
    let ms = |t: &Instant| t.saturating_duration_since(t0).as_secs_f64() * 1000.0;
    let sends: Vec<f64> = sends.lock().unwrap().iter().map(ms).collect();
    let swaps: Vec<f64> = heard.swaps.borrow().iter().map(ms).collect();
    let frames: Vec<f64> = heard.frames.borrow().iter().map(ms).collect();
    let counted = heard.counted.get() && frames.len() >= 2;
    let delays = bench::change_delays(&sends, &swaps, counted.then_some(frames.as_slice()));
    let secs = (end.0 - t0).as_secs_f64();
    let (freezes, freeze_ms) = bench::freezes(&heard.gaps.borrow());
    let draw = heard.draw_ms.borrow();
    let m = Measured {
        graphics: g,
        started: counted || !swaps.is_empty(),
        changes: sends.len() as u32,
        seen: delays.len() as u32,
        delay_ms: bench::percentile(&delays, 0.5) as f32,
        delay_slow_ms: bench::percentile(&delays, 0.95) as f32,
        hiccups_min: counted.then(|| bench::hiccups(&frames, &swaps, FADE_MS, video) as f32 * 60.0 / secs as f32),
        freezes,
        freeze_ms,
        draw_ms: (!draw.is_empty()).then(|| bench::percentile(&draw, 0.95) as f32),
        fps: bench::fps(&frames),
        cpu: cpu0.zip(end.1).map(|(a, b)| (b.saturating_sub(a).as_secs_f64() / secs * 100.0) as f32),
        video,
        on_monitor: on_monitor.get(),
        adapter: if g == Graphics::Software { "Software".to_string() } else { graphics::adapter() },
        error: String::new(),
    };
    drop(draw);
    OPEN.with(|s| s.borrow_mut().take());
    rt.shutdown_timeout(Duration::from_millis(500));
    let _ = std::fs::remove_dir_all(&root);
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(name: &str, x: i32) -> MonitorId { MonitorId { name: name.into(), size: (1920, 1080), position: (x, 0) } }

    #[test]
    fn the_room_goes_on_its_own_monitor_never_the_main_one() {
        let (main, proj) = (mon("LAPTOP", 0), mon("EPSON", 1920));
        let both = [main.clone(), proj.clone()];
        assert_eq!(room_monitor(Some("auto"), &both, Some(&main)), Some(proj.clone()));
        assert_eq!(room_monitor(Some("EPSON"), &both, Some(&main)), Some(proj));
        assert_eq!(room_monitor(Some("auto"), std::slice::from_ref(&main), Some(&main)), None, "one monitor: a window");
        assert_eq!(room_monitor(Some("LAPTOP"), &both, Some(&main)), None, "saved on the main one: a window");
    }

    #[test]
    fn the_test_backgrounds_are_made_here() {
        let d = std::env::temp_dir().join(format!("rp-gt-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        write_clip(&d.join("c.gif")).unwrap();
        write_picture(&d.join("p.png")).unwrap();
        assert!(std::fs::metadata(d.join("c.gif")).unwrap().len() > 10_000);
        assert_eq!(image::image_dimensions(d.join("p.png")).unwrap(), (1920, 1080));
        let _ = std::fs::remove_dir_all(&d);
    }
}
