//! One libmpv player per playing video/audio instance. Reports go out tagged
//! with the instance id, so the core can ignore a retired instance.
use super::ffi::{self, *};
use crate::live::{PlayState, RoomEvent};
use crate::model::EndAction;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::cell::RefCell;
use std::ffi::c_int;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// What a player is for; decides looping, sound and video.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Role {
    /// Muted, loops forever.
    Background,
    /// With sound; loops for `Loop`, holds the last frame for `Hold`.
    Foreground(EndAction),
    /// Sound only.
    Audio { looped: bool },
}

/// Options fixed when the player is created.
#[derive(Clone, Debug)]
pub struct Options {
    /// `libmpv` when a render context will be attached, `null` for probes and tests.
    pub vo: &'static str,
    /// mpv audio output driver; empty = mpv's default.
    pub ao: &'static str,
    /// From settings; empty = automatic.
    pub audio_device: String,
}

impl Default for Options {
    fn default() -> Self {
        Options { vo: "libmpv", ao: "", audio_device: String::new() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerReport {
    pub inst: u64,
    /// The [`Player`] that sent it (unique in the process); 0 when none could be made.
    pub player: u64,
    pub state: PlayState,
    pub position: f64,
    pub duration: f64,
    pub error: Option<String>,
}

impl From<PlayerReport> for RoomEvent {
    fn from(r: PlayerReport) -> Self {
        RoomEvent::Media { inst: r.inst, state: r.state, position: r.position, duration: r.duration, error: r.error }
    }
}

/// One mpv core, shared by its player, event thread and render context. libmpv
/// requires the render context freed before the core is destroyed; holding the
/// core keeps it alive until the last of them is gone.
struct Core {
    mpv: Arc<Mpv>,
    h: *mut mpv_handle,
    grabs: Grabs,
}
// The client API is thread-safe; the handle is destroyed only when the last owner drops.
unsafe impl Send for Core {}
unsafe impl Sync for Core {}

impl Drop for Core {
    fn drop(&mut self) {
        // Shutting the core down can take a moment; never on the caller's (UI) thread.
        let (mpv, h) = (self.mpv.clone(), Handle(self.h));
        let destroy = move || {
            let h = h;
            unsafe { (mpv.terminate_destroy)(h.0) }
        };
        let _ = std::thread::Builder::new().name("mpv-destroy".into()).spawn(destroy);
    }
}

struct Handle(*mut mpv_handle);
unsafe impl Send for Handle {}

/// How long a grab waits for mpv. With `vo=libmpv` the screenshot is taken on the render
/// thread (the room's Slint render), so a room that is not drawing never answers.
const GRAB_TIMEOUT: Duration = Duration::from_millis(500);

/// A `screenshot-raw` result, copied out of mpv. The live preview and import probes both read frames this way.
#[derive(Debug)]
pub(crate) struct RawFrame {
    w: u32,
    h: u32,
    stride: usize,
    bgrx: Vec<u8>,
}

impl RawFrame {
    /// Reads the `screenshot-raw` result `{w, h, stride, format, data}`, keeping its pixels without another copy.
    pub(crate) fn from_node(n: Node) -> Option<RawFrame> {
        if n.get("format")?.as_str()? != "bgr0" {
            return None;
        }
        let dim = |key| match n.get(key)? {
            Node::Int(v) => u32::try_from(*v).ok().filter(|&v| v > 0),
            _ => None,
        };
        let (w, h) = (dim("w")?, dim("h")?);
        // A flipped image would have a negative stride; `dim` turns that into a missing frame.
        let stride = dim("stride")? as usize;
        let need = stride.checked_mul(h as usize)?;
        let Node::Map(entries) = n else { return None };
        let mut bgrx = entries.into_iter().find_map(|(k, v)| match v {
            Node::Bytes(data) if k == "data" => Some(data),
            _ => None,
        })?;
        if stride < w as usize * 4 || bgrx.len() < need {
            return None;
        }
        bgrx.truncate(need);
        Some(RawFrame { w, h, stride, bgrx })
    }

    /// RGB at `max_w` pixels wide or less (0: any width), keeping the shape. Never enlarges.
    pub(crate) fn to_rgb(&self, max_w: u32) -> Option<image::RgbImage> {
        let mut rgb = Vec::with_capacity(self.w as usize * self.h as usize * 3);
        for row in self.bgrx.chunks_exact(self.stride) {
            for px in row[..self.w as usize * 4].chunks_exact(4) {
                rgb.extend_from_slice(&[px[2], px[1], px[0]]);
            }
        }
        let img = image::RgbImage::from_raw(self.w, self.h, rgb)?;
        if max_w == 0 || self.w <= max_w {
            return Some(img);
        }
        let h = (self.h as u64 * max_w as u64 / self.w as u64).max(1) as u32;
        Some(image::imageops::resize(&img, max_w, h, image::imageops::FilterType::Triangle))
    }
}

/// The screenshot request mpv owes an answer to. There is only ever one: mpv answers every
/// async command in the end, so a request that timed out keeps the slot until its late reply
/// comes, and a room that is not drawing cannot pile up requests at 4 Hz.
#[derive(Default)]
struct Grabs(Mutex<GrabSlot>);

#[derive(Default)]
struct GrabSlot {
    next_id: u64,
    waiting: Option<(u64, Sender<Option<RawFrame>>)>,
    closed: bool,
}

impl Grabs {
    fn slot(&self) -> std::sync::MutexGuard<'_, GrabSlot> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Reserves the slot: the reply id and where the frame will arrive. `None` while an answer
    /// is still owed, or once the event thread has stopped.
    fn begin(&self) -> Option<(u64, Receiver<Option<RawFrame>>)> {
        let mut s = self.slot();
        if s.closed || s.waiting.is_some() {
            return None;
        }
        s.next_id += 1;
        let id = s.next_id;
        let (tx, rx) = channel();
        s.waiting = Some((id, tx));
        Some((id, rx))
    }

    /// Hands over the reply to request `id`. `read` runs only for the waiting request, because
    /// the reply's data is gone by the next `wait_event`.
    fn finish(&self, id: u64, read: impl FnOnce() -> Option<RawFrame>) {
        let mut s = self.slot();
        if s.waiting.as_ref().is_some_and(|w| w.0 == id) {
            if let Some((_, tx)) = s.waiting.take() {
                let _ = tx.send(read()); // fails when the grab already timed out
            }
        }
    }

    /// The command never reached mpv, so no reply will come.
    fn abandon(&self, id: u64) {
        self.finish(id, || None);
    }

    /// The event thread stopped: wakes a waiting grab and refuses new ones.
    fn close(&self) {
        let mut s = self.slot();
        s.closed = true;
        s.waiting = None;
    }
}

/// Takes frames of a playing video for the operator's live preview, without a second player.
/// It does not keep the player alive. One grab runs at a time; a second one meanwhile gets `None`.
#[derive(Clone)]
pub struct FrameGrabber {
    core: Weak<Core>,
}

impl FrameGrabber {
    /// The current frame, at most `max_w` pixels wide. `None` when there is none within 500 ms,
    /// mpv refuses, or the player is gone. Blocks, so never call it on the UI thread.
    pub fn grab(&self, max_w: u32) -> Option<SharedPixelBuffer<Rgb8Pixel>> {
        // While `core` is held `Core::drop` cannot run, so the handle stays valid for the command.
        let core = self.core.upgrade()?;
        let (id, rx) = core.grabs.begin()?;
        // Asynchronous, because a synchronous screenshot would wait for the room to redraw.
        if unsafe { core.mpv.command_async(core.h, id, &["screenshot-raw", "video"]) }.is_err() {
            core.grabs.abandon(id);
            return None;
        }
        let frame = rx.recv_timeout(GRAB_TIMEOUT).ok().flatten();
        drop(core);
        let img = frame?.to_rgb(max_w)?;
        let mut pb = SharedPixelBuffer::<Rgb8Pixel>::new(img.width(), img.height());
        for (d, s) in pb.make_mut_slice().iter_mut().zip(img.as_raw().chunks_exact(3)) {
            *d = Rgb8Pixel { r: s[0], g: s[1], b: s[2] };
        }
        Some(pb)
    }
}

/// An mpv render context. It keeps its player's core alive, so the core is never
/// destroyed before the context is freed. Free it (drop it) in `RenderingTeardown`,
/// on the thread that owns its GL context, with that context current.
pub struct RenderContext {
    core: Arc<Core>,
    ctx: *mut mpv_render_context,
}

impl RenderContext {
    /// # Safety
    /// `ctx` must come from `render_context_create` on `player.handle()` and not be
    /// freed elsewhere. The value must be dropped on the thread that owns the GL
    /// context it was created with, while that context is current.
    pub unsafe fn from_raw(player: &Player, ctx: *mut mpv_render_context) -> Self {
        RenderContext { core: player.core.clone(), ctx }
    }

    pub fn raw(&self) -> *mut mpv_render_context {
        self.ctx
    }
}

impl Drop for RenderContext {
    fn drop(&mut self) {
        // Runs before `core` is released, so the core is still alive here.
        unsafe { (self.core.mpv.render_context_free)(self.ctx) }
    }
}

/// Observed properties, by reply id.
const OBS_PAUSE: u64 = 1;
const OBS_TIME: u64 = 2;
const OBS_DURATION: u64 = 3;
const OBS_EOF: u64 = 4;
/// Position reports at most this often (as the web room); state changes go out at once.
const TICK: Duration = Duration::from_millis(500);

/// Serials for [`Player::serial`]; 0 is never handed out.
static SERIAL: AtomicU64 = AtomicU64::new(1);

/// A libmpv instance playing one video/audio instance, with a thread turning its
/// events into [`PlayerReport`]s.
pub struct Player {
    core: Arc<Core>,
    inst: u64,
    serial: u64,
    tx: Sender<PlayerReport>,
    name: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// Filled by the drawing code, which takes it out again in `RenderingTeardown`.
    render: RefCell<Option<RenderContext>>,
    /// `vo=libmpv`: `loadfile` waits for the render context, or mpv drops the video track.
    deferred: bool,
    pending: RefCell<Option<(PathBuf, f64)>>,
    /// The last file sent to mpv.
    loaded: RefCell<Option<PathBuf>>,
    /// File and position when the render context was taken out (mpv then turns video off).
    lost: RefCell<Option<(PathBuf, f64)>>,
}

impl Player {
    pub fn new(mpv: Arc<Mpv>, role: Role, inst: u64, reports: Sender<PlayerReport>) -> Result<Player, String> {
        Player::with_options(mpv, role, inst, reports, &Options::default())
    }

    pub fn with_options(mpv: Arc<Mpv>, role: Role, inst: u64, reports: Sender<PlayerReport>, opts: &Options) -> Result<Player, String> {
        let h = unsafe { (mpv.create)() };
        if h.is_null() {
            return Err("The video player could not start.".into());
        }
        // Dropping `core` on an early return destroys the handle.
        let core = Arc::new(Core { mpv: mpv.clone(), h, grabs: Grabs::default() });
        let lp = |on: bool| if on { "inf" } else { "no" };
        let mut set = vec![("vo", opts.vo), ("hwdec", "auto-safe"), ("idle", "yes"), ("input-default-bindings", "no"), ("terminal", "no"), ("keep-open-pause", "no")];
        if !opts.ao.is_empty() {
            set.push(("ao", opts.ao));
        }
        if !opts.audio_device.is_empty() {
            set.push(("audio-device", opts.audio_device.as_str()));
        }
        match role {
            Role::Background => set.extend([("mute", "yes"), ("loop-file", "inf")]),
            Role::Foreground(end) => set.extend([("loop-file", lp(end == EndAction::Loop)), ("keep-open", if end == EndAction::Hold { "yes" } else { "no" })]),
            Role::Audio { looped } => set.extend([("vid", "no"), ("loop-file", lp(looped))]),
        }
        for (k, v) in set {
            if let Err(e) = unsafe { mpv.set_option(h, k, v) } {
                return Err(e);
            }
        }
        let _ = unsafe { mpv.set_option(h, "osc", "no") }; // only exists in builds with Lua
        let r = unsafe { (mpv.initialize)(h) };
        if r < 0 {
            return Err(format!("The video player could not start: {}", mpv.err(r)));
        }
        for (id, name, f) in [(OBS_PAUSE, "pause", FORMAT_FLAG), (OBS_TIME, "time-pos", FORMAT_DOUBLE), (OBS_DURATION, "duration", FORMAT_DOUBLE), (OBS_EOF, "eof-reached", FORMAT_FLAG)] {
            if let Err(e) = unsafe { mpv.observe(h, id, name, f) } {
                return Err(e);
            }
        }
        let name = Arc::new(Mutex::new(String::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let w = Watch { core: core.clone(), inst, serial, tx: reports.clone(), name: name.clone(), stop: stop.clone(), loaded: false, paused: false, position: 0.0, duration: 0.0, last: None, last_at: Instant::now() };
        let thread = match std::thread::Builder::new().name(format!("mpv-events-{inst}")).spawn(move || { crate::recovery::panic::mark_critical(); w.run() }) {
            Ok(t) => t,
            Err(e) => return Err(format!("The video player could not start: {e}")),
        };
        let deferred = opts.vo == "libmpv";
        Ok(Player { core, inst, serial, tx: reports, name, stop, thread: Some(thread), render: RefCell::new(None), deferred, pending: RefCell::new(None), loaded: RefCell::new(None), lost: RefCell::new(None) })
    }

    pub fn inst(&self) -> u64 {
        self.inst
    }

    /// Tells this player's reports from those of another player for the same instance.
    pub fn serial(&self) -> u64 {
        self.serial
    }

    /// Stores the drawing code's render context with this player and starts a
    /// load that was waiting for it.
    pub fn set_render(&self, rc: RenderContext) {
        *self.render.borrow_mut() = Some(rc);
        let pending = self.pending.borrow_mut().take();
        if let Some((path, start)) = pending {
            self.load(&path, start);
        } else if let Some((path, at)) = self.lost.take() {
            // mpv turned video off for good when the old context was freed; reload where it was.
            self.load(&path, at);
        }
    }

    /// Takes the render context out, to free it with its GL context current. Freeing
    /// it while a file plays turns video off for good (or fails the file), so the
    /// file is stopped quietly here and `set_render` reloads it where it was.
    pub fn take_render(&self) -> Option<RenderContext> {
        let rc = self.render.borrow_mut().take();
        if let (true, Some(path)) = (rc.is_some(), self.loaded.borrow_mut().take()) {
            *self.lost.borrow_mut() = Some((path, self.position()));
            let _ = unsafe { self.core.mpv.command(self.core.h, &["stop"]) };
        }
        rc
    }

    pub fn has_render(&self) -> bool {
        self.render.borrow().is_some()
    }

    /// The attached render context, if any (valid until it is taken out).
    pub fn render_raw(&self) -> Option<*mut mpv_render_context> {
        self.render.borrow().as_ref().map(RenderContext::raw)
    }

    pub fn mpv(&self) -> &Arc<Mpv> {
        &self.core.mpv
    }

    /// Starts `path` at `start` seconds, replacing whatever was loaded. With
    /// `vo=libmpv` this waits until a render context is attached.
    pub fn load(&self, path: &Path, start: f64) {
        if self.deferred && !self.has_render() {
            *self.lost.borrow_mut() = None;
            *self.pending.borrow_mut() = Some((path.to_path_buf(), start));
            return;
        }
        let file = path.to_string_lossy();
        let name = path.file_name().map_or_else(|| file.to_string(), |n| n.to_string_lossy().into_owned());
        *self.name.lock().unwrap_or_else(|e| e.into_inner()) = name;
        let (mpv, h) = (&self.core.mpv, self.core.h);
        let r = unsafe { mpv.set_property(h, "start", &format!("{}", start.max(0.0))).and_then(|_| mpv.command(h, &["loadfile", &file])) };
        *self.loaded.borrow_mut() = Some(path.to_path_buf());
        if let Err(e) = r {
            self.fail(e);
        }
    }

    /// Reports an error for this instance (e.g. it cannot be drawn).
    pub fn fail(&self, error: String) {
        let _ = self.tx.send(PlayerReport { inst: self.inst, player: self.serial, state: PlayState::Error, position: 0.0, duration: 0.0, error: Some(error) });
    }

    pub fn set_paused(&self, paused: bool) {
        let _ = unsafe { self.core.mpv.set_property(self.core.h, "pause", if paused { "yes" } else { "no" }) };
    }

    /// Moves this player to another sound output. Empty is the system default, as in `Options`.
    pub fn set_audio_device(&self, name: &str) {
        let name = if name.is_empty() { "auto" } else { name };
        let _ = unsafe { self.core.mpv.set_property(self.core.h, "audio-device", name) };
    }

    /// Current playback position in seconds (0 when nothing is playing).
    pub fn position(&self) -> f64 {
        unsafe { self.core.mpv.get_double(self.core.h, "time-pos") }.unwrap_or(0.0)
    }

    pub fn handle(&self) -> *mut mpv_handle {
        self.core.h
    }

    /// A frame grabber for this player's live preview; it may outlive the player.
    pub fn grabber(&self) -> FrameGrabber {
        FrameGrabber { core: Arc::downgrade(&self.core) }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        unsafe { (self.core.mpv.wakeup)(self.core.h) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // A context left in the slot is freed here, on the dropping thread; the
        // drawing code should have taken it out in `RenderingTeardown`.
        drop(self.render.get_mut().take());
        // The core (and its handle) goes once no render context holds it.
    }
}

/// The event thread's view of one player.
struct Watch {
    core: Arc<Core>,
    inst: u64,
    serial: u64,
    tx: Sender<PlayerReport>,
    name: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
    loaded: bool,
    paused: bool,
    position: f64,
    duration: f64,
    last: Option<PlayState>,
    last_at: Instant,
}

impl Watch {
    fn run(mut self) {
        self.events();
        self.core.grabs.close();
    }

    fn events(&mut self) {
        loop {
            let ev = unsafe { &*(self.core.mpv.wait_event)(self.core.h, -1.0) };
            if self.stop.load(Ordering::Acquire) || ev.event_id == EVENT_SHUTDOWN {
                return;
            }
            match ev.event_id {
                EVENT_START_FILE => (self.loaded, self.last, self.position, self.duration) = (false, None, 0.0, 0.0),
                EVENT_PLAYBACK_RESTART => {
                    self.loaded = true;
                    self.state(if self.paused { PlayState::Paused } else { PlayState::Playing }, None);
                }
                EVENT_END_FILE if !ev.data.is_null() => {
                    self.loaded = false;
                    let end = unsafe { &*(ev.data as *const mpv_event_end_file) };
                    match end.reason {
                        END_FILE_EOF => self.state(PlayState::Ended, None),
                        END_FILE_ERROR => {
                            let name = self.name.lock().unwrap_or_else(|e| e.into_inner()).clone();
                            self.state(PlayState::Error, Some(format!("{name} could not be played ({})", self.core.mpv.err(end.error))));
                        }
                        _ => {}
                    }
                }
                EVENT_COMMAND_REPLY => self.core.grabs.finish(ev.reply_userdata, || {
                    if ev.error < 0 || ev.data.is_null() {
                        return None;
                    }
                    // mpv owns the reply and frees it at the next `wait_event`, so it is copied, never freed.
                    RawFrame::from_node(unsafe { Node::from_raw(&(*(ev.data as *const mpv_event_command)).result) })
                }),
                EVENT_PROPERTY_CHANGE if !ev.data.is_null() => self.property(ev.reply_userdata, unsafe { &*(ev.data as *const mpv_event_property) }),
                _ => {}
            }
        }
    }

    fn property(&mut self, id: u64, p: &mpv_event_property) {
        if p.data.is_null() {
            return; // unavailable (format NONE), e.g. time-pos while idle
        }
        let flag = || p.format == FORMAT_FLAG && unsafe { *(p.data as *const c_int) } != 0;
        let num = || (p.format == FORMAT_DOUBLE).then(|| unsafe { *(p.data as *const f64) });
        let running = matches!(self.last, Some(PlayState::Playing | PlayState::Paused));
        match id {
            OBS_PAUSE => {
                self.paused = flag();
                if self.loaded && running {
                    self.state(if self.paused { PlayState::Paused } else { PlayState::Playing }, None);
                }
            }
            OBS_TIME => {
                let Some(t) = num() else { return };
                self.position = t;
                if self.loaded && running && self.last_at.elapsed() >= TICK {
                    let s = self.last.clone().unwrap_or(PlayState::Playing);
                    self.send(s, None);
                }
            }
            OBS_DURATION => self.duration = num().unwrap_or(self.duration),
            OBS_EOF if self.loaded && flag() => self.state(PlayState::Ended, None),
            _ => {}
        }
    }

    /// Reports a state change (repeats of the same state are dropped).
    fn state(&mut self, s: PlayState, error: Option<String>) {
        if self.last.as_ref() != Some(&s) {
            self.last = Some(s.clone());
            self.send(s, error);
        }
    }

    fn send(&mut self, state: PlayState, error: Option<String>) {
        self.last_at = Instant::now();
        let _ = self.tx.send(PlayerReport { inst: self.inst, player: self.serial, state, position: self.position, duration: self.duration, error });
    }
}

/// Creates players for the room, or reports for the instance why it cannot.
pub struct Players {
    mpv: Option<Arc<Mpv>>,
    reports: Sender<PlayerReport>,
    pub options: Options,
}

/// Player factory; with no libmpv every instance reports [`ffi::MISSING`].
pub fn players(mpv: Option<Arc<Mpv>>, reports: Sender<PlayerReport>) -> Players {
    Players { mpv, reports, options: Options::default() }
}

impl Players {
    /// `audio_device` is `Settings::audio_device` as it is now, so a change applies from the next item.
    pub fn create(&self, role: Role, inst: u64, audio_device: &str) -> Option<Player> {
        let opts = Options { audio_device: audio_device.to_string(), ..self.options.clone() };
        let r = match &self.mpv {
            Some(m) => Player::with_options(m.clone(), role, inst, self.reports.clone(), &opts),
            None => Err(ffi::MISSING.into()),
        };
        r.map_err(|e| {
            let _ = self.reports.send(PlayerReport { inst, player: 0, state: PlayState::Error, position: 0.0, duration: 0.0, error: Some(e) });
        })
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::{Duration, Instant};

    fn mpv() -> Option<Arc<Mpv>> {
        if std::env::var("RP_VIDEO_TESTS").as_deref() != Ok("1") {
            return None;
        }
        Some(Mpv::load().expect("libmpv (set RP_LIBMPV)"))
    }

    /// A silent, windowless player for `inst`.
    fn start(mpv: &Arc<Mpv>, role: Role, inst: u64, src: &str) -> (Player, Receiver<PlayerReport>) {
        let (tx, rx) = channel();
        let opts = Options { vo: "null", ao: "null", audio_device: String::new() };
        let p = Player::with_options(mpv.clone(), role, inst, tx, &opts).unwrap();
        p.load(Path::new(src), 0.0);
        (p, rx)
    }

    /// Reports until `done` matches one (included) or `secs` pass.
    fn until(rx: &Receiver<PlayerReport>, secs: f64, done: impl Fn(&PlayerReport) -> bool) -> (Vec<PlayerReport>, bool) {
        let end = Instant::now() + Duration::from_secs_f64(secs);
        let mut v = Vec::new();
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(r) => {
                    let hit = done(&r);
                    v.push(r);
                    if hit {
                        return (v, true);
                    }
                }
                Err(_) => break,
            }
        }
        (v, false)
    }

    #[test]
    fn audio_sine_plays_then_ends() {
        let Some(mpv) = mpv() else { return };
        let (_p, rx) = start(&mpv, Role::Audio { looped: false }, 7, "av://lavfi:sine=frequency=440:duration=1");
        let (v, ended) = until(&rx, 5.0, |r| r.state == PlayState::Ended);
        assert!(ended, "{v:?}");
        let playing = v.iter().position(|r| r.state == PlayState::Playing).expect("playing first");
        assert!(playing < v.len() - 1, "{v:?}");
        assert!(v.iter().all(|r| r.inst == 7), "{v:?}");
    }

    #[test]
    fn video_reports_duration_and_position() {
        let Some(mpv) = mpv() else { return };
        let (p, rx) = start(&mpv, Role::Foreground(EndAction::Stop), 3, "av://lavfi:testsrc2=size=320x240:rate=30:duration=2");
        let (v, _) = until(&rx, 5.0, |r| r.state == PlayState::Playing && r.position > 0.5);
        assert!(p.position() > 0.5, "{v:?}");
        let (rest, ended) = until(&rx, 5.0, |r| r.state == PlayState::Ended);
        assert!(ended, "{rest:?}");
        let all: Vec<_> = v.into_iter().chain(rest).collect();
        assert!(all.iter().any(|r| (r.duration - 2.0).abs() < 0.2), "{all:?}");
        assert!(all.iter().any(|r| r.position > 1.0), "{all:?}");
        // 2 s of time-pos at most every 500 ms, plus state changes.
        assert!(all.len() <= 8, "{} reports: {all:?}", all.len());
    }

    #[test]
    fn pause_reports_paused() {
        let Some(mpv) = mpv() else { return };
        let (p, rx) = start(&mpv, Role::Foreground(EndAction::Stop), 4, "av://lavfi:testsrc2=size=320x240:rate=30:duration=10");
        assert!(until(&rx, 5.0, |r| r.state == PlayState::Playing).1);
        p.set_paused(true);
        let (v, paused) = until(&rx, 2.0, |r| r.state == PlayState::Paused);
        assert!(paused, "{v:?}");
        p.set_paused(false);
        assert!(until(&rx, 2.0, |r| r.state == PlayState::Playing).1);
    }

    #[test]
    fn missing_file_reports_error_with_reason() {
        let Some(mpv) = mpv() else { return };
        let path = std::env::temp_dir().join("rp-no-such-dir").join("sermon-video.mp4");
        let (_p, rx) = start(&mpv, Role::Foreground(EndAction::Stop), 5, path.to_str().unwrap());
        let (v, failed) = until(&rx, 5.0, |r| r.state == PlayState::Error);
        assert!(failed, "{v:?}");
        let e = v.last().unwrap().error.clone().unwrap_or_default();
        assert!(e.contains("sermon-video.mp4") && e.contains("could not be played"), "{e}");
        assert_eq!(v.last().unwrap().inst, 5);
    }

    #[test]
    fn loop_never_ends() {
        let Some(mpv) = mpv() else { return };
        let (_p, rx) = start(&mpv, Role::Background, 6, "av://lavfi:testsrc2=size=320x240:rate=30:duration=1.5");
        let (v, stopped) = until(&rx, 4.0, |r| matches!(r.state, PlayState::Ended | PlayState::Error));
        assert!(!stopped, "{v:?}");
        assert!(v.iter().any(|r| r.state == PlayState::Playing), "{v:?}");
    }

    #[test]
    fn hold_reports_ended_and_keeps_the_last_frame() {
        let Some(mpv) = mpv() else { return };
        let (p, rx) = start(&mpv, Role::Foreground(EndAction::Hold), 10, "av://lavfi:testsrc2=size=320x240:rate=30:duration=1");
        let (v, ended) = until(&rx, 5.0, |r| r.state == PlayState::Ended);
        assert!(ended, "{v:?}");
        assert!(p.position() > 0.5, "file stays loaded on its last frame");
        let (after, _) = until(&rx, 1.0, |_| true);
        assert!(after.is_empty(), "{after:?}");
    }

    #[test]
    fn drop_stops_reports() {
        let Some(mpv) = mpv() else { return };
        let (p, rx) = start(&mpv, Role::Foreground(EndAction::Loop), 8, "av://lavfi:testsrc2=size=320x240:rate=30:duration=10");
        assert!(until(&rx, 5.0, |r| r.state == PlayState::Playing).1);
        drop(p);
        while rx.try_recv().is_ok() {}
        // The event thread is gone (its sender dropped), so nothing more can arrive.
        assert_eq!(rx.recv_timeout(Duration::from_millis(100)), Err(RecvTimeoutError::Disconnected));
    }

    /// A software render context on `p` (no GL needed).
    fn sw_render(p: &Player) -> RenderContext {
        let mpv = &p.core.mpv;
        let mut params = [
            mpv_render_param { kind: PARAM_API_TYPE, data: RENDER_API_TYPE_SW.as_ptr() as *mut _ },
            mpv_render_param { kind: PARAM_INVALID, data: std::ptr::null_mut() },
        ];
        let mut ctx = std::ptr::null_mut();
        let r = unsafe { (mpv.render_context_create)(&mut ctx, p.handle(), params.as_mut_ptr()) };
        assert!(r >= 0 && !ctx.is_null(), "{}", mpv.err(r));
        unsafe { RenderContext::from_raw(p, ctx) }
    }

    #[test]
    fn versions_are_read_from_a_throwaway_handle() {
        let Some(mpv) = mpv() else { return };
        let v = mpv.versions();
        assert!(v.mpv.starts_with("mpv "), "{}", v.mpv);
        assert!(!v.ffmpeg.is_empty());
        assert!(!v.configuration.is_empty());
        assert_eq!(mpv.versions(), v, "read once, then kept");
    }

    #[test]
    fn render_context_keeps_the_core_alive_after_its_player_drops() {
        let Some(mpv) = mpv() else { return };
        let (tx, _rx) = channel();
        let p = Player::with_options(mpv.clone(), Role::Background, 11, tx, &Options { vo: "libmpv", ao: "null", audio_device: String::new() }).unwrap();
        p.set_render(sw_render(&p));
        assert!(p.has_render());
        let rc = p.take_render().expect("render context");
        assert!(!p.has_render());
        drop(p);
        // The player is gone but the context still holds the core, so the handle is live.
        assert_eq!(Arc::strong_count(&rc.core), 1);
        assert!(unsafe { mpv.get_string(rc.core.h, "mpv-version") }.is_some());
        drop(rc); // frees the context, then destroys the core
    }

    #[test]
    fn player_drop_frees_a_render_context_left_in_its_slot() {
        let Some(mpv) = mpv() else { return };
        let (tx, _rx) = channel();
        let p = Player::with_options(mpv, Role::Background, 12, tx, &Options { vo: "libmpv", ao: "null", audio_device: String::new() }).unwrap();
        p.set_render(sw_render(&p));
        let core = Arc::downgrade(&p.core);
        drop(p);
        assert!(core.upgrade().is_none(), "core released once the context is freed");
    }

    #[test]
    fn libmpv_output_loads_once_a_render_context_exists() {
        let Some(mpv) = mpv() else { return };
        let (tx, rx) = channel();
        let p = Player::with_options(mpv.clone(), Role::Background, 13, tx, &Options { vo: "libmpv", ao: "null", audio_device: String::new() }).unwrap();
        p.load(Path::new("av://lavfi:testsrc2=size=320x240:rate=30"), 0.0);
        // Loading before the render context exists would deselect the video track.
        let (early, _) = until(&rx, 0.5, |_| true);
        assert!(early.is_empty(), "{early:?}");
        p.set_render(sw_render(&p));
        let (v, playing) = until(&rx, 5.0, |r| r.state == PlayState::Playing);
        assert!(playing, "{v:?}");
        assert_ne!(unsafe { mpv.get_string(p.handle(), "vid") }.as_deref(), Some("no"));
        drop(p.take_render());
    }

    /// Frames `rc` draws (in software, 64x64) within `secs`.
    fn frames(p: &Player, rc: *mut mpv_render_context, secs: f64) -> usize {
        let mpv = &p.core.mpv;
        let mut buf = vec![0u8; 64 * 64 * 4 + 64];
        let off = buf.as_ptr().align_offset(64);
        let (mut n, end) = (0, Instant::now() + Duration::from_secs_f64(secs));
        while Instant::now() < end {
            if unsafe { (mpv.render_context_update)(rc) } & UPDATE_FRAME != 0 {
                let mut sz: [c_int; 2] = [64, 64];
                let mut stride: usize = 256;
                let mut params = [
                    mpv_render_param { kind: PARAM_API_TYPE, data: RENDER_API_TYPE_SW.as_ptr() as *mut _ },
                    mpv_render_param { kind: PARAM_SW_SIZE, data: sz.as_mut_ptr() as *mut _ },
                    mpv_render_param { kind: PARAM_SW_FORMAT, data: c"rgb0".as_ptr() as *mut _ },
                    mpv_render_param { kind: PARAM_SW_STRIDE, data: &mut stride as *mut _ as *mut _ },
                    mpv_render_param { kind: PARAM_SW_POINTER, data: unsafe { buf.as_mut_ptr().add(off) } as *mut _ },
                    mpv_render_param { kind: PARAM_INVALID, data: std::ptr::null_mut() },
                ];
                if unsafe { (mpv.render_context_render)(rc, params.as_mut_ptr()) } >= 0 {
                    n += 1;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        n
    }

    #[test]
    fn reattached_render_context_draws_again() {
        let Some(mpv) = mpv() else { return };
        let (tx, rx) = channel();
        let p = Player::with_options(mpv, Role::Background, 14, tx, &Options { vo: "libmpv", ao: "null", audio_device: String::new() }).unwrap();
        p.load(Path::new("av://lavfi:testsrc2=size=320x240:rate=30"), 0.0);
        p.set_render(sw_render(&p));
        assert!(until(&rx, 5.0, |r| r.state == PlayState::Playing).1);
        assert!(frames(&p, p.render_raw().unwrap(), 1.0) > 10);
        // Freeing the context while the file plays makes mpv drop the video track.
        let at = p.position();
        drop(p.take_render());
        std::thread::sleep(Duration::from_millis(300));
        p.set_render(sw_render(&p));
        let n = frames(&p, p.render_raw().unwrap(), 1.5);
        assert!(n > 10, "{n} frames after re-attaching");
        assert!(p.position() > at, "carries on from {at}, not from the start: {}", p.position());
        let (v, _) = until(&rx, 0.2, |_| false);
        assert!(v.iter().all(|r| r.state != PlayState::Error), "{v:?}");
        drop(p.take_render());
    }

    /// A `screenshot-raw` result, as a tree of nodes that `read` can look at.
    fn with_node<R>(w: i64, h: i64, stride: i64, format: &CStr, data: &mut [u8], read: impl FnOnce(&mpv_node) -> R) -> R {
        let int = |v| mpv_node { u: mpv_node_value { int64: v }, format: FORMAT_INT64 };
        let mut ba = mpv_byte_array { data: data.as_mut_ptr() as *mut _, size: data.len() };
        let mut values = [
            int(w),
            int(h),
            int(stride),
            mpv_node { u: mpv_node_value { string: format.as_ptr() as *mut _ }, format: FORMAT_STRING },
            mpv_node { u: mpv_node_value { ba: &mut ba }, format: FORMAT_BYTE_ARRAY },
        ];
        let mut keys = [c"w", c"h", c"stride", c"format", c"data"].map(|k| k.as_ptr() as *mut std::ffi::c_char);
        let mut list = mpv_node_list { num: values.len() as c_int, values: values.as_mut_ptr(), keys: keys.as_mut_ptr() };
        read(&mpv_node { u: mpv_node_value { list: &mut list }, format: FORMAT_NODE_MAP })
    }

    fn raw(w: u32, h: u32, stride: usize, bgrx: Vec<u8>) -> RawFrame {
        RawFrame { w, h, stride, bgrx }
    }

    #[test]
    fn frame_is_read_from_the_reply_node() {
        let mut data = [1, 2, 3, 0, 9, 9, 9, 9, 4, 5, 6, 0, 9, 9, 9, 9];
        let f = with_node(1, 2, 8, c"bgr0", &mut data, |n| RawFrame::from_node(unsafe { Node::from_raw(n) })).expect("frame");
        assert_eq!((f.w, f.h, f.stride, f.bgrx.len()), (1, 2, 8, 16));
    }

    #[test]
    fn frame_with_bad_fields_is_refused() {
        let mut data = [0u8; 16];
        for (w, h, stride, format) in [
            (2, 2, 8, c"rgba"), // another pixel format
            (2, 2, -8, c"bgr0"), // flipped
            (0, 2, 8, c"bgr0"),
            (2, 0, 8, c"bgr0"),
            (3, 2, 8, c"bgr0"), // stride shorter than a row
            (2, 3, 8, c"bgr0"), // less data than the rows need
        ] {
            let f = with_node(w, h, stride, format, &mut data, |n| RawFrame::from_node(unsafe { Node::from_raw(n) }));
            assert!(f.is_none(), "{w}x{h} stride {stride}");
        }
        let not_a_map = mpv_node { u: mpv_node_value { int64: 1 }, format: FORMAT_INT64 };
        assert!(RawFrame::from_node(unsafe { Node::from_raw(&not_a_map) }).is_none());
    }

    #[test]
    fn bgrx_becomes_rgb_and_skips_row_padding() {
        let f = raw(2, 2, 12, vec![1, 2, 3, 0, 4, 5, 6, 0, 99, 99, 99, 99, 7, 8, 9, 0, 10, 11, 12, 0, 99, 99, 99, 99]);
        assert_eq!(f.to_rgb(100).unwrap().into_raw(), [3, 2, 1, 6, 5, 4, 9, 8, 7, 12, 11, 10]);
    }

    #[test]
    fn large_frame_shrinks_to_max_width_keeping_its_shape() {
        let f = raw(1920, 1080, 1920 * 4, vec![200; 1920 * 1080 * 4]);
        let img = f.to_rgb(480).unwrap();
        assert_eq!((img.width(), img.height()), (480, 270));
        assert_eq!(img.get_pixel(240, 135).0, [200, 200, 200]);
    }

    #[test]
    fn small_frame_is_not_enlarged_and_a_thin_one_keeps_a_row() {
        assert_eq!(raw(2, 1, 8, vec![0; 8]).to_rgb(480).unwrap().width(), 2);
        let thin = raw(1000, 1, 4000, vec![0; 4000]).to_rgb(10).unwrap();
        assert_eq!((thin.width(), thin.height()), (10, 1));
    }

    fn frame() -> Option<RawFrame> {
        Some(raw(1, 1, 4, vec![0; 4]))
    }

    #[test]
    fn grab_reply_reaches_the_waiting_request_only() {
        let g = Grabs::default();
        let (id, rx) = g.begin().unwrap();
        g.finish(id + 1, || panic!("not this request's reply"));
        assert!(rx.try_recv().is_err());
        g.finish(id, frame);
        assert!(rx.try_recv().unwrap().is_some());
    }

    #[test]
    fn no_second_request_until_the_late_reply_comes() {
        let g = Grabs::default();
        let (id, rx) = g.begin().unwrap();
        drop(rx); // the grab timed out
        assert!(g.begin().is_none(), "a room that does not draw must not collect requests");
        g.finish(id, frame); // mpv answers late; nobody listens and nothing breaks
        let (next, _rx) = g.begin().expect("free again");
        assert_ne!(next, id);
    }

    #[test]
    fn abandoned_request_frees_the_slot() {
        let g = Grabs::default();
        let (id, rx) = g.begin().unwrap();
        g.abandon(id);
        assert!(rx.try_recv().unwrap().is_none());
        assert!(g.begin().is_some());
    }

    #[test]
    fn closing_wakes_the_waiting_grab_and_refuses_new_ones() {
        let g = Grabs::default();
        let (_, rx) = g.begin().unwrap();
        g.close();
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).map(|f| f.is_some()), Err(RecvTimeoutError::Disconnected));
        assert!(g.begin().is_none());
    }

    #[test]
    fn grabber_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<FrameGrabber>();
    }

    /// Plays `testsrc2` into a software render context.
    fn sw_player(mpv: Arc<Mpv>, inst: u64) -> (Player, Receiver<PlayerReport>) {
        let (tx, rx) = channel();
        let p = Player::with_options(mpv, Role::Background, inst, tx, &Options { vo: "libmpv", ao: "null", audio_device: String::new() }).unwrap();
        p.load(Path::new("av://lavfi:testsrc2=size=320x240:rate=30"), 0.0);
        p.set_render(sw_render(&p));
        (p, rx)
    }

    #[test]
    fn grabber_returns_frame_of_playing_testsrc() {
        let Some(mpv) = mpv() else { return };
        let (p, rx) = sw_player(mpv, 15);
        assert!(until(&rx, 5.0, |r| r.state == PlayState::Playing).1);
        let g = p.grabber();
        let grabs = std::thread::spawn(move || {
            let end = Instant::now() + Duration::from_secs(3);
            while Instant::now() < end {
                if let Some(pb) = g.grab(160) {
                    return Some(pb);
                }
            }
            None
        });
        frames(&p, p.render_raw().unwrap(), 3.0); // the screenshot is served by this render loop
        let pb = grabs.join().unwrap().expect("a frame");
        assert_eq!(pb.width(), 160);
        let first = pb.as_slice()[0];
        assert!(pb.as_slice().iter().any(|px| *px != first), "testsrc2 is not one flat colour");
        drop(p.take_render());
    }

    /// Measured on Windows (libmpv 2.x): a software render context serves `screenshot-raw` without a render
    /// call (as does `vo=null`), so this cannot show the 500 ms timeout; it only shows a grab never waits past
    /// it. Whether an OpenGL context waits for the room to draw is checked by hand (C6).
    #[test]
    fn grab_without_render_answers_within_the_timeout() {
        let Some(mpv) = mpv() else { return };
        let (p, rx) = sw_player(mpv, 16);
        assert!(until(&rx, 5.0, |r| r.state == PlayState::Playing).1);
        let at = Instant::now();
        let _ = p.grabber().grab(160);
        assert!(at.elapsed() < Duration::from_millis(600), "{:?}", at.elapsed());
        drop(p.take_render());
    }

    /// Plan 4's thumbnails take frames from a windowless player.
    #[test]
    fn grab_works_with_vo_null() {
        let Some(mpv) = mpv() else { return };
        let (p, rx) = start(&mpv, Role::Background, 18, "av://lavfi:testsrc2=size=320x240:rate=30");
        assert!(until(&rx, 5.0, |r| r.state == PlayState::Playing).1);
        let g = p.grabber();
        let end = Instant::now() + Duration::from_secs(3);
        let pb = loop {
            if let Some(pb) = g.grab(160) { break pb }
            assert!(Instant::now() < end, "no frame from vo=null");
        };
        assert_eq!((pb.width(), pb.height()), (160, 120));
        let first = pb.as_slice()[0];
        assert!(pb.as_slice().iter().any(|px| *px != first), "testsrc2 is not one flat colour");
    }

    #[test]
    fn grab_returns_none_once_the_core_is_gone() {
        let Some(mpv) = mpv() else { return };
        let (p, rx) = sw_player(mpv, 17);
        assert!(until(&rx, 5.0, |r| r.state == PlayState::Playing).1);
        let g = p.grabber();
        let loops = {
            let g = g.clone();
            std::thread::spawn(move || {
                let end = Instant::now() + Duration::from_secs(1);
                while Instant::now() < end {
                    let _ = g.grab(160);
                }
            })
        };
        drop(p.take_render());
        drop(p);
        loops.join().unwrap();
        let core_gone = Instant::now() + Duration::from_secs(2);
        while g.core.upgrade().is_some() && Instant::now() < core_gone {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(g.grab(160).is_none());
    }

    #[test]
    fn missing_libmpv_reports_error_for_instance() {
        let (tx, rx) = channel();
        assert!(players(None, tx).create(Role::Audio { looped: false }, 42, "").is_none());
        let r = rx.try_recv().expect("report");
        assert_eq!((r.inst, r.state, r.error.as_deref()), (42, PlayState::Error, Some(ffi::MISSING)));
    }

    #[test]
    fn report_converts_to_room_event() {
        let r = PlayerReport { inst: 9, player: 1, state: PlayState::Paused, position: 1.5, duration: 30.0, error: None };
        match RoomEvent::from(r) {
            RoomEvent::Media { inst: 9, state: PlayState::Paused, position, duration, error: None } => assert_eq!((position, duration), (1.5, 30.0)),
            e => panic!("{e:?}"),
        }
    }
}
