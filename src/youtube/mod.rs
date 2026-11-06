//! Downloading a YouTube video into the media library with yt-dlp, so it plays offline like any other video.
//! `fetch` gets the helper programs on first use. A lookup (`Hub::look_up`) runs `yt-dlp -J` for the title, length
//! and picture; a download (`start_download`) is a cancellable core job that runs yt-dlp in its own process tree
//! (killed on Cancel and when the app quits), reads its progress lines, and hands the finished file to
//! `App::import_staged` like an uploaded file. When yt-dlp cannot read a video, it is updated and tried once more.
//! Quality is picked for low-end PCs: H.264 video with AAC sound in MP4, at most 1080p and 30 frames per second.
pub mod fetch;
#[path = "../strings/youtube.rs"]
pub mod strings;
#[cfg(test)]
pub(crate) mod testing;

use crate::{
    app::{App, ImportPurpose, Imported},
    import::media,
    model::ItemKind,
    platform::proc::{Stop, spawn_tree},
    sha256::{Digest, Sha256, hex},
};
use fetch::{FetchError, Fetcher, Tool};
use serde_json::Value;
use std::{
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, atomic::{AtomicBool, Ordering}},
    time::{Duration, Instant},
};
use strings as tr;
use tokio::{io::{AsyncBufReadExt, AsyncReadExt, BufReader}, process::Command};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// A lookup that takes longer than this has stalled.
const LOOK_UP_LIMIT: Duration = Duration::from_secs(180);
/// A download that takes longer than this has stalled.
const DOWNLOAD_LIMIT: Duration = Duration::from_secs(6 * 3600);
/// The share of the bar the download itself fills; joining and adding to the library take the rest.
const DOWNLOAD_SHARE: f64 = 96.0;

// ---------- links ----------

/// A YouTube address, cleaned: one video (`watch?v=`), or a whole playlist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub url: String,
    pub playlist: bool,
}

/// The YouTube link in `text`, if it is one. A video link keeps only its video (`&list=`, `&t=` and the like
/// are dropped: "this video only"); a link to a playlist alone stays a playlist.
pub fn link(text: &str) -> Option<Link> {
    let text = text.trim();
    let with_scheme = if text.contains("://") { text.to_string() } else { format!("https://{text}") };
    let url = url::Url::parse(&with_scheme).ok()?;
    if !matches!(url.scheme(), "http" | "https") { return None }
    let host = url.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").or_else(|| host.strip_prefix("m.")).or_else(|| host.strip_prefix("music.")).unwrap_or(&host);
    let param = |name: &str| url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned());
    let mut path = url.path_segments()?.filter(|s| !s.is_empty());
    let id = match host {
        "youtu.be" => path.next().map(str::to_string),
        "youtube.com" | "youtube-nocookie.com" => match path.next() {
            Some("watch") => param("v"),
            Some("shorts" | "live" | "embed" | "v") => path.next().map(str::to_string),
            Some("playlist") => {
                let list = param("list").filter(|l| l.len() >= 2 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))?;
                return Some(Link { url: format!("https://www.youtube.com/playlist?list={list}"), playlist: true });
            }
            _ => None,
        },
        _ => None,
    }?;
    let ok = id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    ok.then(|| Link { url: format!("https://www.youtube.com/watch?v={id}"), playlist: false })
}

// ---------- quality ----------

/// The optional Quality choice. `Best` is right for nearly everyone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quality {
    #[default]
    Best,
    Smaller,
    Smallest,
    Sound,
}

impl Quality {
    /// The Quality drop-down's index (`strings::QUALITIES`).
    pub fn from_index(i: i32) -> Quality {
        match i { 1 => Quality::Smaller, 2 => Quality::Smallest, 3 => Quality::Sound, _ => Quality::Best }
    }

    fn height(self) -> u32 {
        match self { Quality::Smaller => 720, Quality::Smallest => 480, _ => 1080 }
    }

    /// Whether it needs FFmpeg to join separate picture and sound.
    pub fn joins(self) -> bool {
        self != Quality::Sound
    }
}

/// yt-dlp's format choice. With FFmpeg: the best H.264 picture up to the height, joined with AAC sound into MP4,
/// preferring 30 fps (`-S` ranks codec first, so a 1080p that exists only in VP9 or AV1 loses to H.264). Without
/// it: the best file that already has both, which YouTube now offers only at 360p.
pub fn format_args(q: Quality, can_join: bool) -> Vec<String> {
    let h = q.height();
    let v: Vec<String> = match (q, can_join) {
        (Quality::Sound, _) => vec!["-f".into(), "ba[ext=m4a]/ba[acodec^=mp4a]/ba".into()],
        (_, true) => vec![
            "-f".into(), format!("bv*[height<={h}]+ba/b[height<={h}]/bv*+ba/b"),
            "-S".into(), format!("vcodec:h264,res:{h},fps:30,acodec:aac"),
            "--merge-output-format".into(), "mp4".into(), "--remux-video".into(), "mp4".into(),
        ],
        (_, false) => vec!["-f".into(), format!("b[ext=mp4][height<={h}]/b[ext=mp4]/b"), "-S".into(), format!("vcodec:h264,res:{h},acodec:aac")],
    };
    v
}

