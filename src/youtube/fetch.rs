//! The helper programs YouTube downloads need, fetched the first time they are used and never shipped in the
//! installer: yt-dlp (reads YouTube), Deno (runs YouTube's JavaScript checks for yt-dlp) and FFmpeg (joins
//! YouTube's separate picture and sound into one MP4). Each comes from its official release page and is checked
//! against the SHA-256 fingerprint published beside it before anything is unpacked.
//! Layout: `<data>/deps/<tool>/<version>/…`, with `<data>/deps/<tool>/current.json` naming the one in use. A new
//! version is unpacked beside the old one and switched to by rewriting `current.json`, so a download that runs
//! while yt-dlp updates keeps its own copy. yt-dlp is checked for a newer release at most once a day.
use crate::sha256::{Digest, Sha256, hex};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// How often yt-dlp is checked for a newer release (YouTube changes often).
pub const CHECK_EVERY: u64 = 24 * 3600;
/// No helper is anywhere near this big; a bigger answer is not what we asked for.
const MAX_DOWNLOAD: u64 = 600 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    YtDlp,
    Deno,
    Ffmpeg,
}

impl Tool {
    pub fn folder(self) -> &'static str {
        match self {
            Tool::YtDlp => "yt-dlp",
            Tool::Deno => "deno",
            Tool::Ffmpeg => "ffmpeg",
        }
    }
}

/// yt-dlp's release channels. Stable by default; nightly once stable could not read a video and nightly differs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Nightly,
}

/// `current.json`: the version in use, its program (relative to the tool's folder), the fingerprint of what was
/// downloaded, and when the release page was last checked (Unix seconds).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub version: String,
    pub exe: String,
    pub sha: String,
    pub checked: u64,
    #[serde(default)]
    pub channel: Channel,
}

/// An installed helper: its program and its state.
#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub exe: PathBuf,
    pub state: State,
}

/// Why a helper could not be fetched. `crate::youtube::YtError` words these for the operator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchError {
    /// No connection, a time-out, or the release page could not be reached.
    Offline,
    /// The release page answered with this HTTP status.
    Http(u16),
    /// What arrived did not match its published fingerprint; it was thrown away.
    Checksum,
    /// The release has no build for this computer.
    NoBuild,
    DiskFull,
    Cancelled,
    /// Anything else (an unreadable archive, a missing fingerprint); for the log.
    Other(String),
}

impl From<reqwest::Error> for FetchError {
    fn from(e: reqwest::Error) -> Self {
        if let Some(s) = e.status() { FetchError::Http(s.as_u16()) }
        else if e.is_connect() || e.is_timeout() || e.is_request() || e.is_body() { FetchError::Offline }
        else { FetchError::Other(e.to_string()) }
    }
}

impl From<std::io::Error> for FetchError {
    fn from(e: std::io::Error) -> Self {
        if is_disk_full(&e) { FetchError::DiskFull } else { FetchError::Other(e.to_string()) }
    }
}

/// ENOSPC on Unix, ERROR_DISK_FULL / ERROR_HANDLE_DISK_FULL on Windows.
pub fn is_disk_full(e: &std::io::Error) -> bool {
    match e.raw_os_error() {
        Some(code) if cfg!(windows) => code == 112 || code == 39,
        Some(code) => code == 28,
        None => false,
    }
}

/// Where the releases come from. Tests point these at a local server.
#[derive(Clone, Debug)]
pub struct Sources {
    pub yt_dlp: String,
    pub yt_dlp_nightly: String,
    pub deno: String,
    /// yt-dlp's own FFmpeg builds (Windows).
    pub ffmpeg_builds: String,
    /// Martin Riedl's static FFmpeg builds (macOS and Linux).
    pub ffmpeg_static: String,
}

impl Default for Sources {
    fn default() -> Self {
        Sources {
            yt_dlp: "https://github.com/yt-dlp/yt-dlp/releases".into(),
            yt_dlp_nightly: "https://github.com/yt-dlp/yt-dlp-nightly-builds/releases".into(),
            deno: "https://github.com/denoland/deno/releases".into(),
            ffmpeg_builds: "https://github.com/yt-dlp/FFmpeg-Builds/releases".into(),
            ffmpeg_static: "https://ffmpeg.martin-riedl.de".into(),
        }
    }
}

