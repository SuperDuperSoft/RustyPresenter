//! One recording: the `ffmpeg` child (in its own process tree, which dies with the app), the writer thread that
//! feeds it, the disk checks, and turning the Matroska part into the MP4 at the end (`finish_file`). A part left by
//! a crash is finished the next time a recording starts in that folder (`finish_leftovers`).
use super::{Preset, Sound, args, pump::{FrameSink, Pump}, sound::Segment, tr};
use crate::platform::{disk, proc::{Stop, spawn_tree}, tools};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

/// The recording stops when the disk has less than this left, so the computer keeps working.
const DISK_FLOOR: u64 = 500 * 1024 * 1024;
/// It does not start with less than 20 minutes' room.
const START_SHARE: u64 = 3;
/// How often the disk is checked and the sound kept on disk for a crash.
const CHECK_EVERY: Duration = Duration::from_secs(10);
/// How long FFmpeg may take to end its file once the pictures stop.
const END_LIMIT: Duration = Duration::from_secs(30);
/// How long making the MP4 may take (a long service with its sound mixed in).
const FINISH_LIMIT: Duration = Duration::from_secs(30 * 60);
/// How often the small Live and Next pictures are made while live with cameras.
const PEEK_EVERY: Duration = Duration::from_millis(250);

/// `Recording 2026-11-07.mp4` → `Recording 2026-11-07.recording.mkv`: the file written while it records.
pub fn partial_of(out: &Path) -> PathBuf { with_suffix(out, ".recording.mkv") }

/// The notes beside the part: where the MP4 goes and the app's sound heard so far.
fn notes_of(out: &Path) -> PathBuf { with_suffix(out, ".recording.json") }

fn with_suffix(out: &Path, suffix: &str) -> PathBuf {
    let stem = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    out.with_file_name(format!("{stem}{suffix}"))
}

#[derive(Serialize, Deserialize, Default)]
struct Notes {
    out: PathBuf,
    sound: Vec<Segment>,
    /// The chapters so far, and how long the recording was then (where the last chapter ends).
    #[serde(default)]
    chapters: Vec<super::chapters::Chapter>,
    #[serde(default)]
    secs: f64,
    /// How the finished file is made (More settings), kept so a part left by a crash is finished the same way.
    #[serde(default)]
    finish: Finish,
}

/// How a part is finished: MP4 or MKV, and whether its picture is HEVC (marked so Apple's players play it).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct Finish {
    pub format: super::advanced::Format,
    pub hevc: bool,
}

/// The FFmpeg to record with: the one RustyPresenter fetched (`<data>/deps/ffmpeg`), else the computer's own.
pub fn find_ffmpeg(data_dir: &Path) -> Option<PathBuf> {
    if let Some(i) = crate::youtube::fetch::installed_in(&data_dir.join("deps"), crate::youtube::fetch::Tool::Ffmpeg) { return Some(i.exe) }
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    tools::ffmpeg_candidates(&tools::ToolEnv::current(), exe_dir.as_deref()).into_iter().find(|p| p.is_file())
}

