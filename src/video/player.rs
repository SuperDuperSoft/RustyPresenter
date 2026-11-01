//! One libmpv player per playing video/audio instance. Reports go out tagged
//! with the instance id, so the core can ignore a retired instance.
use super::ffi::{self, *};
use crate::live::{PlayState, RoomEvent};
use crate::model::EndAction;
use std::cell::RefCell;
use std::ffi::c_int;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
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

/// An mpv render context. It must be freed before its player's handle, so it lives
/// in [`Player::render`] and the drawing code takes it out (and frees it) first.
pub struct RenderContext {
    mpv: Arc<Mpv>,
    ctx: *mut mpv_render_context,
}

impl RenderContext {
    /// # Safety
    /// `ctx` must come from `mpv.render_context_create` and not be freed elsewhere.
    pub unsafe fn from_raw(mpv: Arc<Mpv>, ctx: *mut mpv_render_context) -> Self {
        RenderContext { mpv, ctx }
    }

    pub fn raw(&self) -> *mut mpv_render_context {
        self.ctx
    }
}

impl Drop for RenderContext {
    fn drop(&mut self) {
        unsafe { (self.mpv.render_context_free)(self.ctx) }
    }
}

/// Observed properties, by reply id.
const OBS_PAUSE: u64 = 1;
const OBS_TIME: u64 = 2;
const OBS_DURATION: u64 = 3;
const OBS_EOF: u64 = 4;
/// Position reports at most this often (as the web room); state changes go out at once.
const TICK: Duration = Duration::from_millis(500);

struct Handle(*mut mpv_handle);
// The client API is thread-safe; the handle outlives the event thread (joined in Drop).
unsafe impl Send for Handle {}

/// A libmpv instance playing one video/audio instance, with a thread turning its
/// events into [`PlayerReport`]s.
pub struct Player {
    mpv: Arc<Mpv>,
    h: *mut mpv_handle,
    inst: u64,
    tx: Sender<PlayerReport>,
    name: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// Filled by the drawing code; dropped before the handle is destroyed.
    pub render: RefCell<Option<RenderContext>>,
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
        let fail = |e: String| {
            unsafe { (mpv.terminate_destroy)(h) };
            Err(e)
        };
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
                return fail(e);
            }
        }
        let _ = unsafe { mpv.set_option(h, "osc", "no") }; // only exists in builds with Lua
        let r = unsafe { (mpv.initialize)(h) };
        if r < 0 {
            return fail(format!("The video player could not start: {}", mpv.err(r)));
        }
        for (id, name, f) in [(OBS_PAUSE, "pause", FORMAT_FLAG), (OBS_TIME, "time-pos", FORMAT_DOUBLE), (OBS_DURATION, "duration", FORMAT_DOUBLE), (OBS_EOF, "eof-reached", FORMAT_FLAG)] {
            if let Err(e) = unsafe { mpv.observe(h, id, name, f) } {
                return fail(e);
            }
        }
        let name = Arc::new(Mutex::new(String::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let w = Watch { mpv: mpv.clone(), h: Handle(h), inst, tx: reports.clone(), name: name.clone(), stop: stop.clone(), loaded: false, paused: false, position: 0.0, duration: 0.0, last: None, last_at: Instant::now() };
        let thread = match std::thread::Builder::new().name(format!("mpv-events-{inst}")).spawn(move || w.run()) {
            Ok(t) => t,
            Err(e) => return fail(format!("The video player could not start: {e}")),
        };
        Ok(Player { mpv, h, inst, tx: reports, name, stop, thread: Some(thread), render: RefCell::new(None) })
    }

    pub fn inst(&self) -> u64 {
        self.inst
    }

    /// Starts `path` at `start` seconds, replacing whatever was loaded.
    pub fn load(&self, path: &Path, start: f64) {
        let file = path.to_string_lossy();
        let name = path.file_name().map_or_else(|| file.to_string(), |n| n.to_string_lossy().into_owned());
        *self.name.lock().unwrap_or_else(|e| e.into_inner()) = name;
        let r = unsafe { self.mpv.set_property(self.h, "start", &format!("{}", start.max(0.0))).and_then(|_| self.mpv.command(self.h, &["loadfile", &file])) };
        if let Err(e) = r {
            let _ = self.tx.send(PlayerReport { inst: self.inst, state: PlayState::Error, position: 0.0, duration: 0.0, error: Some(e) });
        }
    }

    pub fn set_paused(&self, paused: bool) {
        let _ = unsafe { self.mpv.set_property(self.h, "pause", if paused { "yes" } else { "no" }) };
    }

    /// Current playback position in seconds (0 when nothing is playing).
    pub fn position(&self) -> f64 {
        unsafe { self.mpv.get_double(self.h, "time-pos") }.unwrap_or(0.0)
    }

    pub fn handle(&self) -> *mut mpv_handle {
        self.h
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        unsafe { (self.mpv.wakeup)(self.h) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        drop(self.render.get_mut().take());
        // Shutting the core down can take a moment; never on the caller's (UI) thread.
        let (mpv, h) = (self.mpv.clone(), Handle(self.h));
        let destroy = move || {
            let h = h;
            unsafe { (mpv.terminate_destroy)(h.0) }
        };
        let _ = std::thread::Builder::new().name("mpv-destroy".into()).spawn(destroy);
    }
}

/// The event thread's view of one player.
struct Watch {
    mpv: Arc<Mpv>,
    h: Handle,
    inst: u64,
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
        loop {
            let ev = unsafe { &*(self.mpv.wait_event)(self.h.0, -1.0) };
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
                            self.state(PlayState::Error, Some(format!("{name} could not be played ({})", self.mpv.err(end.error))));
                        }
                        _ => {}
                    }
                }
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
        let _ = self.tx.send(PlayerReport { inst: self.inst, state, position: self.position, duration: self.duration, error });
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
    pub fn create(&self, role: Role, inst: u64) -> Option<Player> {
        let r = match &self.mpv {
            Some(m) => Player::with_options(m.clone(), role, inst, self.reports.clone(), &self.options),
            None => Err(ffi::MISSING.into()),
        };
        r.map_err(|e| {
            let _ = self.reports.send(PlayerReport { inst, state: PlayState::Error, position: 0.0, duration: 0.0, error: Some(e) });
        })
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
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

    #[test]
    fn missing_libmpv_reports_error_for_instance() {
        let (tx, rx) = channel();
        assert!(players(None, tx).create(Role::Audio { looped: false }, 42).is_none());
        let r = rx.try_recv().expect("report");
        assert_eq!((r.inst, r.state, r.error.as_deref()), (42, PlayState::Error, Some(ffi::MISSING)));
    }

    #[test]
    fn report_converts_to_room_event() {
        let r = PlayerReport { inst: 9, state: PlayState::Paused, position: 1.5, duration: 30.0, error: None };
        match RoomEvent::from(r) {
            RoomEvent::Media { inst: 9, state: PlayState::Paused, position, duration, error: None } => assert_eq!((position, duration), (1.5, 30.0)),
            e => panic!("{e:?}"),
        }
    }
}
