//! NDI in and out (docs/superpowers/specs/2026-11-16-ndi-in-and-out-design.md). NDI is proprietary: the runtime the
//! church installs (the free NDI Tools from ndi.video) is loaded at run time with `libloading`, never linked or
//! shipped. Without it NDI cameras are not listed and NDI out does nothing.
//!
//! - In: an NDI source as a camera (`ndi://<source name>`, `camera_feed` runs `receive` for it).
//! - Out: the stream's picture and sound as the source "RustyPresenter Stream", and the words over the camera with
//!   a see-through background as "RustyPresenter Words" (`Out`).
use std::ffi::{CStr, CString, OsString, c_char, c_void};
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How a saved camera names an NDI source: `ndi://` and the source's name ("PC (OBS)").
pub const SCHEME: &str = "ndi://";
/// The first of a camera's input arguments when it is an NDI source (`record::args::camera_input`); the second is
/// the source's name.
pub const INPUT: &str = "<ndi>";
/// The names the app's NDI sources have (the computer's name goes before them on the network).
pub const STREAM_NAME: &str = "RustyPresenter Stream";
pub const WORDS_NAME: &str = "RustyPresenter Words";
/// The stream's sound as the recorder's FFmpeg sends it to `Out`: 48 kHz stereo, 32-bit float.
pub const SOUND_RATE: u32 = 48_000;
pub const SOUND_CHANNELS: usize = 2;

/// The NDI source a saved camera names, if it is one.
pub fn source_name(camera: &str) -> Option<&str> {
    let c = camera.trim();
    c.get(..SCHEME.len()).filter(|s| s.eq_ignore_ascii_case(SCHEME)).map(|_| &c[SCHEME.len()..]).filter(|n| !n.trim().is_empty())
}

/// The NDI source in a camera's input arguments, if it is one.
pub fn input_source(input: &[String]) -> Option<&str> {
    match input { [mark, name] if mark == INPUT => Some(name), _ => None }
}

/// How an NDI camera is listed.
pub fn label(name: &str) -> String { format!("NDI: {name}") }

// ---- The runtime ----

#[cfg(windows)]
const LIBRARY: &str = "Processing.NDI.Lib.x64.dll";
#[cfg(target_os = "macos")]
const LIBRARY: &str = "libndi.dylib";
#[cfg(all(unix, not(target_os = "macos")))]
const LIBRARY: &str = "libndi.so.6";

/// Where the runtime is looked for: the folders the NDI installers name, then the system's own search.
fn candidates(env: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let mut c: Vec<PathBuf> = ["NDI_RUNTIME_DIR_V6", "NDI_RUNTIME_DIR_V5"].iter().filter_map(|v| env(v)).filter(|d| !d.is_empty()).map(|d| PathBuf::from(d).join(LIBRARY)).collect();
    c.push(PathBuf::from(LIBRARY));
    c.extend(known(crate::platform::tools::OS).iter().map(PathBuf::from));
    c
}

/// Where the runtime is put on `os` when the system's own search does not find it (the places OBS's NDI plugin
/// looks too). Windows' installer always sets `NDI_RUNTIME_DIR_V6`.
fn known(os: crate::platform::tools::Os) -> &'static [&'static str] {
    use crate::platform::tools::Os;
    match os {
        Os::Windows => &[],
        Os::MacOs => &["/usr/local/lib/libndi.dylib", "/Library/NDI SDK for Apple/lib/macOS/libndi.dylib"],
        Os::Linux => &["/usr/local/lib/libndi.so.6", "/usr/lib/x86_64-linux-gnu/libndi.so.6", "/usr/lib64/libndi.so.6", "libndi.so.5", "/usr/local/lib/libndi.so.5"],
    }
}

#[repr(C)]
struct Source { name: *const c_char, url: *const c_char }

#[repr(C)]
struct VideoFrame {
    xres: i32,
    yres: i32,
    fourcc: u32,
    rate_n: i32,
    rate_d: i32,
    aspect: f32,
    format: i32,
    timecode: i64,
    data: *mut u8,
    stride: i32,
    metadata: *const c_char,
    timestamp: i64,
}

impl VideoFrame {
    fn empty() -> VideoFrame {
        VideoFrame { xres: 0, yres: 0, fourcc: 0, rate_n: 0, rate_d: 0, aspect: 0.0, format: 0, timecode: 0, data: std::ptr::null_mut(), stride: 0, metadata: std::ptr::null(), timestamp: 0 }
    }
}

