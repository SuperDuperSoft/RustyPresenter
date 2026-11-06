//! What is in a media file, found with a short-lived headless mpv core (or `image`, for pictures).
//! Probes never use the GPU decoder, so they cannot take it from the live room, and at most two run at once.
use super::CANCELLED;
use crate::model::MediaKind;
use crate::video::ffi::{self, ERROR_OPTION_NOT_FOUND, Mpv, Node, mpv_event_end_file, mpv_handle};
use crate::video::player::RawFrame;
use std::ffi::{CString, c_int};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, SemaphorePermit};
use tokio_util::sync::CancellationToken;

pub use super::strings::{BAD_PICTURE, CANT_PLAY, TOO_SLOW};

/// A probe's time limit, from opening the file to the thumbnail.
const DEADLINE: Duration = Duration::from_secs(20);
/// How often the event loop looks at the deadline and the cancel token.
const TICK_SECONDS: f64 = 0.05;
const THUMB_WIDTH: u32 = 320;
/// So a picture that is one pixel wide stays a small file.
const THUMB_MAX_HEIGHT: u64 = 1280;
const THUMB_QUALITY: u8 = 85;

const OPTIONS: &[(&str, &str)] = &[
    ("vo", "null"),
    ("ao", "null"),
    ("sid", "no"),
    ("pause", "yes"),
    ("idle", "yes"),
    ("config", "no"),
    ("load-scripts", "no"),
    ("ytdl", "no"),
    ("osc", "no"),
    ("input-default-bindings", "no"),
    ("input-vo-keyboard", "no"),
    ("terminal", "no"),
    ("hwdec", "no"),
    ("cache", "no"),
    ("demuxer-readahead-secs", "1"),
    ("hr-seek", "yes"),
    ("vd-lavc-threads", "2"),
    ("msg-level", "all=error"),
];

/// Options that only exist in builds with Lua. The packaged libmpv has none.
const LUA_OPTIONS: &[&str] = &["load-scripts", "ytdl", "osc"];

#[derive(Debug)]
pub struct Probe {
    pub kind: MediaKind,
    pub duration: Option<f64>,
    pub size: Option<(u32, u32)>,
    pub thumb: Option<image::RgbImage>,
    /// mpv's `track-list`, for the C6 check (`examples/probe.rs`). `Node::None` for pictures.
    pub tracks: Node,
}

/// The kind of file a `track-list` describes. Cover art does not make an audio file a video.
pub fn classify(track_list: &Node) -> Option<MediaKind> {
    let Node::Array(tracks) = track_list else { return None };
    let kind = |t: &Node| t.get("type").and_then(Node::as_str).unwrap_or_default().to_string();
    let art = |t: &Node| t.get("albumart").and_then(Node::as_bool).unwrap_or(false);
    if tracks.iter().any(|t| kind(t) == "video" && !art(t)) {
        Some(MediaKind::Video)
    } else if tracks.iter().any(|t| kind(t) == "audio") {
        Some(MediaKind::Audio)
    } else {
        None
    }
}

/// Where the thumbnail comes from: past a fade-in, but inside a very short clip.
pub fn thumb_time(duration: Option<f64>) -> f64 {
    duration.filter(|d| d.is_finite() && *d > 0.0).map_or(0.0, |d| (d * 0.1).min(1.0))
}

/// A 320 px wide JPEG.
pub fn thumb_jpeg(img: &image::RgbImage) -> Vec<u8> {
    let (w, h) = img.dimensions();
    let height = (h as u64 * THUMB_WIDTH as u64 / w.max(1) as u64).clamp(1, THUMB_MAX_HEIGHT) as u32;
    let small = image::imageops::resize(img, THUMB_WIDTH, height, image::imageops::FilterType::Triangle);
    let mut out = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, THUMB_QUALITY);
    small.write_with_encoder(encoder).expect("a JPEG goes into a Vec");
    out
}