/// A quick FFmpeg run that only prints (never a console window on Windows), with its two outputs together.
pub fn ask_ffmpeg(ffmpeg: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(args).stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(crate::platform::proc::creation_flags());
    }
    let out = cmd.output().ok()?;
    Some(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

/// The cameras and capture cards FFmpeg can take pictures from on this computer.
pub fn list_cameras(ffmpeg: &Path) -> Vec<args::Input> { super::devices::cameras(ffmpeg) }

/// The microphones and line-ins FFmpeg can record from on this computer.
pub fn list_inputs(ffmpeg: &Path) -> Vec<args::Input> { super::devices::inputs(ffmpeg) }

/// Everything a recording needs, found before it starts.
#[derive(Clone, Debug)]
pub struct Plan {
    pub ffmpeg: PathBuf,
    pub encoder: &'static str,
    pub preset: Preset,
    pub sound: Sound,
    /// The finished MP4.
    pub out: PathBuf,
    /// Go Live: the addresses the same picture and sound are sent to (`stream::ready`); empty: only the file.
    pub stream: Vec<String>,
    /// Go Live: the sound output whose sound goes out too (`stream::pc_sound`; empty: the default output).
    pub pc_sound: Option<String>,
    /// Go Live: each camera's FFmpeg input arguments (`args::camera_input`), for the camera scenes; the one in the
    /// picture is `Controls::camera`.
    pub cameras: Vec<Vec<String>>,
    /// Go Live: how the scenes are chosen and what the cards say (`stream::pictures`). `None`: the room screen as is.
    pub look: Option<crate::stream::pictures::Look>,
    /// The stream's panic buttons.
    pub controls: Controls,
    /// Go Live: the sound held back this long to line up with the camera.
    pub sound_fx: args::SoundFx,
    /// What is live, for the recording's chapters (`None`: none).
    pub chapters: Option<super::chapters::Source>,
    /// Which of `stream` get the tall picture (phones), in the same order; missing: not tall.
    pub tall: Vec<bool>,
    /// The video bitrate set by hand (kbit/s); 0: the preset's.
    pub kbps: u32,
    /// Go Live: the stream held back this many seconds before it goes out (`stream::delay`); 0: none. The
    /// recording is not held back.
    pub delay_secs: u32,
    /// Go Live: a destination whose internet cannot keep up gets a lighter stream (`stream::send::Ladder`).
    pub lighter: bool,
    /// Go Live: what goes out over NDI too (`stream::ndi::Out`).
    pub ndi: crate::stream::ndi::OutPrefs,
    /// Go Live: the words alone are recorded beside the recording too (`stream::words_file`).
    pub words_file: bool,
    /// Go Live: a second and third microphone or line-in, `(name, dB, filters)` (`RecordPrefs::more_inputs`).
    pub more_inputs: Vec<(String, i32, args::Filters)>,
    /// Go Live: the output the stream's sound is played on to listen to it (`stream::listen`); `None`: nowhere.
    pub listen: Option<String>,
    /// More settings (`advanced`): pictures a second, how the picture and sound are made, the network, and what
    /// is kept of the recording.
    pub more: super::advanced::More,
    /// Go Live: how each of `stream` is sent beyond copying (its own picture), in the same order; missing: copied.
    pub ways: Vec<crate::stream::send::Way>,
}

/// What `Plan::make_with` needs to know beyond Settings.
#[derive(Clone, Debug, Default)]
pub struct Making {
    /// The service's name, for the file name's `{title}`.
    pub title: String,
    /// Go Live: the services the stream goes to (HEVC only when all of them take it: `advanced::codec_for`).
    pub services: Vec<crate::stream::Service>,
    pub live: bool,
}

/// The graphics cards' H.264 encoders, tried in turn when the processor's could not keep up before.
const FASTER: [&str; 4] = ["h264_nvenc", "h264_qsv", "h264_amf", "h264_videotoolbox"];

/// Whether `ffmpeg -encoders`'s `list` has the video encoder `name`.
fn listed(list: &str, name: &str) -> bool { list.lines().any(|l| l.split_whitespace().nth(1) == Some(name) && l.trim_start().starts_with('V')) }

/// The stream's panic buttons, shared with the feeds while the recording runs. The file and the stream are one
/// encode, so the recording holds and goes quiet with the stream.
#[derive(Clone, Debug, Default)]
pub struct Controls {
    /// Hold: every picture stays as it is (the room screen, the camera, the words over it).
    pub hold: Arc<std::sync::atomic::AtomicBool>,
    /// Mute: silence in place of the sound input and the PC's sound.
    pub mute: Arc<std::sync::atomic::AtomicBool>,
    /// Which of `Plan::cameras` is in the picture.
    pub camera: Arc<std::sync::atomic::AtomicUsize>,
    /// Which of `Plan::cameras` is chosen to go out next (its small picture is made: `record::peek`).
    pub next: Arc<std::sync::atomic::AtomicUsize>,
    /// The next change of `camera` fades (set with it by `Recording::switch_camera`).
    pub fade: Arc<std::sync::atomic::AtomicBool>,
    /// Live captions' picture, laid over the stream when there are no song words or card (`stream::captions`).
    pub captions: crate::stream::captions::Slot,
}

impl Plan {
    /// Finds FFmpeg and its encoder and checks the folder. Touches the disk and runs FFmpeg: not on the UI thread.
    /// `Err(None)`: no FFmpeg (the caller offers to get it).
    pub fn make(data_dir: &Path, prefs: &super::RecordPrefs) -> Result<Plan, Option<String>> { Plan::make_with(data_dir, prefs, &Making::default()) }

    /// `make`, for a stream to `making.services` (or a recording alone), named for the service `making.title`.
    pub fn make_with(data_dir: &Path, prefs: &super::RecordPrefs, making: &Making) -> Result<Plan, Option<String>> {
        let ffmpeg = find_ffmpeg(data_dir).ok_or(None)?;
        let encoders = ask_ffmpeg(&ffmpeg, &["-hide_banner", "-encoders"]).ok_or(None)?;
        let works = |e: &str| encoder_works(&ffmpeg, e);
        // The processor could not keep up last time, and Settings leaves the choice to the app: a graphics card's
        // encoder that works here comes first.
        let faster = (prefs.encoder.is_empty() && prefs.x264_too_slow).then(|| FASTER.into_iter().find(|e| listed(&encoders, e) && works(e))).flatten();
        let h264 = faster.or_else(|| args::choose_encoder(&encoders, &prefs.encoder, &works)).ok_or_else(|| Some(tr::no_h264()))?;
        // HEVC or AV1 when chosen, every destination takes it, and an encoder for it works here; else H.264.
        let codec = super::advanced::codec_for(prefs.more.codec, &making.services, making.live);
        let encoder = if codec == super::advanced::Codec::H264 { h264 } else {
            let found = codec.encoders(h264).into_iter().find(|e| listed(&encoders, e) && works(e));
            if found.is_none() { eprintln!("recording: no {} encoder works here; H.264 instead", codec.label()) }
            found.unwrap_or(h264)
        };
        let folder = prefs.folder();
        std::fs::create_dir_all(&folder).map_err(|e| Some(tr::no_folder(&folder.display().to_string(), &e.to_string())))?;
        if let Some(free) = disk::free_space(&folder) && free < prefs.size.bytes_per_hour() / START_SHARE {
            return Err(Some(tr::low_disk(&gb(free), &gb(prefs.size.bytes_per_hour()))));
        }
        let keep = &prefs.more.keep;
        let out = super::named(&folder, &super::advanced::file_stem(&keep.name, chrono::Local::now().naive_local(), &making.title), keep.format.extension());
        Ok(Plan { ffmpeg, encoder, preset: prefs.size, sound: prefs.sound(), out, stream: vec![], pc_sound: None, cameras: vec![], look: None, controls: Controls::default(), sound_fx: args::SoundFx::default(), chapters: None, tall: vec![], kbps: prefs.bitrate_kbps, delay_secs: 0, lighter: false, ndi: Default::default(), words_file: prefs.words_file, more_inputs: prefs.more_inputs(), listen: Some(prefs.stream_listen.clone()).filter(|l| !l.is_empty()),
            more: prefs.more.clone(), ways: vec![] })
    }
}

/// Whether `encoder` runs on this computer: one small picture encoded with it (a graphics card's encoder needs
/// that card and its driver). Takes a moment; only for an encoder chosen by hand.
pub fn encoder_works(ffmpeg: &Path, encoder: &str) -> bool {
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=black:s=256x256:r=30", "-frames:v", "3", "-c:v", encoder, "-f", "null", "-"])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; cmd.creation_flags(0x0800_0000); }
    cmd.status().is_ok_and(|s| s.success())
}

fn gb(bytes: u64) -> String { format!("{:.1} GB", bytes as f64 / 1e9) }

/// How a recording ended: the file it left, and what went wrong, if anything.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Ended {
    pub file: Option<PathBuf>,
    /// The words for the operator about a problem (the file may still be there).
    pub problem: Option<String>,
    /// More settings cut it into pieces: every piece, the first being `file`.
    pub pieces: Vec<PathBuf>,
    /// The MP4 could not be made: `file` is the MKV written while recording, kept as it was.
    pub kept_mkv: bool,
}

impl Ended {
    /// The toast: its words, and whether it is an error.
    pub fn toast(&self) -> (String, bool) {
        let file = self.file.as_ref().map(|f| {
            let path = f.display().to_string();
            if self.pieces.len() > 1 { (tr::saved_pieces(&path, self.pieces.len()), false) }
            else if self.kept_mkv { (tr::saved_mkv(&path), true) } else { (tr::saved(&path), false) }
        });
        match (file, &self.problem) {
            (Some((saved, err)), None) => (saved, err),
            (Some((saved, _)), Some(p)) => (format!("{p} {saved}"), true),
            (None, Some(p)) => (p.clone(), true),
            (None, None) => (tr::failed(&tr::ffmpeg_ended()), true),
        }
    }
}

