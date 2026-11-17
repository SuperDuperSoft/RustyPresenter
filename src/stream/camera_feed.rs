//! The camera, kept going through unplugging. FFmpeg opening a camera itself loses it for good when it stops. So a
//! small FFmpeg of its own reads the camera (scaled to the stream's size, black bars where its shape differs, as
//! RGBA) and its newest picture is kept here, for the recorder's writer to put into the stream's picture
//! (`compose`). When no picture has come for `STALE` (not yet, unplugged, failed) there is none, and the camera's
//! FFmpeg is started again every `RETRY`; when pictures come again they are used again. The stream itself never
//! sees the camera stop.
use crate::platform::proc::spawn_tree;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long after the camera stopped it is tried again.
pub const RETRY: Duration = Duration::from_secs(2);
/// A camera picture older than this is no picture.
const STALE: Duration = Duration::from_millis(500);

/// Bytes of one `w`×`h` RGBA picture.
pub fn frame_bytes((w, h): (u32, u32)) -> usize { w as usize * h as usize * 4 }

/// The camera's own FFmpeg: `input` (as `record::args::camera_input`) to raw RGBA pictures of `size` at `fps` on
/// its output, kept in shape with black bars.
pub fn camera_args(input: &[String], (w, h): (u32, u32), fps: u32) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin"].map(String::from).into();
    a.extend(input.iter().cloned());
    a.extend(["-an".into(), "-vf".into(),
        format!("scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1,fps={fps},format=rgba"),
        "-f".into(), "rawvideo".into(), "-".into()]);
    a
}

/// The newest camera picture and when it came.
pub type Latest = Arc<Mutex<Option<(Arc<Vec<u8>>, Instant)>>>;

/// The picture in `latest` if it came less than `STALE` before `now`.
pub fn fresh(latest: &Latest, now: Instant) -> Option<Arc<Vec<u8>>> {
    latest.lock().unwrap_or_else(|e| e.into_inner()).as_ref().filter(|(_, at)| now.saturating_duration_since(*at) < STALE).map(|(p, _)| p.clone())
}

/// The feed under way. Dropping it stops it and the camera's FFmpeg.
pub struct Feed {
    pub latest: Latest,
    /// Whether camera pictures are coming now.
    pub seeing: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Feed {
    /// The newest camera picture, `None` while none is coming.
    pub fn picture(&self) -> Option<Arc<Vec<u8>>> { fresh(&self.latest, Instant::now()) }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() { let _ = t.join(); }
    }
}

impl Drop for Feed {
    fn drop(&mut self) { self.stop() }
}

/// Starts the feed of camera `input` at `size` and `fps`, its FFmpeg being `ffmpeg`.
/// An NDI source (`super::ndi::input_source`) is received over NDI instead.
pub fn start(ffmpeg: PathBuf, input: Vec<String>, size: (u32, u32), fps: u32) -> Result<Feed, String> {
    let (stop, seeing) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let latest: Latest = Arc::default();
    let (s, v, l) = (stop.clone(), seeing.clone(), latest.clone());
    let thread = match super::ndi::input_source(&input).map(str::to_string) {
        Some(name) => std::thread::Builder::new().name("stream-camera-ndi".into()).spawn(move || run_ndi(&name, size, &s, &v, &l)),
        None => std::thread::Builder::new().name("stream-camera".into()).spawn(move || run(ffmpeg, input, size, fps, &s, &v, &l)),
    }.map_err(|e| e.to_string())?;
    Ok(Feed { latest, seeing, stop, thread: Some(thread) })
}

/// An NDI camera: its receiver on a thread of its own (it connects again by itself), `seeing` kept here.
fn run_ndi(name: &str, size: (u32, u32), stop: &Arc<AtomicBool>, seeing: &AtomicBool, latest: &Latest) {
    let (n, s, l) = (name.to_string(), stop.clone(), latest.clone());
    let receiver = std::thread::Builder::new().name("stream-camera-ndi-read".into()).spawn(move || super::ndi::receive(&n, size, &l, &s, RETRY)).ok();
    while !stop.load(Ordering::Relaxed) {
        seeing.store(fresh(latest, Instant::now()).is_some(), Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(50));
    }
    seeing.store(false, Ordering::Relaxed);
    if let Some(t) = receiver { let _ = t.join(); }
}