pub fn probe_image(path: &Path) -> Result<Probe, String> {
    let reader = image::ImageReader::open(path).map_err(|_| BAD_PICTURE.to_string())?;
    let picture = reader.with_guessed_format().map_err(|_| BAD_PICTURE.to_string())?.decode().map_err(|_| BAD_PICTURE.to_string())?;
    let thumb = picture.to_rgb8();
    Ok(Probe { kind: MediaKind::Image, duration: None, size: Some(thumb.dimensions()), thumb: Some(thumb), tracks: Node::None })
}

/// True for an option error a probe can ignore.
fn may_ignore(option: &str, code: c_int) -> bool {
    code == ERROR_OPTION_NOT_FOUND && LUA_OPTIONS.contains(&option)
}

/// The operator sees "This file can't be played."; what went wrong goes to the log.
fn cant_play(what: &str, e: impl std::fmt::Display) -> String {
    eprintln!("probe: {what}: {e}");
    CANT_PLAY.to_string()
}

/// One mpv core, destroyed on drop.
struct Core {
    mpv: Arc<Mpv>,
    h: *mut mpv_handle,
}

// SAFETY: the client API is thread-safe, and only one thread owns a `Core` at a time.
unsafe impl Send for Core {}

impl Core {
    fn new(mpv: &Arc<Mpv>) -> Result<Core, String> {
        let h = unsafe { (mpv.create)() };
        if h.is_null() {
            return Err(CANT_PLAY.into());
        }
        let core = Core { mpv: mpv.clone(), h };
        for (k, v) in OPTIONS {
            let option = || format!("mpv option {k}={v}");
            let (key, value) = (CString::new(*k).map_err(|e| cant_play(&option(), e))?, CString::new(*v).map_err(|e| cant_play(&option(), e))?);
            let code = unsafe { (mpv.set_option_string)(h, key.as_ptr(), value.as_ptr()) };
            if code < 0 && !may_ignore(k, code) {
                return Err(cant_play(&option(), mpv.err(code)));
            }
        }
        let code = unsafe { (mpv.initialize)(h) };
        if code < 0 {
            return Err(cant_play("mpv start", mpv.err(code)));
        }
        Ok(core)
    }

    /// The picture on screen, if mpv can give a whole one: a broken frame gives no thumbnail, not a black one.
    fn grab(&self) -> Option<image::RgbImage> {
        let frame = unsafe { self.mpv.command_node(self.h, &["screenshot-raw", "video"]) }.ok()?;
        RawFrame::from_node(frame)?.to_rgb(0)
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        unsafe { (self.mpv.terminate_destroy)(self.h) }
    }
}

#[derive(Debug, PartialEq)]
enum Stage {
    Loading,
    /// Waiting for the seek to the thumbnail, then for its first frame.
    Seeking { seen_seek: bool },
}

/// Ask a headless mpv what `path` is. `Err` is a message for the operator (or [`CANCELLED`]).
pub fn probe_blocking(mpv: &Arc<Mpv>, path: &Path, deadline: Duration, cancel: &CancellationToken) -> Result<Probe, String> {
    probe_holding(mpv, path, deadline, cancel, None)
}

/// As `probe_blocking`, holding `slot` (a place among the probes that may run at once) until the core is gone.
fn probe_holding(mpv: &Arc<Mpv>, path: &Path, deadline: Duration, cancel: &CancellationToken, slot: Option<SemaphorePermit<'static>>) -> Result<Probe, String> {
    let core = Core::new(mpv)?;
    let result = run(&core, path, deadline, cancel);
    // A file stuck in opening can hold mpv's shutdown for good, so a probe that gave up never waits for it.
    if matches!(&result, Err(e) if e == TOO_SLOW || e == CANCELLED) {
        drop_later(core, slot);
    }
    result
}

/// Drops `core` on its own thread. The slot goes with it, so a core that never shuts down keeps its slot and
/// stuck files cannot pile up live mpv cores beside the room.
fn drop_later<T: Send + 'static>(core: T, slot: Option<SemaphorePermit<'static>>) {
    std::thread::spawn(move || {
        drop(core);
        drop(slot);
    });
}

