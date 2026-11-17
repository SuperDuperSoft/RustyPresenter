//! A second recording while live, without the words over the camera (More settings → "Also record the stream
//! without the words"): the same scenes, cameras, cards and audience screen, so an editor can cut a clean video, or lay
//! other words over it. Its pictures are put together by the writer beside the stream's (`record::run`) and handed
//! here; a thread of its own feeds a FFmpeg of its own, which writes Matroska (readable after a crash) and times
//! each picture as it comes, filling any gap. So it never holds up the recording or the stream: when it cannot keep
//! up, pictures are left out here, never there. At the end it gets the recording's sound
//! (`finish_args`) and becomes "<recording name> clean.mp4".
use crate::platform::proc::spawn_tree;
use crate::record::advanced::{self, Format};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// How long its FFmpeg may take to finish the part.
const END_LIMIT: Duration = Duration::from_secs(30);
/// Pictures waiting for its FFmpeg; more are left out.
const WAITING: usize = 3;

/// "<recording name> clean.mp4" (or `.mkv`) beside the recording `out`, never over another file.
pub fn name_for(out: &Path, format: Format) -> PathBuf {
    let stem = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = format.extension();
    (1..).map(|i| if i == 1 { format!("{stem} clean.{ext}") } else { format!("{stem} clean ({i}).{ext}") })
        .map(|n| out.with_file_name(n)).find(|p| !p.exists() && !part_of(p).exists()).expect("a free name")
}

/// The part written while it records.
pub fn part_of(file: &Path) -> PathBuf { crate::record::run::partial_of(file) }

/// Its FFmpeg: raw RGBA pictures of `size`, each timed as it comes, encoded as the recording is (`video`), into
/// the Matroska `part`.
pub fn args((w, h): (u32, u32), fps: u32, encoder: &str, kbps: u32, video: &advanced::Video, part: &Path) -> Vec<OsString> {
    let mut a: Vec<OsString> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{w}x{h}"),
        "-framerate", &fps.to_string(), "-use_wallclock_as_timestamps", "1", "-i", "-", "-an", "-fps_mode", "cfr", "-r", &fps.to_string()]
        .iter().map(OsString::from).collect();
    a.extend(advanced::video_args(encoder, kbps, fps, video).into_iter().map(OsString::from));
    a.extend(["-flush_packets", "1", "-cluster_time_limit", "2000", "-f", "matroska", "-y"].map(OsString::from));
    a.push(part.into());
    a
}

/// FFmpeg making the finished `file` from the `part` and the finished recording's sound (all its tracks).
pub fn finish_args(part: &Path, recording: &Path, file: &Path, format: Format) -> Vec<OsString> {
    let mut a: Vec<OsString> = ["-hide_banner", "-loglevel", "warning", "-y", "-i"].map(OsString::from).into();
    a.push(part.into());
    a.push("-i".into());
    a.push(recording.into());
    a.extend(["-map", "0:v", "-map", "1:a?", "-c", "copy", "-shortest"].map(OsString::from));
    if format == Format::Mp4 { a.extend(["-movflags", "+faststart"].map(OsString::from)) }
    a.extend(["-f", format.muxer()].map(OsString::from));
    a.push(file.into());
    a
}

/// The clean recording being made.
pub struct CleanFile {
    tx: Option<mpsc::SyncSender<Vec<u8>>>,
    thread: Option<std::thread::JoinHandle<bool>>,
    /// The finished file it becomes.
    pub file: PathBuf,
    pub part: PathBuf,
    /// Pictures left out because its FFmpeg could not keep up.
    pub left_out: u64,
}

