//! The moving pictures of the church's scenes while live (design `docs/superpowers/specs/2026-11-17-scene-extras-design.md`):
//! each video file played again and again without its sound (its own FFmpeg to raw RGBA at the stream's size and
//! frame rate, as a camera: `camera_feed`), each web page (`web_page`), and the stinger videos of the scene changes.
//! All of them run off the writer's thread; the writer only takes their newest pictures.
//!
//! A stinger's FFmpeg is started ahead: it opens the file, decodes its first pictures and waits, blocked on its full
//! output, until a scene change reads it. So a stinger plays at once, and the next one is started ahead again.
use super::camera_feed;
use super::own_scenes::{Moving, OwnScene};
use super::transition::{Playing, Stinger, Transition};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// FFmpeg's input for a video file played again and again at its own speed.
pub fn video_input(path: &str) -> Vec<String> {
    ["-stream_loop", "-1", "-re", "-i", path].map(String::from).to_vec()
}

/// The stinger's FFmpeg: the file's pictures, filling `(w, h)` (cut off at the sides when its shape differs), at
/// `fps`, raw RGBA (straight) on its output. `vp9_alpha`: read a WebM with the decoder that keeps its see-through.
pub fn stinger_args(path: &str, (w, h): (u32, u32), fps: u32, vp9_alpha: bool) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin"].map(String::from).into();
    if vp9_alpha { a.extend(["-c:v".into(), "libvpx-vp9".into()]) }
    a.extend(["-i".into(), path.into(), "-an".into(), "-vf".into(),
        format!("scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},setsar=1,fps={fps},format=rgba"),
        "-f".into(), "rawvideo".into(), "-".into()]);
    a
}

/// `ffmpeg -progress` lines: the last `frame=` count.
pub fn frames_from_progress(text: &str) -> Option<u32> {
    text.lines().filter_map(|l| l.trim().strip_prefix("frame=")).filter_map(|n| n.trim().parse().ok()).last()
}

fn command(ffmpeg: &Path) -> Command {
    let mut cmd = Command::new(ffmpeg);
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; cmd.creation_flags(crate::platform::proc::creation_flags()); }
    cmd
}

/// One stinger video at one size, its FFmpeg started ahead.
struct Ready {
    ffmpeg: PathBuf,
    path: String,
    size: (u32, u32),
    fps: u32,
    vp9_alpha: bool,
    /// Its length in frames at `fps`, once counted (0: not yet).
    frames: Arc<AtomicU32>,
    next: Option<Child>,
}

impl Ready {
    fn new(ffmpeg: &Path, path: &str, size: (u32, u32), fps: u32, vp9_alpha: bool) -> Ready {
        let frames: Arc<AtomicU32> = Arc::default();
        // Counted once, on a thread of its own, small and as fast as it goes.
        let (ff, p, f) = (ffmpeg.to_path_buf(), path.to_string(), frames.clone());
        let _ = std::thread::Builder::new().name("stream-stinger-count".into()).spawn(move || {
            let out = command(&ff).args(["-hide_banner", "-loglevel", "error", "-nostdin", "-progress", "pipe:1", "-i", &p, "-an", "-vf", &format!("fps={fps},scale=64:-2"), "-f", "null", "-"])
                .stdin(Stdio::null()).stderr(Stdio::null()).output();
            if let Ok(o) = out && let Some(n) = frames_from_progress(&String::from_utf8_lossy(&o.stdout)) { f.store(n, Ordering::Relaxed) }
        });
        let mut r = Ready { ffmpeg: ffmpeg.to_path_buf(), path: path.to_string(), size, fps, vp9_alpha, frames, next: None };
        r.next = r.spawn();
        r
    }

    fn spawn(&self) -> Option<Child> {
        command(&self.ffmpeg).args(stinger_args(&self.path, self.size, self.fps, self.vp9_alpha)).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn().map_err(|e| eprintln!("stream stinger: could not start: {e}")).ok()
    }

    /// Plays it now (and starts the next one ahead).
    fn play(&mut self) -> Option<Playing> {
        let mut child = self.next.take().or_else(|| self.spawn())?;
        let mut out = child.stdout.take()?;
        let bytes = camera_feed::frame_bytes(self.size);
        let (tx, frames) = std::sync::mpsc::sync_channel::<Vec<u8>>(2);
        let started = std::thread::Builder::new().name("stream-stinger".into()).spawn(move || {
            loop {
                let mut buf = vec![0u8; bytes];
                if out.read_exact(&mut buf).is_err() { break }
                premultiply(&mut buf);
                if tx.send(buf).is_err() { break }
            }
            let _ = child.kill();
            let _ = child.wait();
        });
        if started.is_err() { return None }
        self.next = self.spawn();
        let counted = self.frames.load(Ordering::Relaxed);
        let middle = if counted > 0 { counted / 2 } else { self.fps / 2 };
        Some(Playing { frames, middle, fps: self.fps })
    }
}