fn run(core: &Core, path: &Path, deadline: Duration, cancel: &CancellationToken) -> Result<Probe, String> {
    let (mpv, h) = (&core.mpv, core.h);
    let end = Instant::now() + deadline;
    let file = path.to_str().ok_or(CANT_PLAY)?;
    unsafe { mpv.command(h, &["loadfile", file]) }.map_err(|_| CANT_PLAY.to_string())?;
    let mut stage = Stage::Loading;
    let mut found = None;
    loop {
        if cancel.is_cancelled() {
            return Err(CANCELLED.into());
        }
        if Instant::now() >= end {
            // A video whose thumbnail never came still plays.
            return found.ok_or_else(|| TOO_SLOW.to_string());
        }
        let ev = unsafe { &*(mpv.wait_event)(h, TICK_SECONDS) };
        match ev.event_id {
            ffi::EVENT_FILE_LOADED if stage == Stage::Loading => {
                let probe = loaded(core)?;
                if probe.kind != MediaKind::Video {
                    return Ok(probe);
                }
                let at = format!("{:.3}", thumb_time(probe.duration));
                found = Some(probe);
                if unsafe { mpv.command(h, &["seek", &at, "absolute+exact"]) }.is_err() {
                    return finish(core, found);
                }
                stage = Stage::Seeking { seen_seek: false };
            }
            ffi::EVENT_SEEK => {
                if let Stage::Seeking { seen_seek } = &mut stage {
                    *seen_seek = true;
                }
            }
            // The file's first frame also restarts playback, so only the one after the seek counts.
            ffi::EVENT_PLAYBACK_RESTART if stage == (Stage::Seeking { seen_seek: true }) => return finish(core, found),
            ffi::EVENT_END_FILE => {
                let reason = unsafe { (ev.data as *const mpv_event_end_file).as_ref() }.map(|e| e.reason);
                return match found {
                    Some(_) if reason != Some(ffi::END_FILE_ERROR) => finish(core, found),
                    Some(probe) => Ok(probe),
                    None => Err(CANT_PLAY.into()),
                };
            }
            ffi::EVENT_SHUTDOWN => return Err(CANT_PLAY.into()),
            _ => {}
        }
    }
}

/// What mpv knows once a file has opened.
fn loaded(core: &Core) -> Result<Probe, String> {
    let (mpv, h) = (&core.mpv, core.h);
    let tracks = unsafe { mpv.get_node(h, "track-list") }.ok_or(CANT_PLAY)?;
    let kind = classify(&tracks).ok_or(CANT_PLAY)?;
    // Read once, here: a source that states no length (lavfi while paused) reports only the part demuxed so far
    // later on (0.17 s of a 3 s clip, check C6), which is worse than no duration.
    let duration = unsafe { mpv.get_double(h, "duration") }.filter(|d| d.is_finite() && *d > 0.0);
    Ok(Probe { kind, duration, size: None, thumb: None, tracks })
}

/// Adds the thumbnail and the size as shown, when mpv has them.
fn finish(core: &Core, found: Option<Probe>) -> Result<Probe, String> {
    let mut probe = found.ok_or(CANT_PLAY)?;
    let shown = |name| unsafe { core.mpv.get_double(core.h, name) }.filter(|v| *v > 0.0).map(|v| v as u32);
    probe.thumb = core.grab();
    probe.size = probe.thumb.as_ref().map(|t| t.dimensions()).or(shown("dwidth").zip(shown("dheight")));
    Ok(probe)
}

/// Mpv is heavy on a laptop that is also running the service, so only two files are probed at once.
static PROBES: Semaphore = Semaphore::const_new(2);

pub(crate) fn is_picture(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
    super::media::IMAGE.contains(&ext.as_str())
}

/// Waits for a turn, until the import is cancelled.
async fn permit<'a>(gate: &'a Semaphore, cancel: &CancellationToken) -> Result<SemaphorePermit<'a>, String> {
    tokio::select! {
        permit = gate.acquire() => permit.map_err(|e| e.to_string()),
        _ = cancel.cancelled() => Err(CANCELLED.into()),
    }
}