/// yt-dlp's file for this computer. Windows takes the unpacked build (`_win.zip`): it starts faster than the
/// one-file exe, which unpacks itself to a temp folder on every run and leaves it behind when cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YtBuild {
    pub asset: &'static str,
    pub exe: &'static str,
    pub zip: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FfBuild {
    /// A zip from yt-dlp/FFmpeg-Builds; its `bin/` folder is kept.
    Builds(&'static str),
    /// `<os>/<arch>` at ffmpeg.martin-riedl.de: one zip for ffmpeg and one for ffprobe.
    Static(&'static str),
}

/// Which builds fit this computer (`std::env::consts::OS` and `ARCH`). `None`: there is none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Builds {
    pub yt_dlp: Option<YtBuild>,
    pub deno: Option<&'static str>,
    pub ffmpeg: Option<FfBuild>,
}

pub fn builds(os: &str, arch: &str) -> Builds {
    let yt = |asset, exe, zip| Some(YtBuild { asset, exe, zip });
    let yt_dlp = match (os, arch) {
        ("windows", "x86_64") => yt("yt-dlp_win.zip", "yt-dlp.exe", true),
        ("windows", "aarch64") => yt("yt-dlp_win_arm64.zip", "yt-dlp.exe", true),
        // One universal build for Intel and Apple Silicon.
        ("macos", _) => yt("yt-dlp_macos", "yt-dlp_macos", false),
        ("linux", "x86_64") => yt("yt-dlp_linux", "yt-dlp_linux", false),
        ("linux", "aarch64") => yt("yt-dlp_linux_aarch64", "yt-dlp_linux_aarch64", false),
        _ => None,
    };
    let deno = match (os, arch) {
        ("windows", "x86_64") => Some("deno-x86_64-pc-windows-msvc.zip"),
        ("windows", "aarch64") => Some("deno-aarch64-pc-windows-msvc.zip"),
        ("macos", "x86_64") => Some("deno-x86_64-apple-darwin.zip"),
        ("macos", "aarch64") => Some("deno-aarch64-apple-darwin.zip"),
        ("linux", "x86_64") => Some("deno-x86_64-unknown-linux-gnu.zip"),
        ("linux", "aarch64") => Some("deno-aarch64-unknown-linux-gnu.zip"),
        _ => None,
    };
    let ffmpeg = match (os, arch) {
        // The shared build is half the size of the static one: ffmpeg and ffprobe share their libraries.
        ("windows", "x86_64") => Some(FfBuild::Builds("ffmpeg-master-latest-win64-gpl-shared.zip")),
        ("windows", "aarch64") => Some(FfBuild::Builds("ffmpeg-master-latest-winarm64-gpl-shared.zip")),
        ("macos", "aarch64") => Some(FfBuild::Static("macos/arm64")),
        ("macos", "x86_64") => Some(FfBuild::Static("macos/amd64")),
        ("linux", "x86_64") => Some(FfBuild::Static("linux/amd64")),
        ("linux", "aarch64") => Some(FfBuild::Static("linux/arm64")),
        _ => None,
    };
    Builds { yt_dlp, deno, ffmpeg }
}

/// The program's name inside its folder for Deno and FFmpeg.
fn exe_name(base: &str) -> String {
    if cfg!(windows) { format!("{base}.exe") } else { base.to_string() }
}

/// The fingerprint `sums` gives `name`: the line naming it (GNU style, `*name` for binary mode), or the only
/// fingerprint in a file that holds one (Deno's PowerShell-style `.sha256sum` on Windows). Lowercase hex.
pub fn sum_for(sums: &str, name: &str) -> Option<String> {
    let is_hash = |t: &str| t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit());
    let mut all = Vec::new();
    for line in sums.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(hash) = tokens.iter().copied().find(|t| is_hash(t)) else { continue };
        all.push(hash.to_ascii_lowercase());
        if tokens.last().map(|t| t.trim_start_matches('*')) == Some(name) {
            return Some(hash.to_ascii_lowercase());
        }
    }
    let named = sums.lines().any(|l| l.split_whitespace().count() >= 2 && l.split_whitespace().next().is_some_and(is_hash));
    (all.len() == 1 && !named).then(|| all.remove(0))
}

/// The release a download address points into: the path segment before the file name
/// (`…/download/2026.10.26/SHA2-256SUMS` → `2026.10.26`).
pub fn tag_of(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let mut parts = path.rsplit('/');
    parts.next()?;
    parts.next().filter(|t| !t.is_empty() && *t != "download").map(str::to_string)
}

/// A folder name from a version: letters, digits, `.`, `_` and `-` only.
fn folder_name(version: &str) -> String {
    let clean: String = version.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).collect();
    if clean.trim_matches('.').is_empty() { "v".into() } else { clean }
}

#[derive(Clone, Debug, PartialEq)]
enum Unpack {
    /// The download is the program itself, saved under this name.
    Save(String),
    /// Every file of the zip, at its path.
    Zip,
    /// FFmpeg's zip: only what is in its `bin/` folder, without the ffplay player.
    ZipBin,
}

#[derive(Clone, Debug, PartialEq)]
struct Part {
    url: String,
    /// Known before the download, or `None` when it is published beside the file (`<final address>.sha256`).
    sha: Option<String>,
    unpack: Unpack,
}

/// What to fetch for one version of a tool.
#[derive(Clone, Debug, PartialEq)]
struct Plan {
    tool: Tool,
    /// Empty when only the download's final address tells it.
    version: String,
    exe: String,
    channel: Channel,
    parts: Vec<Part>,
}

impl Plan {
    /// The fingerprint `current.json` keeps, when known before downloading.
    fn sha(&self) -> Option<&str> {
        self.parts.first().and_then(|p| p.sha.as_deref())
    }
}

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Fetches, checks and installs the helpers into `<deps>/<tool>/`.
pub struct Fetcher {
    deps: PathBuf,
    sources: std::sync::Mutex<Sources>,
    builds: std::sync::Mutex<Builds>,
    /// One install or check at a time, so two lookups never fetch the same thing twice.
    gate: tokio::sync::Mutex<()>,
}