#[repr(C)]
struct AudioFrame { rate: i32, channels: i32, samples: i32, timecode: i64, data: *mut f32, channel_stride: i32, metadata: *const c_char, timestamp: i64 }

#[repr(C)]
struct FindCreate { show_local: bool, groups: *const c_char, extra_ips: *const c_char }

#[repr(C)]
struct RecvCreate { source: Source, color: i32, bandwidth: i32, fields: bool, name: *const c_char }

#[repr(C)]
struct SendCreate { name: *const c_char, groups: *const c_char, clock_video: bool, clock_audio: bool }

const fn fourcc(c: &[u8; 4]) -> u32 { c[0] as u32 | (c[1] as u32) << 8 | (c[2] as u32) << 16 | (c[3] as u32) << 24 }
const RGBA: u32 = fourcc(b"RGBA");
const RGBX: u32 = fourcc(b"RGBX");
/// `NDIlib_recv_color_format_RGBX_RGBA`, `NDIlib_recv_bandwidth_highest`, `NDIlib_frame_format_type_progressive`.
const COLOR_RGB: i32 = 2;
const BANDWIDTH_HIGHEST: i32 = 100;
const PROGRESSIVE: i32 = 1;
/// `NDIlib_send_timecode_synthesize`: NDI makes the time codes.
const SYNTHESIZE: i64 = i64::MAX;
/// `NDIlib_frame_type_video`, `NDIlib_frame_type_error`.
const FRAME_VIDEO: i32 = 1;
const FRAME_ERROR: i32 = 4;

type Ptr = *mut c_void;

/// The loaded runtime: the calls the app makes.
pub struct Ndi {
    _lib: libloading::Library,
    find_create: unsafe extern "C" fn(*const FindCreate) -> Ptr,
    find_destroy: unsafe extern "C" fn(Ptr),
    find_wait: unsafe extern "C" fn(Ptr, u32) -> bool,
    find_sources: unsafe extern "C" fn(Ptr, *mut u32) -> *const Source,
    recv_create: unsafe extern "C" fn(*const RecvCreate) -> Ptr,
    recv_destroy: unsafe extern "C" fn(Ptr),
    recv_capture: unsafe extern "C" fn(Ptr, *mut VideoFrame, *mut AudioFrame, *mut c_void, u32) -> i32,
    recv_free_video: unsafe extern "C" fn(Ptr, *const VideoFrame),
    send_create: unsafe extern "C" fn(*const SendCreate) -> Ptr,
    send_destroy: unsafe extern "C" fn(Ptr),
    send_video: unsafe extern "C" fn(Ptr, *const VideoFrame),
    send_audio: unsafe extern "C" fn(Ptr, *const AudioFrame),
}

impl Ndi {
    fn load(path: &std::path::Path) -> Result<Ndi, String> {
        let lib = open(path).map_err(|e| e.to_string())?;
        // SAFETY: the symbols are the NDI SDK's C functions (v6 headers), declared with their C types above.
        unsafe {
            macro_rules! sym { ($n:literal) => { *lib.get($n).map_err(|e| format!("{}: {e}", String::from_utf8_lossy($n)))? } }
            let initialize: unsafe extern "C" fn() -> bool = sym!(b"NDIlib_initialize\0");
            if !initialize() { return Err("NDI does not run on this computer's processor".into()) }
            Ok(Ndi {
                find_create: sym!(b"NDIlib_find_create_v2\0"),
                find_destroy: sym!(b"NDIlib_find_destroy\0"),
                find_wait: sym!(b"NDIlib_find_wait_for_sources\0"),
                find_sources: sym!(b"NDIlib_find_get_current_sources\0"),
                recv_create: sym!(b"NDIlib_recv_create_v3\0"),
                recv_destroy: sym!(b"NDIlib_recv_destroy\0"),
                recv_capture: sym!(b"NDIlib_recv_capture_v2\0"),
                recv_free_video: sym!(b"NDIlib_recv_free_video_v2\0"),
                send_create: sym!(b"NDIlib_send_create\0"),
                send_destroy: sym!(b"NDIlib_send_destroy\0"),
                send_video: sym!(b"NDIlib_send_send_video_v2\0"),
                send_audio: sym!(b"NDIlib_send_send_audio_v2\0"),
                _lib: lib,
            })
        }
    }
}

