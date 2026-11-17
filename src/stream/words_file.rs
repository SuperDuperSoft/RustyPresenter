//! The words alone, in a video of their own, while live (docs/superpowers/specs/2026-11-17-ndi-extras-design.md):
//! the words over the camera (what NDI's "RustyPresenter Words" carries), see-through around them, as
//! "<recording name> words.mov" beside the recording, for an editor to lay over the service video afterwards.
//!
//! QuickTime Animation (`qtrle`) in MOV: lossless with straight alpha, read by the common video editors, and it
//! keeps only what changed from the picture before, so words that change a few times a minute make a small file.
//! The writer hands over the words only when they change; a thread of its own feeds a FFmpeg of its own, which puts
//! the time on each picture as it comes and fills the gaps itself. So it never holds up the recording or the
//! stream, and if it fails only this file is lost.
//!
//! It survives a crash: while live it is written as "<recording name> words.recording.mov", a MOV in pieces of a
//! second each (fragmented), every piece readable on its own, so a stop in the middle loses at most the last
//! second. At the end it is copied, as it is, into an ordinary MOV ("… words.mov"), which editors open and jump
//! about in quickly; one left by a crash is copied so the next time a recording starts (`finish_leftovers`).
use crate::platform::proc::spawn_tree;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// The picture is sent again this often while the words stay the same (FFmpeg fills between).
const AGAIN: Duration = Duration::from_secs(1);
/// How long its FFmpeg may take to finish the file.
const FINISH_LIMIT: Duration = Duration::from_secs(30);
/// A whole picture this often (in pictures), so an editor can jump about quickly: 10 s at 30 a second.
const WHOLE_EVERY: u32 = 300;

/// "<recording name> words.mov" beside the recording `out`, never over another file (nor one being written).
pub fn name_for(out: &Path) -> PathBuf {
    let stem = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    (1..).map(|i| if i == 1 { format!("{stem} words.mov") } else { format!("{stem} words ({i}).mov") })
        .map(|n| out.with_file_name(n)).find(|p| !p.exists() && !part_of(p).exists()).expect("a free name")
}

/// `… words.mov` → `… words.recording.mov`: the file written while live.
pub fn part_of(file: &Path) -> PathBuf {
    let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    file.with_file_name(format!("{stem}{PART}"))
}

const PART: &str = ".recording.mov";

/// FFmpeg copying the fragmented `part` into the ordinary MOV `file`.
pub fn finish_args(part: &Path, file: &Path) -> Vec<std::ffi::OsString> {
    let mut a: Vec<std::ffi::OsString> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"].iter().map(Into::into).collect();
    a.push(part.into());
    a.extend(["-map", "0", "-c", "copy", "-f", "mov"].iter().map(Into::into));
    a.push(file.into());
    a
}

/// Makes the ordinary MOV from `part`, then removes the part; if it cannot, the part becomes the file as it is
/// (it plays: only jumping about is slower).
fn finish_part(ffmpeg: &Path, part: &Path, file: &Path) {
    if std::fs::metadata(part).map(|m| m.len()).unwrap_or(0) == 0 { let _ = std::fs::remove_file(part); return }
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(finish_args(part, file)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; cmd.creation_flags(crate::platform::proc::creation_flags()); }
    let ok = cmd.status().is_ok_and(|s| s.success()) && std::fs::metadata(file).is_ok_and(|m| m.len() > 0);
    if ok { let _ = std::fs::remove_file(part); } else { let _ = std::fs::remove_file(file); let _ = std::fs::rename(part, file); }
}

/// The words files in `folder` left unfinished by a stop in the middle, each made into its ordinary MOV. Blocks.
pub fn finish_leftovers(ffmpeg: &Path, folder: &Path) {
    let Ok(dir) = std::fs::read_dir(folder) else { return };
    let suffix = format!(" words{PART}");
    for part in dir.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(&suffix)) {
        // Still being written by a stream under way: it is finished with that stream.
        if recently_written(&part) { continue }
        let name = part.file_name().map(|n| n.to_string_lossy().trim_end_matches(PART).to_string()).unwrap_or_default();
        let file = part.with_file_name(format!("{name}.mov"));
        let file = if file.exists() { part.with_file_name(format!("{name} (found).mov")) } else { file };
        finish_part(ffmpeg, &part, &file);
    }
}

