//! N11: cameras and capture cards as a background. libmpv opens a device through FFmpeg's capture inputs:
//! `av://dshow:video=<name>` on Windows, `av://v4l2:/dev/video0` on Linux and `av://avfoundation:<name>:none` on
//! macOS. Finding the devices: on Windows FFmpeg's `-list_devices` (when an FFmpeg is to be had), else the
//! computer's camera list; on Linux the `/dev/video*` files; on macOS FFmpeg. A name can always be typed instead.
use std::{io::Read, path::{Path, PathBuf}, process::{Command, Stdio}, time::{Duration, Instant}};

/// How long a device search may take.
const LIST_FOR: Duration = Duration::from_secs(8);

/// Which capture input the address uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os { Windows, Linux, Mac }

impl Os {
    pub fn here() -> Os {
        if cfg!(windows) { Os::Windows } else if cfg!(target_os = "macos") { Os::Mac } else { Os::Linux }
    }
}

/// The address mpv opens for a device as it was listed or typed. An address typed in full (`av://…`) is kept.
pub fn url(device: &str, os: Os) -> String {
    let d = device.trim();
    if d.starts_with("av://") { return d.to_string() }
    match os {
        Os::Windows => format!("av://dshow:video={}", d.strip_prefix("video=").unwrap_or(d)),
        Os::Linux if d.starts_with('/') => format!("av://v4l2:{d}"),
        Os::Linux => format!("av://v4l2:/dev/{d}"),
        Os::Mac => format!("av://avfoundation:{d}:none"),
    }
}

/// Whether a path the room plays is a capture device rather than a file.
pub fn is_device(path: &Path) -> bool { path.to_string_lossy().starts_with("av://") }

/// The video devices in FFmpeg's `-f dshow -list_devices true` report (written to stderr). Older builds list the
/// video devices under a heading; newer ones mark each line `(video)`.
pub fn parse_dshow(report: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_video = false;
    for line in report.lines() {
        let text = line.split_once(']').map_or(line, |(_, rest)| rest).trim();
        if text.contains("DirectShow video devices") { in_video = true; continue }
        if text.contains("DirectShow audio devices") { in_video = false; continue }
        if text.starts_with("Alternative name") { continue }
        let Some(name) = quoted(text) else { continue };
        let tagged = text.ends_with("(video)");
        if (tagged || (in_video && !text.ends_with("(audio)") && !text.ends_with("(none)"))) && !out.contains(&name) { out.push(name) }
    }
    out
}

/// The video devices in FFmpeg's `-f avfoundation -list_devices true` report: `[0] FaceTime HD Camera` lines
/// between the video and audio headings.
pub fn parse_avfoundation(report: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_video = false;
    for line in report.lines() {
        let text = line.split_once("] ").map_or(line, |(_, rest)| rest).trim();
        if text.contains("AVFoundation video devices") { in_video = true; continue }
        if text.contains("AVFoundation audio devices") { in_video = false; continue }
        if !in_video { continue }
        if let Some(name) = text.strip_prefix('[').and_then(|t| t.split_once("] ")).map(|(_, n)| n.trim()) {
            // Screen capture is not a camera.
            if !name.is_empty() && !name.starts_with("Capture screen") { out.push(name.to_string()) }
        }
    }
    out
}

/// The `video*` device files in `dir` (`/dev`), in number order.
pub fn list_v4l2(dir: &Path) -> Vec<String> {
    let mut found: Vec<(u32, String)> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        let n = name.strip_prefix("video")?.parse().ok()?;
        Some((n, dir.join(&name).to_string_lossy().into_owned()))
    }).collect();
    found.sort();
    found.into_iter().map(|(_, p)| p).collect()
}

/// The camera names in PowerShell's list of camera and imaging devices (one per line).
pub fn parse_lines(report: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in report.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !out.iter().any(|o| o == l) { out.push(l.to_string()) }
    }
    out
}

/// The text between the first pair of double quotes.
fn quoted(text: &str) -> Option<String> {
    let (_, rest) = text.split_once('"')?;
    let (name, _) = rest.split_once('"')?;
    (!name.is_empty()).then(|| name.to_string())
}

/// Runs a program for at most `LIST_FOR` and returns what it wrote (stdout and stderr); `None` if it could not start.
fn run(mut cmd: Command) -> Option<String> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(crate::platform::proc::creation_flags());
    }
    let mut child = cmd.spawn().ok()?;
    let (mut out, mut err) = (child.stdout.take()?, child.stderr.take()?);
    let reader = std::thread::spawn(move || { let mut s = String::new(); let _ = err.read_to_string(&mut s); s });
    let out_reader = std::thread::spawn(move || { let mut s = String::new(); let _ = out.read_to_string(&mut s); s });
    let end = Instant::now() + LIST_FOR;
    loop {
        if matches!(child.try_wait(), Ok(Some(_))) { break }
        if Instant::now() > end { let _ = child.kill(); let _ = child.wait(); break }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut text = out_reader.join().unwrap_or_default();
    text.push('\n');
    text.push_str(&reader.join().unwrap_or_default());
    Some(text)
}

