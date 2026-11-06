//! Slide-change benchmark: a development aid, never part of the app. It opens the real native operator window,
//! and (by default) a windowed room and stage window on the same monitor, over a fresh data folder holding five
//! ten-slide songs, then fires `Next` every 250 ms and measures:
//!
//! * commit latency: from each command to the room reporting that version (or a newer one) on air;
//! * UI-thread stalls: a 1 ms repeating Slint timer; every gap over 20 ms counts as a stall;
//! * process CPU time over the measured stretch (Windows only), as % of one core;
//! * frames drawn per window (rendering-notifier `AfterRendering`; the plain `software` renderer has no notifier),
//!   as fps and as frames per slide change;
//! * the room's first frame after each swap: from the room putting a new view on screen (the start of its
//!   crossfade) to the end of the first room frame drawn after it, in ms;
//! * the room's frame time: from `BeforeRendering` to `AfterRendering` on the UI thread (median, ms);
//! * GL textures in the room's context (OpenGL renderers): after each room frame every texture name up to 4096 is
//!   checked with `glIsTexture`; a name that was not a texture and now is counts as one texture made (an image
//!   upload, or a FemtoVG offscreen layer), reported per slide change, with the most alive at once.
//!
//! Environment:
//! * `RP_RENDERER`: femtovg (the app's default, OpenGL), femtovg-wgpu (Direct3D 12 / Vulkan / Metal through wgpu;
//!   build with `--features renderer-wgpu`), skia-opengl, skia, skia-vulkan, skia-software (`--features
//!   renderer-skia`, which on Windows needs the MSVC toolchain: rust-skia has no MinGW binaries) or software;
//! * `RP_BENCH_WINDOWS`: 3 (operator + room + stage, the default) or 1 (operator only);
//! * `RP_BENCH_CHANGES` (40), `RP_BENCH_INTERVAL_MS` (250), `RP_BENCH_ROOM` (room window size, `1280x720` pixels);
//! * `RP_BENCH_ROOM_MONITOR`, `RP_BENCH_STAGE_MONITOR`: put the room / stage full screen on monitor n (1-based, as
//!   listed at start), the way the app does, instead of a plain window on the operator's monitor. With two
//!   monitors, `RP_BENCH_ROOM_MONITOR=2` is the setup that lags in church;
//! * `RP_BENCH_SCENARIO`: `slides` (the default, above); `hover`: the same, while a fake mouse sweeps over the
//!   operator every 8 ms (a hover storm); `playback`: a looping 25 fps clip plays in the room for
//!   `RP_BENCH_SECONDS` (10) with no slide changes (needs libmpv: `RP_LIBMPV` or the usual places);
//! * `RP_BENCH_RENDERERS`: with `--matrix`, only these renderers (comma-separated).
//!
//! One run prints a `BENCH,...` line. `--matrix [runs]` runs every renderer with 1 and 3 windows, `runs` times each
//! (default 3), as child processes, and prints a table of medians. Build and run (release only):
//!
//! ```text
//! cargo run --release --features renderer-wgpu --example slide_bench -- --matrix 3
//! ```
//!
//! Each `BENCH` line: renderer, windows, graphics API the operator window got, commit latency p50/p95/max (ms),
//! stalls over 20 ms (count, longest gap of any size in ms, total ms of stalls), CPU % of one core, fps of operator/room/stage,
//! room first frame after a swap p50/p95/max (ms), frames per slide change of operator/room/stage, room frame time
//! p50 (ms), room textures made per slide change, most room textures alive.
use rustypresenter::{
    app::{App, Update},
    live::{Command, Envelope},
    model::{EndAction, ItemKind, MediaFile, MediaKind, MediaStatus, ServiceItem, Song},
    platform::monitors,
    render::Renderer,
    store::Store,
    ui::{operator::window::{Operator, OperatorScreens}, ops::Ops, pacing, room::Room, screens::NativeScreens, stage::Stage},
    video,
};
use slint::{LogicalPosition, PhysicalPosition, PhysicalSize, RenderingState, Timer, TimerMode, platform::WindowEvent};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}},
    time::{Duration, Instant},
};
use uuid::Uuid;