// ---------- errors ----------

/// Why a lookup or a download did not work. `message` says it in plain words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum YtError {
    Offline,
    Private,
    SignIn,
    Members,
    BotCheck,
    Live,
    Upcoming,
    Processing,
    NotAVideo,
    Unavailable,
    EmptyPlaylist,
    DiskFull,
    /// yt-dlp could not read YouTube's page: yt-dlp is out of date. Updated and tried once more.
    Broken,
    Setup(FetchError),
    Cancelled,
    Other(String),
}

impl YtError {
    pub fn message(&self) -> String {
        match self {
            YtError::Offline => tr::OFFLINE.into(),
            YtError::Private => tr::PRIVATE.into(),
            YtError::SignIn => tr::SIGN_IN.into(),
            YtError::Members => tr::MEMBERS.into(),
            YtError::BotCheck => tr::BOT_CHECK.into(),
            YtError::Live => tr::LIVE.into(),
            YtError::Upcoming => tr::UPCOMING.into(),
            YtError::Processing => tr::PROCESSING.into(),
            YtError::NotAVideo => tr::NOT_A_VIDEO.into(),
            YtError::Unavailable => tr::UNAVAILABLE.into(),
            YtError::EmptyPlaylist => tr::EMPTY_PLAYLIST.into(),
            YtError::DiskFull => tr::DISK_FULL.into(),
            YtError::Broken => tr::BROKEN.into(),
            YtError::Setup(FetchError::Offline | FetchError::Http(_)) => tr::SETUP_OFFLINE.into(),
            YtError::Setup(FetchError::Checksum) => tr::SETUP_CHECKSUM.into(),
            YtError::Setup(FetchError::NoBuild) => tr::SETUP_NO_BUILD.into(),
            YtError::Setup(FetchError::DiskFull) => tr::DISK_FULL.into(),
            YtError::Setup(FetchError::Cancelled) | YtError::Cancelled => tr::CANCELLED.into(),
            YtError::Setup(FetchError::Other(e)) => tr::setup_failed(e),
            YtError::Other(e) => tr::other(e),
        }
    }

    fn from_fetch(e: FetchError) -> YtError {
        if e == FetchError::Cancelled { YtError::Cancelled } else { YtError::Setup(e) }
    }
}

/// What yt-dlp's error output means. The checks run most specific first: a private video's page also fails to
/// "extract", and a full disk can show up as a failed download.
pub fn classify(stderr: &str) -> YtError {
    let low = stderr.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| low.contains(w));
    if has(&["no space left on device", "not enough space on the disk", "[errno 28]", "winerror 112", "disk quota exceeded"]) { return YtError::DiskFull }
    if has(&["private video", "video is private"]) { return YtError::Private }
    if has(&["confirm your age", "age-restricted", "age restricted", "inappropriate for some users"]) { return YtError::SignIn }
    if has(&["not a bot", "http error 429", "too many requests"]) { return YtError::BotCheck }
    if has(&["members-only", "join this channel", "available to this channel's members"]) { return YtError::Members }
    if has(&["live event will begin", "premieres in", "will begin in a few moments", "this live event has not started"]) { return YtError::Upcoming }
    if has(&["live stream recording is not available", "is still being processed"]) { return YtError::Processing }
    if has(&["is currently live", "live stream is not finished"]) { return YtError::Live }
    if has(&["unsupported url", "is not a valid url", "incomplete youtube id", "not a valid youtube"]) { return YtError::NotAVideo }
    if has(&["video unavailable", "has been removed", "not available in your country", "blocked it in your country", "account associated with this video has been terminated", "this video is no longer available"]) { return YtError::Unavailable }
    if has(&["getaddrinfo failed", "name or service not known", "temporary failure in name resolution", "nodename nor servname", "network is unreachable", "no route to host", "errno 11001", "failed to resolve", "connection refused", "connection reset", "timed out", "unable to connect", "no address associated"]) { return YtError::Offline }
    if has(&["unable to extract", "requested format is not available", "signature extraction failed", "nsig extraction failed", "n challenge", "challenge solving failed", "failed to extract any player response", "unable to decode n-parameter", "only images are available", "please report this issue"]) { return YtError::Broken }
    let line = stderr.lines().rev().find(|l| l.starts_with("ERROR:")).or_else(|| stderr.lines().rev().find(|l| !l.trim().is_empty())).unwrap_or("yt-dlp stopped");
    // `ERROR: [youtube] <id>: <message>` → `<message>`.
    let msg = line.trim_start_matches("ERROR:").trim();
    let msg = if msg.starts_with('[') { msg.split_once("]: ").map(|(_, m)| m).or_else(|| msg.split_once(": ").map(|(_, m)| m)).unwrap_or(msg) } else { msg };
    let msg = msg.split_once(": ").filter(|(id, _)| id.len() == 11 && !id.contains(' ')).map_or(msg, |(_, m)| m);
    YtError::Other(msg.trim().to_string())
}