impl Drop for Ready {
    fn drop(&mut self) {
        if let Some(mut c) = self.next.take() { let _ = c.kill(); let _ = c.wait(); }
    }
}

/// Straight RGBA to premultiplied, in place.
pub fn premultiply(rgba: &mut [u8]) {
    for p in rgba.chunks_exact_mut(4) {
        let a = u32::from(p[3]);
        if a == 255 { continue }
        for c in 0..3 { p[c] = ((u32::from(p[c]) * a + 127) / 255) as u8 }
    }
}

/// Everything moving in the church's scenes while live. Dropping it stops it all.
#[derive(Default)]
pub struct Media {
    videos: Vec<(String, camera_feed::Feed)>,
    pages: Vec<(String, super::web_page::Page)>,
    /// Each stinger video at the wide size and, with a tall picture, at the tall size.
    stingers: Vec<(String, Ready, Option<Ready>)>,
}

impl Media {
    /// Starts the videos and web pages of `own`, and gets their stinger videos (and the default's, `default`) ready,
    /// for a stream of `size` (and a tall picture of `tall`) at `fps`.
    pub fn start(ffmpeg: &Path, own: &[OwnScene], default: &Transition, size: (u32, u32), tall: Option<(u32, u32)>, fps: u32) -> Media {
        let mut m = Media::default();
        for path in super::own_scenes::videos(own) {
            if !Path::new(&path).is_file() { eprintln!("stream scene: no video file {path}"); continue }
            match camera_feed::start(ffmpeg.to_path_buf(), video_input(&path), size, fps) {
                Ok(f) => m.videos.push((path, f)),
                Err(e) => eprintln!("stream scene: the video {path} could not start: {e}"),
            }
        }
        for address in super::own_scenes::web_pages(own) {
            match super::web_page::Page::start(&address, size) {
                Ok(p) => m.pages.push((address, p)),
                Err(e) => eprintln!("stream scene: the web page {address} could not start: {e}"),
            }
        }
        let mut stingers: Vec<String> = own.iter().filter_map(|s| s.transition.as_ref()?.stinger().map(str::to_string)).collect();
        if let Some(p) = default.stinger() { stingers.push(p.to_string()) }
        stingers.dedup();
        let decoders = if stingers.iter().any(|p| p.to_lowercase().ends_with(".webm")) {
            crate::record::run::ask_ffmpeg(ffmpeg, &["-hide_banner", "-decoders"]).unwrap_or_default()
        } else { String::new() };
        for path in stingers {
            if m.stingers.iter().any(|(p, ..)| *p == path) { continue }
            if !Path::new(&path).is_file() { eprintln!("stream scene: no stinger video {path}"); continue }
            let vp9_alpha = path.to_lowercase().ends_with(".webm") && decoders.contains("libvpx-vp9");
            let wide = Ready::new(ffmpeg, &path, size, fps, vp9_alpha);
            let tall = tall.map(|t| Ready::new(ffmpeg, &path, t, fps, vp9_alpha));
            m.stingers.push((path, wide, tall));
        }
        m
    }

    /// The newest picture of a video or web page, `None` while it has none.
    pub fn picture(&self, m: &Moving) -> Option<Arc<Vec<u8>>> {
        match m {
            Moving::Video(path) => self.videos.iter().find(|(p, _)| p == path)?.1.picture(),
            Moving::Web(address) => self.pages.iter().find(|(a, _)| a == address)?.1.picture(),
        }
    }

    /// Plays the stinger `path` now, over the wide picture and (`tall`) the tall one.
    pub fn stinger(&mut self, path: &str, tall: bool) -> Option<Stinger> {
        let (_, wide, t) = self.stingers.iter_mut().find(|(p, ..)| p == path)?;
        let wide = wide.play()?;
        let tall = if tall { t.as_mut().and_then(Ready::play) } else { None };
        Some(Stinger { wide, tall })
    }

    pub fn stop(&mut self) {
        for (_, f) in &mut self.videos { f.stop() }
        for (_, p) in &mut self.pages { p.stop() }
        self.stingers.clear();
    }
}

