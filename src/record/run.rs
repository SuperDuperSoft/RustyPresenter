//! One recording: the `ffmpeg` child (in its own process tree, which dies with the app), the writer thread that
//! feeds it, the disk checks, and turning the Matroska part into the MP4 at the end (`finish_file`). A part left by
//! a crash is finished the next time a recording starts in that folder (`finish_leftovers`).
use super::{FPS, Preset, Sound, args, pump::{FrameSink, Pump}, sound::Segment, tr};
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

/// The microphones and line-ins FFmpeg can record from on this computer.
pub fn list_inputs(ffmpeg: &Path) -> Vec<args::Input> {
    let os = tools::OS;
    ask_ffmpeg(ffmpeg, &args::list_inputs_args(os)).map(|out| args::parse_inputs(os, &out)).unwrap_or_default()
}

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
}

impl Plan {
    /// Finds FFmpeg and its encoder and checks the folder. Touches the disk and runs FFmpeg: not on the UI thread.
    /// `Err(None)`: no FFmpeg (the caller offers to get it).
    pub fn make(data_dir: &Path, prefs: &super::RecordPrefs) -> Result<Plan, Option<String>> {
        let ffmpeg = find_ffmpeg(data_dir).ok_or(None)?;
        let encoders = ask_ffmpeg(&ffmpeg, &["-hide_banner", "-encoders"]).ok_or(None)?;
        let encoder = args::pick_encoder(&encoders).ok_or_else(|| Some(tr::no_h264()))?;
        let folder = prefs.folder();
        std::fs::create_dir_all(&folder).map_err(|e| Some(tr::no_folder(&folder.display().to_string(), &e.to_string())))?;
        if let Some(free) = disk::free_space(&folder) && free < prefs.size.bytes_per_hour() / START_SHARE {
            return Err(Some(tr::low_disk(&gb(free), &gb(prefs.size.bytes_per_hour()))));
        }
        let out = super::file_name(&folder, chrono::Local::now().naive_local());
        Ok(Plan { ffmpeg, encoder, preset: prefs.size, sound: prefs.sound(), out, stream: vec![], pc_sound: None })
    }
}

fn gb(bytes: u64) -> String { format!("{:.1} GB", bytes as f64 / 1e9) }

/// How a recording ended: the file it left, and what went wrong, if anything.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Ended {
    pub file: Option<PathBuf>,
    /// The words for the operator about a problem (the file may still be there).
    pub problem: Option<String>,
}

impl Ended {
    /// The toast: its words, and whether it is an error.
    pub fn toast(&self) -> (String, bool) {
        let file = self.file.as_ref().map(|f| {
            let path = f.display().to_string();
            if f.extension().is_some_and(|e| e == "mp4") { (tr::saved(&path), false) } else { (tr::saved_mkv(&path), true) }
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
}

impl Recording {
    /// Starts FFmpeg and the writer. `done` runs on the writer thread once the file is finished.
    pub fn start(plan: Plan, done: impl FnOnce(Ended) + Send + 'static) -> Result<Recording, String> {
        let app_sound = plan.sound == Sound::App;
        let sink = Arc::new(FrameSink::new(plan.preset.size(), app_sound));
        let cancel = CancellationToken::new();
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished) = mpsc::channel();
        let (s, c, out) = (sink.clone(), cancel.clone(), plan.out.clone());
        let links: crate::stream::send::Links = Arc::new(Mutex::new(vec![false; plan.stream.len()]));
        let l = links.clone();
        std::thread::Builder::new().name("recorder".into()).spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => { let _ = started_tx.send(Err(e.to_string())); return }
            };
            let ended = rt.block_on(write(plan, s, c, started_tx, l));
            if let Some(ended) = ended { done(ended) }
            let _ = finished_tx.send(());
        }).map_err(|e| tr::not_started(&e.to_string()))?;
        match started_rx.recv() {
            Ok(Ok(())) => Ok(Recording { sink, cancel, finished, begun: Instant::now(), out, links }),
            Ok(Err(e)) => Err(tr::not_started(&e)),
            Err(_) => Err(tr::not_started(&tr::ffmpeg_ended())),
        }
    }

    /// Where the room hands its pictures (and sound) in.
    pub fn sink(&self) -> &Arc<FrameSink> { &self.sink }

    /// Go Live: whether each destination is connected now, in the order of `Plan::stream`.
    pub fn links(&self) -> Vec<bool> { self.links.lock().unwrap_or_else(|e| e.into_inner()).clone() }

    /// Whole seconds recorded.
    pub fn secs(&self) -> u64 { self.begun.elapsed().as_secs() }

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