// ---------- lookups ----------

/// A video's details from `yt-dlp -J`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Video {
    pub url: String,
    pub id: String,
    pub title: String,
    pub channel: String,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    /// The playlist the link named, and how many videos it has, when only its first video is used.
    pub playlist: Option<(String, usize)>,
    /// Each file yt-dlp will download (format id, bytes; 0 when unknown), for one smooth bar.
    pub parts: Vec<(String, u64)>,
}

/// `yt-dlp -J`'s answer: a video, or a playlist and its first video's address.
#[derive(Debug, PartialEq)]
enum Info {
    Video(Video),
    Playlist { title: String, count: usize, first: Option<String> },
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn size(f: &Value) -> u64 {
    f.get("filesize").and_then(Value::as_u64).or_else(|| f.get("filesize_approx").and_then(Value::as_f64).map(|s| s as u64)).unwrap_or(0)
}

/// Reads `-J`, refusing what cannot be downloaded (a live stream still running, a private video).
fn parse_info(json: &str) -> Result<Info, YtError> {
    let v: Value = serde_json::from_str(json).map_err(|e| YtError::Other(format!("unreadable details ({e})")))?;
    if v.get("_type").and_then(Value::as_str) == Some("playlist") {
        let entries = v.get("entries").and_then(Value::as_array).cloned().unwrap_or_default();
        let first = entries.iter().find_map(|e| {
            let url = e.get("url").and_then(Value::as_str).and_then(link).map(|l| l.url);
            url.or_else(|| e.get("id").and_then(Value::as_str).map(|id| format!("https://www.youtube.com/watch?v={id}")))
        });
        return Ok(Info::Playlist { title: text(&v, "title"), count: entries.len(), first });
    }
    match v.get("live_status").and_then(Value::as_str) {
        Some("is_live") => return Err(YtError::Live),
        Some("is_upcoming") => return Err(YtError::Upcoming),
        Some("post_live") => return Err(YtError::Processing),
        _ => {}
    }
    match v.get("availability").and_then(Value::as_str) {
        Some("private") => return Err(YtError::Private),
        Some("needs_auth") => return Err(YtError::SignIn),
        Some("subscriber_only" | "premium_only") => return Err(YtError::Members),
        _ => {}
    }
    let parts = match v.get("requested_formats").and_then(Value::as_array) {
        Some(fs) => fs.iter().map(|f| (text(f, "format_id"), size(f))).collect(),
        None => vec![(text(&v, "format_id"), size(&v))],
    };
    let channel = [text(&v, "channel"), text(&v, "uploader")].into_iter().find(|c| !c.is_empty()).unwrap_or_default();
    let url = v.get("webpage_url").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("https://www.youtube.com/watch?v={}", text(&v, "id")));
    Ok(Info::Video(Video {
        url, id: text(&v, "id"), title: text(&v, "title"), channel,
        duration: v.get("duration").and_then(Value::as_f64),
        thumbnail: v.get("thumbnail").and_then(Value::as_str).map(str::to_string),
        playlist: None, parts,
    }))
}

/// A lookup's answer: the video and its picture (320 px wide), when the picture could be fetched.
pub struct Found {
    pub video: Video,
    pub picture: Option<image::RgbaImage>,
}

// ---------- progress ----------

/// One progress line: `RPJ <format id> <yt-dlp's progress JSON>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    pub format: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub finished: bool,
}

pub fn parse_tick(line: &str) -> Option<Tick> {
    let rest = line.strip_prefix("RPJ ")?;
    let (format, json) = rest.split_once(' ')?;
    let v: Value = serde_json::from_str(json).ok()?;
    let num = |k: &str| v.get(k).and_then(|n| n.as_u64().or_else(|| n.as_f64().map(|f| f as u64)));
    let total = num("total_bytes").or_else(|| num("total_bytes_estimate")).filter(|t| *t > 0);
    let finished = v.get("status").and_then(Value::as_str) == Some("finished");
    let downloaded = num("downloaded_bytes").or(if finished { total } else { None }).unwrap_or(0);
    Some(Tick { format: format.to_string(), downloaded, total, finished })
}