const RENDERERS: [&str; 6] = ["femtovg", "femtovg-wgpu", "skia-opengl", "skia", "skia-software", "software"];

/// Whether this build has `renderer` (the others need the crate's `renderer-wgpu` / `renderer-skia` features).
fn built_with(renderer: &str) -> bool {
    match renderer {
        "femtovg" | "software" => true,
        "femtovg-wgpu" => cfg!(feature = "renderer-wgpu"),
        r if r.starts_with("skia") => cfg!(feature = "renderer-skia"),
        _ => false,
    }
}
const STALL: Duration = Duration::from_millis(20);
const WARMUP: Duration = Duration::from_secs(3);
const SETTLE: Duration = Duration::from_millis(1500);

fn env_num(name: &str, default: u64) -> u64 { std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default) }

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--matrix") {
        let runs = args.get(i + 1).and_then(|r| r.parse().ok()).unwrap_or(3);
        return matrix(runs);
    }
    run();
}

// ---------- one run ----------

/// Frames drawn by one window while measuring (when each one ended).
#[derive(Clone, Default)]
struct Frames {
    times: Rc<RefCell<Vec<Instant>>>,
    counting: Rc<Cell<bool>>,
    supported: Rc<Cell<bool>>,
    api: Rc<RefCell<String>>,
    /// When the frame being drawn started, and how long each counted frame took (ms).
    began: Rc<Cell<Option<Instant>>>,
    took: Rc<RefCell<Vec<f64>>>,
}

impl Frames {
    fn tick(&self) {
        let now = Instant::now();
        let began = self.began.take();
        if !self.counting.get() { return }
        self.times.borrow_mut().push(now);
        if let Some(b) = began { self.took.borrow_mut().push((now - b).as_secs_f64() * 1000.0) }
    }
    fn count(&self) -> usize { self.times.borrow().len() }

    /// Takes in one rendering-notifier event.
    fn heard(&self, state: &RenderingState, api: &slint::GraphicsAPI) {
        match state {
            RenderingState::RenderingSetup => {
                self.supported.set(true);
                // `NativeOpenGL { .. }`, `WGPU30 { .. }`, ... (with or without the `GraphicsAPI::` path).
                let name = format!("{api:?}");
                let head = name.split(['{', '(', ' ']).next().unwrap_or("");
                *self.api.borrow_mut() = head.rsplit("::").next().unwrap_or("").to_string();
            }
            RenderingState::BeforeRendering => self.began.set(Some(Instant::now())),
            RenderingState::AfterRendering => self.tick(),
            _ => {}
        }
    }

    /// Counts `window`'s frames through its rendering notifier (a window the app gave none).
    fn watch(window: &slint::Window) -> Frames {
        let f = Frames::default();
        let g = f.clone();
        let set = window.set_rendering_notifier(move |state, api| g.heard(&state, api));
        match set {
            Ok(()) => f.supported.set(true),
            Err(e) => eprintln!("frames not counted ({e:?})"),
        }
        f
    }

    fn fps(&self, secs: f64) -> Option<f64> { self.supported.get().then(|| self.count() as f64 / secs) }

    /// Mean frames per slide change.
    fn per_change(&self, changes: usize) -> Option<f64> { (self.supported.get() && changes > 0).then(|| self.count() as f64 / changes as f64) }
}

/// GL textures alive in the room's context, checked after each room frame (`VideoSurface::set_gl_probe`).
#[derive(Clone, Default)]
struct Textures { live: Rc<RefCell<Vec<bool>>>, made: Rc<Cell<usize>>, most: Rc<Cell<usize>>, seen: Rc<Cell<bool>> }

/// Texture names checked: FemtoVG and mpv use far fewer.
const TEX_NAMES: u32 = 4096;