impl Fetcher {
    pub fn new(deps: PathBuf) -> Self {
        Fetcher {
            deps,
            sources: std::sync::Mutex::new(Sources::default()),
            builds: std::sync::Mutex::new(builds(std::env::consts::OS, std::env::consts::ARCH)),
            gate: tokio::sync::Mutex::new(()),
        }
    }

    pub fn deps(&self) -> &Path {
        &self.deps
    }

    #[cfg(test)]
    pub(crate) fn set_sources(&self, s: Sources) {
        *self.sources.lock().unwrap() = s;
    }

    #[cfg(test)]
    pub(crate) fn set_builds(&self, b: Builds) {
        *self.builds.lock().unwrap() = b;
    }

    pub fn builds(&self) -> Builds {
        *self.builds.lock().unwrap()
    }

    fn root(&self, tool: Tool) -> PathBuf {
        self.deps.join(tool.folder())
    }

    /// The version in use, if its program is still there.
    pub fn installed(&self, tool: Tool) -> Option<Installed> {
        installed_in(&self.deps, tool)
    }

    /// The installed tool, installing it first if needed. With `daily`, a yt-dlp last checked a day or more ago is
    /// checked now and updated quietly; a failed check keeps the installed one.
    pub async fn ensure(&self, tool: Tool, daily: bool, progress: &(dyn Fn(u8) + Send + Sync), cancel: &CancellationToken) -> Result<Installed, FetchError> {
        let _gate = self.gate.lock().await;
        match self.installed(tool) {
            Some(i) if !(daily && tool == Tool::YtDlp && now_secs().saturating_sub(i.state.checked) >= CHECK_EVERY) => Ok(i),
            Some(i) => match self.check(&i, i.state.channel, progress, cancel).await {
                Ok(i) => Ok(i),
                Err(FetchError::Cancelled) => Err(FetchError::Cancelled),
                Err(e) => {
                    eprintln!("yt-dlp update check failed, keeping {}: {e:?}", i.state.version);
                    Ok(i)
                }
            },
            None => {
                let plan = self.plan(tool, Channel::Stable).await?;
                self.install(plan, progress, cancel).await
            }
        }
    }

    /// After yt-dlp could not read a video: the newest stable release, or else the nightly one, when it differs
    /// from what is installed. True when a different yt-dlp is now in use.
    pub async fn update(&self, progress: &(dyn Fn(u8) + Send + Sync), cancel: &CancellationToken) -> Result<bool, FetchError> {
        let _gate = self.gate.lock().await;
        let Some(old) = self.installed(Tool::YtDlp) else {
            let plan = self.plan(Tool::YtDlp, Channel::Stable).await?;
            self.install(plan, progress, cancel).await?;
            return Ok(true);
        };
        let channels: &[Channel] = if old.state.channel == Channel::Stable { &[Channel::Stable, Channel::Nightly] } else { &[Channel::Nightly] };
        for channel in channels {
            let plan = self.plan(Tool::YtDlp, *channel).await?;
            if plan.sha() != Some(old.state.sha.as_str()) {
                self.install(plan, progress, cancel).await?;
                return Ok(true);
            }
        }
        self.touch(&old);
        Ok(false)
    }

    /// The daily check: installs the channel's newest release when it differs, else records the check.
    async fn check(&self, old: &Installed, channel: Channel, progress: &(dyn Fn(u8) + Send + Sync), cancel: &CancellationToken) -> Result<Installed, FetchError> {
        let plan = self.plan(Tool::YtDlp, channel).await?;
        if plan.sha() == Some(old.state.sha.as_str()) {
            return Ok(self.touch(old));
        }
        self.install(plan, progress, cancel).await
    }

    /// Records that the release page was checked now.
    fn touch(&self, old: &Installed) -> Installed {
        let mut i = old.clone();
        i.state.checked = now_secs();
        let _ = write_state(&self.root(Tool::YtDlp), &i.state);
        i
    }