/// Turns the separate files' progress into one percent that never goes back. Each file counts by its expected
/// size when every size is known, else equally.
#[derive(Debug, Default)]
pub struct Meter {
    parts: Vec<(String, u64)>,
    done: Vec<(String, f64)>,
    best: u8,
}

impl Meter {
    pub fn new(parts: &[(String, u64)]) -> Meter {
        Meter { parts: parts.to_vec(), done: Vec::new(), best: 0 }
    }

    pub fn tick(&mut self, t: &Tick) -> u8 {
        let frac = if t.finished { 1.0 } else { t.total.map_or(0.0, |total| (t.downloaded as f64 / total as f64).clamp(0.0, 1.0)) };
        match self.done.iter_mut().find(|(f, _)| *f == t.format) {
            Some(d) => d.1 = frac,
            None => self.done.push((t.format.clone(), frac)),
        }
        let known = !self.parts.is_empty() && self.parts.iter().all(|(_, s)| *s > 0) && self.done.iter().all(|(f, _)| self.parts.iter().any(|(p, _)| p == f));
        let share = if known {
            let total: u64 = self.parts.iter().map(|(_, s)| s).sum();
            self.parts.iter().map(|(f, s)| *s as f64 * self.frac(f)).sum::<f64>() / total as f64
        } else {
            let n = self.parts.len().max(self.done.len()).max(1);
            self.done.iter().map(|(_, d)| d).sum::<f64>() / n as f64
        };
        self.best = self.best.max((share * DOWNLOAD_SHARE).floor().clamp(0.0, DOWNLOAD_SHARE) as u8);
        self.best
    }

    fn frac(&self, format: &str) -> f64 {
        self.done.iter().find(|(f, _)| f == format).map_or(0.0, |d| d.1)
    }
}

// ---------- the hub ----------

/// The programs one run uses. `ffmpeg` is the folder holding ffmpeg (and ffprobe).
#[derive(Clone, Debug, PartialEq)]
pub struct Toolset {
    pub yt_dlp: PathBuf,
    pub deno: Option<PathBuf>,
    pub ffmpeg: Option<PathBuf>,
}

/// What getting the helpers ready reports: which one, whether it is a first install, and how far along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setup {
    pub tool: Tool,
    pub first: bool,
    pub percent: u8,
}

impl Setup {
    /// The words for the panel or the job card.
    pub fn text(&self) -> String {
        match (self.tool, self.first) {
            (Tool::Ffmpeg, _) => tr::joiner_ready(self.percent),
            (_, true) => tr::getting_ready(self.percent),
            (_, false) => tr::updating(self.percent),
        }
    }
}

/// YouTube downloads for one data folder: the helpers in `<data>/deps`, temporary files in `<data>/tmp`.
pub struct Hub {
    pub fetch: Fetcher,
    tmp: PathBuf,
    /// Use an FFmpeg already on this computer before fetching one.
    system_ffmpeg: AtomicBool,
    #[cfg(test)]
    pub(crate) fake: std::sync::Mutex<Option<testing_fake::Fake>>,
}

impl Hub {
    pub fn new(root: &Path) -> Hub {
        let hub = Hub {
            fetch: Fetcher::new(root.join("deps")),
            tmp: root.join("tmp"),
            system_ffmpeg: AtomicBool::new(!cfg!(test)),
            #[cfg(test)]
            fake: std::sync::Mutex::new(None),
        };
        // Tests never reach the internet unless they ask for it (`RP_NET_TESTS`).
        #[cfg(test)]
        hub.fetch.set_sources(fetch::Sources { yt_dlp: "http://127.0.0.1:9/yt-dlp".into(), yt_dlp_nightly: "http://127.0.0.1:9/n".into(), deno: "http://127.0.0.1:9/deno".into(), ffmpeg_builds: "http://127.0.0.1:9/ff".into(), ffmpeg_static: "http://127.0.0.1:9/ffs".into() });
        hub
    }

    /// yt-dlp and Deno, fetched or updated if needed, and FFmpeg when `join`. A missing Deno or FFmpeg is not an
    /// error: yt-dlp then manages without (FFmpeg: at 360p).
    pub async fn tools(&self, join: bool, setup: &(dyn Fn(Setup) + Send + Sync), cancel: &CancellationToken) -> Result<Toolset, YtError> {
        #[cfg(test)]
        if let Some(t) = self.fake.lock().unwrap().as_ref().and_then(|f| f.tools.clone()) {
            return Ok(Toolset { ffmpeg: t.ffmpeg.filter(|_| join), ..t });
        }
        let first = self.fetch.installed(Tool::YtDlp).is_none();
        let yt = self.fetch.ensure(Tool::YtDlp, true, &|percent| setup(Setup { tool: Tool::YtDlp, first, percent }), cancel).await.map_err(YtError::from_fetch)?;
        let deno = match self.fetch.ensure(Tool::Deno, false, &|percent| setup(Setup { tool: Tool::Deno, first: true, percent }), cancel).await {
            Ok(d) => Some(d.exe),
            Err(FetchError::Cancelled) => return Err(YtError::Cancelled),
            Err(e) => {
                eprintln!("Deno could not be fetched; yt-dlp runs without it: {e:?}");
                None
            }
        };
        let ffmpeg = if join { self.ffmpeg(setup, cancel).await? } else { None };
        Ok(Toolset { yt_dlp: yt.exe, deno, ffmpeg })
    }