impl Textures {
    /// Checks every name; while `counting`, adds the ones that became textures since the last check.
    fn check(&self, gl: &glow::Context, counting: bool) {
        use glow::HasContext;
        let mut live = self.live.borrow_mut();
        live.resize(TEX_NAMES as usize + 1, false);
        let (mut made, mut alive) = (0, 0);
        for n in 1..=TEX_NAMES {
            let Some(name) = std::num::NonZeroU32::new(n) else { continue };
            let now = unsafe { gl.is_texture(glow::NativeTexture(name)) };
            if now && !live[n as usize] { made += 1 }
            alive += usize::from(now);
            live[n as usize] = now;
        }
        // The first check only learns what is there.
        if counting && self.seen.get() {
            self.made.set(self.made.get() + made);
            self.most.set(self.most.get().max(alive));
        }
        self.seen.set(true);
    }
}

/// What is measured besides slide changes.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Scenario { Slides, Hover, Playback }

fn scenario() -> Scenario {
    match std::env::var("RP_BENCH_SCENARIO").as_deref() {
        Ok("hover") => Scenario::Hover,
        Ok("playback") => Scenario::Playback,
        Ok("" | "slides") | Err(_) => Scenario::Slides,
        Ok(other) => { eprintln!("unknown RP_BENCH_SCENARIO {other:?} (slides, hover or playback)"); std::process::exit(2) }
    }
}

/// A looping test clip for the playback scenario: 2 s of 25 fps frames, each with a bar in a new place, so every
/// frame differs (mpv plays an animated GIF as video).
fn write_clip(path: &std::path::Path) {
    let (w, h, n) = (320u32, 180u32, 50u32);
    let frames = (0..n).map(|i| {
        let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([(i * 5) as u8, 60, 255 - (i * 5) as u8, 255]));
        let x0 = i * w / n;
        for y in 0..h { for x in x0..(x0 + 12).min(w) { img.put_pixel(x, y, image::Rgba([255, 255, 255, 255])) } }
        image::Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(40, 1))
    });
    let file = std::fs::File::create(path).expect("clip file");
    let mut enc = image::codecs::gif::GifEncoder::new_with_speed(file, 30);
    enc.encode_frames(frames).expect("clip");
}

/// For each swap, how long until the first `frames` time at or after it (ms); swaps never followed by a frame
/// are left out.
fn first_frame_delays(swaps: &[Instant], frames: &[Instant]) -> Vec<f64> {
    swaps.iter().filter_map(|s| {
        let i = frames.partition_point(|f| f < s);
        frames.get(i).map(|f| (*f - *s).as_secs_f64() * 1000.0)
    }).collect()
}