/// A recording under way. Dropping it stops it and waits (up to a minute) for the file to be finished.
pub struct Recording {
    sink: Arc<FrameSink>,
    cancel: CancellationToken,
    finished: mpsc::Receiver<()>,
    begun: Instant,
    pub out: PathBuf,
    /// Go Live: each destination's connection, up or not (`stream::send`); empty when only recording.
    links: crate::stream::send::Links,
    controls: Controls,
    /// Go Live with a camera: whether its pictures are coming now (`stream::camera_feed`); `None` without one.
    camera_seen: Arc<Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>>,
    /// With a sound input: whether it has been quiet for `board_feed::QUIET_FOR` (`None` without one).
    silent: Arc<Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>>,
    /// Go Live with scenes: what goes out now (`stream::pictures`).
    scenes: Arc<Mutex<Option<crate::stream::pictures::Slot>>>,
    /// How loud the sound going out is (the sound input's and the PC's), for the meter.
    levels: Levels,
    /// The most the writer was behind (ms) since `lag` was last asked (`record::slow`).
    lag: Arc<std::sync::atomic::AtomicU64>,
    /// Go Live with cameras: the small Live and Next pictures, a few times a second.
    peek: Arc<Mutex<super::peek::Peek>>,
    /// The part being written and the FFmpeg writing it ("Save the last minutes" copies from it).
    part: PathBuf,
    ffmpeg: PathBuf,
    format: super::advanced::Format,
    /// Pictures left out since it started because the writer fell behind (the report after a stream).
    dropped: Arc<std::sync::atomic::AtomicU64>,
}

type Levels = Arc<Mutex<Vec<Arc<crate::stream::level::Level>>>>;

impl Recording {
    /// Starts FFmpeg and the writer. `done` runs on the writer thread once the file is finished.
    pub fn start(plan: Plan, done: impl FnOnce(Ended) + Send + 'static) -> Result<Recording, String> {
        let app_sound = plan.sound == Sound::App;
        let sink = Arc::new(FrameSink::new(plan.preset.size(), app_sound));
        let cancel = CancellationToken::new();
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished) = mpsc::channel();
        let (s, c, out, controls) = (sink.clone(), cancel.clone(), plan.out.clone(), plan.controls.clone());
        let links: crate::stream::send::Links = Arc::new(Mutex::new(vec![crate::stream::send::Link::default(); plan.stream.len()]));
        let l = links.clone();
        let camera_seen: Arc<Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>> = Arc::default();
        let cs = camera_seen.clone();
        let silent: Arc<Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>> = Arc::default();
        let si = silent.clone();
        let scenes: Arc<Mutex<Option<crate::stream::pictures::Slot>>> = Arc::default();
        let sc = scenes.clone();
        let levels: Levels = Arc::default();
        let lv = levels.clone();
        let lag: Arc<std::sync::atomic::AtomicU64> = Arc::default();
        let lg = lag.clone();
        let peek: Arc<Mutex<super::peek::Peek>> = Arc::default();
        let pk = peek.clone();
        let dropped: Arc<std::sync::atomic::AtomicU64> = Arc::default();
        let dr = dropped.clone();
        let (part, ffmpeg, format) = (partial_of(&plan.out), plan.ffmpeg.clone(), plan.more.keep.format);
        std::thread::Builder::new().name("recorder".into()).spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => { let _ = started_tx.send(Err(e.to_string())); return }
            };
            let ended = rt.block_on(write(plan, s, c, started_tx, l, cs, si, sc, lv, lg, pk, dr));
            if let Some(ended) = ended { done(ended) }
            let _ = finished_tx.send(());
        }).map_err(|e| tr::not_started(&e.to_string()))?;
        match started_rx.recv() {
            Ok(Ok(())) => Ok(Recording { sink, cancel, finished, begun: Instant::now(), out, links, controls, camera_seen, silent, scenes, levels, lag, peek, part, ffmpeg, format, dropped }),
            Ok(Err(e)) => Err(tr::not_started(&e)),
            Err(_) => Err(tr::not_started(&tr::ffmpeg_ended())),
        }
    }

    /// The most the writer was behind since this was last asked: a computer too slow for the recording shows
    /// here (`record::slow`).
    pub fn lag(&self) -> Duration { Duration::from_millis(self.lag.swap(0, std::sync::atomic::Ordering::Relaxed)) }

    /// Pictures left out since it started because the computer could not make them in time.
    pub fn dropped(&self) -> u64 { self.dropped.load(std::sync::atomic::Ordering::Relaxed) }

    /// "Save the last minutes": the work that copies the last `minutes` of what was recorded so far into a file of
    /// its own beside the recording (`advanced::replay_name`). It blocks: run it off the UI thread. It gives the
    /// file, or the file it could not make. The app's own sound is added to recordings only when they stop, so it
    /// is not in this copy (a sound input and the computer's sound are).
    pub fn save_last(&self, minutes: u32) -> impl FnOnce() -> Result<PathBuf, String> + Send + 'static {
        let (part, ffmpeg, elapsed) = (self.part.clone(), self.ffmpeg.clone(), self.elapsed());
        let file = super::advanced::replay_name(&self.out, minutes, self.format);
        move || {
            let made = tokio::runtime::Builder::new_current_thread().enable_all().build().is_ok_and(|rt| {
                rt.block_on(run_ffmpeg(&ffmpeg, super::advanced::replay_args(&part, elapsed, minutes * 60, &file), FINISH_LIMIT))
            });
            if made && std::fs::metadata(&file).is_ok_and(|m| m.len() > 0) { Ok(file) } else { let _ = std::fs::remove_file(&file); Err(file.display().to_string()) }
        }
    }

    /// Where the room hands its pictures (and sound) in.
    pub fn sink(&self) -> &Arc<FrameSink> { &self.sink }

    /// Go Live: whether each destination is connected now, in the order of `Plan::stream`.
    pub fn links(&self) -> Vec<crate::stream::send::Link> { self.links.lock().unwrap_or_else(|e| e.into_inner()).clone() }

    /// Go Live with a camera: whether its pictures are coming now; `None` without a camera.
    pub fn camera_seen(&self) -> Option<bool> {
        self.camera_seen.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// With a sound input: whether it has gone silent (a muted send, a pulled cable); `None` without one.
    pub fn silent(&self) -> Option<bool> {
        self.silent.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// The loudest sample of the sound going out since the last call (16-bit scale; 0 while muted); `None` with no
    /// sound input and no PC sound.
    pub fn level(&self) -> Option<i32> {
        self.levels.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|l| l.take()).max()
    }

    /// Go Live with scenes: the scene going out now.
    pub fn scene(&self) -> Option<crate::stream::scene::Scene> {
        self.scenes.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.lock().unwrap_or_else(|e| e.into_inner()).scene)
    }

    /// Hold: the stream (and the recording) keeps showing what it shows now, until set off again.
    pub fn hold(&self, on: bool) { self.controls.hold.store(on, std::sync::atomic::Ordering::Relaxed) }

    /// The camera in the picture: `Plan::cameras[i]`, at once.
    pub fn set_camera(&self, i: usize) { self.switch_camera(i, false) }

    /// The camera in the picture: `Plan::cameras[i]`, with the scenes' short fade from the one before when `fade`.
    pub fn switch_camera(&self, i: usize, fade: bool) {
        use std::sync::atomic::Ordering::Relaxed;
        self.controls.fade.store(fade, Relaxed);
        self.controls.camera.store(i, Relaxed);
    }

    /// The camera chosen to go out next: `Plan::cameras[i]` (its small picture is made).
    pub fn set_next(&self, i: usize) { self.controls.next.store(i, std::sync::atomic::Ordering::Relaxed) }

    /// Go Live with cameras: the small Live and Next pictures made last (`record::peek`).
    pub fn peek(&self) -> super::peek::Peek { self.peek.lock().unwrap_or_else(|e| e.into_inner()).clone() }

    /// Mute: silence goes out in place of the sound, until set off again.
    pub fn mute(&self, on: bool) { self.controls.mute.store(on, std::sync::atomic::Ordering::Relaxed) }

    /// Whole seconds recorded.
    pub fn secs(&self) -> u64 { self.begun.elapsed().as_secs() }

    /// Seconds since it started, to the millisecond (clips).
    pub fn elapsed(&self) -> f64 { self.begun.elapsed().as_secs_f64() }

    /// Stops the recording; `done` follows once the file is finished.
    pub fn stop(&self) { self.cancel.cancel() }

    /// Waits up to `limit` for the file to be finished. True when it was.
    pub fn wait(&self, limit: Duration) -> bool {
        matches!(self.finished.recv_timeout(limit), Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected))
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.stop();
        self.wait(Duration::from_secs(60));
    }
}