/// The video devices on this computer, found with `ffmpeg` when there is one. Slow (up to a few seconds): call it
/// off the UI thread. Empty when none could be found; a name can then be typed.
pub fn list(ffmpeg: Option<PathBuf>) -> Vec<String> {
    match Os::here() {
        Os::Linux => list_v4l2(Path::new("/dev")),
        Os::Mac => ffmpeg.and_then(|f| {
            let mut c = Command::new(f);
            c.args(["-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""]);
            run(c)
        }).map(|r| parse_avfoundation(&r)).unwrap_or_default(),
        Os::Windows => {
            let found = ffmpeg.and_then(|f| {
                let mut c = Command::new(f);
                c.args(["-hide_banner", "-f", "dshow", "-list_devices", "true", "-i", "dummy"]);
                run(c)
            }).map(|r| parse_dshow(&r)).unwrap_or_default();
            if !found.is_empty() { return found }
            // No FFmpeg: the camera list Windows keeps. Its names are the DirectShow names for most cameras.
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-NonInteractive", "-Command",
                "Get-CimInstance Win32_PnPEntity | Where-Object { $_.PNPClass -in 'Camera','Image' -and $_.Status -eq 'OK' } | ForEach-Object { $_.Name }"]);
            run(c).map(|r| parse_lines(&r)).unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_addresses_for_each_system() {
        assert_eq!(url("Integrated Camera", Os::Windows), "av://dshow:video=Integrated Camera");
        assert_eq!(url("video=USB Capture HDMI", Os::Windows), "av://dshow:video=USB Capture HDMI");
        assert_eq!(url("/dev/video2", Os::Linux), "av://v4l2:/dev/video2");
        assert_eq!(url("video0", Os::Linux), "av://v4l2:/dev/video0");
        assert_eq!(url("FaceTime HD Camera", Os::Mac), "av://avfoundation:FaceTime HD Camera:none");
        assert_eq!(url(" av://dshow:video=Cam ", Os::Linux), "av://dshow:video=Cam", "typed in full is kept");
        assert!(is_device(Path::new("av://v4l2:/dev/video0")));
        assert!(!is_device(Path::new("/media/loop.mp4")));
    }

    #[test]
    fn dshow_report_new_style() {
        let r = r#"[dshow @ 0000020f1b5c] "Integrated Camera" (video)
[dshow @ 0000020f1b5c]   Alternative name "@device_pnp_\\?\usb#vid_04f2&pid_b6d9"
[dshow @ 0000020f1b5c] "OBS Virtual Camera" (none)
[dshow @ 0000020f1b5c] "USB Capture HDMI" (video)
[dshow @ 0000020f1b5c] "Microphone (Realtek(R) Audio)" (audio)
dummy: Immediate exit requested"#;
        assert_eq!(parse_dshow(r), ["Integrated Camera", "USB Capture HDMI"]);
    }

    #[test]
    fn dshow_report_old_style() {
        let r = r#"[dshow @ 03a1c0] DirectShow video devices (some may be both video and audio devices)
[dshow @ 03a1c0]  "HD Pro Webcam C920"
[dshow @ 03a1c0]     Alternative name "@device_pnp_\\?\usb#vid_046d"
[dshow @ 03a1c0]  "Game Capture HD60 S"
[dshow @ 03a1c0] DirectShow audio devices
[dshow @ 03a1c0]  "Microphone (HD Pro Webcam C920)""#;
        assert_eq!(parse_dshow(r), ["HD Pro Webcam C920", "Game Capture HD60 S"]);
        assert!(parse_dshow("'ffmpeg' is not recognized").is_empty());
    }

    #[test]
    fn avfoundation_report() {
        let r = "[AVFoundation indev @ 0x7f8] AVFoundation video devices:
[AVFoundation indev @ 0x7f8] [0] FaceTime HD Camera
[AVFoundation indev @ 0x7f8] [1] Capture screen 0
[AVFoundation indev @ 0x7f8] [2] Elgato Cam Link 4K
[AVFoundation indev @ 0x7f8] AVFoundation audio devices:
[AVFoundation indev @ 0x7f8] [0] MacBook Pro Microphone";
        assert_eq!(parse_avfoundation(r), ["FaceTime HD Camera", "Elgato Cam Link 4K"]);
    }

    #[test]
    fn v4l2_files_in_number_order() {
        let dir = std::env::temp_dir().join(format!("rp-v4l2-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["video10", "video2", "video0", "vchiq", "videox"] { std::fs::write(dir.join(n), "").unwrap() }
        let d = dir.to_string_lossy().into_owned();
        assert_eq!(list_v4l2(&dir), [format!("{d}/video0"), format!("{d}/video2"), format!("{d}/video10")]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn powershell_lines_without_blanks_or_twins() {
        assert_eq!(parse_lines("Integrated Camera\r\n\r\nIntegrated Camera\r\nUSB Video\r\n"), ["Integrated Camera", "USB Video"]);
    }
}