impl Drop for Media {
    fn drop(&mut self) { self.stop() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn ffmpeg() -> Option<PathBuf> {
        std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).flat_map(|d| [d.join("ffmpeg"), d.join("ffmpeg.exe")]).find(|p| p.is_file()))
    }

    #[test]
    fn the_arguments_loop_a_video_and_fill_with_a_stinger() {
        assert_eq!(video_input("C:/a b.mp4"), ["-stream_loop", "-1", "-re", "-i", "C:/a b.mp4"]);
        let a = stinger_args("s.webm", (1280, 720), 30, true).join(" ");
        assert!(a.contains("-c:v libvpx-vp9 -i s.webm -an") && a.contains("force_original_aspect_ratio=increase,crop=1280:720") && a.ends_with("format=rgba -f rawvideo -"), "{a}");
        assert_eq!(frames_from_progress("frame=10\nfps=0\nprogress=continue\nframe=61\nprogress=end\n"), Some(61));
        let mut px = vec![200, 100, 50, 128];
        premultiply(&mut px);
        assert_eq!(px, [100, 50, 25, 128]);
    }

    #[test]
    fn a_video_layer_plays_its_file() {
        let Some(ff) = ffmpeg() else { eprintln!("no ffmpeg here: skipped"); return };
        let dir = std::env::temp_dir().join(format!("rp-test-video-layer-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("red.mp4");
        let made = Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=red:size=160x90:rate=30:duration=1", "-pix_fmt", "yuv420p"]).arg(&path).status().unwrap();
        assert!(made.success());
        let scene = OwnScene { layers: vec![super::super::own_scenes::Layer { shows: super::super::own_scenes::Shows::Video { path: path.to_string_lossy().into() }, ..Default::default() }], ..OwnScene::new("v".into()) };
        let media = Media::start(&ff, &[scene], &Transition::Fade, (64, 36), None, 30);
        let began = Instant::now();
        let got = loop {
            if let Some(p) = media.picture(&Moving::Video(path.to_string_lossy().into())) { break Some(p) }
            if began.elapsed() > Duration::from_secs(20) { break None }
            std::thread::sleep(Duration::from_millis(100));
        };
        // Still playing after the one-second file ended: it plays again and again.
        std::thread::sleep(Duration::from_millis(1500));
        let later = media.picture(&Moving::Video(path.to_string_lossy().into()));
        drop(media);
        let _ = std::fs::remove_dir_all(&dir);
        let p = got.expect("pictures from the video");
        assert_eq!(p.len(), 64 * 36 * 4);
        let mid = (18 * 64 + 32) * 4;
        assert!(p[mid] > 200 && p[mid + 1] < 60, "red: {:?}", &p[mid..mid + 4]);
        assert!(later.is_some(), "the video goes round again");
    }

    #[test]
    fn a_stinger_is_ready_at_once_and_counted() {
        let Some(ff) = ffmpeg() else { eprintln!("no ffmpeg here: skipped"); return };
        let dir = std::env::temp_dir().join(format!("rp-test-stinger-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sting.mov");
        // Twenty frames (two-thirds of a second at 30), half see-through white, with a see-through picture kept.
        let made = Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=white@0.5:size=64x36:rate=30:duration=0.667,format=rgba", "-c:v", "qtrle"]).arg(&path).status().unwrap();
        assert!(made.success());
        let p = path.to_string_lossy().to_string();
        let mut media = Media::start(&ff, &[], &Transition::Stinger { path: p.clone() }, (32, 18), Some((18, 32)), 30);
        // Its length is counted on a thread of its own.
        let began = Instant::now();
        while media.stingers[0].1.frames.load(Ordering::Relaxed) == 0 && began.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(50)) }
        let s = media.stinger(&p, true).expect("a stinger");
        assert_eq!(s.wide.middle, 10, "the middle of its twenty frames");
        let first = s.wide.frames.recv_timeout(Duration::from_secs(10)).expect("its first picture");
        assert_eq!(first.len(), 32 * 18 * 4);
        assert!((120..=135).contains(&first[0]) && (120..=135).contains(&first[3]), "half see-through white, premultiplied: {:?}", &first[..4]);
        let tall = s.tall.expect("the tall one too").frames.recv_timeout(Duration::from_secs(10)).expect("a tall picture");
        assert_eq!(tall.len(), 18 * 32 * 4);
        let n = 1 + s.wide.frames.iter().count();
        assert!((19..=21).contains(&n), "all its frames: {n}");
        assert!(media.stinger(&p, false).is_some(), "ready again for the next change");
        drop(media);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