/// `permit`, and `waiting` hears `true` if the turn has to be waited for and `false` when that wait is over.
async fn turn<'a>(gate: &'a Semaphore, cancel: &CancellationToken, waiting: &(impl Fn(bool) + Sync)) -> Result<SemaphorePermit<'a>, String> {
    if let Ok(permit) = gate.try_acquire() {
        return Ok(permit);
    }
    waiting(true);
    let permit = permit(gate, cancel).await;
    waiting(false);
    permit
}

/// Tests only: a file that starts with this reads as a one-second 640×360 video, so flows that import a video (a
/// YouTube download) can be tested without libmpv.
#[cfg(test)]
pub(crate) const FAKE_VIDEO: &[u8] = b"RP-FAKE-VIDEO";

/// Probe a file off the async threads.
pub async fn probe(mpv: Option<Arc<Mpv>>, path: PathBuf, cancel: CancellationToken) -> Result<Probe, String> {
    probe_waiting(mpv, path, cancel, |_| {}).await
}

/// [`probe`], telling `waiting` when the file is behind other probes.
pub async fn probe_waiting(mpv: Option<Arc<Mpv>>, path: PathBuf, cancel: CancellationToken, waiting: impl Fn(bool) + Send + Sync) -> Result<Probe, String> {
    if is_picture(&path) {
        let joined = |e: tokio::task::JoinError| {
            eprintln!("probe: picture thread: {e}");
            BAD_PICTURE.to_string()
        };
        return tokio::task::spawn_blocking(move || probe_image(&path)).await.map_err(joined)?;
    }
    #[cfg(test)]
    if std::fs::read(&path).is_ok_and(|b| b.starts_with(FAKE_VIDEO)) {
        return Ok(Probe { kind: MediaKind::Video, duration: Some(1.0), size: Some((640, 360)), thumb: None, tracks: Node::None });
    }
    let mpv = mpv.ok_or(ffi::MISSING)?;
    let slot = turn(&PROBES, &cancel, &waiting).await?;
    let probing = tokio::task::spawn_blocking(move || probe_holding(&mpv, &path, DEADLINE, &cancel, Some(slot)));
    probing.await.map_err(|e| cant_play("probe thread", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(kind: &str, albumart: bool) -> Node {
        Node::Map(vec![("id".into(), Node::Int(1)), ("type".into(), Node::Str(kind.into())), ("albumart".into(), Node::Flag(albumart))])
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rp-probe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[tokio::test]
    async fn a_free_turn_is_not_a_wait() {
        let heard = std::sync::Mutex::new(vec![]);
        let gate = Semaphore::new(1);
        let _turn = turn(&gate, &CancellationToken::new(), &|w| heard.lock().unwrap().push(w)).await.unwrap();
        assert!(heard.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn waiting_for_a_turn_is_reported_and_its_end_too() {
        let heard = Arc::new(std::sync::Mutex::new(vec![]));
        let gate = Arc::new(Semaphore::new(1));
        let held = gate.clone().acquire_owned().await.unwrap();
        let (g, h) = (gate.clone(), heard.clone());
        let waiter = tokio::spawn(async move {
            let h2 = h.clone();
            turn(&g, &CancellationToken::new(), &move |w| h2.lock().unwrap().push(w)).await.map(|_| ())
        });
        while heard.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        assert_eq!(*heard.lock().unwrap(), [true]);
        drop(held);
        waiter.await.unwrap().unwrap();
        assert_eq!(*heard.lock().unwrap(), [true, false]);
    }

    #[tokio::test]
    async fn cancelling_while_waiting_ends_the_wait() {
        let heard = std::sync::Mutex::new(vec![]);
        let gate = Semaphore::new(0);
        let cancel = CancellationToken::new();
        cancel.cancel();
        let e = turn(&gate, &cancel, &|w| heard.lock().unwrap().push(w)).await.map(|_| ()).unwrap_err();
        assert_eq!(e, CANCELLED);
        assert_eq!(*heard.lock().unwrap(), [true, false]);
    }

    #[test]
    fn classify_video_audio_albumart_and_empty() {
        assert_eq!(classify(&Node::Array(vec![track("audio", false), track("video", false)])), Some(MediaKind::Video));
        assert_eq!(classify(&Node::Array(vec![track("audio", false), track("video", true)])), Some(MediaKind::Audio));
        assert_eq!(classify(&Node::Array(vec![track("audio", false)])), Some(MediaKind::Audio));
        assert_eq!(classify(&Node::Array(vec![track("video", true)])), None);
        assert_eq!(classify(&Node::Array(vec![track("sub", false)])), None);
        assert_eq!(classify(&Node::Array(vec![])), None);
        assert_eq!(classify(&Node::None), None);
    }

    #[test]
    fn thumb_time_rules() {
        assert_eq!(thumb_time(None), 0.0);
        assert_eq!(thumb_time(Some(0.0)), 0.0);
        assert_eq!(thumb_time(Some(f64::INFINITY)), 0.0);
        assert_eq!(thumb_time(Some(5.0)), 0.5);
        assert_eq!(thumb_time(Some(10.0)), 1.0);
        assert_eq!(thumb_time(Some(3600.0)), 1.0);
    }

    /// A probe that gave up keeps its slot until its mpv core is really gone, however long that takes.
    #[test]
    fn a_core_dropped_later_keeps_its_slot_until_it_is_gone() {
        struct Stuck(std::sync::mpsc::Receiver<()>);
        impl Drop for Stuck {
            fn drop(&mut self) {
                let _ = self.0.recv();
            }
        }
        static GATE: Semaphore = Semaphore::const_new(1);
        let (release, held) = std::sync::mpsc::channel();
        drop_later(Stuck(held), Some(GATE.try_acquire().unwrap()));
        std::thread::sleep(Duration::from_millis(100));
        assert!(GATE.try_acquire().is_err(), "the slot came back while the core was still shutting down");
        release.send(()).unwrap();
        let free = Instant::now();
        while GATE.try_acquire().is_err() {
            assert!(free.elapsed() < Duration::from_secs(5), "the slot never came back");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn only_lua_options_may_be_missing() {
        assert!(may_ignore("osc", ERROR_OPTION_NOT_FOUND));
        assert!(may_ignore("ytdl", ERROR_OPTION_NOT_FOUND));
        assert!(may_ignore("load-scripts", ERROR_OPTION_NOT_FOUND));
        assert!(!may_ignore("hwdec", ERROR_OPTION_NOT_FOUND));
        assert!(!may_ignore("osc", ffi::ERROR_INVALID_PARAMETER));
    }

    #[test]
    fn probe_never_uses_the_gpu_decoder() {
        assert!(OPTIONS.contains(&("hwdec", "no")));
    }

    #[test]
    fn probe_image_reads_png_dimensions() {
        let p = temp("a.png");
        image::RgbImage::from_pixel(7, 5, image::Rgb([9, 8, 7])).save(&p).unwrap();
        let probe = probe_image(&p).unwrap();
        assert_eq!((probe.kind, probe.duration, probe.size), (MediaKind::Image, None, Some((7, 5))));
        assert_eq!(probe.thumb.unwrap().get_pixel(0, 0), &image::Rgb([9, 8, 7]));
    }

    #[test]
    fn probe_image_refuses_undecodable_picture() {
        let p = temp("a.png");
        image::RgbImage::new(64, 64).save(&p).unwrap();
        let bytes = std::fs::read(&p).unwrap();
        std::fs::write(&p, &bytes[..bytes.len() / 2]).unwrap();
        assert_eq!(probe_image(&p).unwrap_err(), BAD_PICTURE);
        // AVIF is not among the enabled `image` features.
        let avif = temp("a.avif");
        std::fs::write(&avif, b"\0\0\0\x1cftypavif\0\0\0\0avifmif1miaf").unwrap();
        assert_eq!(probe_image(&avif).unwrap_err(), BAD_PICTURE);
        assert_eq!(probe_image(&temp("gone.png")).unwrap_err(), BAD_PICTURE);
    }

    #[test]
    fn thumb_jpeg_is_320_wide() {
        let img = image::RgbImage::from_pixel(1920, 1080, image::Rgb([200, 10, 10]));
        let jpeg = thumb_jpeg(&img);
        let back = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg).unwrap();
        assert_eq!((back.width(), back.height()), (320, 180));
    }

    #[test]
    fn thumb_jpeg_survives_extreme_shapes() {
        let size = |w, h| {
            let back = image::load_from_memory(&thumb_jpeg(&image::RgbImage::new(w, h))).unwrap();
            (back.width(), back.height())
        };
        assert_eq!(size(4000, 1), (320, 1));
        assert_eq!(size(1, 4000), (320, 1280));
    }

    #[tokio::test]
    async fn missing_libmpv_gives_missing_message() {
        let e = probe(None, temp("a.mp4"), CancellationToken::new()).await.unwrap_err();
        assert_eq!(e, ffi::MISSING);
    }

    #[tokio::test]
    async fn pictures_do_not_need_libmpv() {
        let p = temp("A.PNG");
        image::RgbImage::new(3, 2).save_with_format(&p, image::ImageFormat::Png).unwrap();
        let probe = probe(None, p, CancellationToken::new()).await.unwrap();
        assert_eq!(probe.size, Some((3, 2)));
    }

    #[tokio::test]
    async fn waiting_for_a_probe_can_be_cancelled() {
        let gate = Semaphore::new(1);
        let held = gate.acquire().await.unwrap();
        let cancel = CancellationToken::new();
        let waiting = permit(&gate, &cancel);
        cancel.cancel();
        assert_eq!(waiting.await.err().as_deref(), Some(CANCELLED));
        drop(held);
        assert!(permit(&gate, &CancellationToken::new()).await.is_ok());
    }

    /// `RP_VIDEO_TESTS=1` and a real libmpv (`RP_LIBMPV`), as the gated tests in `video::player`.
    fn real_mpv() -> Option<Arc<Mpv>> {
        std::env::var_os("RP_VIDEO_TESTS").and_then(|_| Mpv::load().ok())
    }

    fn lavfi(graph: &str) -> PathBuf {
        PathBuf::from(format!("av://lavfi:{graph}"))
    }

    /// A source that opens but never delivers a byte. The handle stays open for the whole test run.
    #[cfg(unix)]
    fn silent_source() -> PathBuf {
        let p = temp("never");
        let c = CString::new(p.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        p
    }

    #[cfg(windows)]
    fn silent_source() -> PathBuf {
        use std::ffi::c_void;
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CreateNamedPipeW(name: *const u16, open: u32, mode: u32, max: u32, out: u32, input: u32, timeout: u32, security: *const c_void) -> *mut c_void;
        }
        const PIPE_ACCESS_OUTBOUND: u32 = 2;
        let p = PathBuf::from(format!(r"\\.\pipe\rp-probe-{}", uuid::Uuid::new_v4()));
        let wide: Vec<u16> = p.as_os_str().encode_wide().chain([0]).collect();
        let pipe = unsafe { CreateNamedPipeW(wide.as_ptr(), PIPE_ACCESS_OUTBOUND, 0, 1, 4096, 4096, 0, std::ptr::null()) };
        assert_ne!(pipe as isize, -1);
        p
    }

    fn media(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("experiments").join("media").join(name)
    }

    fn uniform(thumb: &image::RgbImage) -> bool {
        let first = *thumb.get_pixel(0, 0);
        thumb.pixels().all(|p| *p == first)
    }

    /// lavfi states no length while paused, so only the size and thumbnail are checked here (check C6).
    #[test]
    fn probes_lavfi_video_size_and_nonuniform_thumb() {
        let Some(mpv) = real_mpv() else { return };
        let path = lavfi("testsrc2=size=640x360:rate=30:duration=3");
        let probe = probe_blocking(&mpv, &path, Duration::from_secs(10), &CancellationToken::new()).unwrap();
        assert_eq!(probe.kind, MediaKind::Video);
        assert_eq!(probe.size, Some((640, 360)));
        assert!(!uniform(&probe.thumb.unwrap()));
    }

    /// `testsrc2-3s.mp4`: 3 s of testsrc2 at 320x180, H.264, made with ffmpeg (check C6).
    #[test]
    fn probes_mp4_duration_and_nonuniform_thumb() {
        let Some(mpv) = real_mpv() else { return };
        let probe = probe_blocking(&mpv, &media("testsrc2-3s.mp4"), Duration::from_secs(10), &CancellationToken::new()).unwrap();
        assert_eq!(probe.kind, MediaKind::Video);
        assert!((probe.duration.unwrap() - 3.0).abs() < 0.2, "{:?}", probe.duration);
        assert_eq!(probe.size, Some((320, 180)));
        assert!(!uniform(&probe.thumb.unwrap()));
    }

    /// `rotated-phone.mp4`: the same clip with a phone's 90 degree display rotation. Size and thumbnail are as shown.
    #[test]
    fn a_rotated_phone_video_is_probed_upright() {
        let Some(mpv) = real_mpv() else { return };
        let probe = probe_blocking(&mpv, &media("rotated-phone.mp4"), Duration::from_secs(10), &CancellationToken::new()).unwrap();
        assert_eq!(probe.size, Some((180, 320)));
        assert_eq!(probe.thumb.unwrap().dimensions(), (180, 320));
    }

    /// The packaged libmpv is the LGPL build without Lua, which has no `osc`, `ytdl` or `load-scripts` options.
    /// A probe must work there, and on a build with Lua.
    #[test]
    fn probe_works_on_lgpl_build_without_lua() {
        let Some(mpv) = real_mpv() else { return };
        let h = unsafe { (mpv.create)() };
        assert!(!h.is_null());
        let lua = unsafe { (mpv.set_option_string)(h, c"osc".as_ptr(), c"no".as_ptr()) } != ERROR_OPTION_NOT_FOUND;
        unsafe { (mpv.terminate_destroy)(h) };
        eprintln!("libmpv {} has Lua: {lua}", mpv.path().display());
        let probe = probe_blocking(&mpv, &media("testsrc2-3s.mp4"), Duration::from_secs(10), &CancellationToken::new()).unwrap();
        assert_eq!(probe.kind, MediaKind::Video);
        assert!(probe.thumb.is_some());
    }

    #[test]
    fn probes_sine_as_audio_without_thumb() {
        let Some(mpv) = real_mpv() else { return };
        let path = lavfi("sine=frequency=440:duration=2");
        let probe = probe_blocking(&mpv, &path, Duration::from_secs(10), &CancellationToken::new()).unwrap();
        assert_eq!(probe.kind, MediaKind::Audio);
        assert!(probe.thumb.is_none());
    }

    #[test]
    fn garbage_file_fails_readably() {
        let Some(mpv) = real_mpv() else { return };
        let p = temp("junk.mp4");
        std::fs::write(&p, [0x5a; 4096]).unwrap();
        let e = probe_blocking(&mpv, &p, Duration::from_secs(10), &CancellationToken::new()).unwrap_err();
        assert_eq!(e, CANT_PLAY);
    }

    #[test]
    fn probe_times_out() {
        let Some(mpv) = real_mpv() else { return };
        let started = Instant::now();
        let e = probe_blocking(&mpv, &silent_source(), Duration::from_secs(1), &CancellationToken::new()).unwrap_err();
        assert_eq!(e, TOO_SLOW);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn probe_cancel_returns_quickly() {
        let Some(mpv) = real_mpv() else { return };
        let cancel = CancellationToken::new();
        let later = cancel.clone();
        std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(200)); later.cancel() });
        let started = Instant::now();
        let e = probe_blocking(&mpv, &silent_source(), Duration::from_secs(30), &cancel).unwrap_err();
        assert_eq!(e, CANCELLED);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