/// The writer: pictures into FFmpeg at the plan's pictures a second, until stopped, the disk fills or FFmpeg ends. Then the MP4.
/// `None` when FFmpeg never started (the starter heard why).
async fn write(plan: Plan, sink: Arc<FrameSink>, cancel: CancellationToken, started: mpsc::Sender<Result<(), String>>, links: crate::stream::send::Links,
    camera_seen: Arc<Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>>, silent: Arc<Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>>,
    scenes: Arc<Mutex<Option<crate::stream::pictures::Slot>>>, levels: Levels, lag: Arc<std::sync::atomic::AtomicU64>, peek: Arc<Mutex<super::peek::Peek>>,
    dropped: Arc<std::sync::atomic::AtomicU64>) -> Option<Ended> {
    let part = partial_of(&plan.out);
    // Pictures a second (More settings; 30 untouched).
    let fps = plan.more.fps();
    // Go Live: the recorder sends to a local feed for each destination; a sender each passes it on and
    // reconnects (`stream::send`). With no free ports there is no stream, and the recording goes on.
    let ports = if plan.stream.is_empty() { vec![] } else { crate::stream::send::free_ports(plan.stream.len()).unwrap_or_default() };
    if ports.len() != plan.stream.len() { eprintln!("recording: no local ports for the stream; recording only") }
    let feeds: Vec<String> = ports.iter().map(|p| crate::stream::send::feed(*p)).collect();
    // Go Live with scenes: the camera through a feed of its own, which keeps it going through unplugging, and the
    // worker that picks the scene and draws the words and cards. The writer puts the picture together.
    // NDI out alone makes it live too (no destinations: the scenes still go out over NDI).
    let live = !ports.is_empty() || plan.ndi.any() && plan.look.is_some();
    // Each camera keeps its place (camera 2 is Settings' second camera even if the first could not start).
    let mut camera_feeds: Vec<Option<crate::stream::camera_feed::Feed>> = if live && plan.look.is_some() {
        plan.cameras.iter().map(|input| crate::stream::camera_feed::start(plan.ffmpeg.clone(), input.clone(), sink.size, fps)
            .map_err(|e| eprintln!("recording: the stream goes out without a camera: {e}")).ok()).collect()
    } else { vec![] };
    // Whether the camera in the picture is sending pictures (kept by the writer as the operator switches).
    let seeing: Arc<std::sync::atomic::AtomicBool> = Arc::default();
    if !camera_feeds.is_empty() { *camera_seen.lock().unwrap_or_else(|e| e.into_inner()) = Some(seeing.clone()) }
    let mut worker = match (&plan.look, live) {
        (Some(look), true) => {
            crate::stream::pictures::start(look.clone(), sink.size, seeing.clone(), plan.controls.hold.clone())
                .map_err(|e| eprintln!("recording: the stream shows the room screen only: {e}")).ok()
        }
        _ => None,
    };
    if let Some(w) = &worker { *scenes.lock().unwrap_or_else(|e| e.into_inner()) = Some(w.slot.clone()) }
    // The PC's sound, when the stream has it. If it cannot be captured the stream goes out without it.
    let mut pc = match (&plan.pc_sound, !live) {
        (Some(device), false) => crate::stream::pc_sound::start(&plan.ffmpeg, device, plan.controls.mute.clone()).map_err(|e| eprintln!("recording: the stream goes out without the PC's sound: {e}")).ok(),
        _ => None,
    };
    let format = args::input_format(tools::OS);
    let input = match &plan.sound { Sound::Input(name) => Some((format, name.as_str())), _ => None };
    // The sound input through the app: Mute, its level watched, and it survives being unplugged. If that cannot
    // start, FFmpeg reads the input itself.
    let mut board = input.and_then(|(format, name)| crate::stream::board_feed::start(plan.ffmpeg.clone(), format, name, plan.controls.mute.clone())
        .map_err(|e| eprintln!("recording: FFmpeg reads the sound input itself: {e}")).ok());
    // The phone can only come through the app.
    let input = input.filter(|(_, name)| board.is_some() || *name != crate::stream::phone_mic::INPUT);
    if let Some(b) = &board { *silent.lock().unwrap_or_else(|e| e.into_inner()) = Some(b.silent.clone()) }
    // Go Live: the second and third microphone or line-in, each through the app like the first.
    let mut more: Vec<(crate::stream::board_feed::Board, i32, args::Filters)> = if live && input.is_some() {
        plan.more_inputs.iter().filter_map(|(name, db, filters)| crate::stream::board_feed::start(plan.ffmpeg.clone(), format, name, plan.controls.mute.clone())
            .map_err(|e| eprintln!("recording: the stream goes out without {name}: {e}")).ok().map(|b| (b, *db, *filters))).collect()
    } else { vec![] };
    let extras: Vec<args::Extra> = more.iter().map(|(b, db, filters)| args::Extra { feed: b.feed, db: *db, filters: *filters }).collect();
    *levels.lock().unwrap_or_else(|e| e.into_inner()) = board.iter().chain(more.iter().map(|(b, _, _)| b)).map(|b| b.level.clone()).chain(pc.iter().map(|c| c.level.clone())).collect();
    // Listen: the stream's sound in headphones; it listens for its copy before FFmpeg starts.
    let mut listen = match (&plan.listen, live) {
        (Some(output), true) => crate::stream::listen::Listen::start(output, &plan.ffmpeg).map_err(|e| eprintln!("recording: {}", crate::stream::listen::tr::not_started(&e))).ok(),
        _ => None,
    };
    // Stream delay: a relay between each feed and its sender, listening before the recorder starts writing.
    let delay = Duration::from_secs(u64::from(crate::stream::delay::checked(plan.delay_secs)));
    let mut delayed: Vec<(u16, tokio::net::UdpSocket)> = vec![];
    if live && !delay.is_zero() {
        let outs = crate::stream::send::free_ports(ports.len()).unwrap_or_default();
        for (from, to) in ports.iter().zip(&outs) {
            match crate::stream::delay::listen(*from).await {
                Ok(s) => delayed.push((*to, s)),
                Err(e) => { eprintln!("recording: the stream goes out without its delay: {e}"); delayed.clear(); break }
            }
        }
        if outs.len() != ports.len() { eprintln!("recording: no local ports for the stream delay; it goes out at once") }
    }
    // NDI out, with the scenes: it listens for its sound before FFmpeg starts.
    let mut ndi = if live && worker.is_some() && plan.ndi.any() {
        crate::stream::ndi::Out::start(plan.ndi, sink.size, fps).map_err(|e| eprintln!("recording: nothing goes out over NDI: {e}")).ok()
    } else { None };
    let a = args::record(&args::Record { size: sink.size, fps, preset: plan.preset, encoder: plan.encoder, input, board: board.as_ref().map(|b| b.feed), out: &part, stream: &feeds, tall: &plan.tall, kbps: plan.kbps, pc: pc.as_ref().map(|c| c.feed), fx: if live { plan.sound_fx } else { args::SoundFx::default() }, ndi_sound: ndi.as_ref().and_then(|n| n.sound_port),
        more: &extras, listen: listen.as_ref().map(|l| l.port), video: plan.more.video, sound: plan.more.sound });
    let mut cmd = tokio::process::Command::new(&plan.ffmpeg);
    cmd.args(a).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut tree = match spawn_tree(cmd) {
        Ok(t) => t,
        Err(e) => { let _ = started.send(Err(e.to_string())); return None }
    };
    let (Some(mut stdin), Some(err)) = (tree.stdin(), tree.stderr()) else { let _ = started.send(Err(tr::ffmpeg_ended())); return None };
    let _ = started.send(Ok(()));
    let senders = CancellationToken::new();
    let relays = CancellationToken::new();
    let full_kbps = if plan.kbps > 0 { plan.kbps } else { plan.preset.max_kbps() };
    for (i, (port, address)) in ports.iter().zip(&plan.stream).enumerate() {
        let port = delayed.get(i).map_or(*port, |(p, _)| *p);
        // Its own way of sending (a picture of its own), and the network settings every destination shares.
        let mut way = plan.ways.get(i).cloned().unwrap_or_default();
        way.bind = plan.more.network.bind.clone();
        way.srt_latency_ms = plan.more.network.srt_latency_ms;
        // A lighter stream goes down from its own bitrate when it has one.
        let lighter = plan.lighter.then(|| crate::stream::send::Lighter { encoder: plan.encoder, full_kbps: way.own.map_or(full_kbps, |o| o.kbps), fps });
        tokio::spawn(crate::stream::send::keep_sending(plan.ffmpeg.clone(), port, address.clone(), plan.stream.clone(), i, links.clone(), lighter, way, senders.clone()));
    }
    for (socket, to) in delayed.into_iter().map(|(to, s)| (s, to)) {
        tokio::spawn(crate::stream::delay::relay(socket, to, delay, relays.clone()));
    }
    let said: Arc<Mutex<VecDeque<String>>> = Arc::default();
    let said2 = said.clone();
    let keys = plan.stream.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(err).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            // FFmpeg names the address it could not reach, and a stream key is part of it.
            let l = crate::stream::redact(&l, &keys);
            eprintln!("ffmpeg (recording): {l}");
            let mut s = said2.lock().unwrap_or_else(|e| e.into_inner());
            s.push_back(l);
            if s.len() > 20 { s.pop_front(); }
        }
    });
    let folder = plan.out.parent().map(Path::to_path_buf).unwrap_or_default();
    let how = Finish { format: plan.more.keep.format, hevc: super::advanced::Codec::of(plan.encoder) == super::advanced::Codec::Hevc };
    let notes = |sink: &FrameSink, t: f64, chapters: &super::chapters::Log| Notes {
        out: plan.out.clone(), sound: sink.sound.as_ref().map(|s| s.lock().unwrap_or_else(|e| e.into_inner()).segments(t)).unwrap_or_default(),
        chapters: chapters.list.clone(), secs: t, finish: how,
    };
    // What is live, looked at once a second, for the chapters.
    let mut chapters = super::chapters::Log::default();
    let mut next_chapter = Duration::ZERO;
    let t0 = sink.started();
    let mut pump = Pump::new(fps);
    // Black until the room's first picture comes.
    let mut current: Vec<u8> = [0, 0, 0, 255].repeat(sink.bytes() / 4);
    // With scenes: the picture put together, written in place of the room's.
    let mut mix = worker.as_ref().map(|_| Mixer { layout: plan.look.as_ref().map(|l| l.layout).unwrap_or_default(), ..Mixer::new(sink.size) });
    let mut room_new = true;
    // The words alone in a file of their own, beside the recording, when asked for (`stream::words_file`).
    let mut words_file = if live && worker.is_some() && plan.words_file {
        crate::stream::words_file::WordsFile::start(&plan.ffmpeg, &plan.out, sink.size, fps).map_err(|e| eprintln!("recording: no words file: {e}")).ok()
    } else { None };
    // The stream without the words, in a recording of its own, when asked for (`stream::clean_file`): its own
    // picture put together beside the stream's.
    let mut clean = if live && worker.is_some() && plan.more.keep.clean {
        crate::stream::clean_file::CleanFile::start(&plan.ffmpeg, &plan.out, plan.more.keep.format, sink.size, fps, plan.encoder, full_kbps, &plan.more.video)
            .map_err(|e| eprintln!("recording: no clean recording: {e}")).ok()
    } else { None };
    let mut clean_mix = clean.as_ref().and(mix.as_ref()).map(|m| Mixer { layout: m.layout, ..Mixer::new(sink.size) });
    // The camera in the picture at the last picture: a change to another may fade (`Controls::fade`).
    let mut on_camera = plan.controls.camera.load(std::sync::atomic::Ordering::Relaxed).min(camera_feeds.len().saturating_sub(1));
    // The small Live and Next pictures, every `PEEK_EVERY` while there are cameras.
    let mut next_peek = Duration::ZERO;
    let mut next_check = Duration::ZERO;
    let mut problem: Option<String> = None;
    'pump: loop {
        // Hold: the picture taken is handed back unused, and the last one goes on.
        if let Some(new) = sink.take() {
            if plan.controls.hold.load(std::sync::atomic::Ordering::Relaxed) { sink.give_back(new) } else { sink.give_back(std::mem::replace(&mut current, new)); room_new = true }
        }
        // The camera in the picture now; a change asked to fade starts the fade from the picture going out.
        let wanted = plan.controls.camera.load(std::sync::atomic::Ordering::Relaxed).min(camera_feeds.len().saturating_sub(1));
        if wanted != on_camera {
            on_camera = wanted;
            let fade = plan.controls.fade.swap(false, std::sync::atomic::Ordering::Relaxed);
            if fade && let Some(m) = mix.as_mut() && !plan.controls.hold.load(std::sync::atomic::Ordering::Relaxed) {
                m.fade_from_now(Instant::now());
                if let Some(c) = clean_mix.as_mut() { c.fade_from_now(Instant::now()) }
            }
        }
        let camera = camera_feeds.get(on_camera).and_then(Option::as_ref);
        seeing.store(camera.is_some_and(|f| f.seeing.load(std::sync::atomic::Ordering::Relaxed)), std::sync::atomic::Ordering::Relaxed);
        // Freeze keeps the picture going out as it is, mid-fade included.
        if let (Some(m), Some(w)) = (mix.as_mut(), worker.as_ref()) && !plan.controls.hold.load(std::sync::atomic::Ordering::Relaxed) {
            let shown = w.slot.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some(n) = ndi.as_mut() { n.words(&shown.words) }
            if let Some(f) = words_file.as_mut() { f.words(&shown.words) }
            // The church's own scenes can show any camera, not only the one on air.
            if matches!(shown.scene, crate::stream::scene::Scene::Own(_)) { m.cameras = camera_feeds.iter().map(|f| f.as_ref().and_then(|f| f.picture())).collect() }
            m.captions = plan.controls.captions.lock().unwrap_or_else(|e| e.into_inner()).clone().filter(|_| crate::stream::captions::shows_over(&shown));
            m.update(&shown, &current, camera.and_then(|f| f.picture()), room_new, Instant::now());
            // The same without the words.
            if let Some(c) = clean_mix.as_mut() {
                c.cameras = m.cameras.clone();
                let bare = crate::stream::pictures::Shown { words: None, ..shown.clone() };
                c.update(&bare, &current, camera.and_then(|f| f.picture()), room_new, Instant::now());
            }
            room_new = false;
        }
        let out: &[u8] = mix.as_ref().map_or(&current, |m| m.out());
        let due = pump.due(t0.elapsed());
        lag.fetch_max(due.lag(fps).as_millis() as u64, std::sync::atomic::Ordering::Relaxed);
        if due.dropped > 0 {
            dropped.fetch_add(due.dropped as u64, std::sync::atomic::Ordering::Relaxed);
            eprintln!("recording: {} frames dropped (the writer fell behind)", due.dropped)
        }
        if due.write > 0 && let (Some(c), Some(m)) = (clean.as_mut(), clean_mix.as_ref()) { c.picture(m.out()) }
        if due.write > 0 && let Some(n) = &ndi { n.picture(out) }
        // The small pictures for the operator: shrunk here from the picture going out and the next camera's newest.
        if !camera_feeds.is_empty() && t0.elapsed() >= next_peek {
            next_peek = t0.elapsed() + PEEK_EVERY;
            let next = camera_feeds.get(plan.controls.next.load(std::sync::atomic::Ordering::Relaxed)).and_then(Option::as_ref).and_then(|f| f.picture());
            let small = |p: &[u8]| super::peek::shrink(p, sink.size, super::peek::WIDTH).map(Arc::new);
            let made = super::peek::Peek { live: small(out), next: next.as_deref().and_then(|p| small(p)) };
            *peek.lock().unwrap_or_else(|e| e.into_inner()) = made;
        }
        for _ in 0..due.write {
            if stdin.write_all(out).await.is_err() {
                let last = said.lock().unwrap_or_else(|e| e.into_inner()).back().cloned();
                problem = Some(tr::failed(&last.unwrap_or_else(tr::ffmpeg_ended)));
                break 'pump;
            }
        }
        if t0.elapsed() >= next_check {
            next_check += CHECK_EVERY;
            if disk::free_space(&folder).is_some_and(|f| f < DISK_FLOOR) {
                problem = Some(tr::disk_full());
                break;
            }
            write_notes(&plan.out, &notes(&sink, t0.elapsed().as_secs_f64(), &chapters));
        }
        if let Some(source) = &plan.chapters && t0.elapsed() >= next_chapter {
            next_chapter += Duration::from_secs(1);
            chapters.note(t0.elapsed().as_secs_f64(), (source.0)());
        }
        let wait = pump.next_at().saturating_sub(t0.elapsed());
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(wait) => {}
        }
    }
    sink.close();
    // The words file finishes beside the recording (waited for below).
    if let Some(f) = &words_file { f.stop() }
    let _ = stdin.flush().await;
    drop(stdin);
    // The PC's sound has no end of its own: stopping it lets FFmpeg end its file.
    if let Some(c) = pc.as_mut() { c.stop() }
    if let Some(b) = board.as_mut() { b.stop() }
    for (b, _, _) in more.iter_mut() { b.stop() }
    if let Some(w) = worker.as_mut() { w.stop() }
    for c in camera_feeds.iter_mut().flatten() { c.stop() }
    let end = t0.elapsed().as_secs_f64();
    let ended = tree.wait(END_LIMIT, &CancellationToken::new()).await;
    // After FFmpeg: it sends NDI its sound to the end.
    if let Some(n) = ndi.as_mut() { n.stop() }
    if let Some(l) = listen.as_mut() { l.stop() }
    if let Some(f) = words_file.take() { let _ = tokio::task::spawn_blocking(move || f.finish()).await; }
    let clean = match clean.take() {
        Some(mut c) => tokio::task::spawn_blocking(move || { let ok = c.stop(); (ok, c.part.clone(), c.file.clone()) }).await.ok(),
        None => None,
    };
    // The last of the stream has gone to the senders: a moment for them to pass it on, then they stop.
    // With a delay, what is still held goes out first (the stream ends as the service did); the file is finished
    // meanwhile.
    if !ports.is_empty() && !delay.is_zero() {
        tokio::spawn(async move {
            tokio::time::sleep(delay + Duration::from_millis(300)).await;
            relays.cancel();
            tokio::time::sleep(Duration::from_millis(500)).await;
            senders.cancel();
        });
    } else {
        if !ports.is_empty() { tokio::time::sleep(Duration::from_millis(500)).await }
        senders.cancel();
        relays.cancel();
    }
    if let Err(e @ (Stop::Timeout | Stop::Io(_))) = &ended { eprintln!("recording: FFmpeg did not end its file by itself: {e:?}") }
    let notes = notes(&sink, end, &chapters);
    write_notes(&plan.out, &notes);
    let mut done = finish_file(&plan.ffmpeg, &part, &notes.sound, &notes.chapters, notes.secs, &plan.out, how).await;
    // The clean recording gets the finished recording's sound.
    if let Some((written, clean_part, clean_file)) = clean {
        let made = written && done.file.is_some() && !done.kept_mkv && {
            let recording = done.file.clone().unwrap_or_default();
            run_ffmpeg(&plan.ffmpeg, crate::stream::clean_file::finish_args(&clean_part, &recording, &clean_file, plan.more.keep.format), FINISH_LIMIT).await
                && std::fs::metadata(&clean_file).is_ok_and(|m| m.len() > 0)
        };
        if made { let _ = std::fs::remove_file(&clean_part); }
        else if std::fs::metadata(&clean_part).is_ok_and(|m| m.len() > 0) {
            // Kept as written: the picture without sound.
            let _ = std::fs::rename(&clean_part, clean_file.with_extension("mkv"));
        } else { let _ = std::fs::remove_file(&clean_part); }
    }
    // Long recordings cut into pieces (More settings).
    if let Some(file) = done.file.clone().filter(|_| !done.kept_mkv) {
        let bytes = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        let piece = super::advanced::piece_secs(&plan.more.keep, bytes, notes.secs);
        if piece > 0 { done.pieces = split(&plan.ffmpeg, &file, piece, plan.more.keep.format).await }
        if let Some(first) = done.pieces.first() { done.file = Some(first.clone()) }
    }
    if let Some(p) = problem { done.problem = Some(match (&done.file, done.problem) { (Some(_), Some(q)) => format!("{p} {q}"), _ => p }) }
    Some(done)
}