    /// Reads the release's fingerprints (small files) to learn what to download.
    async fn plan(&self, tool: Tool, channel: Channel) -> Result<Plan, FetchError> {
        let sources = self.sources.lock().unwrap().clone();
        let builds = self.builds();
        match tool {
            Tool::YtDlp => {
                let b = builds.yt_dlp.ok_or(FetchError::NoBuild)?;
                let base = if channel == Channel::Nightly { &sources.yt_dlp_nightly } else { &sources.yt_dlp };
                let (at, sums) = get_text(&format!("{base}/latest/download/SHA2-256SUMS")).await?;
                let tag = tag_of(&at).ok_or_else(|| FetchError::Other(format!("no release in {at}")))?;
                let sha = sum_for(&sums, b.asset).ok_or(FetchError::NoBuild)?;
                let unpack = if b.zip { Unpack::Zip } else { Unpack::Save(b.exe.into()) };
                let parts = vec![Part { url: format!("{base}/download/{tag}/{}", b.asset), sha: Some(sha), unpack }];
                Ok(Plan { tool, version: tag, exe: b.exe.into(), channel, parts })
            }
            Tool::Deno => {
                let asset = builds.deno.ok_or(FetchError::NoBuild)?;
                let base = &sources.deno;
                let (at, sums) = get_text(&format!("{base}/latest/download/{asset}.sha256sum")).await?;
                let tag = tag_of(&at).ok_or_else(|| FetchError::Other(format!("no release in {at}")))?;
                let sha = sum_for(&sums, asset).ok_or_else(|| FetchError::Other("no fingerprint for Deno".into()))?;
                let parts = vec![Part { url: format!("{base}/download/{tag}/{asset}"), sha: Some(sha), unpack: Unpack::Zip }];
                Ok(Plan { tool, version: tag, exe: exe_name("deno"), channel, parts })
            }
            Tool::Ffmpeg => match builds.ffmpeg.ok_or(FetchError::NoBuild)? {
                FfBuild::Builds(asset) => {
                    let base = &sources.ffmpeg_builds;
                    let (at, sums) = get_text(&format!("{base}/latest/download/checksums.sha256")).await?;
                    let tag = tag_of(&at).ok_or_else(|| FetchError::Other(format!("no release in {at}")))?;
                    let sha = sum_for(&sums, asset).ok_or(FetchError::NoBuild)?;
                    // The release is rebuilt in place ("latest"), so its fingerprint names the version.
                    let version = format!("{tag}-{}", &sha[..12]);
                    let parts = vec![Part { url: format!("{base}/download/{tag}/{asset}"), sha: Some(sha), unpack: Unpack::ZipBin }];
                    Ok(Plan { tool, version, exe: exe_name("ffmpeg"), channel, parts })
                }
                FfBuild::Static(os_arch) => {
                    let base = &sources.ffmpeg_static;
                    let part = |name: &str| Part { url: format!("{base}/redirect/latest/{os_arch}/release/{name}.zip"), sha: None, unpack: Unpack::Zip };
                    Ok(Plan { tool, version: String::new(), exe: "ffmpeg".into(), channel, parts: vec![part("ffmpeg"), part("ffprobe")] })
                }
            },
        }
    }

    /// Downloads and checks every part into a new folder beside the old version, then switches to it.
    async fn install(&self, plan: Plan, progress: &(dyn Fn(u8) + Send + Sync), cancel: &CancellationToken) -> Result<Installed, FetchError> {
        let root = self.root(plan.tool);
        tokio::fs::create_dir_all(&root).await?;
        let stage = root.join(format!(".new-{}", Uuid::new_v4().simple()));
        tokio::fs::create_dir_all(&stage).await?;
        let r = self.fill(&plan, &stage, progress, cancel).await;
        let (version, sha) = match r {
            Ok(v) => v,
            Err(e) => {
                let _ = tokio::fs::remove_dir_all(&stage).await;
                return Err(e);
            }
        };
        let exe_in_stage = stage.join(&plan.exe);
        if !exe_in_stage.is_file() {
            let _ = tokio::fs::remove_dir_all(&stage).await;
            return Err(FetchError::Other(format!("{} is missing from the download", plan.exe)));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in [plan.exe.as_str(), "ffprobe"] {
                let p = stage.join(name);
                if p.is_file() { let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)); }
            }
        }
        // A folder of the same name may be in use (a running yt-dlp): then this one gets a fresh name.
        let mut name = folder_name(&version);
        let mut dest = root.join(&name);
        if dest.exists() && std::fs::remove_dir_all(&dest).is_err() {
            name = format!("{name}-{}", &Uuid::new_v4().simple().to_string()[..8]);
            dest = root.join(&name);
        }
        if let Err(e) = tokio::fs::rename(&stage, &dest).await {
            let _ = tokio::fs::remove_dir_all(&stage).await;
            return Err(e.into());
        }
        let state = State { version, exe: format!("{name}/{}", plan.exe), sha, checked: now_secs(), channel: plan.channel };
        if let Err(e) = write_state(&root, &state) {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(e.into());
        }
        clean_old(&root, &name);
        progress(100);
        Ok(Installed { exe: dest.join(&plan.exe), state })
    }

    /// Downloads, checks and unpacks each part into `stage`. Returns the version and the first part's fingerprint.
    async fn fill(&self, plan: &Plan, stage: &Path, progress: &(dyn Fn(u8) + Send + Sync), cancel: &CancellationToken) -> Result<(String, String), FetchError> {
        let (mut version, mut first) = (plan.version.clone(), None);
        let n = plan.parts.len() as u64;
        for (i, part) in plan.parts.iter().enumerate() {
            let file = stage.join(format!("download-{i}.part"));
            let at = i as u64;
            let (url, got) = download(&part.url, &file, &|done, total| {
                let frac = total.filter(|t| *t > 0).map_or(0, |t| done.min(t) * 100 / t);
                progress(((at * 100 + frac) / n).min(99) as u8);
            }, cancel).await?;
            let want = match &part.sha {
                Some(s) => s.clone(),
                None => {
                    let name = url.rsplit('/').next().unwrap_or_default().to_string();
                    let (_, sums) = get_text(&format!("{url}.sha256")).await?;
                    sum_for(&sums, &name).ok_or_else(|| FetchError::Other(format!("no fingerprint for {name}")))?
                }
            };
            if got != want {
                eprintln!("{} did not match its fingerprint: got {got}, want {want}", part.url);
                return Err(FetchError::Checksum);
            }
            if version.is_empty() {
                version = tag_of(&url).unwrap_or_else(|| want[..12].to_string());
            }
            first.get_or_insert(want);
            let (unpack, stage2, file2) = (part.unpack.clone(), stage.to_path_buf(), file.clone());
            tokio::task::spawn_blocking(move || unpack_into(&file2, &stage2, &unpack))
                .await
                .map_err(|e| FetchError::Other(e.to_string()))??;
            let _ = tokio::fs::remove_file(&file).await;
        }
        Ok((version, first.unwrap_or_default()))
    }
}