    /// The folder of an FFmpeg: ours, one already on this computer, or a fresh one.
    async fn ffmpeg(&self, setup: &(dyn Fn(Setup) + Send + Sync), cancel: &CancellationToken) -> Result<Option<PathBuf>, YtError> {
        if let Some(i) = self.fetch.installed(Tool::Ffmpeg) { return Ok(i.exe.parent().map(Path::to_path_buf)) }
        if self.system_ffmpeg.load(Ordering::Relaxed) && let Some(f) = system_ffmpeg() { return Ok(f.parent().map(Path::to_path_buf)) }
        match self.fetch.ensure(Tool::Ffmpeg, false, &|percent| setup(Setup { tool: Tool::Ffmpeg, first: true, percent }), cancel).await {
            Ok(i) => Ok(i.exe.parent().map(Path::to_path_buf)),
            Err(FetchError::Cancelled) => Err(YtError::Cancelled),
            Err(e) => {
                eprintln!("FFmpeg could not be fetched; YouTube downloads fall back to 360p: {e:?}");
                Ok(None)
            }
        }
    }

    /// After yt-dlp could not read a video: a newer yt-dlp, if there is one. True when `tools` now has it.
    async fn refresh(&self, tools: &mut Toolset, cancel: &CancellationToken) -> Result<bool, YtError> {
        #[cfg(test)]
        if self.fake.lock().unwrap().as_ref().is_some_and(|f| f.tools.is_some()) { return Ok(false) }
        match self.fetch.update(&|_| {}, cancel).await {
            Ok(true) => match self.fetch.installed(Tool::YtDlp) {
                Some(i) => { tools.yt_dlp = i.exe; Ok(true) }
                None => Ok(false),
            },
            Ok(false) => Ok(false),
            Err(FetchError::Cancelled) => Err(YtError::Cancelled),
            Err(e) => {
                eprintln!("yt-dlp could not be updated: {e:?}");
                Ok(false)
            }
        }
    }

    /// The arguments every run gets: no config file or self-update of the user's, our cache, UTF-8 output, and
    /// where Deno and FFmpeg are.
    fn common_args(&self, tools: &Toolset) -> Vec<OsString> {
        let mut a: Vec<OsString> = ["--ignore-config", "--no-update", "--no-playlist", "--encoding", "utf-8", "--cache-dir"].map(OsString::from).to_vec();
        a.push(self.fetch.deps().join(Tool::YtDlp.folder()).join("cache").into());
        if let Some(deno) = &tools.deno {
            let mut rt = OsString::from("deno:");
            rt.push(deno);
            a.extend([OsString::from("--js-runtimes"), rt]);
        }
        if let Some(dir) = &tools.ffmpeg {
            a.extend([OsString::from("--ffmpeg-location"), dir.into()]);
        }
        a
    }

    /// Runs yt-dlp to the end in its own process tree, handing each output line to `line`. Its error output
    /// decides the error when it fails.
    async fn run(&self, tools: &Toolset, args: Vec<OsString>, limit: Duration, line: &mut (dyn FnMut(&str) + Send), cancel: &CancellationToken) -> Result<(), YtError> {
        // The one-file builds unpack themselves into the temp folder; this one is ours to clear.
        let temp = self.tmp.join(format!("yt-dlp-{}", Uuid::new_v4().simple()));
        let _ = tokio::fs::create_dir_all(&temp).await;
        let r = self.run_in(tools, args, limit, line, cancel, &temp).await;
        remove_dir_soon(&temp).await;
        r
    }