impl CleanFile {
    /// Starts it beside recording `out`.
    pub fn start(ffmpeg: &Path, out: &Path, format: Format, size: (u32, u32), fps: u32, encoder: &str, kbps: u32, video: &advanced::Video) -> Result<CleanFile, String> {
        let file = name_for(out, format);
        let part = part_of(&file);
        let a = args(size, fps, encoder, kbps, video, &part);
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(WAITING);
        let ff = ffmpeg.to_path_buf();
        let thread = std::thread::Builder::new().name("clean-file".into()).spawn(move || run(&ff, a, rx)).map_err(|e| e.to_string())?;
        Ok(CleanFile { tx: Some(tx), thread: Some(thread), file, part, left_out: 0 })
    }

    /// The picture without the words, as it goes out now. Never waits.
    pub fn picture(&mut self, rgba: &[u8]) {
        let Some(tx) = &self.tx else { return };
        match tx.try_send(rgba.to_vec()) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => self.left_out += 1,
            Err(mpsc::TrySendError::Disconnected(_)) => self.tx = None,
        }
    }

    /// Ends its FFmpeg and waits for the part to be written. True when it was.
    pub fn stop(&mut self) -> bool {
        self.tx = None;
        self.thread.take().is_some_and(|t| t.join().unwrap_or(false))
    }
}

impl Drop for CleanFile {
    fn drop(&mut self) { self.stop(); }
}

fn run(ffmpeg: &Path, args: Vec<OsString>, rx: mpsc::Receiver<Vec<u8>>) -> bool {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return false };
    rt.block_on(async move {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let mut cmd = tokio::process::Command::new(ffmpeg);
        cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut tree = match spawn_tree(cmd) { Ok(t) => t, Err(e) => { eprintln!("clean recording: could not start: {e}"); return false } };
        let (Some(mut stdin), Some(err)) = (tree.stdin(), tree.stderr()) else { return false };
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(err).lines();
            while let Ok(Some(l)) = lines.next_line().await { eprintln!("ffmpeg (clean recording): {l}") }
        });
        // The pictures come from a thread that never waits: they are taken here one by one.
        let (atx, mut arx) = tokio::sync::mpsc::channel::<Vec<u8>>(1);
        std::thread::spawn(move || { while let Ok(p) = rx.recv() { if atx.blocking_send(p).is_err() { break } } });
        while let Some(p) = arx.recv().await {
            if stdin.write_all(&p).await.is_err() { eprintln!("clean recording: its FFmpeg stopped; the recording goes on without it"); break }
        }
        let _ = stdin.flush().await;
        drop(stdin);
        matches!(tree.wait(END_LIMIT, &CancellationToken::new()).await, Ok(s) if s.success())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(a: Vec<OsString>) -> String { a.iter().map(|x| x.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ") }

    #[test]
    fn it_sits_beside_the_recording_and_gets_its_sound_at_the_end() {
        let dir = std::env::temp_dir().join(format!("rp-clean-name-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("Recording a.mp4");
        assert_eq!(name_for(&out, Format::Mp4), dir.join("Recording a clean.mp4"));
        std::fs::write(dir.join("Recording a clean.recording.mkv"), b"x").unwrap();
        assert_eq!(name_for(&out, Format::Mp4), dir.join("Recording a clean (2).mp4"), "one being written is taken too");
        assert_eq!(name_for(&out, Format::Mkv), dir.join("Recording a clean (2).mkv"), "nor beside one being written");
        let a = s(args((1280, 720), 30, "libx264", 3000, &Default::default(), Path::new("c.recording.mkv")));
        assert!(a.contains("-s 1280x720 -framerate 30 -use_wallclock_as_timestamps 1 -i - -an -fps_mode cfr -r 30 -c:v libx264 -preset veryfast"), "{a}");
        assert!(a.ends_with("-flush_packets 1 -cluster_time_limit 2000 -f matroska -y c.recording.mkv"), "{a}");
        let f = s(finish_args(Path::new("c.recording.mkv"), Path::new("r.mp4"), Path::new("c.mp4"), Format::Mp4));
        assert_eq!(f, "-hide_banner -loglevel warning -y -i c.recording.mkv -i r.mp4 -map 0:v -map 1:a? -c copy -shortest -movflags +faststart -f mp4 c.mp4");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
