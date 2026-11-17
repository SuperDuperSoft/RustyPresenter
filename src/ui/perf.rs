//! N33: how smoothly the screens draw (frames a second) and the graphics card's free memory, for the line under the
//! live preview, so the operator can tell when the laptop is struggling. Every window paints on the UI thread and
//! tells `pacing::on_render`, which passes it here; the counts are kept on that thread. Nothing is counted or asked
//! of the driver while the line is off (`set_wanted`).
use crate::tr;
use slint::{GraphicsAPI, Rgba8Pixel, RenderingState, SharedPixelBuffer};
use std::{cell::RefCell, collections::{HashMap, HashSet, VecDeque}, ffi::{CStr, c_void}, sync::atomic::{AtomicBool, Ordering}, time::{Duration, Instant}};

/// Frames this old are forgotten.
const SPAN: Duration = Duration::from_secs(2);
/// No frame for this long: the screen is showing a still picture.
const STILL_AFTER: Duration = Duration::from_secs(1);
/// How often the driver is asked for its free memory.
const MEMORY_EVERY: Duration = Duration::from_secs(2);
/// Below this, a moving audience screen is struggling.
pub const SLOW_FPS: f32 = 20.0;

static WANTED: AtomicBool = AtomicBool::new(false);

/// Turns the counting on or off (Settings → Video → Show performance).
pub fn set_wanted(on: bool) {
    if !WANTED.swap(on, Ordering::Relaxed) && on { METER.with(|m| { let mut m = m.borrow_mut(); m.frames.clear(); m.memory_at = None; }) }
}

pub fn wanted() -> bool { WANTED.load(Ordering::Relaxed) }

/// The graphics card's memory in KiB: what is free, and the total when the driver says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoMemory { pub free_kb: i64, pub total_kb: Option<i64> }

/// Frame times per window (`room`, `stage`, `operator`) and the last memory reading.
#[derive(Default)]
pub struct Meter {
    frames: HashMap<&'static str, VecDeque<Instant>>,
    /// Windows that report their frames (the software renderer has no notifier, so it never does).
    measured: HashSet<&'static str>,
    /// `None`: not asked yet; `Some(None)`: this driver does not say.
    memory: Option<Option<VideoMemory>>,
    memory_at: Option<Instant>,
}