/// What the UI thread keeps while the windows are open.
struct Open { _op: Rc<Operator>, _room: Option<Rc<Room>>, _stage: Option<Rc<Stage>>, frames: Vec<(&'static str, Frames)>, textures: Option<Textures>, _stall_timer: Timer, _hover: Option<Timer> }

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
    /// Opens the windows, given the connected monitors (run once the event loop is active).
    static SETUP: RefCell<Option<Box<dyn FnOnce(Vec<monitors::MonitorId>)>>> = const { RefCell::new(None) };
}

/// Lyrics of ten sections (ten slides), four long lines each.
fn lyrics(n: usize) -> String {
    (1..=10).map(|s| format!(
        "Verse {s}\nSong {n} verse {s}: amazing grace how sweet the sound\nThat saved a wretch like me, I once was lost\nBut now am found, was blind but now I see the light\nThrough many dangers, toils and snares we have come"
    )).collect::<Vec<_>>().join("\n\n")
}

fn cpu_time() -> Option<Duration> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{Foundation::FILETIME, System::Threading::{GetCurrentProcess, GetProcessTimes}};
        let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut c, mut e, mut k, mut u) = (z, z, z, z);
        if GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) == 0 { return None }
        let t = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
        Some(Duration::from_nanos((t(k) + t(u)) * 100))
    }
    #[cfg(not(windows))]
    None
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() { return f64::NAN }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn run() {
    let renderer_name = std::env::var("RP_RENDERER").ok().filter(|r| !r.is_empty()).unwrap_or_else(|| "femtovg".into());
    let windows = env_num("RP_BENCH_WINDOWS", 3);
    let scenario = scenario();
    let seconds = env_num("RP_BENCH_SECONDS", 10);
    let changes = env_num("RP_BENCH_CHANGES", 40) as usize;
    let interval = Duration::from_millis(env_num("RP_BENCH_INTERVAL_MS", 250));
    let (room_w, room_h) = std::env::var("RP_BENCH_ROOM").ok()
        .and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))))
        .unwrap_or((1280u32, 720u32));
    if !built_with(&renderer_name) {
        eprintln!("this build has no {renderer_name} renderer (see the features at the top of examples/slide_bench.rs)");
        std::process::exit(2);
    }

    // A data folder of its own (never the user's), five ten-slide songs in the service.
    let root: PathBuf = std::env::temp_dir().join(format!("rp-slide-bench-{}", Uuid::new_v4()));
    let store = Store::open(root.clone()).expect("data folder");
    let data = store.load().expect("data");
    let app = App::new(store, data, 4397, None);
    let clip = (scenario == Scenario::Playback).then(|| {
        write_clip(&app.store.asset_path("bench-clip.gif"));
        let media = MediaFile { id: Uuid::new_v4(), name: "Bench clip".into(), kind: MediaKind::Video, asset: "bench-clip.gif".into(), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() };
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Media { media_id: media.id, autoplay: true, end: EndAction::Loop, looped: true }, background: None, notes: String::new(), theme: None };
        let id = item.id;
        let mut data = app.data.lock().unwrap();
        data.media.push(media);
        data.services[0].items.push(item);
        id
    });
    let first = {
        let mut data = app.data.lock().unwrap();
        let mut first = None;
        for n in 1..=5 {
            let song = Song { id: Uuid::new_v4(), title: format!("Bench song {n}"), lyrics: lyrics(n), ..Default::default() };
            let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: song.id }, background: None, notes: String::new(), theme: None };
            first.get_or_insert(item.id);
            data.songs.push(song);
            data.services[0].items.push(item);
        }
        first.unwrap()
    };

    let mode = video::choose_backend();
    let mpv = match video::ffi::Mpv::load() {
        Ok(m) => Some(m),
        Err(e) if scenario == Scenario::Playback => { eprintln!("the playback scenario needs libmpv: {e}"); std::process::exit(2) }
        Err(_) => None,
    };
    let rt = tokio::runtime::Runtime::new().expect("tokio");
    let renderer = Renderer::new();
    let screens = NativeScreens::new(&app, renderer.clone(), mpv.clone());
    app.set_screens(screens.clone());
    let changes = if scenario == Scenario::Playback { 0 } else { changes };
    eprintln!("renderer {renderer_name} ({mode:?}), {windows} window(s), scenario {scenario:?}, {changes} changes every {interval:?}");

    // Commits as the live engine publishes them (subscribed before anything goes live).
    let commits: Arc<Mutex<Vec<(u64, Instant)>>> = Arc::default();
    {
        let (mut rx, commits) = (app.subscribe(), commits.clone());
        std::thread::spawn(move || loop {
            match rx.blocking_recv() {
                Ok(Update::Live(u)) => commits.lock().unwrap().push((u.committed, Instant::now())),
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => return,
            }
        });
    }

    // UI-thread stalls: every gap between ticks of a 1 ms timer while measuring.
    let stalls: Rc<RefCell<Vec<Duration>>> = Rc::default();
    let measuring = Rc::new(Cell::new(false));
    let last_tick = Rc::new(Cell::new(Instant::now()));
    // When the room put each new view on screen while measuring.
    let swaps: Rc<RefCell<Vec<Instant>>> = Rc::default();

    let (app2, handle) = (app.clone(), rt.handle().clone());
    let (stalls2, measuring2, last2) = (stalls.clone(), measuring.clone(), last_tick.clone());
    let (swaps2, measuring3) = (swaps.clone(), measuring.clone());
    let setup = move |mons: Vec<monitors::MonitorId>| {
        let ops = Ops::new(app2.clone(), handle);
        // The operator and stage (and the room, in OpenGL mode) have the app's own rendering notifier
        // (`ui::pacing`); their frames are counted through its probe.
        let paced: Vec<(&'static str, Frames)> = ["operator", "room", "stage"].map(|l| (l, Frames::default())).into();
        let heard = paced.clone();
        pacing::set_probe(move |label, state, api| if let Some((_, f)) = heard.iter().find(|(l, _)| *l == label) { f.heard(state, api) });
        let paced_frames = |label: &str| paced.iter().find(|(l, _)| *l == label).map(|(_, f)| f.clone()).unwrap_or_default();
        let op = Operator::open(app2.clone(), ops, Rc::new(screens.clone()) as Rc<dyn OperatorScreens>).expect("operator window");
        op.window().set_maximized(true);
        let mut frames = vec![("operator", paced_frames("operator"))];
        let (mut room, mut stage, mut textures) = (None, None, None);
        if windows > 1 {
            // RP_BENCH_ROOM_MONITOR / RP_BENCH_STAGE_MONITOR = n: full screen on the n-th monitor (1-based), as
            // the app puts them; otherwise a plain window on the operator's monitor.
            for (i, m) in mons.iter().enumerate() { eprintln!("monitor {}: {} {}x{} at {:?}", i + 1, m.name, m.size.0, m.size.1, m.position) }
            let on = |var: &str| std::env::var(var).ok().and_then(|n| n.parse::<usize>().ok()).and_then(|n| mons.get(n.wrapping_sub(1)).cloned());
            let (room_mon, stage_mon) = (on("RP_BENCH_ROOM_MONITOR"), on("RP_BENCH_STAGE_MONITOR"));
            let r = Room::open(app2.clone(), renderer.clone(), mpv.clone(), room_mon.clone()).expect("room window");
            let (swaps, measuring) = (swaps2.clone(), measuring3.clone());
            r.set_swap_probe(move || if measuring.get() { swaps.borrow_mut().push(Instant::now()) });
            if room_mon.is_none() {
                r.window().set_position(PhysicalPosition::new(160, 80));
                r.window().set_size(PhysicalSize::new(room_w, room_h));
            }
            // In OpenGL mode the video surface holds the notifier and reports to the pacing probe.
            let f = if mode == video::DrawMode::OpenGl { paced_frames("room") } else { Frames::watch(r.window()) };
            if mode == video::DrawMode::OpenGl {
                let (t, counting) = (Textures::default(), f.counting.clone());
                let t2 = t.clone();
                r.surface().set_gl_probe(move |gl, _| t2.check(gl, counting.get()));
                textures = Some(t);
            }
            frames.push(("room", f));
            let s = Stage::open(app2.clone(), stage_mon.clone()).expect("stage window");
            if stage_mon.is_none() {
                s.window().set_position(PhysicalPosition::new(40, 420));
                s.window().set_size(PhysicalSize::new(640, 360));
            }
            frames.push(("stage", paced_frames("stage")));
            room = Some(r);
            stage = Some(s);
        }
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_millis(1), move || {
            let now = Instant::now();
            let gap = now - last2.replace(now);
            if measuring2.get() { stalls2.borrow_mut().push(gap) }
        });
        // The hover storm: a pointer sweeping over the operator window every 8 ms, as a mouse moved over its buttons.
        let hover = (scenario == Scenario::Hover).then(|| {
            let (t, w, start) = (Timer::default(), Rc::downgrade(&op), Instant::now());
            t.start(TimerMode::Repeated, Duration::from_millis(8), move || {
                let Some(op) = w.upgrade() else { return };
                let win = op.window();
                let size = win.size().to_logical(win.scale_factor());
                let s = start.elapsed().as_secs_f32();
                let x = (s * 900.0) % size.width.max(1.0);
                let y = size.height * (0.5 + 0.4 * (s * 1.7).sin());
                win.dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(x, y) });
            });
            t
        });
        OPEN.with(|o| *o.borrow_mut() = Some(Open { _op: op, _room: room, _stage: stage, frames, textures, _stall_timer: timer, _hover: hover }));
    };
    // As the app opens its operator: from the active event loop, which also lists the monitors.
    SETUP.with(|s| *s.borrow_mut() = Some(Box::new(setup)));
    slint::winit_030::invoke_from_active_event_loop(|el| {
        let (mons, _) = monitors::on_event_loop(el);
        if let Some(f) = SETUP.with(|s| s.borrow_mut().take()) { f(mons) }
    }).expect("event loop");

    // The driver: show the first slide, warm up, then `Next` every `interval` from a plain thread (as the phone
    // remote or the operator's actions do, off the UI thread).
    let sends: Arc<Mutex<Vec<(u64, Instant)>>> = Arc::default();
    let failed = Arc::new(Mutex::new(0usize));
    let started = Arc::new(AtomicBool::new(false));
    let measure_from: Arc<Mutex<Option<(Instant, Option<Duration>)>>> = Arc::default();
    {
        let (app, sends, failed, started) = (app.clone(), sends.clone(), failed.clone(), started.clone());
        let measure_from = measure_from.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            let env = |base, cmd| Envelope { id: Uuid::new_v4().to_string(), base, cmd };
            let out = app.command(env(None, Command::Show { item_id: clip.unwrap_or(first), slide: 0 }));
            assert!(out.ok, "{out:?}");
            std::thread::sleep(WARMUP);
            *measure_from.lock().unwrap() = Some((Instant::now(), cpu_time()));
            started.store(true, Ordering::Release);
            let _ = slint::invoke_from_event_loop(|| OPEN.with(|o| if let Some(o) = o.borrow().as_ref() { for (_, f) in &o.frames { f.counting.set(true) } }));
            let t0 = Instant::now();
            for i in 0..changes {
                let at = t0 + interval * i as u32;
                if let Some(d) = at.checked_duration_since(Instant::now()) { std::thread::sleep(d) }
                let base = app.live.lock().unwrap().version;
                let sent = Instant::now();
                let out = app.command(env(Some(base), Command::Next));
                if out.ok { sends.lock().unwrap().push((out.version, sent)) } else { *failed.lock().unwrap() += 1 }
            }
            if clip.is_some() { std::thread::sleep(Duration::from_secs(seconds)) } else { std::thread::sleep(interval + SETTLE) }
            let _ = slint::invoke_from_event_loop(|| { let _ = slint::quit_event_loop(); });
        });
    }
    let started_ui = started.clone();
    let start_timer = Timer::default();
    {
        let measuring = measuring.clone();
        start_timer.start(TimerMode::Repeated, Duration::from_millis(5), move || if started_ui.load(Ordering::Acquire) { measuring.set(true) });
    }

    slint::run_event_loop_until_quit().expect("event loop");
    let end = Instant::now();
    let cpu_end = cpu_time();
    measuring.set(false);

    let (t_start, cpu_start) = measure_from.lock().unwrap().expect("measured");
    let wall = (end - t_start).as_secs_f64();
    let cpu_pct = match (cpu_start, cpu_end) { (Some(a), Some(b)) => Some((b - a).as_secs_f64() / wall * 100.0), _ => None };

    // Latency: from each send to the first published commit at or past its version.
    let commits = commits.lock().unwrap().clone();
    let sends = sends.lock().unwrap().clone();
    let mut lat: Vec<f64> = vec![];
    let mut lost = 0;
    for (v, sent) in &sends {
        match commits.iter().find(|(c, t)| c >= v && t >= sent) {
            Some((_, t)) => lat.push((*t - *sent).as_secs_f64() * 1000.0),
            None => lost += 1,
        }
    }
    lat.sort_by(f64::total_cmp);
    let gaps = stalls.borrow().clone();
    let stall_max = gaps.iter().max().map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    let mean_gap = if gaps.is_empty() { 0.0 } else { gaps.iter().sum::<Duration>().as_secs_f64() * 1000.0 / gaps.len() as f64 };
    let stall_ms: Vec<f64> = gaps.iter().filter(|d| **d > STALL).map(|d| d.as_secs_f64() * 1000.0).collect();
    let stall_sum: f64 = stall_ms.iter().sum::<f64>() + 0.0;
    let api = OPEN.with(|o| o.borrow().as_ref().map(|o| o.frames[0].1.api.borrow().clone())).filter(|a| !a.is_empty()).unwrap_or_else(|| "-".into());
    let fps: Vec<(&str, Option<f64>)> = OPEN.with(|o| o.borrow().as_ref().map(|o| o.frames.iter().map(|(n, f)| (*n, f.fps(wall))).collect()).unwrap_or_default());
    let fps_of = |n: &str| fps.iter().find(|(m, _)| *m == n).and_then(|(_, f)| *f);
    let per_change: Vec<(&str, Option<f64>)> = OPEN.with(|o| o.borrow().as_ref().map(|o| o.frames.iter().map(|(n, f)| (*n, f.per_change(sends.len()))).collect()).unwrap_or_default());
    let per_of = |n: &str| per_change.iter().find(|(m, _)| *m == n).and_then(|(_, f)| *f);
    let room_frames: Option<Vec<Instant>> = OPEN.with(|o| o.borrow().as_ref().and_then(|o| o.frames.iter().find(|(n, f)| *n == "room" && f.supported.get()).map(|(_, f)| f.times.borrow().clone())));
    let mut first: Vec<f64> = room_frames.map(|r| first_frame_delays(&swaps.borrow(), &r)).unwrap_or_default();
    first.sort_by(f64::total_cmp);
    let ff = |v: f64| if first.is_empty() { "-".to_string() } else { format!("{v:.1}") };
    let mut room_took: Vec<f64> = OPEN.with(|o| o.borrow().as_ref().and_then(|o| o.frames.iter().find(|(n, _)| *n == "room").map(|(_, f)| f.took.borrow().clone()))).unwrap_or_default();
    room_took.sort_by(f64::total_cmp);
    let room_ms = (!room_took.is_empty()).then(|| pct(&room_took, 0.5));
    let tex = OPEN.with(|o| o.borrow().as_ref().and_then(|o| o.textures.as_ref().map(|t| (t.made.get(), t.most.get()))));
    let tex_per = tex.and_then(|(m, _)| (!sends.is_empty()).then(|| m as f64 / sends.len() as f64));
    let tex_most = tex.map(|(_, a)| a as f64);
    let opt = |v: Option<f64>| v.map(|v| format!("{v:.1}")).unwrap_or_else(|| "-".into());
    let committed = |v: f64| if lat.is_empty() { "-".to_string() } else { format!("{v:.0}") };

    eprintln!("measured {wall:.1} s; {} sends ({} refused), {lost} never committed", sends.len(), failed.lock().unwrap());
    eprintln!("commit latency ms: median {} p95 {} max {}", committed(pct(&lat, 0.5)), committed(pct(&lat, 0.95)), committed(pct(&lat, 1.0)));
    eprintln!("UI stalls > 20 ms: {} (total {stall_sum:.0} ms); longest gap {stall_max:.0} ms, mean gap {mean_gap:.1} ms", stall_ms.len());
    eprintln!("CPU: {}% of one core; fps {:?}", opt(cpu_pct), fps);
    eprintln!("room first frame after a swap, ms: median {} p95 {} max {} ({} swaps); frames per change {:?}", ff(pct(&first, 0.5)), ff(pct(&first, 0.95)), ff(pct(&first, 1.0)), swaps.borrow().len(), per_change);
    eprintln!("room frame time median {} ms; room GL textures made {:?} ({} per change), most alive {}", opt(room_ms), tex.map(|t| t.0), opt(tex_per), opt(tex_most));
    println!(
        "BENCH,{renderer_name},{windows},{api},{},{},{},{},{:.0},{:.0},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        committed(pct(&lat, 0.5)), committed(pct(&lat, 0.95)), committed(pct(&lat, 1.0)),
        stall_ms.len(), stall_max, stall_sum, opt(cpu_pct), opt(fps_of("operator")), opt(fps_of("room")), opt(fps_of("stage")),
        ff(pct(&first, 0.5)), ff(pct(&first, 0.95)), ff(pct(&first, 1.0)), opt(per_of("operator")), opt(per_of("room")), opt(per_of("stage")),
        opt(room_ms), opt(tex_per), opt(tex_most),
    );

    OPEN.with(|o| o.borrow_mut().take());
    rt.shutdown_timeout(Duration::from_secs(1));
    let _ = std::fs::remove_dir_all(&root);
    // Threads still blocked on the app (commit watcher, room threads) end with the process.
    std::process::exit(0);
}