/// The version of `tool` installed under `deps`, if its program is still there.
pub fn installed_in(deps: &Path, tool: Tool) -> Option<Installed> {
    let root = deps.join(tool.folder());
    let state: State = serde_json::from_slice(&std::fs::read(root.join("current.json")).ok()?).ok()?;
    let exe = root.join(&state.exe);
    exe.is_file().then_some(Installed { exe, state })
}

fn write_state(root: &Path, state: &State) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?;
    crate::store::write_atomic(&root.join("current.json"), &bytes)
}

/// Removes the versions no longer in use and stale half-done downloads. One still running stays until next time.
fn clean_old(root: &Path, keep: &str) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name == keep || !e.path().is_dir() || name == "cache" { continue }
        let _ = std::fs::remove_dir_all(e.path());
    }
}

/// The shared client for helper downloads: no total time limit (a download can take minutes), but a stalled
/// connection gives up. rustls with ring, like `bible::sources::client`.
fn client() -> Result<&'static reqwest::Client, FetchError> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let b = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .user_agent(concat!("RustyPresenter/", env!("CARGO_PKG_VERSION")));
        // Tests talk to a server on this computer; a proxy in the environment must not get in the way.
        #[cfg(test)]
        let b = b.no_proxy();
        b.build().map_err(|e| e.to_string())
    }).as_ref().map_err(|e| FetchError::Other(e.clone()))
}

/// A small text file, and the address it came from after redirects.
pub(crate) async fn get_text(url: &str) -> Result<(String, String), FetchError> {
    let resp = client()?.get(url).send().await?.error_for_status()?;
    let at = resp.url().to_string();
    if resp.content_length().is_some_and(|n| n > 1024 * 1024) {
        return Err(FetchError::Other(format!("{url} is too big")));
    }
    Ok((at, resp.text().await?))
}

/// A small binary file (a video's picture).
pub(crate) async fn get_bytes(url: &str, limit: u64) -> Result<Vec<u8>, FetchError> {
    let resp = client()?.get(url).send().await?.error_for_status()?;
    if resp.content_length().is_some_and(|n| n > limit) {
        return Err(FetchError::Other(format!("{url} is too big")));
    }
    Ok(resp.bytes().await?.to_vec())
}

/// Streams `url` into `file`, hashing as it goes. Returns the final address and the file's SHA-256.
async fn download(url: &str, file: &Path, progress: &(dyn Fn(u64, Option<u64>) + Send + Sync), cancel: &CancellationToken) -> Result<(String, String), FetchError> {
    let mut resp = tokio::select! {
        _ = cancel.cancelled() => return Err(FetchError::Cancelled),
        r = client()?.get(url).send() => r?.error_for_status()?,
    };
    let at = resp.url().to_string();
    let total = resp.content_length();
    if total.is_some_and(|n| n > MAX_DOWNLOAD) {
        return Err(FetchError::Other(format!("{url} is too big")));
    }
    let mut out = tokio::fs::File::create(file).await?;
    let (mut hasher, mut done) = (Sha256::new(), 0u64);
    progress(0, total);
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return Err(FetchError::Cancelled),
            c = resp.chunk() => c?,
        };
        let Some(chunk) = chunk else { break };
        done += chunk.len() as u64;
        if done > MAX_DOWNLOAD {
            return Err(FetchError::Other(format!("{url} is too big")));
        }
        hasher.update(&chunk);
        out.write_all(&chunk).await?;
        progress(done, total);
    }
    out.flush().await?;
    Ok((at, hex(&hasher.finalize())))
}