/// The stream's picture put together from the room, the camera and the scene worker's words and cards
/// (`stream::compose`), with a fade at each scene change. Put together again only when something in it changed.
struct Mixer {
    size: (u32, u32),
    /// Where the corner camera goes (Settings).
    layout: crate::stream::compose::Layout,
    out: Vec<u8>,
    next: Vec<u8>,
    /// The picture going out when the scene changed, and when: the fade runs from it.
    fade: Option<(Vec<u8>, Instant)>,
    scene: Option<crate::stream::scene::Scene>,
    camera: Option<Arc<Vec<u8>>>,
    words: Option<Arc<Vec<u8>>>,
    card: Option<Arc<Vec<u8>>>,
    /// The church's own scene: it made ready, every camera's newest picture (set by the writer, the first camera
    /// first), and the pictures of the cameras it shows, as last put together.
    own: Option<Arc<crate::stream::own_scenes::Prepared>>,
    cameras: Vec<Option<Arc<Vec<u8>>>>,
    used: Vec<Option<Arc<Vec<u8>>>>,
    /// How many times the picture was put together (tests).
    composes: u64,
    /// Live captions to lay on top (set by the writer), and the ones in the picture as last put together.
    captions: Option<Arc<Vec<u8>>>,
    captions_used: Option<Arc<Vec<u8>>>,
}

