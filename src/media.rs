//! Making "any video" playable, and PowerPoint → slide images.
//!
//! The room output plays H.264/VP8/VP9/AV1 video with AAC/MP3/Opus/Vorbis audio
//! in MP4/WebM. Anything else (ProRes, HEVC, WMV, AVI, MKV, MPEG-2, DV, ...) is
//! converted once on import with FFmpeg, when FFmpeg is installed.
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[cfg(windows)]
const NO_WINDOW: u32 = 0x08000000;

fn cmd(program: &Path) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    c.creation_flags(NO_WINDOW);
    c.kill_on_drop(true);
    c
}

/// Find an executable next to the app, on PATH, or in common install folders.
pub fn find_tool(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let mut dirs: Vec<PathBuf> = vec![];
    if let Ok(me) = std::env::current_exe() { if let Some(d) = me.parent() { dirs.push(d.to_path_buf()); dirs.push(d.join("tools")); } }
    if let Some(d) = dirs::data_local_dir() {
        dirs.push(d.join("RustyPresenter").join("tools"));
        dirs.push(d.join("Microsoft").join("WinGet").join("Links"));
    }
    if let Some(path) = std::env::var_os("PATH") { dirs.extend(std::env::split_paths(&path)); }
    // Newer winget adds `Packages\Gyan.FFmpeg_…\ffmpeg-<version>\bin` to the user PATH instead of a Links shim;
    // look there directly so an app started before that PATH change still finds it.
    if let Some(pkgs) = dirs::data_local_dir().map(|d| d.join("Microsoft").join("WinGet").join("Packages")) {
        let subdirs = |p: &std::path::Path| std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect::<Vec<_>>();
        for pkg in subdirs(&pkgs).into_iter().filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("Gyan.FFmpeg"))) {
            dirs.extend(subdirs(&pkg).into_iter().map(|v| v.join("bin")));
        }
    }
    dirs.push(PathBuf::from(r"C:\ffmpeg\bin"));
    dirs.push(PathBuf::from(r"C:\Program Files\ffmpeg\bin"));
    dirs.into_iter().map(|d| d.join(&exe)).find(|p| p.is_file())
}

pub fn ffmpeg() -> Option<PathBuf> { find_tool("ffmpeg") }
pub fn ffprobe() -> Option<PathBuf> { find_tool("ffprobe") }

#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// Plays as-is.
    Ready,
    /// Codecs are fine; only the container changes (fast, no quality loss).
    Remux,
    /// Needs re-encoding.
    Transcode,
    /// Can't tell / can't fix (no FFmpeg).
    Unknown(String),
}

pub const NATIVE_VIDEO: &[&str] = &["mp4", "m4v", "webm"];
pub const NATIVE_AUDIO: &[&str] = &["mp3", "m4a", "aac", "wav", "ogg", "oga", "opus", "flac", "webm"];
pub use crate::import::media::{AUDIO, IMAGE, VIDEO};

/// Ask ffprobe for the codecs and duration.
pub async fn probe(file: &Path) -> Option<(Vec<String>, Vec<String>, f64)> {
    let out = cmd(&ffprobe()?).args(["-v", "error", "-show_entries", "stream=codec_type,codec_name:format=duration", "-of", "default=nw=1"]).arg(file).output().await.ok()?;
    if !out.status.success() { return None; }
    let text = String::from_utf8_lossy(&out.stdout);
    let (mut v, mut a, mut dur) = (vec![], vec![], 0.0);
    let mut name = String::new();
    for line in text.lines() {
        if let Some(n) = line.strip_prefix("codec_name=") { name = n.to_string(); }
        if let Some(t) = line.strip_prefix("codec_type=") {
            match t { "video" => v.push(name.clone()), "audio" => a.push(name.clone()), _ => {} }
        }
        if let Some(d) = line.strip_prefix("duration=") { dur = d.parse().unwrap_or(0.0); }
    }
    Some((v, a, dur))
}

pub async fn plan_video(file: &Path, ext: &str) -> Plan {
    match probe(file).await {
        Some((v, a, _)) => {
            // Attached cover images show up as mjpeg/png "video" streams; ignore them.
            let real: Vec<&String> = v.iter().filter(|c| !matches!(c.as_str(), "mjpeg" | "png" | "bmp")).collect();
            let v_ok = !real.is_empty() && real.iter().all(|c| matches!(c.as_str(), "h264" | "vp8" | "vp9" | "av1"));
            let a_ok = a.iter().all(|c| matches!(c.as_str(), "aac" | "mp3" | "opus" | "vorbis"));
            if v_ok && a_ok { if NATIVE_VIDEO.contains(&ext) { Plan::Ready } else { Plan::Remux } } else { Plan::Transcode }
        }
        None if ffmpeg().is_some() => if NATIVE_VIDEO.contains(&ext) { Plan::Ready } else { Plan::Transcode },
        None => if NATIVE_VIDEO.contains(&ext) { Plan::Ready } else {
            Plan::Unknown("This kind of video needs the free FFmpeg converter. Open Settings → Video to install it, then click Convert.".into())
        },
    }
}