impl Meter {
    /// `label`'s window reports its frames.
    pub fn seen(&mut self, label: &'static str) { self.measured.insert(label); }

    pub fn frame(&mut self, label: &'static str, at: Instant) {
        self.seen(label);
        let q = self.frames.entry(label).or_default();
        q.push_back(at);
        while q.front().is_some_and(|t| at.duration_since(*t) > SPAN) { q.pop_front(); }
    }

    /// Frames a second lately: 0 for a still picture, `None` when the window cannot be measured.
    pub fn fps(&self, label: &str, now: Instant) -> Option<f32> {
        if !self.measured.contains(label) { return None }
        let Some(q) = self.frames.get(label) else { return Some(0.0) };
        let recent: Vec<Instant> = q.iter().copied().filter(|t| now.saturating_duration_since(*t) <= SPAN).collect();
        match (recent.first(), recent.last()) {
            (_, Some(last)) if now.saturating_duration_since(*last) > STILL_AFTER => Some(0.0),
            (Some(first), Some(last)) if recent.len() >= 2 => {
                let secs = last.duration_since(*first).as_secs_f32();
                Some(if secs > 0.0 { (recent.len() - 1) as f32 / secs } else { 0.0 })
            }
            (Some(_), _) => Some(1.0),
            _ => Some(0.0),
        }
    }

    fn memory_due(&self, now: Instant) -> bool {
        self.memory != Some(None) && self.memory_at.is_none_or(|t| now.duration_since(t) >= MEMORY_EVERY)
    }

    /// The last memory reading: `None` while not asked yet, `Some(None)` when the driver does not say.
    pub fn memory(&self) -> Option<Option<VideoMemory>> { self.memory }
}

thread_local! { static METER: RefCell<Meter> = RefCell::default(); }

/// Runs `f` with this thread's meter (the UI thread's holds every window's frames).
pub fn with_meter<T>(f: impl FnOnce(&Meter) -> T) -> T { METER.with(|m| f(&m.borrow())) }

/// A rendering-notifier event of window `label` (from `pacing::on_render`).
pub(crate) fn observe(label: &'static str, state: &RenderingState, api: &GraphicsAPI) {
    METER.with(|m| {
        let mut m = m.borrow_mut();
        m.seen(label);
        if !wanted() || !matches!(state, RenderingState::AfterRendering) { return }
        let now = Instant::now();
        m.frame(label, now);
        if let GraphicsAPI::NativeOpenGL { get_proc_address } = api {
            if m.memory_due(now) {
                m.memory_at = Some(now);
                m.memory = Some(query_memory(*get_proc_address));
            }
        }
    });
}

const GL_NO_ERROR: u32 = 0;
/// `GL_NVX_gpu_memory_info` (NVIDIA): total and free memory, in KiB.
const GPU_MEMORY_TOTAL_NVX: u32 = 0x9048;
const GPU_MEMORY_FREE_NVX: u32 = 0x9049;
/// `GL_ATI_meminfo` (AMD): four numbers, the first is the free memory in KiB.
const TEXTURE_FREE_MEMORY_ATI: u32 = 0x87FC;

/// Asks the OpenGL driver that is current how much video memory is free. Intel and most others do not say.
pub fn query_memory(gpa: &dyn Fn(&CStr) -> *const c_void) -> Option<VideoMemory> {
    let (Some(get), Some(err)) = (super::pacing::found(gpa(c"glGetIntegerv")), super::pacing::found(gpa(c"glGetError"))) else { return None };
    let get: unsafe extern "system" fn(u32, *mut i32) = unsafe { std::mem::transmute(get) };
    let err: unsafe extern "system" fn() -> u32 = unsafe { std::mem::transmute(err) };
    // Older errors would read as ours; a bounded loop, as a lost context can report errors forever.
    for _ in 0..16 { if unsafe { err() } == GL_NO_ERROR { break } }
    let mut v = [-1i32; 4];
    unsafe { get(GPU_MEMORY_FREE_NVX, v.as_mut_ptr()) };
    if unsafe { err() } == GL_NO_ERROR && v[0] >= 0 {
        let mut total = -1i32;
        unsafe { get(GPU_MEMORY_TOTAL_NVX, &mut total) };
        let total = (unsafe { err() } == GL_NO_ERROR && total > 0).then_some(i64::from(total));
        return Some(VideoMemory { free_kb: i64::from(v[0]), total_kb: total });
    }
    let mut v = [-1i32; 4];
    unsafe { get(TEXTURE_FREE_MEMORY_ATI, v.as_mut_ptr()) };
    (unsafe { err() } == GL_NO_ERROR && v[0] >= 0).then(|| VideoMemory { free_kb: i64::from(v[0]), total_kb: None })
}

/// What the line under the preview shows. A window that is not open is left out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading { pub room: Option<Option<f32>>, pub stage: Option<Option<f32>>, pub operator: Option<f32>, pub memory: Option<Option<VideoMemory>> }

impl Reading {
    /// The meter's view now; `room` and `stage` say whether those windows are open.
    pub fn now(room: bool, stage: bool) -> Self {
        let now = Instant::now();
        with_meter(|m| Reading {
            room: room.then(|| m.fps("room", now)),
            stage: stage.then(|| m.fps("stage", now)),
            operator: m.fps("operator", now),
            memory: m.memory(),
        })
    }

    /// The audience screen is moving, but slowly.
    pub fn slow(&self) -> bool { matches!(self.room, Some(Some(f)) if f > 0.0 && f < SLOW_FPS) }