impl Mixer {
    fn new(size: (u32, u32)) -> Mixer {
        let black = [0, 0, 0, 255].repeat(size.0 as usize * size.1 as usize);
        Mixer { size, layout: Default::default(), out: black.clone(), next: black, fade: None, scene: None, camera: None, words: None, card: None, own: None, cameras: vec![], used: vec![], composes: 0, captions: None, captions_used: None }
    }

    fn out(&self) -> &[u8] { &self.out }

    /// A camera switched with a fade: the half-second fade runs from the picture going out now, as at a scene change.
    fn fade_from_now(&mut self, now: Instant) { self.fade = Some((self.out.clone(), now)) }

    fn update(&mut self, shown: &crate::stream::pictures::Shown, room: &[u8], camera: Option<Arc<Vec<u8>>>, room_new: bool, now: Instant) {
        use crate::stream::compose::{FADE, Inputs, blend, compose};
        use crate::stream::scene::Scene;
        let same = |a: &Option<Arc<Vec<u8>>>, b: &Option<Arc<Vec<u8>>>| match (a, b) { (Some(a), Some(b)) => Arc::ptr_eq(a, b), (None, None) => true, _ => false };
        // Only what the scene shows counts: a card needs neither the room nor the camera, only Camera + words
        // and Screen + camera need the camera, and the camera scenes need the room only while there is no camera.
        // The church's own scene needs the cameras it has layers of, and the room when it has a layer of it or one
        // of its cameras sends nothing.
        let own = shown.own.clone().filter(|_| matches!(shown.scene, Scene::Own(_)));
        let on_air = own.as_ref().is_some_and(|p| p.cameras().any(|n| n == 0));
        let camera = camera.filter(|_| shown.scene.needs_camera() || on_air);
        let used: Vec<Option<Arc<Vec<u8>>>> = own.as_ref().map(|p| p.cameras().filter(|n| *n > 0).map(|n| self.cameras.get(usize::from(n) - 1).cloned().flatten()).collect()).unwrap_or_default();
        let needs_room = match &own {
            Some(p) => p.shows_room() || (on_air && camera.is_none()) || used.iter().any(Option::is_none),
            None => !shown.scene.is_card() && (camera.is_none() || shown.scene == Scene::ScreenCamera),
        };
        let room_new = room_new && needs_room;
        let same_own = match (&self.own, &own) { (Some(a), Some(b)) => Arc::ptr_eq(a, b), (None, None) => true, _ => false };
        let same_used = self.used.len() == used.len() && self.used.iter().zip(&used).all(|(a, b)| same(a, b));
        let scene_changed = self.scene.is_some_and(|s| s != shown.scene);
        let changed = self.scene != Some(shown.scene) || room_new || !same(&self.camera, &camera) || !same(&self.words, &shown.words) || !same(&self.card, &shown.card)
            || !same_own || !same_used || !same(&self.captions_used, &self.captions);
        if scene_changed { self.fade = Some((self.out.clone(), now)) }
        if changed {
            let cameras: Vec<Option<&[u8]>> = self.cameras.iter().map(|c| c.as_deref().map(Vec::as_slice)).collect();
            compose(shown.scene, &Inputs { size: self.size, room, camera: camera.as_deref().map(Vec::as_slice), words: shown.words.as_deref().map(Vec::as_slice),
                card: shown.card.as_deref().map(Vec::as_slice), cameras: &cameras, own: own.as_deref() }, &self.layout, &mut self.next);
            if let Some(c) = self.captions.as_deref().filter(|c| c.len() == self.next.len()) { crate::stream::compose::over(&mut self.next, c) }
            self.captions_used = self.captions.clone();
            (self.scene, self.camera, self.words, self.card, self.own, self.used) = (Some(shown.scene), camera, shown.words.clone(), shown.card.clone(), own, used);
            self.composes += 1;
        }
        match &self.fade {
            Some((from, at)) if now.duration_since(*at) < FADE => {
                let t = now.duration_since(*at).as_secs_f32() / FADE.as_secs_f32();
                blend(from, &self.next, t, &mut self.out);
            }
            Some(_) => { self.fade = None; self.out.copy_from_slice(&self.next) }
            None if changed => self.out.copy_from_slice(&self.next),
            None => {}
        }
    }
}