#[cfg(windows)]
fn open(path: &std::path::Path) -> Result<libloading::Library, libloading::Error> {
    use libloading::os::windows::{LOAD_WITH_ALTERED_SEARCH_PATH, Library};
    let flags = if path.is_absolute() { LOAD_WITH_ALTERED_SEARCH_PATH } else { 0 };
    unsafe { Library::load_with_flags(path, flags).map(Into::into) }
}

#[cfg(not(windows))]
fn open(path: &std::path::Path) -> Result<libloading::Library, libloading::Error> {
    unsafe { libloading::Library::new(path) }
}

/// The NDI runtime, loaded the first time it is asked for; `None` when it is not installed.
pub fn runtime() -> Option<&'static Ndi> {
    static NDI: OnceLock<Option<Ndi>> = OnceLock::new();
    NDI.get_or_init(|| {
        let mut last = String::new();
        for path in candidates(|v| std::env::var_os(v)) {
            match Ndi::load(&path) {
                Ok(n) => return Some(n),
                Err(e) => last = format!("{}: {e}", path.display()),
            }
        }
        eprintln!("ndi: no runtime ({last})");
        None
    }).as_ref()
}

// ---- Finding sources ----

/// The names of the NDI sources on the network, looked for for `wait`. Empty without the runtime.
pub fn sources(wait: Duration) -> Vec<String> {
    let Some(ndi) = runtime() else { return vec![] };
    let settings = FindCreate { show_local: true, groups: std::ptr::null(), extra_ips: std::ptr::null() };
    // SAFETY: the finder is made, read and destroyed here; the sources it gives are copied before it goes.
    unsafe {
        let finder = (ndi.find_create)(&settings);
        if finder.is_null() { return vec![] }
        let end = Instant::now() + wait;
        while let Some(left) = end.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) {
            (ndi.find_wait)(finder, left.as_millis().min(u128::from(u32::MAX)) as u32);
        }
        let mut n = 0u32;
        let list = (ndi.find_sources)(finder, &mut n);
        let mut names: Vec<String> = if list.is_null() { vec![] } else {
            std::slice::from_raw_parts(list, n as usize).iter().filter(|s| !s.name.is_null()).map(|s| CStr::from_ptr(s.name).to_string_lossy().into_owned()).collect()
        };
        (ndi.find_destroy)(finder);
        names.sort();
        names.dedup();
        names
    }
}

// ---- Receiving (an NDI camera) ----

/// A connection to an NDI source. NDI finds the source by its name and connects again by itself when it comes back.
pub struct Receiver { ndi: &'static Ndi, me: Ptr }

// SAFETY: an NDI receiver may be used from any one thread at a time.
unsafe impl Send for Receiver {}

impl Receiver {
    pub fn connect(source: &str) -> Option<Receiver> {
        let ndi = runtime()?;
        let (name, me) = (CString::new(source).ok()?, CString::new("RustyPresenter").ok()?);
        let settings = RecvCreate { source: Source { name: name.as_ptr(), url: std::ptr::null() }, color: COLOR_RGB, bandwidth: BANDWIDTH_HIGHEST, fields: false, name: me.as_ptr() };
        // SAFETY: the settings live until the call returns; NDI copies them.
        let r = unsafe { (ndi.recv_create)(&settings) };
        (!r.is_null()).then_some(Receiver { ndi, me: r })
    }

