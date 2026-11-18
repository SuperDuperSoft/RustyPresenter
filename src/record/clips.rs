//! Clips: parts of a recording marked while it records (the sermon, a solo), each also saved as its own video
//! once the recording is finished. The operator presses Clip to start one and again to end it; one still open
//! at the end runs to the end. The parts are copied out of the finished file, not encoded again: quick, and as
//! good as the recording (a clip starts at the keyframe before its mark, at most 2 s early).
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// A clip shorter than this is a slip of the finger, not a clip.
pub const SHORTEST: f64 = 1.0;

/// The clips marked so far: `(from, to)` seconds into the recording, and one started but not ended.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Marks {
    open: Option<f64>,
    done: Vec<(f64, f64)>,
}

impl Marks {
    /// Clip pressed `at` seconds in: starts a clip, or ends the open one. True when a clip is open now.
    pub fn toggle(&mut self, at: f64) -> bool {
        match self.open.take() {
            Some(from) => { self.done.push((from, at)); false }
            None => { self.open = Some(at); true }
        }
    }

    /// Whether a clip is open.
    pub fn is_open(&self) -> bool { self.open.is_some() }

    /// The recording ended: every clip, `None` as the end of one that runs to the end. Slips left out.
    pub fn finish(self) -> Vec<(f64, Option<f64>)> {
        self.done.into_iter().filter(|(a, b)| b - a >= SHORTEST).map(|(a, b)| (a, Some(b)))
            .chain(self.open.map(|a| (a, None))).collect()
    }
}

/// The `n`th clip's file (from 1), beside `recording`: "Recording 2026-11-22 clip 1.mp4" (a number further on
/// when that is taken).
pub fn name(recording: &Path, n: usize) -> PathBuf {
    let stem = recording.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = recording.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (1..).map(|k| recording.with_file_name(if k == 1 { format!("{stem} clip {n}{ext}") } else { format!("{stem} clip {n} ({k}){ext}") }))
        .find(|p| !p.exists()).expect("a free name")
}

/// FFmpeg copying `from` to `to` (or the end) of `src` into `out`.
pub fn args(src: &Path, from: f64, to: Option<f64>, out: &Path) -> Vec<OsString> {
    let secs = |v: f64| format!("{:.3}", v.max(0.0));
    let mut a: Vec<OsString> = ["-hide_banner", "-loglevel", "error", "-y", "-ss"].map(OsString::from).to_vec();
    a.extend([OsString::from(secs(from)), "-i".into(), src.into()]);
    if let Some(to) = to { a.extend([OsString::from("-t"), secs(to - from).into()]) }
    a.extend(["-map", "0", "-c", "copy", "-avoid_negative_ts", "make_zero", "-movflags", "+faststart"].map(OsString::from));
    a.push(out.into());
    a
}

/// Copies each clip out of the finished `recording`. Blocks: run it off the UI thread. Each clip's file, or why
/// it could not be made.
pub fn cut(ffmpeg: &Path, recording: &Path, clips: &[(f64, Option<f64>)]) -> Vec<Result<PathBuf, String>> {
    clips.iter().enumerate().map(|(i, (from, to))| {
        let out = name(recording, i + 1);
        let ok = std::process::Command::new(ffmpeg).args(args(recording, *from, *to, &out)).stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
        if ok && out.is_file() { Ok(out) } else { let _ = std::fs::remove_file(&out); Err(out.display().to_string()) }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_starts_and_ends_a_clip_and_slips_are_left_out() {
        let mut m = Marks::default();
        assert!(m.toggle(60.0) && m.is_open());
        assert!(!m.toggle(1800.0) && !m.is_open());
        assert!(m.toggle(2000.0));
        assert!(!m.toggle(2000.4), "pressed twice by mistake");
        assert!(m.toggle(2500.0), "open until the end");
        assert_eq!(m.finish(), [(60.0, Some(1800.0)), (2500.0, None)]);
    }

    #[test]
    fn a_clip_is_named_after_its_recording() {
        let dir = std::env::temp_dir().join(format!("rp-test-clips-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let rec = dir.join("Recording 2026-11-22 10-30.mp4");
        assert_eq!(name(&rec, 1), dir.join("Recording 2026-11-22 10-30 clip 1.mp4"));
        std::fs::write(dir.join("Recording 2026-11-22 10-30 clip 2.mp4"), b"x").unwrap();
        assert_eq!(name(&rec, 2), dir.join("Recording 2026-11-22 10-30 clip 2 (2).mp4"), "never over another file");
        assert_eq!(name(&dir.join("Old.mkv"), 1), dir.join("Old clip 1.mkv"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_clip_is_copied_not_encoded() {
        let s = |a: Vec<OsString>| a.into_iter().map(|x| x.to_string_lossy().into_owned()).collect::<Vec<_>>();
        let a = s(args(Path::new("r.mp4"), 60.5, Some(1800.0), Path::new("c.mp4")));
        assert!(a.windows(4).any(|w| w == ["-ss", "60.500", "-i", "r.mp4"]), "{a:?}");
        assert!(a.windows(2).any(|w| w == ["-t", "1739.500"]) && a.windows(2).any(|w| w == ["-c", "copy"]), "{a:?}");
        assert_eq!(a.last().unwrap(), "c.mp4");
        assert!(!s(args(Path::new("r.mp4"), 60.0, None, Path::new("c.mp4"))).contains(&"-t".to_string()), "to the end");
    }

    #[test]
    fn a_clip_is_cut_from_a_real_video() {
        let Some(ff) = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).flat_map(|d| [d.join("ffmpeg"), d.join("ffmpeg.exe")]).find(|p| p.is_file())) else {
            return eprintln!("no ffmpeg here: skipped");
        };
        let dir = std::env::temp_dir().join(format!("rp-test-clipcut-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let rec = dir.join("Recording.mp4");
        // Six seconds, a keyframe every second.
        assert!(std::process::Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30:duration=6", "-c:v", "libx264", "-g", "30"])
            .arg(&rec).status().unwrap().success());
        let made = cut(&ff, &rec, &[(1.0, Some(3.0)), (4.0, None)]);
        let files: Vec<PathBuf> = made.into_iter().map(|r| r.unwrap()).collect();
        assert_eq!(files, [dir.join("Recording clip 1.mp4"), dir.join("Recording clip 2.mp4")]);
        let secs = |f: &Path| {
            let o = std::process::Command::new("ffprobe").args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"]).arg(f).output().unwrap();
            String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().unwrap_or(0.0)
        };
        assert!((1.8..2.4).contains(&secs(&files[0])), "{}", secs(&files[0]));
        assert!((1.8..2.4).contains(&secs(&files[1])), "{}", secs(&files[1]));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