// ---------- the matrix ----------

const COLS: [&str; 19] = [
    "lat p50", "lat p95", "lat max", "stalls", "max gap", "stall sum", "cpu %", "op fps", "room fps", "stage fps",
    "room 1st p50", "room 1st p95", "room 1st max", "op f/chg", "room f/chg", "stage f/chg", "room ms", "room tex/chg",
    "room tex max",
];
/// Decimals shown per column.
const DECIMALS: [usize; 19] = [0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 1, 0];

fn matrix(runs: usize) {
    let exe = std::env::current_exe().expect("own path");
    let mut rows = vec![];
    let only: Option<Vec<String>> = std::env::var("RP_BENCH_RENDERERS").ok().filter(|v| !v.is_empty()).map(|v| v.split(',').map(|r| r.trim().to_string()).collect());
    for r in RENDERERS {
        if only.as_ref().is_some_and(|o| !o.iter().any(|x| x == r)) { continue }
        if !built_with(r) {
            eprintln!("== {r}: not in this build, skipped");
            continue;
        }
        for w in [1, 3] {
            let mut results: Vec<Vec<Option<f64>>> = vec![];
            let mut api = String::from("-");
            for i in 0..runs {
                eprintln!("== {r}, {w} window(s), run {}/{runs}", i + 1);
                let out = std::process::Command::new(&exe).env("RP_RENDERER", r).env("RP_BENCH_WINDOWS", w.to_string())
                    .stderr(std::process::Stdio::inherit()).output();
                let line = out.ok().and_then(|o| String::from_utf8_lossy(&o.stdout).lines().find(|l| l.starts_with("BENCH,")).map(str::to_string));
                match line {
                    Some(l) => {
                        println!("{l}");
                        let f: Vec<&str> = l.split(',').collect();
                        api = f.get(3).unwrap_or(&"-").to_string();
                        results.push(f.iter().skip(4).map(|v| v.parse().ok()).collect());
                    }
                    None => eprintln!("   run failed"),
                }
            }
            rows.push((r, w, api, results));
        }
    }
    println!("\nMedians of {runs} run(s); latency and stalls in ms, fps per window (- = not measurable).\n");
    println!("| renderer | windows | api | {} |", COLS.join(" | "));
    println!("|---|---|---|{}", "---|".repeat(COLS.len()));
    for (r, w, api, results) in rows {
        let cells: Vec<String> = (0..COLS.len()).map(|c| {
            let mut v: Vec<f64> = results.iter().filter_map(|row| row.get(c).copied().flatten()).collect();
            v.sort_by(f64::total_cmp);
            if v.is_empty() { "-".into() } else { format!("{:.*}", DECIMALS[c], pct(&v, 0.5)) }
        }).collect();
        let ok = if results.is_empty() { " (failed)" } else { "" };
        println!("| {r}{ok} | {w} | {api} | {} |", cells.join(" | "));
    }
}