    /// Waits up to `wait` for a picture and hands it to `f` as `(width, height, bytes per row, RGBX/RGBA rows)`.
    /// False when the connection has failed.
    pub fn picture(&self, wait: Duration, f: impl FnOnce(u32, u32, usize, &[u8])) -> bool {
        let mut v = VideoFrame::empty();
        // SAFETY: a video frame given is read and then handed back to NDI.
        unsafe {
            let kind = (self.ndi.recv_capture)(self.me, &mut v, std::ptr::null_mut(), std::ptr::null_mut(), wait.as_millis().min(5000) as u32);
            if kind == FRAME_ERROR { return false }
            if kind == FRAME_VIDEO {
                let (w, h, stride) = (v.xres.max(0) as u32, v.yres.max(0) as u32, v.stride.max(0) as usize);
                if !v.data.is_null() && w > 0 && h > 0 && stride >= w as usize * 4 && (v.fourcc == RGBA || v.fourcc == RGBX) {
                    f(w, h, stride, std::slice::from_raw_parts(v.data, stride * h as usize));
                }
                (self.ndi.recv_free_video)(self.me, &v);
            }
        }
        true
    }
}

impl Drop for Receiver {
    fn drop(&mut self) { unsafe { (self.ndi.recv_destroy)(self.me) } }
}

/// Puts an NDI picture (`w`×`h`, rows `stride` bytes apart) into `dst`, `size` RGBA, keeping its shape with black
/// bars.
pub fn fit_picture(w: u32, h: u32, stride: usize, rows: &[u8], dst: &mut [u8], size: (u32, u32)) {
    let row = w as usize * 4;
    let tight: std::borrow::Cow<[u8]> = if stride == row { rows[..row * h as usize].into() } else {
        rows.chunks(stride).take(h as usize).flat_map(|r| &r[..row]).copied().collect::<Vec<u8>>().into()
    };
    crate::record::pump::fit(&crate::record::pump::Raw { w, h, bottom_up: false, rgba: &tight }, dst, size.0, size.1);
}

/// Receives NDI source `name` into `latest` (`camera_feed`), `size` RGBA, until `stop`. Without the runtime, or
/// while the receiver cannot be made, it is tried again every `retry`.
pub fn receive(name: &str, size: (u32, u32), latest: &super::camera_feed::Latest, stop: &AtomicBool, retry: Duration) {
    while !stop.load(Ordering::Relaxed) {
        let Some(r) = Receiver::connect(name) else {
            let end = Instant::now() + retry;
            while Instant::now() < end && !stop.load(Ordering::Relaxed) { std::thread::sleep(Duration::from_millis(50)) }
            continue;
        };
        while !stop.load(Ordering::Relaxed) {
            let ok = r.picture(Duration::from_millis(200), |w, h, stride, rows| {
                let mut buf = vec![0u8; super::camera_feed::frame_bytes(size)];
                fit_picture(w, h, stride, rows, &mut buf, size);
                *latest.lock().unwrap_or_else(|e| e.into_inner()) = Some((Arc::new(buf), Instant::now()));
            });
            if !ok { break }
        }
    }
}

// ---- Sending ----

/// One of the app's NDI sources.
pub struct Sender { ndi: &'static Ndi, me: Ptr }

// SAFETY: NDI allows sending pictures and sound from different threads at once.
unsafe impl Send for Sender {}
unsafe impl Sync for Sender {}

impl Sender {
    pub fn new(name: &str) -> Option<Sender> {
        let ndi = runtime()?;
        let name = CString::new(name).ok()?;
        // The app sends at its own pace: NDI is not to hold the calls back to a clock.
        let settings = SendCreate { name: name.as_ptr(), groups: std::ptr::null(), clock_video: false, clock_audio: false };
        // SAFETY: the settings live until the call returns; NDI copies them.
        let s = unsafe { (ndi.send_create)(&settings) };
        (!s.is_null()).then_some(Sender { ndi, me: s })
    }

    /// Sends a `size` RGBA picture; `see_through`: its alpha counts.
    pub fn picture(&self, (w, h): (u32, u32), fps: u32, rgba: &[u8], see_through: bool) {
        if rgba.len() != w as usize * h as usize * 4 { return }
        let v = VideoFrame {
            xres: w as i32, yres: h as i32, fourcc: if see_through { RGBA } else { RGBX }, rate_n: fps as i32, rate_d: 1,
            aspect: w as f32 / h.max(1) as f32, format: PROGRESSIVE, timecode: SYNTHESIZE, data: rgba.as_ptr() as *mut u8,
            stride: (w * 4) as i32, metadata: std::ptr::null(), timestamp: 0,
        };
        // SAFETY: the send is synchronous: NDI is done with the picture when it returns.
        unsafe { (self.ndi.send_video)(self.me, &v) }
    }