/// Written to in the last minute: a stream under way is writing it.
fn recently_written(p: &Path) -> bool {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < Duration::from_secs(60))
}

/// Its FFmpeg: raw RGBA pictures of `size` on its input, each timed as it comes, into `file` at `fps`.
pub fn args((w, h): (u32, u32), fps: u32, file: &Path) -> Vec<std::ffi::OsString> {
    let mut a: Vec<std::ffi::OsString> = [
        "-hide_banner", "-loglevel", "error", "-nostdin",
        "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{w}x{h}"), "-framerate", &fps.to_string(), "-use_wallclock_as_timestamps", "1", "-i", "-",
        "-an", "-fps_mode", "cfr", "-r", &fps.to_string(), "-c:v", "qtrle", "-pix_fmt", "argb", "-g", &WHOLE_EVERY.to_string(),
        // In pieces of a second, each readable on its own: a stop in the middle loses only the last.
        "-movflags", "+frag_keyframe+empty_moov+default_base_moof", "-frag_duration", "1000000", "-f", "mov", "-y",
    ].iter().map(Into::into).collect();
    a.push(file.into());
    a
}

#[derive(Default)]
struct Shared {
    words: Option<Arc<Vec<u8>>>,
    new: bool,
    stop: bool,
}

/// The words file being made. Dropping it finishes it.
pub struct WordsFile {
    shared: Arc<(Mutex<Shared>, tokio::sync::Notify)>,
    last: Option<Arc<Vec<u8>>>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// Where it is written.
    pub file: PathBuf,
}

impl WordsFile {
    /// Starts the words file beside recording `out`, for pictures of `size` at `fps`, its FFmpeg being `ffmpeg`.
    pub fn start(ffmpeg: &Path, out: &Path, size: (u32, u32), fps: u32) -> Result<WordsFile, String> {
        let file = name_for(out);
        let part = part_of(&file);
        let shared: Arc<(Mutex<Shared>, tokio::sync::Notify)> = Arc::new((Mutex::default(), tokio::sync::Notify::new()));
        let (sh, ff, f) = (shared.clone(), ffmpeg.to_path_buf(), file.clone());
        let thread = std::thread::Builder::new().name("words-file".into()).spawn(move || {
            run(&ff, &part, size, fps, &sh);
            finish_part(&ff, &part, &f);
        }).map_err(|e| e.to_string())?;
        Ok(WordsFile { shared, last: None, thread: Some(thread), file })
    }

    /// The words over the camera now (premultiplied RGBA, `None`: none). Only a change is handed over, and it never
    /// waits for the file.
    pub fn words(&mut self, words: &Option<Arc<Vec<u8>>>) {
        let same = match (&self.last, words) { (None, None) => true, (Some(a), Some(b)) => Arc::ptr_eq(a, b), _ => false };
        if same { return }
        self.last = words.clone();
        let (m, wake) = &*self.shared;
        let mut s = m.lock().unwrap_or_else(|e| e.into_inner());
        s.words = words.clone();
        s.new = true;
        wake.notify_one();
    }

    /// Asks it to end: the last picture goes in and its FFmpeg finishes the file. Does not wait (`finish` does).
    pub fn stop(&self) {
        let (m, wake) = &*self.shared;
        m.lock().unwrap_or_else(|e| e.into_inner()).stop = true;
        wake.notify_one();
    }

    /// Stops it and waits for the file to be finished.
    pub fn finish(mut self) { self.end() }

    fn end(&mut self) {
        self.stop();
        if let Some(t) = self.thread.take() { let _ = t.join(); }
    }
}

impl Drop for WordsFile {
    fn drop(&mut self) { self.end() }
}