    /// "Room 60 fps · Stage 1 fps · This window 30 fps · Video memory 1.2 GB free of 4.0 GB".
    pub fn line(&self) -> String {
        let fps = |f: Option<f32>, name: &str| match f {
            None => tr!("{screen}: not measured", screen = name),
            Some(f) if f <= 0.0 => tr!("{screen}: still picture", screen = name),
            Some(f) => tr!("{screen} {fps} fps", screen = name, fps = f.round() as i64),
        };
        let mut parts = vec![];
        match self.room {
            Some(f) => parts.push(fps(f, &tr!("Room"))),
            None => parts.push(tr!("Audience screen is off")),
        }
        if let Some(f) = self.stage { parts.push(fps(f, &tr!("Stage"))) }
        if let Some(f) = self.operator { parts.push(fps(Some(f), &tr!("This window"))) }
        match self.memory {
            Some(Some(VideoMemory { free_kb, total_kb: Some(t) })) => parts.push(tr!("Video memory {free} free of {total}", free = size(free_kb), total = size(t))),
            Some(Some(VideoMemory { free_kb, total_kb: None })) => parts.push(tr!("Video memory {free} free", free = size(free_kb))),
            Some(None) => parts.push(tr!("Video memory: this graphics card does not say")),
            None => {}
        }
        parts.join(" · ")
    }
}

/// KiB as "512 MB" or "1.2 GB".
pub fn size(kb: i64) -> String {
    let mb = kb as f64 / 1024.0;
    if mb >= 1024.0 { format!("{:.1} GB", mb / 1024.0) } else { format!("{} MB", mb.round() as i64) }
}

/// `px` made at most `max_w` wide (each new pixel the average of the ones it covers), for the stage preview.
pub fn shrink(px: &SharedPixelBuffer<Rgba8Pixel>, max_w: u32) -> SharedPixelBuffer<Rgba8Pixel> {
    let (w, h) = (px.width(), px.height());
    if w <= max_w || w == 0 || h == 0 || max_w == 0 { return px.clone() }
    let nw = max_w;
    let nh = ((u64::from(h) * u64::from(nw)) / u64::from(w)).max(1) as u32;
    let src = px.as_slice();
    let mut out = SharedPixelBuffer::<Rgba8Pixel>::new(nw, nh);
    let dst = out.make_mut_slice();
    for y in 0..nh {
        let (y0, y1) = (y * h / nh, ((y + 1) * h / nh).max(y * h / nh + 1));
        for x in 0..nw {
            let (x0, x1) = (x * w / nw, ((x + 1) * w / nw).max(x * w / nw + 1));
            let mut sum = [0u32; 4];
            for sy in y0..y1 {
                for p in &src[(sy * w + x0) as usize..(sy * w + x1) as usize] {
                    sum[0] += u32::from(p.r); sum[1] += u32::from(p.g); sum[2] += u32::from(p.b); sum[3] += u32::from(p.a);
                }
            }
            let n = (y1 - y0) * (x1 - x0);
            dst[(y * nw + x) as usize] = Rgba8Pixel::new((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8, (sum[3] / n) as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(t0: Instant, n: u64) -> Instant { t0 + Duration::from_millis(n) }

    #[test]
    fn frames_a_second_from_the_last_frames() {
        let t0 = Instant::now();
        let mut m = Meter::default();
        assert_eq!(m.fps("room", t0), None, "a window that never reported cannot be measured (software drawing)");
        m.seen("room");
        assert_eq!(m.fps("room", t0), Some(0.0), "reported, but drew nothing: a still picture");
        // A 400 ms fade at 60 frames a second reads as 60, not as 24 frames in the last second.
        for i in 0..25 { m.frame("room", ms(t0, i * 1000 / 60)) }
        let f = m.fps("room", ms(t0, 450)).unwrap();
        assert!((f - 60.0).abs() < 1.5, "{f}");
        assert_eq!(m.fps("room", ms(t0, 1500)), Some(0.0), "nothing drawn for over a second: still");
        // The stage's clock draws once a second.
        m.frame("stage", ms(t0, 3000));
        assert_eq!(m.fps("stage", ms(t0, 3100)), Some(1.0));
        m.frame("stage", ms(t0, 4000));
        assert_eq!(m.fps("stage", ms(t0, 4100)), Some(1.0));
        // Old frames are forgotten.
        m.frame("room", ms(t0, 10_000));
        m.frame("room", ms(t0, 10_100));
        assert_eq!(m.fps("room", ms(t0, 10_100)), Some(10.0));
    }

    #[test]
    fn the_line_says_what_each_screen_does() {
        let r = Reading { room: Some(Some(59.6)), stage: Some(Some(0.0)), operator: Some(30.0), memory: Some(Some(VideoMemory { free_kb: 1258291, total_kb: Some(4194304) })) };
        assert_eq!(r.line(), "Room 60 fps · Stage: still picture · This window 30 fps · Video memory 1.2 GB free of 4.0 GB");
        assert!(!r.slow());
        let r = Reading { room: Some(None), stage: None, operator: None, memory: Some(None) };
        assert_eq!(r.line(), "Room: not measured · Video memory: this graphics card does not say");
        let r = Reading { room: None, memory: Some(Some(VideoMemory { free_kb: 524288, total_kb: None })), ..Default::default() };
        assert_eq!(r.line(), "Audience screen is off · Video memory 512 MB free");
        assert!(Reading { room: Some(Some(12.0)), ..Default::default() }.slow(), "a room moving at 12 frames a second is struggling");
        assert!(!Reading { room: Some(Some(0.0)), ..Default::default() }.slow(), "a still picture is fine");
    }

    thread_local! {
        static VRAM: RefCell<(u32, Vec<u32>)> = RefCell::default();
    }
    /// A driver with NVIDIA's memory call: 3 GiB free of 8.
    unsafe extern "system" fn nv_get(p: u32, v: *mut i32) {
        let ok = match p { GPU_MEMORY_FREE_NVX => { unsafe { *v = 3 * 1024 * 1024 }; true } GPU_MEMORY_TOTAL_NVX => { unsafe { *v = 8 * 1024 * 1024 }; true } _ => false };
        VRAM.with(|s| s.borrow_mut().1.push(p));
        if !ok { VRAM.with(|s| s.borrow_mut().0 = 0x0500) }
    }
    /// Intel: neither call is known (GL_INVALID_ENUM).
    unsafe extern "system" fn intel_get(_p: u32, _v: *mut i32) { VRAM.with(|s| s.borrow_mut().0 = 0x0500) }
    unsafe extern "system" fn get_error() -> u32 { VRAM.with(|s| std::mem::take(&mut s.borrow_mut().0)) }

    #[test]
    fn video_memory_from_the_driver_when_it_says() {
        let nv = |n: &CStr| match n.to_bytes() { b"glGetIntegerv" => nv_get as *const c_void, b"glGetError" => get_error as *const c_void, _ => std::ptr::null() };
        assert_eq!(query_memory(&nv), Some(VideoMemory { free_kb: 3 << 20, total_kb: Some(8 << 20) }));
        let intel = |n: &CStr| match n.to_bytes() { b"glGetIntegerv" => intel_get as *const c_void, b"glGetError" => get_error as *const c_void, _ => std::ptr::null() };
        assert_eq!(query_memory(&intel), None);
        assert_eq!(VRAM.with(|s| s.borrow().0), 0, "the error it caused is cleared");
        assert_eq!(query_memory(&|_| std::ptr::null()), None);
        assert_eq!(size(300 * 1024), "300 MB");
    }

    #[test]
    fn counting_only_while_wanted() {
        let api = GraphicsAPI::NativeOpenGL { get_proc_address: &|_| std::ptr::null() };
        set_wanted(false);
        observe("stage", &RenderingState::AfterRendering, &api);
        with_meter(|m| assert_eq!(m.fps("stage", Instant::now()), Some(0.0), "seen, but its frames are not counted"));
        set_wanted(true);
        observe("stage", &RenderingState::BeforeRendering, &api);
        observe("stage", &RenderingState::AfterRendering, &api);
        with_meter(|m| assert_eq!(m.fps("stage", Instant::now()), Some(1.0)));
        assert_eq!(with_meter(Meter::memory), Some(None), "a driver without glGetIntegerv does not say");
        set_wanted(false);
    }

    #[test]
    fn shrink_averages_pixels() {
        let mut px = SharedPixelBuffer::<Rgba8Pixel>::new(4, 2);
        for (i, p) in px.make_mut_slice().iter_mut().enumerate() { *p = if i % 2 == 0 { Rgba8Pixel::new(200, 0, 0, 255) } else { Rgba8Pixel::new(0, 100, 0, 255) } }
        let s = shrink(&px, 2);
        assert_eq!((s.width(), s.height()), (2, 1));
        assert_eq!(s.as_slice()[0], Rgba8Pixel::new(100, 50, 0, 255));
        assert_eq!(shrink(&px, 10).width(), 4, "never made bigger");
    }
}