    async fn run_in(&self, tools: &Toolset, args: Vec<OsString>, limit: Duration, line: &mut (dyn FnMut(&str) + Send), cancel: &CancellationToken, temp: &Path) -> Result<(), YtError> {
        #[allow(unused_mut)]
        let mut cmd = Command::new(&tools.yt_dlp);
        #[cfg(test)]
        if let Some(f) = self.fake.lock().unwrap().as_ref() {
            cmd = Command::new(std::env::current_exe().unwrap());
            cmd.args(["--exact", testing_fake::NAME, "--nocapture", "--"]).envs(f.env.iter().cloned()).env("RP_FAKE_AS", &tools.yt_dlp);
        }
        cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .env_remove("PYTHONHOME").env_remove("PYTHONPATH").env("PYTHONUTF8", "1").env("PYTHONIOENCODING", "utf-8")
            .env("TMP", temp).env("TEMP", temp).env("TMPDIR", temp);
        let mut tree = spawn_tree(cmd).map_err(|e| YtError::Other(format!("yt-dlp could not start ({e})")))?;
        let (Some(out), Some(mut err)) = (tree.stdout(), tree.stderr()) else { return Err(YtError::Other("yt-dlp could not start".into())) };
        // The error output is read alongside, so a chatty yt-dlp never blocks on a full pipe.
        let errors = tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = err.read_to_end(&mut buf).await;
            String::from_utf8_lossy(&buf).into_owned()
        });
        let mut lines = BufReader::new(out).lines();
        let ended = {
            let wait = tree.wait(limit, cancel);
            tokio::pin!(wait);
            let mut open = true;
            loop {
                tokio::select! {
                    l = lines.next_line(), if open => match l {
                        Ok(Some(l)) => line(l.trim_end()),
                        _ => open = false,
                    },
                    status = &mut wait => break status,
                }
            }
        };
        // What it printed just before exiting, for a short while (a grandchild may still hold the pipe).
        let _ = tokio::time::timeout(Duration::from_secs(2), async { while let Ok(Some(l)) = lines.next_line().await { line(l.trim_end()) } }).await;
        drop(tree);
        let stderr = tokio::time::timeout(Duration::from_secs(2), errors).await.ok().and_then(Result::ok).unwrap_or_default();
        match ended {
            Ok(status) if status.success() => Ok(()),
            Ok(_) => {
                eprintln!("yt-dlp failed:\n{}", stderr.trim_end());
                Err(classify(&stderr))
            }
            Err(Stop::Cancelled) => Err(YtError::Cancelled),
            Err(Stop::Timeout) => Err(YtError::Offline),
            Err(Stop::Io(e)) => Err(YtError::Other(e)),
        }
    }

    /// `yt-dlp -J` for one address, updating yt-dlp and trying once more when it cannot read the page.
    async fn info(&self, tools: &mut Toolset, url: &str, quality: Quality, cancel: &CancellationToken) -> Result<Info, YtError> {
        let mut retried = false;
        loop {
            let mut args = self.common_args(tools);
            args.extend(format_args(quality, tools.ffmpeg.is_some()).into_iter().map(OsString::from));
            args.extend(["-J", "--flat-playlist", "--no-warnings", url].map(OsString::from));
            let mut json = None;
            let r = self.run(tools, args, LOOK_UP_LIMIT, &mut |l: &str| if l.starts_with('{') { json = Some(l.to_string()) }, cancel).await;
            match r {
                Err(YtError::Broken) if !retried => {
                    retried = true;
                    if self.refresh(tools, cancel).await? { continue }
                    return Err(YtError::Broken);
                }
                Err(e) => return Err(e),
                Ok(()) => return parse_info(&json.ok_or_else(|| YtError::Other("yt-dlp gave no details".into()))?),
            }
        }
    }

    /// The title, length and picture of the video `link` names (a playlist's first video). Gets the helpers
    /// ready first, reporting through `setup` (only when something is fetched).
    pub async fn look_up(&self, link: &Link, quality: Quality, setup: &(dyn Fn(Setup) + Send + Sync), cancel: &CancellationToken) -> Result<Found, YtError> {
        let mut tools = self.tools(quality.joins(), setup, cancel).await?;
        let video = match self.info(&mut tools, &link.url, quality, cancel).await? {
            Info::Video(v) => v,
            Info::Playlist { title, count, first } => {
                let first = first.ok_or(YtError::EmptyPlaylist)?;
                match self.info(&mut tools, &first, quality, cancel).await? {
                    Info::Video(v) => Video { playlist: Some((title, count)), ..v },
                    Info::Playlist { .. } => return Err(YtError::NotAVideo),
                }
            }
        };
        let picture = match &video.thumbnail {
            Some(url) => tokio::time::timeout(Duration::from_secs(10), picture(url)).await.ok().flatten(),
            None => None,
        };
        Ok(Found { video, picture })
    }

    /// Downloads `video` into `staging`, reporting `Event`s. Returns the finished file.
    async fn get(&self, tools: &mut Toolset, video: &Video, quality: Quality, staging: &Path, event: &mut (dyn FnMut(Event) + Send), cancel: &CancellationToken) -> Result<PathBuf, YtError> {
        let mut retried = false;
        loop {
            let mut args = self.common_args(tools);
            args.extend(format_args(quality, tools.ffmpeg.is_some()).into_iter().map(OsString::from));
            // `%` is yt-dlp's template character; a folder name with one in it stays as it is.
            let out = staging.join("%(id)s.%(ext)s").to_string_lossy().replace('%', "%%").replace("%%(id)s.%%(ext)s", "%(id)s.%(ext)s");
            args.extend(["--quiet", "--progress", "--newline", "--no-simulate", "--progress-template", "download:RPJ %(info.format_id)s %(progress)j",
                "--progress-template", "postprocess:RPP %(progress.postprocessor)s", "--print", "after_move:RPF %(filepath)s",
                "--windows-filenames", "--no-mtime", "-o"].map(OsString::from));
            args.push(out.into());
            args.push(video.url.as_str().into());
            let mut meter = Meter::new(&video.parts);
            let mut file: Option<PathBuf> = None;
            let r = self.run(tools, args, DOWNLOAD_LIMIT, &mut |l: &str| {
                if let Some(t) = parse_tick(l) { event(Event::Percent(meter.tick(&t))) }
                else if l.starts_with("RPP ") { event(Event::Finishing) }
                else if let Some(p) = l.strip_prefix("RPF ") { file = Some(PathBuf::from(p)) }
            }, cancel).await;
            match r {
                Err(YtError::Broken) if !retried => {
                    retried = true;
                    if self.refresh(tools, cancel).await? { continue }
                    return Err(YtError::Broken);
                }
                Err(e) => return Err(e),
                Ok(()) => return file.filter(|f| f.is_file()).or_else(|| finished_file(staging)).ok_or_else(|| YtError::Other("yt-dlp saved no video".into())),
            }
        }
    }
}