/// The writer: pictures into FFmpeg at `FPS`, until stopped, the disk fills or FFmpeg ends. Then the MP4.
/// `None` when FFmpeg never started (the starter heard why).
async fn write(plan: Plan, sink: Arc<FrameSink>, cancel: CancellationToken, started: mpsc::Sender<Result<(), String>>, links: crate::stream::send::Links) -> Option<Ended> {
    let part = partial_of(&plan.out);
    // Go Live: the recorder sends to a local feed for each destination; a sender each passes it on and
    // reconnects (`stream::send`). With no free ports there is no stream, and the recording goes on.
    let ports = if plan.stream.is_empty() { vec![] } else { crate::stream::send::free_ports(plan.stream.len()).unwrap_or_default() };
    if ports.len() != plan.stream.len() { eprintln!("recording: no local ports for the stream; recording only") }
    let feeds: Vec<String> = ports.iter().map(|p| crate::stream::send::feed(*p)).collect();
    // The PC's sound, when the stream has it. If it cannot be captured the stream goes out without it.
    let mut pc = match (&plan.pc_sound, ports.is_empty()) {
        (Some(device), false) => crate::stream::pc_sound::start(device).map_err(|e| eprintln!("recording: the stream goes out without the PC's sound: {e}")).ok(),
        _ => None,
    };
    let format = args::input_format(tools::OS);
    let input = match &plan.sound { Sound::Input(name) => Some((format, name.as_str())), _ => None };
    let a = args::record(&args::Record { size: sink.size, fps: FPS, preset: plan.preset, encoder: plan.encoder, input, out: &part, stream: &feeds, pc: pc.as_ref().map(|c| c.feed) });
    let mut cmd = tokio::process::Command::new(&plan.ffmpeg);
    cmd.args(a).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut tree = match spawn_tree(cmd) {
        Ok(t) => t,
        Err(e) => { let _ = started.send(Err(e.to_string())); return None }
    };
    let (Some(mut stdin), Some(err)) = (tree.stdin(), tree.stderr()) else { let _ = started.send(Err(tr::ffmpeg_ended())); return None };
    let _ = started.send(Ok(()));
    let senders = CancellationToken::new();
    for (i, (port, address)) in ports.iter().zip(&plan.stream).enumerate() {
        tokio::spawn(crate::stream::send::keep_sending(plan.ffmpeg.clone(), *port, address.clone(), plan.stream.clone(), i, links.clone(), senders.clone()));
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
    let notes = |sink: &FrameSink, t: f64| Notes { out: plan.out.clone(), sound: sink.sound.as_ref().map(|s| s.lock().unwrap_or_else(|e| e.into_inner()).segments(t)).unwrap_or_default() };
    let t0 = sink.started();
    let mut pump = Pump::new(FPS);
    // Black until the room's first picture comes.
    let mut current: Vec<u8> = [0, 0, 0, 255].repeat(sink.bytes() / 4);
    let mut next_check = Duration::ZERO;
    let mut problem: Option<String> = None;
    'pump: loop {
        if let Some(new) = sink.take() { sink.give_back(std::mem::replace(&mut current, new)) }
        let due = pump.due(t0.elapsed());
        if due.dropped > 0 { eprintln!("recording: {} frames dropped (the writer fell behind)", due.dropped) }
        for _ in 0..due.write {
            if stdin.write_all(&current).await.is_err() {
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
            write_notes(&plan.out, &notes(&sink, t0.elapsed().as_secs_f64()));
        }
        let wait = pump.next_at().saturating_sub(t0.elapsed());
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(wait) => {}
        }
    }
    sink.close();
    let _ = stdin.flush().await;
    drop(stdin);
    // The PC's sound has no end of its own: stopping it lets FFmpeg end its file.
    if let Some(c) = pc.as_mut() { c.stop() }
    let end = t0.elapsed().as_secs_f64();
    let ended = tree.wait(END_LIMIT, &CancellationToken::new()).await;
    // The last of the stream has gone to the senders: a moment for them to pass it on, then they stop.
    if !ports.is_empty() { tokio::time::sleep(Duration::from_millis(500)).await }
    senders.cancel();
    if let Err(e @ (Stop::Timeout | Stop::Io(_))) = &ended { eprintln!("recording: FFmpeg did not end its file by itself: {e:?}") }
    let notes = notes(&sink, end);
    write_notes(&plan.out, &notes);
    let mut done = finish_file(&plan.ffmpeg, &part, &notes.sound, &plan.out).await;
    if let Some(p) = problem { done.problem = Some(match (&done.file, done.problem) { (Some(_), Some(q)) => format!("{p} {q}"), _ => p }) }
    Some(done)
}