fn run(ffmpeg: PathBuf, input: Vec<String>, size: (u32, u32), fps: u32, stop: &AtomicBool, seeing: &AtomicBool, latest: &Latest) {
    // The camera's FFmpeg on a thread of its own: its stop flag, and the thread (finished: the camera stopped).
    let mut camera: Option<(Arc<AtomicBool>, std::thread::JoinHandle<()>)> = None;
    let mut next_try = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if camera.as_ref().is_some_and(|(_, t)| t.is_finished()) { camera = None; next_try = Instant::now() + RETRY }
        if camera.is_none() && Instant::now() >= next_try {
            camera = spawn_camera(ffmpeg.clone(), input.clone(), size, fps, latest.clone());
            if camera.is_none() { next_try = Instant::now() + RETRY }
        }
        seeing.store(fresh(latest, Instant::now()).is_some(), Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(50));
    }
    seeing.store(false, Ordering::Relaxed);
    if let Some((s, t)) = camera { s.store(true, Ordering::Relaxed); let _ = t.join(); }
}

/// Starts the camera's FFmpeg on a thread of its own that keeps its newest picture in `latest`, until the camera
/// stops or the flag is set (the FFmpeg is then stopped). `None` when the thread could not start.
fn spawn_camera(ffmpeg: PathBuf, input: Vec<String>, size: (u32, u32), fps: u32, latest: Latest) -> Option<(Arc<AtomicBool>, std::thread::JoinHandle<()>)> {
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    let thread = std::thread::Builder::new().name("stream-camera-read".into()).spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
        rt.block_on(async move {
            use tokio::io::AsyncReadExt;
            let mut cmd = tokio::process::Command::new(&ffmpeg);
            cmd.args(camera_args(&input, size, fps)).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
            let mut tree = match spawn_tree(cmd) { Ok(t) => t, Err(e) => { eprintln!("stream camera: could not start: {e}"); return } };
            let Some(mut stdout) = tree.stdout() else { return };
            while !s.load(Ordering::Relaxed) {
                let mut buf = vec![0u8; frame_bytes(size)];
                match tokio::time::timeout(Duration::from_secs(1), stdout.read_exact(&mut buf)).await {
                    Ok(Ok(_)) => *latest.lock().unwrap_or_else(|e| e.into_inner()) = Some((Arc::new(buf), Instant::now())),
                    // No picture for a second: still waiting for a slow camera to start, or stuck; looked at again.
                    Err(_) => continue,
                    Ok(Err(_)) => break,
                }
            }
            tree.kill();
        });
    }).ok()?;
    Some((stop, thread))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feed_is_rgba_with_black_bars() {
        assert_eq!(frame_bytes((4, 2)), 32);
        let a = camera_args(&["-f".into(), "dshow".into(), "-i".into(), "video=Cam".into()], (1280, 720), 30).join(" ");
        assert!(a.contains("-i video=Cam -an -vf scale=1280:720") && a.contains("color=black") && a.ends_with("format=rgba -f rawvideo -"), "{a}");
    }

    #[test]
    fn a_network_camera_sends_its_pictures() {
        let Some(ff) = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).flat_map(|d| [d.join("ffmpeg"), d.join("ffmpeg.exe")]).find(|p| p.is_file())) else {
            eprintln!("no ffmpeg here: skipped");
            return;
        };
        // A camera on the network, played by FFmpeg: red pictures, waiting for the app to connect.
        let port = crate::platform::ports::tcp().unwrap();
        let address = format!("http://127.0.0.1:{port}/camera");
        let mut camera = std::process::Command::new(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-re", "-f", "lavfi", "-i", "color=c=red:size=320x240:rate=30", "-c:v", "mpeg2video", "-f", "mpegts", "-listen", "1"])
            .arg(&address).stdin(Stdio::null()).spawn().unwrap();
        let feed = start(ff, crate::record::args::camera_input(crate::platform::tools::OS, &address), (64, 36), 30).unwrap();
        let began = Instant::now();
        let picture = loop {
            // The picture, and the feed saying it sees the camera (looked at every 50 ms).
            if let Some(p) = feed.picture().filter(|_| feed.seeing.load(Ordering::Relaxed)) { break Some(p) }
            if began.elapsed() > Duration::from_secs(20) { break None }
            std::thread::sleep(Duration::from_millis(100));
        };
        let _ = camera.kill();
        let picture = picture.expect("pictures from the network camera");
        let middle = (18 * 64 + 32) * 4;
        let px = &picture[middle..middle + 4];
        assert!(px[0] > 200 && px[1] < 60 && px[2] < 60, "red: {px:?}");
    }

    #[test]
    fn the_camera_slot_goes_empty_when_pictures_stop() {
        let latest: Latest = Arc::default();
        let now = Instant::now();
        assert!(fresh(&latest, now).is_none(), "nothing yet");
        *latest.lock().unwrap() = Some((Arc::new(vec![1, 2, 3, 4]), now));
        assert!(fresh(&latest, now + Duration::from_millis(100)).is_some());
        assert!(fresh(&latest, now + Duration::from_secs(1)).is_none(), "a picture a second old is no picture");
    }
}