fn write_notes(out: &Path, notes: &Notes) {
    if let Ok(bytes) = serde_json::to_vec(notes) && let Err(e) = crate::store::write_atomic(&notes_of(out), &bytes) {
        eprintln!("recording: could not keep the sound notes: {e}");
    }
}

/// Cuts the finished `file` into pieces of `secs` (`advanced::split_args`). The pieces, in order, when it worked
/// (`file` is then removed); none when it did not (`file` is kept whole, and any piece made goes).
async fn split(ffmpeg: &Path, file: &Path, secs: u32, format: super::advanced::Format) -> Vec<PathBuf> {
    let ok = run_ffmpeg(ffmpeg, super::advanced::split_args(file, secs, format), FINISH_LIMIT).await;
    let pieces: Vec<PathBuf> = (1..).map(|n| super::advanced::piece_name(file, n)).take_while(|p| p.is_file()).collect();
    if ok && pieces.len() > 1 { let _ = std::fs::remove_file(file); return pieces }
    for p in &pieces { let _ = std::fs::remove_file(p); }
    eprintln!("recording: it could not be cut into pieces; kept whole");
    vec![]
}

/// Runs FFmpeg to the end (at most `limit`). True when it succeeded.
async fn run_ffmpeg(ffmpeg: &Path, args: Vec<std::ffi::OsString>, limit: Duration) -> bool {
    let mut cmd = tokio::process::Command::new(ffmpeg);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    let Ok(mut tree) = spawn_tree(cmd) else { return false };
    let err = tree.stderr();
    let log = tokio::spawn(async move {
        let Some(err) = err else { return };
        let mut lines = BufReader::new(err).lines();
        while let Ok(Some(l)) = lines.next_line().await { eprintln!("ffmpeg (finishing): {l}") }
    });
    let ok = matches!(tree.wait(limit, &CancellationToken::new()).await, Ok(s) if s.success());
    let _ = tokio::time::timeout(Duration::from_secs(2), log).await;
    ok
}