    /// Sends sound: `planar` holds each channel's samples one after the other.
    pub fn sound(&self, rate: u32, channels: usize, planar: &[f32]) {
        let samples = planar.len() / channels.max(1);
        if samples == 0 { return }
        let a = AudioFrame {
            rate: rate as i32, channels: channels as i32, samples: samples as i32, timecode: SYNTHESIZE, data: planar.as_ptr() as *mut f32,
            channel_stride: (samples * 4) as i32, metadata: std::ptr::null(), timestamp: 0,
        };
        // SAFETY: as for pictures.
        unsafe { (self.ndi.send_audio)(self.me, &a) }
    }
}

impl Drop for Sender {
    fn drop(&mut self) { unsafe { (self.ndi.send_destroy)(self.me) } }
}

/// Interleaved samples (left, right, left, …) to each channel's samples one after the other, as NDI takes them.
pub fn planar(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    let n = interleaved.len() / channels.max(1);
    out.clear();
    out.resize(n * channels, 0.0);
    for (i, frame) in interleaved.chunks_exact(channels.max(1)).enumerate() {
        for (c, s) in frame.iter().enumerate() { out[c * n + i] = *s }
    }
}

/// A premultiplied RGBA picture (the words, `stream::pictures`) to straight alpha, as NDI takes it.
pub fn straight(premultiplied: &[u8], out: &mut Vec<u8>) {
    out.clear();
    out.extend_from_slice(premultiplied);
    for p in out.chunks_exact_mut(4) {
        let a = u32::from(p[3]);
        if a > 0 && a < 255 {
            for c in &mut p[..3] { *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8 }
        }
    }
}

/// What NDI out sends (Settings → Media → Livestream).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutPrefs {
    /// The stream's picture and sound.
    pub stream: bool,
    /// The words over the camera, see-through around them.
    pub words: bool,
}

impl OutPrefs {
    pub fn any(&self) -> bool { self.stream || self.words }
}

/// The words are sent again this often when they do not change, so a receiver that joins sees them.
const WORDS_AGAIN: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Shared {
    picture: Vec<u8>,
    picture_new: bool,
    words: Option<Arc<Vec<u8>>>,
    words_new: bool,
    stop: bool,
}

/// NDI out while the livestream runs: the writer hands it each picture and the words; threads of its own send them,
/// so the writer never waits for NDI. Dropping it stops it.
pub struct Out {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    stop: Arc<AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
    /// The local port the recorder's FFmpeg sends the stream's sound to (`record::args::Record::ndi_sound`).
    pub sound_port: Option<u16>,
    send_pictures: bool,
    send_words: bool,
    last_words: Option<Arc<Vec<u8>>>,
}

impl Out {
    /// Starts the NDI sources `prefs` asks for, for pictures of `size` at `fps`. `Err` when NDI is not installed.
    pub fn start(prefs: OutPrefs, size: (u32, u32), fps: u32) -> Result<Out, String> {
        if runtime().is_none() { return Err("the NDI runtime is not installed".into()) }
        let shared: Arc<(Mutex<Shared>, Condvar)> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let mut out = Out { shared: shared.clone(), stop: stop.clone(), threads: vec![], sound_port: None, send_pictures: false, send_words: false, last_words: None };
        let stream = if prefs.stream { Sender::new(STREAM_NAME).map(Arc::new) } else { None };
        let words = if prefs.words { Sender::new(WORDS_NAME) } else { None };
        if prefs.stream && stream.is_none() || prefs.words && words.is_none() { eprintln!("ndi: a source could not be made") }
        (out.send_pictures, out.send_words) = (stream.is_some(), words.is_some());
        if let Some(s) = &stream {
            // The sound: FFmpeg connects here and sends the stream's mix.
            match std::net::TcpListener::bind(("127.0.0.1", 0)) {
                Ok(listener) => {
                    out.sound_port = listener.local_addr().ok().map(|a| a.port());
                    let (s, stop) = (s.clone(), stop.clone());
                    if let Ok(t) = std::thread::Builder::new().name("ndi-sound".into()).spawn(move || pass_sound(listener, &s, &stop)) { out.threads.push(t) }
                }
                Err(e) => eprintln!("ndi: the stream goes out over NDI without its sound: {e}"),
            }
        }
        let sh = shared.clone();
        let t = std::thread::Builder::new().name("ndi-out".into()).spawn(move || send_pictures(&sh, stream, words, size, fps)).map_err(|e| e.to_string())?;
        out.threads.push(t);
        Ok(out)
    }

    /// The stream's picture now (`size` RGBA). Copied; the newest wins if NDI is still busy with the last.
    pub fn picture(&self, rgba: &[u8]) {
        if !self.send_pictures { return }
        let (m, cv) = &*self.shared;
        let mut s = m.lock().unwrap_or_else(|e| e.into_inner());
        s.picture.clear();
        s.picture.extend_from_slice(rgba);
        s.picture_new = true;
        cv.notify_one();
    }