/// What a download reports as it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Percent(u8),
    /// Joining picture and sound.
    Finishing,
}

/// The one finished media file in `dir`, when yt-dlp's own report of it went missing.
fn finished_file(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        p.is_file() && (media::VIDEO.contains(&ext.as_str()) || media::AUDIO.contains(&ext.as_str()))
    })
}

/// A folder of a program that was just killed: Windows may hold its files a moment longer.
async fn remove_dir_soon(dir: &Path) {
    for _ in 0..10 {
        match tokio::fs::remove_dir_all(dir).await {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => tokio::time::sleep(Duration::from_millis(200)).await,
            _ => return,
        }
    }
}

/// The video's picture, 320 px wide (JPEG or WebP from i.ytimg.com).
async fn picture(url: &str) -> Option<image::RgbaImage> {
    let bytes = fetch::get_bytes(url, 8 * 1024 * 1024).await.ok()?;
    tokio::task::spawn_blocking(move || {
        let img = image::load_from_memory(&bytes).ok()?;
        Some(img.thumbnail(320, 180).to_rgba8())
    }).await.ok().flatten()
}

/// A title as a library name: no folder separators, nothing a file name cannot hold, not too long.
pub fn library_name(title: &str) -> String {
    let clean: String = title.chars().map(|c| if matches!(c, '/' | '\\') { '-' } else if c.is_control() { ' ' } else { c }).collect();
    let clean = clean.trim();
    let clean: String = clean.chars().take(150).collect();
    if clean.is_empty() { "YouTube video".into() } else { clean }
}

// ---------- the download job ----------

/// Starts downloading `video` as a cancellable job card and returns its id. The finished video goes into the
/// media library under its title and, with `service`, at the end of that service.
pub fn start_download(app: &Arc<App>, video: Video, quality: Quality, service: Option<Uuid>) -> Uuid {
    let title = library_name(&video.title);
    let (job, cancel) = app.job_start_cancellable(&tr::downloading(&title));
    let app = app.clone();
    tokio::spawn(async move {
        let staging = app.store.tmp().join(format!("youtube-{}", Uuid::new_v4().simple()));
        let r = download(&app, job, &video, &title, quality, service, &staging, &cancel).await;
        remove_dir_soon(&staging).await;
        match r {
            Ok(note) => {
                app.job_label(job, &tr::downloaded(&title));
                app.job_note(job, note);
                app.job_end(job, None);
            }
            Err(YtError::Cancelled) => app.job_end_cancelled(job),
            Err(e) => app.job_end(job, Some(e.message())),
        }
    });
    job
}