/// Makes the MP4 `out` from `part` (with the app's `sound` mixed in), then removes the part and its notes. If the
/// sound cannot be added the MP4 is made without it; if no MP4 can be made the part is kept, renamed `.mkv`.
pub async fn finish_file(ffmpeg: &Path, part: &Path, sound: &[Segment], chapters: &[super::chapters::Chapter], secs: f64, out: &Path, how: Finish) -> Ended {
    if std::fs::metadata(part).map(|m| m.len()).unwrap_or(0) == 0 {
        let _ = std::fs::remove_file(part);
        let _ = std::fs::remove_file(notes_of(out));
        return Ended { file: None, problem: Some(tr::failed(&tr::ffmpeg_ended())), ..Ended::default() };
    }
    let tmp = with_suffix(out, &format!(".{}.part", how.format.extension()));
    let made = |ok: bool| ok && std::fs::metadata(&tmp).is_ok_and(|m| m.len() > 0) && std::fs::rename(&tmp, out).is_ok();
    // Sound files that are gone (moved since) are left out rather than failing the whole mix.
    let sound: Vec<Segment> = sound.iter().filter(|s| s.path.is_file()).cloned().collect();
    let mut problem = None;
    // The chapters, in a file of FFmpeg's for it to read (none when it cannot be written: the video matters more).
    let meta = with_suffix(out, ".chapters.txt");
    let meta = (!chapters.is_empty() && std::fs::write(&meta, super::chapters::metadata(chapters, secs)).is_ok()).then_some(meta);
    let finish = |sound: &[Segment], meta: Option<&Path>| args::finish_as(part, sound, meta, &tmp, how.format, how.hevc);
    let mut ok = made(run_ffmpeg(ffmpeg, finish(&sound, meta.as_deref()), FINISH_LIMIT).await);
    if !ok && !sound.is_empty() {
        ok = made(run_ffmpeg(ffmpeg, finish(&[], meta.as_deref()), FINISH_LIMIT).await);
        if ok { problem = Some(tr::no_app_sound()) }
    }
    if !ok && meta.is_some() { ok = made(run_ffmpeg(ffmpeg, finish(&[], None), FINISH_LIMIT).await) }
    if let Some(m) = &meta { let _ = std::fs::remove_file(m); }
    let _ = std::fs::remove_file(&tmp);
    if ok {
        let _ = std::fs::remove_file(part);
        let _ = std::fs::remove_file(notes_of(out));
        return Ended { file: Some(out.to_path_buf()), problem, ..Ended::default() };
    }
    // Keep what was recorded: Matroska plays in most players.
    let mkv = (1..).map(|i| if i == 1 { out.with_extension("mkv") } else { with_suffix(out, &format!(" ({i}).mkv")) }).find(|p| !p.exists()).expect("a free name");
    let file = if std::fs::rename(part, &mkv).is_ok() { let _ = std::fs::remove_file(notes_of(out)); mkv } else { part.to_path_buf() };
    Ended { file: Some(file), problem: None, kept_mkv: true, ..Ended::default() }
}

/// Parts left in `folder` by a recording that never finished (the app or the computer stopped).
pub fn leftovers(folder: &Path) -> Vec<PathBuf> {
    let Ok(dir) = std::fs::read_dir(folder) else { return vec![] };
    let mut found: Vec<PathBuf> = dir.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".recording.mkv")).collect();
    found.sort();
    found
}

/// Whether part `p` is `current` itself or the clean recording beside it (`stream::clean_file`), still being written.
fn belongs_to(p: &Path, current: &Path) -> bool {
    let stem = |x: &Path| x.file_name().map(|n| n.to_string_lossy().trim_end_matches(".recording.mkv").to_string()).unwrap_or_default();
    let (n, c) = (stem(p), stem(current));
    p == current || (!c.is_empty() && n.strip_prefix(&c).is_some_and(|rest| rest.starts_with(" clean")))
}

/// Finishes every part in `folder` except `current`'s (and words files left unfinished: `words_file::leftovers`).
/// Blocks: run it on a thread of its own.
pub fn finish_leftovers(ffmpeg: &Path, folder: &Path, current: &Path) -> Vec<Ended> {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return vec![] };
    crate::stream::words_file::finish_leftovers(ffmpeg, folder);
    leftovers(folder).into_iter().filter(|p| !belongs_to(p, current)).map(|part| {
        let name = part.file_name().map(|n| n.to_string_lossy().trim_end_matches(".recording.mkv").to_string()).unwrap_or_default();
        let out = part.with_file_name(format!("{name}.mp4"));
        let notes: Notes = std::fs::read(notes_of(&out)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let out = if notes.out.as_os_str().is_empty() || notes.out.exists() { out } else { notes.out };
        rt.block_on(finish_file(ffmpeg, &part, &notes.sound, &notes.chapters, notes.secs, &out, notes.finish))
    }).collect()
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