/// Puts a checked download in place: saved as the program, or unpacked. Paths that would leave `dest` are skipped.
fn unpack_into(file: &Path, dest: &Path, unpack: &Unpack) -> Result<(), FetchError> {
    let zip_only_bin = match unpack {
        Unpack::Save(name) => {
            std::fs::rename(file, dest.join(name))?;
            return Ok(());
        }
        Unpack::Zip => false,
        Unpack::ZipBin => true,
    };
    let bad = |e: zip::result::ZipError| FetchError::Other(format!("unreadable archive: {e}"));
    let mut archive = zip::ZipArchive::new(std::fs::File::open(file)?).map_err(bad)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(bad)?;
        if entry.is_dir() || entry.is_symlink() { continue }
        let Some(rel) = entry.enclosed_name() else { continue };
        let rel = if zip_only_bin {
            let Some(rel) = after_bin(&rel) else { continue };
            if rel.file_stem().is_some_and(|s| s.to_string_lossy().starts_with("ffplay")) { continue }
            rel
        } else {
            rel
        };
        let out = dest.join(&rel);
        if let Some(dir) = out.parent() { std::fs::create_dir_all(dir)?; }
        let mut w = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut w)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode((mode & 0o777) | 0o600));
        }
    }
    Ok(())
}