pub async fn plan_audio(file: &Path, ext: &str) -> Plan {
    if NATIVE_AUDIO.contains(&ext) && ext != "webm" {
        if let Some((_, a, _)) = probe(file).await {
            if a.iter().all(|c| matches!(c.as_str(), "aac" | "mp3" | "opus" | "vorbis" | "flac") || c.starts_with("pcm_")) { return Plan::Ready; }
            return Plan::Transcode;
        }
        return Plan::Ready;
    }
    if ffmpeg().is_some() { Plan::Transcode } else { Plan::Unknown("This kind of audio needs the free FFmpeg converter (Settings → Video).".into()) }
}

/// Convert to a file the room can play. Calls `progress(0..=100)` as it goes.
pub async fn convert(input: &Path, output: &Path, kind: &str, plan: &Plan, mut progress: impl FnMut(u8)) -> Result<(), String> {
    let ff = ffmpeg().ok_or("FFmpeg is not installed.")?;
    let duration = probe(input).await.map(|p| p.2).unwrap_or(0.0);
    let mut c = cmd(&ff);
    c.args(["-y", "-hide_banner", "-loglevel", "error", "-progress", "pipe:1", "-nostats", "-i"]).arg(input);
    match (kind, plan) {
        ("video", Plan::Remux) => { c.args(["-map", "0:v:0", "-map", "0:a:0?", "-c", "copy", "-movflags", "+faststart"]); }
        ("video", _) => {
            c.args(["-map", "0:v:0", "-map", "0:a:0?", "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-pix_fmt", "yuv420p",
                // Keep it sane for laptops: at most 4K, even dimensions.
                "-vf", "scale='min(3840,iw)':'-2'",
                "-c:a", "aac", "-b:a", "192k", "-ac", "2", "-movflags", "+faststart"]);
        }
        _ => { c.args(["-vn", "-c:a", "aac", "-b:a", "256k"]); }
    }
    c.arg(output);
    c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = c.spawn().map_err(|e| format!("Could not start FFmpeg: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let mut lines = tokio::io::AsyncBufReadExt::lines(tokio::io::BufReader::new(stdout));
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(us) = line.strip_prefix("out_time_us=").or_else(|| line.strip_prefix("out_time_ms=")) {
            if let (Ok(us), true) = (us.trim().parse::<f64>(), duration > 0.0) {
                progress(((us / 1_000_000.0 / duration) * 100.0).clamp(0.0, 99.0) as u8);
            }
        }
    }
    let out = child.wait_with_output().await.map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("FFmpeg could not convert this file: {}", err.lines().last().unwrap_or("unknown error")));
    }
    progress(100);
    Ok(())
}

const PPT_SCRIPT: &str = r#"
param([string]$Path, [string]$Out)
$ErrorActionPreference = 'Stop'
$ppt = New-Object -ComObject PowerPoint.Application
$hadOpen = $ppt.Presentations.Count
# ReadOnly, Untitled, WithWindow = false. Never touches presentations the user has open.
$pres = $ppt.Presentations.Open($Path, -1, 0, 0)
try {
  $w = [int]($pres.PageSetup.SlideWidth); $h = [int]($pres.PageSetup.SlideHeight)
  $outW = 1920; $outH = [int](1920 * $h / $w)
  foreach ($s in $pres.Slides) {
    if ($s.SlideShowTransition.Hidden) { continue }
    $file = Join-Path $Out ('slide{0:D4}.png' -f $s.SlideIndex)
    $s.Export($file, 'PNG', $outW, $outH)
  }
} finally {
  $pres.Close()
  if ($hadOpen -eq 0 -and $ppt.Presentations.Count -eq 0) { $ppt.Quit() }
}
"#;

/// Export each visible slide of a PowerPoint file as a PNG using the installed PowerPoint.
pub async fn export_powerpoint(input: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, String> {
    if !cfg!(windows) { return Err("PowerPoint import needs Windows with PowerPoint installed.".into()); }
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let script = out_dir.join("export.ps1");
    std::fs::write(&script, PPT_SCRIPT).map_err(|e| e.to_string())?;
    let run = cmd(Path::new("powershell.exe"))
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]).arg(&script)
        .arg("-Path").arg(input).arg("-Out").arg(out_dir).output();
    let out = tokio::time::timeout(std::time::Duration::from_secs(600), run).await
        .map_err(|_| "PowerPoint took too long (over 10 minutes).".to_string())?
        .map_err(|e| format!("Could not start PowerShell: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let hint = if err.contains("80040154") || err.contains("ComObject") { " Is Microsoft PowerPoint installed?" } else { "" };
        return Err(format!("PowerPoint export failed.{hint} {}", err.lines().find(|l| !l.trim().is_empty()).unwrap_or("")));
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(out_dir).map_err(|e| e.to_string())?.flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png"))).collect();
    files.sort();
    if files.is_empty() { return Err("That presentation has no visible slides.".into()); }
    Ok(files)
}