    /// The words over the camera now (premultiplied RGBA, `None`: none).
    pub fn words(&mut self, words: &Option<Arc<Vec<u8>>>) {
        if !self.send_words { return }
        let same = match (&self.last_words, words) { (None, None) => true, (Some(a), Some(b)) => Arc::ptr_eq(a, b), _ => false };
        if same { return }
        self.last_words = words.clone();
        let (m, cv) = &*self.shared;
        let mut s = m.lock().unwrap_or_else(|e| e.into_inner());
        s.words = words.clone();
        s.words_new = true;
        cv.notify_one();
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let (m, cv) = &*self.shared;
        m.lock().unwrap_or_else(|e| e.into_inner()).stop = true;
        cv.notify_all();
        for t in self.threads.drain(..) { let _ = t.join(); }
    }
}

impl Drop for Out {
    fn drop(&mut self) { self.stop() }
}

fn send_pictures(shared: &(Mutex<Shared>, Condvar), stream: Option<Arc<Sender>>, words: Option<Sender>, size: (u32, u32), fps: u32) {
    let (m, cv) = shared;
    let mut picture: Vec<u8> = vec![];
    let clear = vec![0u8; size.0 as usize * size.1 as usize * 4];
    let mut words_now: Vec<u8> = clear.clone();
    let mut words_sent = Instant::now() - WORDS_AGAIN;
    loop {
        let (new_picture, new_words) = {
            let mut s = m.lock().unwrap_or_else(|e| e.into_inner());
            let wait = WORDS_AGAIN.saturating_sub(words_sent.elapsed());
            if !s.stop && !s.picture_new && !s.words_new { s = cv.wait_timeout(s, wait).unwrap_or_else(|e| e.into_inner()).0 }
            if s.stop { return }
            let p = s.picture_new.then(|| { std::mem::swap(&mut picture, &mut s.picture); s.picture_new = false });
            let w = s.words_new.then(|| { s.words_new = false; s.words.clone() });
            (p.is_some(), w)
        };
        if new_picture && let Some(st) = &stream { st.picture(size, fps, &picture, false) }
        if let Some(w) = &words {
            if let Some(new) = &new_words {
                match new { Some(p) if p.len() == clear.len() => straight(p, &mut words_now), _ => words_now.clone_from(&clear) }
            }
            if new_words.is_some() || words_sent.elapsed() >= WORDS_AGAIN {
                w.picture(size, fps, &words_now, true);
                words_sent = Instant::now();
            }
        }
    }
}

/// Takes the stream's sound from the recorder's FFmpeg (raw 48 kHz stereo float) and sends it over NDI, until
/// `stop`. It reads as fast as FFmpeg writes, so FFmpeg never waits for NDI.
fn pass_sound(listener: std::net::TcpListener, sender: &Sender, stop: &AtomicBool) {
    let _ = listener.set_nonblocking(true);
    let mut conn = loop {
        if stop.load(Ordering::Relaxed) { return }
        match listener.accept() {
            Ok((c, _)) => break c,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => { eprintln!("ndi: no sound from the recorder: {e}"); return }
        }
    };
    let _ = conn.set_nonblocking(false);
    let _ = conn.set_read_timeout(Some(Duration::from_millis(200)));
    // 10 ms of sound at a time: each send is a short piece, close to the picture.
    let frame = SOUND_CHANNELS * 4;
    let mut buf = vec![0u8; (SOUND_RATE as usize / 100) * frame];
    let (mut have, mut samples, mut planar_buf) = (0usize, Vec::<f32>::new(), Vec::<f32>::new());
    while !stop.load(Ordering::Relaxed) {
        match conn.read(&mut buf[have..]) {
            Ok(0) => break,
            Ok(n) => {
                have += n;
                let whole = have / frame * frame;
                if whole == 0 { continue }
                samples.clear();
                samples.extend(buf[..whole].chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])));
                planar(&samples, SOUND_CHANNELS, &mut planar_buf);
                sender.sound(SOUND_RATE, SOUND_CHANNELS, &planar_buf);
                buf.copy_within(whole..have, 0);
                have -= whole;
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ndi_camera_is_named_by_its_source() {
        assert_eq!(source_name("ndi://STUDIO (Camera 1)"), Some("STUDIO (Camera 1)"));
        assert_eq!(source_name("  NDI://PC (OBS) "), Some("PC (OBS)"));
        assert_eq!(source_name("ndi://"), None);
        assert_eq!(source_name("Integrated Camera"), None);
        assert_eq!(source_name("rtsp://cam/1"), None);
        assert_eq!(input_source(&[INPUT.into(), "PC (OBS)".into()]), Some("PC (OBS)"));
        assert_eq!(input_source(&["-f".into(), "dshow".into()]), None);
        assert_eq!(label("PC (OBS)"), "NDI: PC (OBS)");
    }

    #[test]
    fn the_runtime_is_looked_for_where_the_installers_say() {
        let env = |v: &str| (v == "NDI_RUNTIME_DIR_V6").then(|| OsString::from("C:/NDI/Runtime"));
        let c = candidates(env);
        assert_eq!(c, vec![PathBuf::from("C:/NDI/Runtime").join(LIBRARY), PathBuf::from(LIBRARY)]);
        assert_eq!(candidates(|_| None)[0], PathBuf::from(LIBRARY));
        use crate::platform::tools::Os;
        assert!(known(Os::Windows).is_empty());
        assert!(known(Os::MacOs).iter().all(|p| p.ends_with("libndi.dylib")));
        assert!(known(Os::Linux).iter().all(|p| p.contains("libndi.so.")));
    }

    #[test]
    fn the_sound_goes_to_ndi_a_channel_at_a_time() {
        let mut out = vec![];
        planar(&[1.0, -1.0, 2.0, -2.0, 3.0, -3.0], 2, &mut out);
        assert_eq!(out, vec![1.0, 2.0, 3.0, -1.0, -2.0, -3.0]);
    }

    #[test]
    fn the_words_go_out_with_straight_alpha() {
        let mut out = vec![];
        // Clear, half see-through white, solid red.
        straight(&[0, 0, 0, 0, 128, 128, 128, 128, 255, 0, 0, 255], &mut out);
        assert_eq!(out, vec![0, 0, 0, 0, 255, 255, 255, 128, 255, 0, 0, 255]);
    }

    #[test]
    fn an_ndi_picture_keeps_its_shape() {
        // 2×1 red and blue, rows padded to 12 bytes; into 4×4: black bars above and below.
        let rows = [255, 0, 0, 7, 0, 0, 255, 7, 9, 9, 9, 9];
        let mut dst = vec![1u8; 4 * 4 * 4];
        fit_picture(2, 1, 12, &rows, &mut dst, (4, 4));
        let px = |x: usize, y: usize| &dst[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4];
        assert_eq!(px(0, 0), [0, 0, 0, 255], "bar");
        assert_eq!(px(0, 1), [255, 0, 0, 255], "red, opaque");
        assert_eq!(px(3, 2), [0, 0, 255, 255], "blue");
    }

    /// With the NDI runtime installed (NDI Tools): the app's own source is found and its pictures come back
    /// through a receiver, as an NDI camera's would.
    #[test]
    fn a_picture_sent_over_ndi_comes_back() {
        if runtime().is_none() { eprintln!("no NDI runtime here: skipped"); return }
        let name = format!("RustyPresenter test {}", std::process::id());
        let sender = Sender::new(&name).expect("sender");
        let size = (64u32, 36u32);
        let red: Vec<u8> = [200, 10, 10, 255].repeat(64 * 36);
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let sending = std::thread::spawn(move || while !s2.load(Ordering::Relaxed) { sender.picture(size, 30, &red, false); std::thread::sleep(Duration::from_millis(33)) });
        // The full name has the computer's name before it.
        let found = (0..10).find_map(|_| sources(Duration::from_millis(1000)).into_iter().find(|s| s.ends_with(&format!("({name})"))));
        let found = found.expect("the app's NDI source on the network");
        let latest: super::super::camera_feed::Latest = Arc::default();
        let (l2, st) = (latest.clone(), stop.clone());
        let receiving = std::thread::spawn(move || receive(&found, (32, 18), &l2, &st, Duration::from_millis(200)));
        let began = Instant::now();
        let picture = loop {
            if let Some(p) = super::super::camera_feed::fresh(&latest, Instant::now()) { break Some(p) }
            if began.elapsed() > Duration::from_secs(15) { break None }
            std::thread::sleep(Duration::from_millis(50));
        };
        stop.store(true, Ordering::Relaxed);
        let _ = (sending.join(), receiving.join());
        let p = picture.expect("a picture over NDI");
        let mid = (9 * 32 + 16) * 4;
        assert!(p[mid] > 150 && p[mid + 1] < 60 && p[mid + 3] == 255, "red: {:?}", &p[mid..mid + 4]);
    }
}