#[allow(clippy::too_many_arguments)]
async fn download(app: &Arc<App>, job: Uuid, video: &Video, title: &str, quality: Quality, service: Option<Uuid>, staging: &Path, cancel: &CancellationToken) -> Result<String, YtError> {
    let hub = &app.youtube;
    let shown = std::sync::Mutex::new(None::<Setup>);
    let mut tools = hub.tools(quality.joins(), &|s: Setup| {
        let mut last = shown.lock().unwrap();
        if last.is_none_or(|l| l.tool != s.tool) { app.job_label(job, &s.text()) }
        *last = Some(s);
        app.job_progress(job, s.percent.min(99));
    }, cancel).await?;
    if shown.lock().unwrap().is_some() {
        app.job_label(job, &tr::downloading(title));
        app.job_progress(job, 0);
    }
    let low_quality = quality.joins() && tools.ffmpeg.is_none();
    tokio::fs::create_dir_all(staging).await.map_err(|e| if fetch::is_disk_full(&e) { YtError::DiskFull } else { YtError::Other(e.to_string()) })?;

    let began = Instant::now();
    let mut label = tr::downloading(title);
    let file = hub.get(&mut tools, video, quality, staging, &mut |e| match e {
        Event::Percent(p) => {
            app.job_progress(job, p);
            // The time left from the pace so far, once there is a pace to go by; the label changes rarely.
            let elapsed = began.elapsed().as_secs();
            if p >= 3 && elapsed >= 5 {
                let left = elapsed * (DOWNLOAD_SHARE as u64 - p.min(95) as u64) / p as u64;
                let text = tr::downloading_left(title, left);
                if text != label { app.job_label(job, &text); label = text; }
            }
        }
        Event::Finishing => {
            let text = tr::finishing(title);
            if text != label { app.job_label(job, &text); app.job_progress(job, DOWNLOAD_SHARE as u8); label = text; }
        }
    }, cancel).await?;
    if cancel.is_cancelled() { return Err(YtError::Cancelled) }

    app.job_label(job, &tr::adding(title));
    app.job_progress(job, 98);
    let ext = file.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "mp4".into());
    let f = file.clone();
    let sha = tokio::task::spawn_blocking(move || -> std::io::Result<String> {
        let mut input = std::fs::File::open(&f)?;
        let (mut hasher, mut buf) = (Sha256::new(), vec![0u8; 1 << 16]);
        loop {
            let n = input.read(&mut buf)?;
            if n == 0 { return Ok(hex(&hasher.finalize())) }
            hasher.update(&buf[..n]);
        }
    }).await.map_err(|e| YtError::Other(e.to_string()))?.map_err(|e| YtError::Other(e.to_string()))?;
    let name = format!("{title}.{ext}");
    let media = match app.import_staged(file, &name, &sha, ImportPurpose::Media).await.map_err(|e| YtError::Other(e.message))? {
        Imported::Media(m) => m,
        _ => return Err(YtError::Other("not a media file".into())),
    };
    app.job_result(job, media.id);
    let added = service.is_some_and(|s| add_to_service(app, s, media.id));
    let note = if added { tr::IN_SERVICE } else { tr::IN_LIBRARY };
    Ok(if low_quality { format!("{note} {}", tr::LOW_QUALITY) } else { note.to_string() })
}

/// Puts the media at the end of the service it was asked for in, if that service is still there.
fn add_to_service(app: &App, service: Uuid, media_id: Uuid) -> bool {
    let item = crate::model::ServiceItem {
        id: Uuid::new_v4(),
        kind: ItemKind::Media { media_id, autoplay: true, end: Default::default(), looped: false },
        background: None,
        notes: String::new(),
    };
    app.edit(|d| {
        let s = d.services.iter_mut().find(|s| s.id == service).ok_or("That service is gone.")?;
        s.items.push(item);
        Ok(())
    }).is_ok()
}

#[cfg(test)]
pub(crate) mod testing_fake {
    use std::path::PathBuf;

    /// The test standing in for yt-dlp (`tests::fake_yt_dlp`).
    pub const NAME: &str = "youtube::tests::fake_yt_dlp";

    /// Runs the test binary as yt-dlp. `tools`: skip fetching and use these (the fake ignores the program path).
    #[derive(Clone, Default)]
    pub(crate) struct Fake {
        pub env: Vec<(String, String)>,
        pub tools: Option<super::Toolset>,
    }

    impl Fake {
        pub(crate) fn new(info: &str, get: &str, log: &PathBuf) -> Fake {
            let env = vec![("RP_FAKE_INFO".into(), info.into()), ("RP_FAKE_GET".into(), get.into()), ("RP_FAKE_LOG".into(), log.display().to_string())];
            Fake { env, tools: Some(super::Toolset { yt_dlp: "yt-dlp".into(), deno: None, ffmpeg: Some("ffmpeg-dir".into()) }) }
        }
    }
}

#[cfg(test)]
mod tests;

/// An FFmpeg the church installed itself, next to the app or on PATH. Preferred over the one YouTube downloads fetch,
/// so a newer one the user chose wins.
fn system_ffmpeg() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let mut dirs: Vec<PathBuf> = vec![];
    if let Some(d) = std::env::current_exe().ok().and_then(|me| me.parent().map(Path::to_path_buf)) { dirs.push(d.join("tools")); dirs.push(d); }
    if let Some(path) = std::env::var_os("PATH") { dirs.extend(std::env::split_paths(&path)); }
    dirs.into_iter().map(|d| d.join(exe)).find(|p| p.is_file())
}