fn run(ffmpeg: &Path, file: &Path, size: (u32, u32), fps: u32, shared: &(Mutex<Shared>, tokio::sync::Notify)) {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
    rt.block_on(async move {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let mut cmd = tokio::process::Command::new(ffmpeg);
        cmd.args(args(size, fps, file)).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut tree = match spawn_tree(cmd) { Ok(t) => t, Err(e) => { eprintln!("words file: could not start: {e}"); return } };
        let (Some(mut stdin), Some(err)) = (tree.stdin(), tree.stderr()) else { return };
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(err).lines();
            while let Ok(Some(l)) = lines.next_line().await { eprintln!("ffmpeg (words file): {l}") }
        });
        let (m, wake) = shared;
        let clear = vec![0u8; super::camera_feed::frame_bytes(size)];
        let mut picture = clear.clone();
        loop {
            let (new, stop) = {
                let mut s = m.lock().unwrap_or_else(|e| e.into_inner());
                (s.new.then(|| { s.new = false; s.words.clone() }), s.stop)
            };
            if let Some(w) = new {
                match w { Some(p) if p.len() == clear.len() => super::ndi::straight(&p, &mut picture), _ => picture.clone_from(&clear) }
            }
            if stdin.write_all(&picture).await.is_err() { eprintln!("words file: its FFmpeg stopped; the recording goes on without it"); break }
            if stop { break }
            let _ = tokio::time::timeout(AGAIN, wake.notified()).await;
        }
        let _ = stdin.flush().await;
        drop(stdin);
        if let Err(e) = tree.wait(FINISH_LIMIT, &CancellationToken::new()).await { eprintln!("words file: FFmpeg did not finish it: {e:?}") }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn on_path(tool: &str) -> Option<PathBuf> {
        std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).flat_map(|d| [d.join(tool), d.join(format!("{tool}.exe"))]).find(|p| p.is_file()))
    }

    #[test]
    fn the_words_file_sits_beside_the_recording_and_never_over_another() {
        let dir = std::env::temp_dir().join(format!("rp-words-name-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("Recording 2026-11-17 10-00.mp4");
        assert_eq!(name_for(&out), dir.join("Recording 2026-11-17 10-00 words.mov"));
        std::fs::write(dir.join("Recording 2026-11-17 10-00 words.mov"), b"x").unwrap();
        assert_eq!(name_for(&out), dir.join("Recording 2026-11-17 10-00 words (2).mov"));
        let a: Vec<String> = args((1280, 720), 30, Path::new("w.mov")).iter().map(|s| s.to_string_lossy().into_owned()).collect();
        let a = a.join(" ");
        assert!(a.contains("-s 1280x720") && a.contains("-use_wallclock_as_timestamps 1 -i -") && a.contains("-fps_mode cfr -r 30 -c:v qtrle -pix_fmt argb") && a.ends_with("w.mov"), "{a}");
        assert!(a.contains("-movflags +frag_keyframe+empty_moov+default_base_moof -frag_duration 1000000 -f mov"), "{a}");
        // One being written is taken too.
        std::fs::write(dir.join("Recording 2026-11-17 10-00 words (2).recording.mov"), b"x").unwrap();
        assert_eq!(name_for(&out), dir.join("Recording 2026-11-17 10-00 words (3).mov"));
        assert_eq!(part_of(&dir.join("a words.mov")), dir.join("a words.recording.mov"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A words file cut short (as by a crash, the part's end missing) is still readable, and is made into its
    /// ordinary MOV the next time, see-through kept.
    #[test]
    fn a_words_file_cut_short_by_a_crash_is_kept() {
        let Some(ff) = on_path("ffmpeg") else { eprintln!("no ffmpeg here: skipped"); return };
        let dir = std::env::temp_dir().join(format!("rp-words-crash-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let whole = dir.join("whole.mov");
        // Six seconds as the words file writes them, from FFmpeg's test picture (see-through red).
        let mut a: Vec<std::ffi::OsString> = ["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=red@0.5:s=64x36:r=30:d=6,format=rgba"].iter().map(Into::into).collect();
        let made = args((64, 36), 30, &whole);
        let at = made.iter().position(|x| x == "-an").unwrap();
        a.extend(made[at..].iter().cloned());
        assert!(std::process::Command::new(&ff).args(&a).status().unwrap().success());
        // The stop in the middle: only the first 60 % reached the disk.
        let bytes = std::fs::read(&whole).unwrap();
        let part = dir.join("Recording x words.recording.mov");
        std::fs::write(&part, &bytes[..bytes.len() * 6 / 10]).unwrap();
        // Long enough ago that no stream is writing it.
        let old = std::time::SystemTime::now() - Duration::from_secs(600);
        std::fs::File::options().write(true).open(&part).unwrap().set_modified(old).unwrap();
        finish_leftovers(&ff, &dir);
        let file = dir.join("Recording x words.mov");
        assert!(file.is_file() && !part.exists(), "made into its MOV");
        let px = rgba_at(&ff, &file, "1");
        assert_eq!(px.len(), 64 * 36 * 4, "readable");
        assert!((120..=136).contains(&px[3]), "see-through kept: {:?}", &px[..4]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The picture at `t` seconds into `file`, as straight RGBA.
    fn rgba_at(ff: &Path, file: &Path, t: &str) -> Vec<u8> {
        let out = std::process::Command::new(ff).args(["-hide_banner", "-loglevel", "error", "-ss", t, "-i"]).arg(file)
            .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgba", "-"]).output().unwrap();
        out.stdout
    }

    /// For real with FFmpeg (skipped without it): no words, then a white band with a half see-through red dot, then
    /// none again. The file is QuickTime Animation with alpha, as long as it ran, clear where there are no words.
    #[test]
    fn the_words_alone_are_recorded_see_through_around_them() {
        let Some(ff) = on_path("ffmpeg") else { eprintln!("no ffmpeg here: skipped"); return };
        let dir = std::env::temp_dir().join(format!("rp-words-file-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, h) = (64u32, 36u32);
        let mut band = vec![0u8; super::super::camera_feed::frame_bytes((w, h))];
        for y in 24..h as usize { for x in 0..w as usize { band[(y * w as usize + x) * 4..][..4].copy_from_slice(&[255, 255, 255, 255]) } }
        // Premultiplied half see-through red at (4, 4).
        band[(4 * w as usize + 4) * 4..][..4].copy_from_slice(&[128, 0, 0, 128]);
        let band = Some(Arc::new(band));
        let mut f = WordsFile::start(&ff, &dir.join("Recording test.mp4"), (w, h), 30).unwrap();
        let file = f.file.clone();
        assert_eq!(file, dir.join("Recording test words.mov"));
        f.words(&None);
        // Its FFmpeg makes the part once it has the first picture (it can be slow to start while the suite runs).
        let waited = Instant::now();
        while !part_of(&file).is_file() && waited.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(20)) }
        let began = Instant::now();
        std::thread::sleep(Duration::from_millis(800));
        f.words(&band);
        // The same words again: nothing handed over.
        for _ in 0..10 { f.words(&band); std::thread::sleep(Duration::from_millis(150)) }
        f.words(&None);
        std::thread::sleep(Duration::from_millis(800));
        f.finish();
        let ran = began.elapsed().as_secs_f64();
        assert!(file.is_file() && !part_of(&file).exists(), "the words file, made ordinary");
        if let Some(probe) = on_path("ffprobe") {
            let out = std::process::Command::new(&probe).args(["-v", "error", "-show_entries", "stream=codec_name,pix_fmt,width,height:format=duration", "-of", "json"])
                .arg(&file).output().unwrap();
            let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            let s = &v["streams"][0];
            assert_eq!((s["codec_name"].as_str(), s["pix_fmt"].as_str(), s["width"].as_u64(), s["height"].as_u64()), (Some("qtrle"), Some("argb"), Some(64), Some(36)), "{v}");
            let secs: f64 = v["format"]["duration"].as_str().and_then(|d| d.parse().ok()).unwrap_or(0.0);
            eprintln!("words file: {secs:.2} s of {ran:.2} s");
            assert!(secs > ran - 0.5 && secs < ran + 1.0, "as long as it ran: {secs} of {ran}");
        } else {
            eprintln!("no ffprobe here: the format is not looked at");
        }
        let px = |p: &[u8], x: usize, y: usize| p[(y * w as usize + x) * 4..][..4].to_vec();
        let before = rgba_at(&ff, &file, "0.3");
        assert_eq!(before.len(), 64 * 36 * 4);
        assert!(before.chunks_exact(4).all(|p| p[3] == 0), "no words: clear all over");
        let during = rgba_at(&ff, &file, "1.6");
        assert_eq!(px(&during, 10, 30), [255, 255, 255, 255], "the words, solid");
        assert_eq!(px(&during, 10, 10)[3], 0, "see-through around them");
        let dot = px(&during, 4, 4);
        assert!(dot[0] > 240 && dot[1] < 10 && (126..=130).contains(&dot[3]), "half see-through red, straight: {dot:?}");
        let after = rgba_at(&ff, &file, &format!("{:.2}", ran - 0.3));
        assert!(after.chunks_exact(4).all(|p| p[3] == 0), "clear again at the end");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