/// The part of `rel` after its `bin` folder (`ffmpeg-…/bin/ffmpeg.exe` → `ffmpeg.exe`).
fn after_bin(rel: &Path) -> Option<PathBuf> {
    let parts: Vec<_> = rel.components().collect();
    let at = parts.iter().position(|c| c.as_os_str() == "bin")?;
    let rest: PathBuf = parts[at + 1..].iter().collect();
    (!rest.as_os_str().is_empty()).then_some(rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::youtube::testing::{FakeReleases, zip_of};

    const YT_SUMS_NAME: &str = "yt-dlp_win.zip";

    fn quiet(_: u8) {}

    fn temp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("rp-fetch-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn win_builds() -> Builds {
        Builds { yt_dlp: Some(YtBuild { asset: YT_SUMS_NAME, exe: "yt-dlp.exe", zip: true }), deno: Some("deno-x86_64-pc-windows-msvc.zip"), ffmpeg: Some(FfBuild::Builds("ffmpeg-win64.zip")) }
    }

    fn fetcher(server: &FakeReleases, deps: &Path) -> Fetcher {
        let f = Fetcher::new(deps.to_path_buf());
        f.set_sources(server.sources());
        f.set_builds(win_builds());
        f
    }

    #[test]
    fn sums_by_name_or_single() {
        let gnu = "aaaa  x\n0f922c15ccc0b83eefa1c8bc9104e434318c4bf16e408e1ee93b7745a6efe0aa  yt-dlp_macos\nf5389d3698ecde4ef06f4e6b1ce0294fe9ac1396228c8990b8d3416d6d177a7d *yt-dlp.exe\n";
        assert_eq!(sum_for(gnu, "yt-dlp.exe").as_deref(), Some("f5389d3698ecde4ef06f4e6b1ce0294fe9ac1396228c8990b8d3416d6d177a7d"));
        assert_eq!(sum_for(gnu, "yt-dlp_macos").as_deref(), Some("0f922c15ccc0b83eefa1c8bc9104e434318c4bf16e408e1ee93b7745a6efe0aa"));
        assert_eq!(sum_for(gnu, "yt-dlp"), None, "a name list without it is not a match");
        // Deno's Windows `.sha256sum` is PowerShell's Get-FileHash output.
        let ps = "\nAlgorithm : SHA256\nHash      : CBA84B26E0DF5AE7F74FC66B86A95B866872A86292EBD84FA1E959927EDD93BF\nPath      : C:\\a\\deno\\target\\release\\deno-x86_64-pc-windows-msvc.zip\n";
        assert_eq!(sum_for(ps, "deno-x86_64-pc-windows-msvc.zip").as_deref(), Some("cba84b26e0df5ae7f74fc66b86a95b866872a86292ebd84fa1e959927edd93bf"));
        assert_eq!(sum_for("fcbe839537485eaee7a7a8bc5cbc0f90d53617e80943e8a5b2e31cb851197ea6  ffprobe.zip\n", "ffprobe.zip").as_deref(), Some("fcbe839537485eaee7a7a8bc5cbc0f90d53617e80943e8a5b2e31cb851197ea6"));
        assert_eq!(sum_for("", "x"), None);
    }

    #[test]
    fn tag_is_the_folder_before_the_file() {
        assert_eq!(tag_of("https://github.com/yt-dlp/yt-dlp/releases/download/2026.10.26/SHA2-256SUMS").as_deref(), Some("2026.10.26"));
        assert_eq!(tag_of("https://ffmpeg.martin-riedl.de/download/macos/arm64/1789931890_9.0.2/ffmpeg.zip").as_deref(), Some("1789931890_9.0.2"));
        assert_eq!(tag_of("https://x/releases/latest/download/SHA2-256SUMS").as_deref(), None);
        assert_eq!(folder_name("v2.9.8"), "v2.9.8");
        assert_eq!(folder_name("../x"), ".._x");
        assert_eq!(folder_name(".."), "v");
    }

    #[test]
    fn builds_for_each_computer() {
        let w = builds("windows", "x86_64");
        assert_eq!(w.yt_dlp.unwrap().asset, "yt-dlp_win.zip");
        assert_eq!(w.ffmpeg, Some(FfBuild::Builds("ffmpeg-master-latest-win64-gpl-shared.zip")));
        assert_eq!(builds("macos", "aarch64").yt_dlp.unwrap().asset, "yt-dlp_macos");
        assert_eq!(builds("macos", "aarch64").ffmpeg, Some(FfBuild::Static("macos/arm64")));
        assert_eq!(builds("linux", "aarch64").deno, Some("deno-aarch64-unknown-linux-gnu.zip"));
        let none = builds("windows", "x86");
        assert_eq!((none.yt_dlp, none.deno, none.ffmpeg), (None, None, None));
    }

    #[test]
    fn ffmpeg_zip_keeps_only_bin_without_ffplay() {
        let dir = temp();
        let zip = dir.join("f.zip");
        std::fs::write(&zip, zip_of(&[
            ("ffmpeg-x/bin/ffmpeg.exe", b"ff"), ("ffmpeg-x/bin/ffprobe.exe", b"fp"), ("ffmpeg-x/bin/ffplay.exe", b"no"),
            ("ffmpeg-x/bin/avcodec-62.dll", b"lib"), ("ffmpeg-x/doc/readme.txt", b"no"), ("../evil.exe", b"no"),
        ])).unwrap();
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        unpack_into(&zip, &out, &Unpack::ZipBin).unwrap();
        let mut names: Vec<String> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["avcodec-62.dll", "ffmpeg.exe", "ffprobe.exe"]);
        assert!(!dir.join("evil.exe").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn installs_checked_yt_dlp_and_records_version() {
        let server = FakeReleases::start().await;
        server.yt_dlp_release("2026.10.26", b"exe v1");
        let deps = temp();
        let f = fetcher(&server, &deps);
        let seen = std::sync::Mutex::new(Vec::new());
        let i = f.ensure(Tool::YtDlp, true, &|p| seen.lock().unwrap().push(p), &CancellationToken::new()).await.unwrap();
        assert_eq!(i.state.version, "2026.10.26");
        assert_eq!(i.state.channel, Channel::Stable);
        assert_eq!(std::fs::read(&i.exe).unwrap(), b"exe v1");
        assert_eq!(i.exe, deps.join("yt-dlp").join("2026.10.26").join("yt-dlp.exe"));
        assert_eq!(f.installed(Tool::YtDlp), Some(i));
        assert_eq!(seen.lock().unwrap().last(), Some(&100));
        // Nothing half-done is left.
        let left: Vec<String> = std::fs::read_dir(deps.join("yt-dlp")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert!(left.iter().all(|n| !n.starts_with(".new")), "{left:?}");
        let _ = std::fs::remove_dir_all(deps);
    }

    #[tokio::test]
    async fn refuses_a_download_that_does_not_match_its_fingerprint() {
        let server = FakeReleases::start().await;
        server.yt_dlp_release("2026.10.26", b"exe v1");
        // The file changed after the fingerprints were published.
        server.put("/yt-dlp/releases/download/2026.10.26/yt-dlp_win.zip", zip_of(&[("yt-dlp.exe", b"tampered")]));
        let deps = temp();
        let f = fetcher(&server, &deps);
        let r = f.ensure(Tool::YtDlp, true, &quiet, &CancellationToken::new()).await;
        assert_eq!(r, Err(FetchError::Checksum));
        assert_eq!(f.installed(Tool::YtDlp), None);
        let left: Vec<_> = std::fs::read_dir(deps.join("yt-dlp")).unwrap().collect();
        assert!(left.is_empty(), "the refused download is gone");
        let _ = std::fs::remove_dir_all(deps);
    }

    #[tokio::test]
    async fn checks_for_a_newer_yt_dlp_at_most_once_a_day() {
        let server = FakeReleases::start().await;
        server.yt_dlp_release("2026.10.26", b"exe v1");
        let deps = temp();
        let f = fetcher(&server, &deps);
        let cancel = CancellationToken::new();
        f.ensure(Tool::YtDlp, true, &quiet, &cancel).await.unwrap();
        server.yt_dlp_release("2026.11.01", b"exe v2");
        let hits = server.hits();
        let same = f.ensure(Tool::YtDlp, true, &quiet, &cancel).await.unwrap();
        assert_eq!(same.state.version, "2026.10.26", "checked today: no new check");
        assert_eq!(server.hits(), hits, "no request at all");
        // A day later the check finds the new release, installs it and removes the old one.
        let mut st = same.state.clone();
        st.checked -= CHECK_EVERY;
        write_state(&deps.join("yt-dlp"), &st).unwrap();
        let new = f.ensure(Tool::YtDlp, true, &quiet, &cancel).await.unwrap();
        assert_eq!(new.state.version, "2026.11.01");
        assert_eq!(std::fs::read(&new.exe).unwrap(), b"exe v2");
        assert!(!deps.join("yt-dlp").join("2026.10.26").exists());
        // Unchanged release: the check is only recorded.
        let mut st = new.state.clone();
        st.checked -= CHECK_EVERY;
        write_state(&deps.join("yt-dlp"), &st).unwrap();
        let again = f.ensure(Tool::YtDlp, true, &quiet, &cancel).await.unwrap();
        assert_eq!(again.state.version, "2026.11.01");
        assert!(now_secs() - again.state.checked < 60);
        // Offline: the installed one is used.
        let mut st = again.state.clone();
        st.checked -= CHECK_EVERY;
        write_state(&deps.join("yt-dlp"), &st).unwrap();
        server.fail_all(503);
        assert_eq!(f.ensure(Tool::YtDlp, true, &quiet, &cancel).await.unwrap().state.version, "2026.11.01");
        let _ = std::fs::remove_dir_all(deps);
    }

    #[tokio::test]
    async fn update_tries_stable_then_nightly() {
        let server = FakeReleases::start().await;
        server.yt_dlp_release("2026.10.26", b"exe v1");
        server.yt_dlp_nightly("2026.10.29.232701", b"exe nightly");
        let deps = temp();
        let f = fetcher(&server, &deps);
        let cancel = CancellationToken::new();
        f.ensure(Tool::YtDlp, false, &quiet, &cancel).await.unwrap();
        assert!(f.update(&quiet, &cancel).await.unwrap(), "stable has nothing newer: nightly");
        let i = f.installed(Tool::YtDlp).unwrap();
        assert_eq!((i.state.version.as_str(), i.state.channel), ("2026.10.29.232701", Channel::Nightly));
        assert!(!f.update(&quiet, &cancel).await.unwrap(), "nothing newer anywhere");
        let _ = std::fs::remove_dir_all(deps);
    }

    // The ffmpeg-builds zip layout (and its `.exe` names) is only used on Windows; elsewhere FFmpeg is a static build.
    #[cfg(windows)]
    #[tokio::test]
    async fn deno_and_ffmpeg_install_with_their_own_layouts() {
        let server = FakeReleases::start().await;
        server.deno_release("v2.9.8", "deno-x86_64-pc-windows-msvc.zip", &zip_of(&[("deno.exe", b"deno")]), true);
        server.ffmpeg_builds("ffmpeg-win64.zip", &zip_of(&[("x/bin/ffmpeg.exe", b"ff"), ("x/bin/ffprobe.exe", b"fp"), ("x/bin/ffplay.exe", b"no")]));
        let deps = temp();
        let f = fetcher(&server, &deps);
        let cancel = CancellationToken::new();
        let d = f.ensure(Tool::Deno, true, &quiet, &cancel).await.unwrap();
        assert_eq!(d.state.version, "v2.9.8");
        assert_eq!(std::fs::read(&d.exe).unwrap(), b"deno");
        let ff = f.ensure(Tool::Ffmpeg, true, &quiet, &cancel).await.unwrap();
        assert!(ff.exe.ends_with(exe_name("ffmpeg")), "{:?}", ff.exe);
        assert!(ff.exe.with_file_name("ffprobe.exe").is_file());
        assert!(!ff.exe.with_file_name("ffplay.exe").exists());
        let _ = std::fs::remove_dir_all(deps);
    }

    #[tokio::test]
    async fn static_ffmpeg_checks_the_fingerprint_beside_each_file() {
        let server = FakeReleases::start().await;
        server.ffmpeg_static("macos/arm64", "1789931890_9.0.2", &zip_of(&[("ffmpeg", b"ff")]), &zip_of(&[("ffprobe", b"fp")]), false);
        let deps = temp();
        let f = fetcher(&server, &deps);
        f.set_builds(Builds { ffmpeg: Some(FfBuild::Static("macos/arm64")), ..win_builds() });
        let ff = f.ensure(Tool::Ffmpeg, true, &quiet, &CancellationToken::new()).await.unwrap();
        assert_eq!(ff.state.version, "1789931890_9.0.2");
        assert_eq!(std::fs::read(&ff.exe).unwrap(), b"ff");
        assert_eq!(std::fs::read(ff.exe.with_file_name("ffprobe")).unwrap(), b"fp");
        // A wrong published fingerprint refuses it.
        let deps2 = temp();
        server.ffmpeg_static("macos/arm64", "1789931890_9.0.2", &zip_of(&[("ffmpeg", b"ff")]), &zip_of(&[("ffprobe", b"fp")]), true);
        let f2 = fetcher(&server, &deps2);
        f2.set_builds(Builds { ffmpeg: Some(FfBuild::Static("macos/arm64")), ..win_builds() });
        assert_eq!(f2.ensure(Tool::Ffmpeg, true, &quiet, &CancellationToken::new()).await, Err(FetchError::Checksum));
        let _ = (std::fs::remove_dir_all(deps), std::fs::remove_dir_all(deps2));
    }

    #[tokio::test]
    async fn offline_and_missing_builds_are_told_apart() {
        let deps = temp();
        let f = Fetcher::new(deps.clone());
        f.set_sources(Sources { yt_dlp: "http://127.0.0.1:9/yt".into(), ..Sources::default() });
        f.set_builds(win_builds());
        assert_eq!(f.ensure(Tool::YtDlp, true, &quiet, &CancellationToken::new()).await, Err(FetchError::Offline));
        f.set_builds(Builds { yt_dlp: None, deno: None, ffmpeg: None });
        assert_eq!(f.ensure(Tool::YtDlp, true, &quiet, &CancellationToken::new()).await, Err(FetchError::NoBuild));
        let _ = std::fs::remove_dir_all(deps);
    }
}