fn write_notes(out: &Path, notes: &Notes) {
    if let Ok(bytes) = serde_json::to_vec(notes) && let Err(e) = crate::store::write_atomic(&notes_of(out), &bytes) {
        eprintln!("recording: could not keep the sound notes: {e}");
    }
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
pub async fn finish_file(ffmpeg: &Path, part: &Path, sound: &[Segment], out: &Path) -> Ended {
    if std::fs::metadata(part).map(|m| m.len()).unwrap_or(0) == 0 {
        let _ = std::fs::remove_file(part);
        let _ = std::fs::remove_file(notes_of(out));
        return Ended { file: None, problem: Some(tr::failed(&tr::ffmpeg_ended())) };
    }
    let tmp = with_suffix(out, ".mp4.part");
    let made = |ok: bool| ok && std::fs::metadata(&tmp).is_ok_and(|m| m.len() > 0) && std::fs::rename(&tmp, out).is_ok();
    // Sound files that are gone (moved since) are left out rather than failing the whole mix.
    let sound: Vec<Segment> = sound.iter().filter(|s| s.path.is_file()).cloned().collect();
    let mut problem = None;
    let mut ok = made(run_ffmpeg(ffmpeg, args::finish(part, &sound, &tmp), FINISH_LIMIT).await);
    if !ok && !sound.is_empty() {
        ok = made(run_ffmpeg(ffmpeg, args::finish(part, &[], &tmp), FINISH_LIMIT).await);
        if ok { problem = Some(tr::no_app_sound()) }
    }
    let _ = std::fs::remove_file(&tmp);
    if ok {
        let _ = std::fs::remove_file(part);
        let _ = std::fs::remove_file(notes_of(out));
        return Ended { file: Some(out.to_path_buf()), problem };
    }
    // Keep what was recorded: Matroska plays in most players.
    let mkv = (1..).map(|i| if i == 1 { out.with_extension("mkv") } else { with_suffix(out, &format!(" ({i}).mkv")) }).find(|p| !p.exists()).expect("a free name");
    let file = if std::fs::rename(part, &mkv).is_ok() { let _ = std::fs::remove_file(notes_of(out)); mkv } else { part.to_path_buf() };
    Ended { file: Some(file), problem: None }
}

/// Parts left in `folder` by a recording that never finished (the app or the computer stopped).
pub fn leftovers(folder: &Path) -> Vec<PathBuf> {
    let Ok(dir) = std::fs::read_dir(folder) else { return vec![] };
    let mut found: Vec<PathBuf> = dir.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".recording.mkv")).collect();
    found.sort();
    found
}

/// Finishes every part in `folder` except `current`'s. Blocks: run it on a thread of its own.
pub fn finish_leftovers(ffmpeg: &Path, folder: &Path, current: &Path) -> Vec<Ended> {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return vec![] };
    leftovers(folder).into_iter().filter(|p| p != current).map(|part| {
        let name = part.file_name().map(|n| n.to_string_lossy().trim_end_matches(".recording.mkv").to_string()).unwrap_or_default();
        let out = part.with_file_name(format!("{name}.mp4"));
        let notes: Notes = std::fs::read(notes_of(&out)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let out = if notes.out.as_os_str().is_empty() || notes.out.exists() { out } else { notes.out };
        rt.block_on(finish_file(ffmpeg, &part, &notes.sound, &out))
    }).collect()
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
