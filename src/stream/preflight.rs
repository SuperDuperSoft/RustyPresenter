//! Before going live: are the cameras and the sound input chosen in Settings there, and sending? Each is looked
//! for in the computer's list, then opened once for a moment (a camera must send one picture, a sound input half a
//! second of sound), all at the same time, while Go Live gets everything else ready. What is missing is said in
//! plain words; the stream still goes out (with the slides in place of a missing camera, or without that sound),
//! as a stream without one camera is better than none.
use crate::platform::tools::{OS, Os};
use crate::record::args::{self, Input};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// How long a camera or sound input may take to send something.
pub const LIMIT: Duration = Duration::from_secs(6);

/// What is wanted for the stream.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wanted {
    /// The cameras, as Settings has them (`RecordPrefs::cameras`).
    pub cameras: Vec<String>,
    /// The microphone or line-in (`Sound::Input`), if any.
    pub sound: Option<String>,
}

/// Something wanted that is not ready.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Missing {
    /// Camera `n` (from 1) is not in the computer's list (unplugged, or another name now).
    CameraGone { n: usize, label: String },
    /// Camera `n` is there but sent no picture (another program has it, or it is off).
    CameraSilent { n: usize, label: String },
    /// An NDI camera, but NDI is not installed.
    NoNdi { n: usize, label: String },
    /// The sound input is not in the computer's list.
    SoundGone { name: String },
    /// The sound input is there but could not be opened.
    SoundSilent { name: String },
}

impl Missing {
    pub fn words(&self) -> String {
        use crate::tr;
        match self {
            Missing::CameraGone { n, label } => tr!("Camera {n} ({camera}) is not connected. Check its cable, or choose it again in Settings → Livestream → Cameras. The stream shows the slides in its place.", n = n, camera = label),
            Missing::CameraSilent { n, label } => tr!("Camera {n} ({camera}) is connected but sends no picture. Close any other program using it (Zoom, the Camera app, OBS), or switch it on. The stream shows the slides in its place until it comes.", n = n, camera = label),
            Missing::NoNdi { n, label } => tr!("Camera {n} ({camera}) comes over NDI, but NDI is not installed on this computer. Install the free NDI Tools.", n = n, camera = label),
            Missing::SoundGone { name } => tr!("The sound input {name} is not connected. Check its cable, or choose it again in Settings → Livestream → Sound. The stream goes out without it.", name = name),
            Missing::SoundSilent { name } => tr!("The sound input {name} is connected but could not be opened. Close any other program using it, then try again.", name = name),
        }
    }
}

/// FFmpeg asked for one picture from camera `name` (its usual input arguments), thrown away.
pub fn camera_probe_args(os: Os, name: &str) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin"].map(String::from).into();
    a.extend(args::camera_input(os, name));
    a.extend(["-frames:v", "1", "-f", "null", "-"].map(String::from));
    a
}

/// FFmpeg asked for half a second from sound input `name`, thrown away.
pub fn sound_probe_args(os: Os, name: &str) -> Vec<String> {
    let format = args::input_format(os);
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-f", format, "-i"].map(String::from).into();
    a.push(args::device_input(format, name));
    a.extend(["-t", "0.5", "-f", "null", "-"].map(String::from));
    a
}

/// What is missing, from the computer's lists (`cameras`, `inputs`) and whether each probe sent something
/// (`sent(i)`: camera `i`; `sent_sound`). Pure, for the tests; `check` does it for real.
pub fn judge(wanted: &Wanted, cameras: &[Input], inputs: &[Input], ndi: bool, sent: impl Fn(usize) -> bool, sent_sound: bool) -> Vec<Missing> {
    let mut out = vec![];
    for (i, name) in wanted.cameras.iter().enumerate() {
        let (n, label) = (i + 1, args::camera_label(name));
        if crate::stream::ndi::source_name(name).is_some() {
            if !ndi { out.push(Missing::NoNdi { n, label }) }
            continue;
        }
        // A camera on the network has no list to be in: it sends or it does not.
        let listed = args::is_network_camera(name) || cameras.iter().any(|c| c.name == *name);
        if !listed { out.push(Missing::CameraGone { n, label }) } else if !sent(i) { out.push(Missing::CameraSilent { n, label }) }
    }
    if let Some(name) = &wanted.sound {
        let label = inputs.iter().find(|i| i.name == *name).map(|i| i.label.clone());
        match label {
            None if !inputs.is_empty() => out.push(Missing::SoundGone { name: name.clone() }),
            Some(label) if !sent_sound => out.push(Missing::SoundSilent { name: label }),
            _ => {}
        }
    }
    out
}

/// Runs one probe: true when FFmpeg got what it was asked for within `LIMIT`.
fn probe(ffmpeg: &Path, a: &[String]) -> bool {
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(a).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; cmd.creation_flags(crate::platform::proc::creation_flags()); }
    let Ok(mut child) = cmd.spawn() else { return false };
    let began = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return s.success(),
            Ok(None) if began.elapsed() < LIMIT => std::thread::sleep(Duration::from_millis(50)),
            _ => { let _ = child.kill(); let _ = child.wait(); return false }
        }
    }
}

/// Looks for and opens everything `wanted`, all at once. Blocks (up to `LIMIT` and a little): off the UI thread.
pub fn check(ffmpeg: &Path, wanted: &Wanted) -> Vec<Missing> {
    if wanted.cameras.is_empty() && wanted.sound.is_none() { return vec![] }
    let ff: PathBuf = ffmpeg.to_path_buf();
    // Only what is not NDI is opened by FFmpeg.
    let probes: Vec<std::thread::JoinHandle<bool>> = wanted.cameras.iter().map(|name| {
        let (ff, name) = (ff.clone(), name.clone());
        std::thread::spawn(move || crate::stream::ndi::source_name(&name).is_none() && probe(&ff, &camera_probe_args(OS, &name)))
    }).collect();
    let sound = wanted.sound.clone().map(|name| { let ff = ff.clone(); std::thread::spawn(move || probe(&ff, &sound_probe_args(OS, &name))) });
    let cameras = if wanted.cameras.iter().any(|c| crate::stream::ndi::source_name(c).is_none() && !args::is_network_camera(c)) { crate::record::devices::cameras(ffmpeg) } else { vec![] };
    let inputs = if wanted.sound.is_some() { crate::record::devices::inputs(ffmpeg) } else { vec![] };
    let sent: Vec<bool> = probes.into_iter().map(|p| p.join().unwrap_or(false)).collect();
    let sent_sound = sound.is_some_and(|s| s.join().unwrap_or(false));
    judge(wanted, &cameras, &inputs, crate::stream::ndi::runtime().is_some(), |i| sent.get(i).copied().unwrap_or(false), sent_sound)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str) -> Input { Input { name: name.into(), label: name.into() } }

    #[test]
    fn what_is_missing_is_said_camera_by_camera() {
        let wanted = Wanted { cameras: vec!["Cam A".into(), "Cam B".into(), "rtsp://user:pw@10.0.0.9/s".into(), "ndi://STUDIO (Cam 4)".into()], sound: Some("Line In".into()) };
        let list = [input("Cam A"), input("Other")];
        let got = judge(&wanted, &list, &[input("Line In")], false, |i| i == 0, true);
        assert_eq!(got, [
            Missing::CameraGone { n: 2, label: "Cam B".into() },
            Missing::CameraSilent { n: 3, label: "Network camera 10.0.0.9".into() },
            Missing::NoNdi { n: 4, label: "NDI: STUDIO (Cam 4)".into() },
        ]);
        assert!(!got.iter().any(|m| m.words().contains("pw")), "never the camera's password");
        // Everything there and sending: nothing to say.
        let ok = Wanted { cameras: vec!["Cam A".into()], sound: Some("Line In".into()) };
        assert!(judge(&ok, &list, &[input("Line In")], true, |_| true, true).is_empty());
        // The sound input gone, or there but not opening.
        assert_eq!(judge(&ok, &list, &[input("Mic")], true, |_| true, true), [Missing::SoundGone { name: "Line In".into() }]);
        assert_eq!(judge(&ok, &list, &[input("Line In")], true, |_| true, false), [Missing::SoundSilent { name: "Line In".into() }]);
        // No list of inputs at all (it could not be asked): not said to be gone.
        assert!(judge(&ok, &list, &[], true, |_| true, true).is_empty());
        assert!(Missing::CameraSilent { n: 1, label: "Cam A".into() }.words().contains("Camera 1 (Cam A)"));
    }

    #[test]
    fn probes_ask_for_one_picture_or_half_a_second() {
        let c = camera_probe_args(Os::Windows, "Integrated Camera").join(" ");
        assert!(c.contains("-f dshow") && c.contains("-i video=Integrated Camera") && c.ends_with("-frames:v 1 -f null -"), "{c}");
        let s = sound_probe_args(Os::Windows, "Line In").join(" ");
        assert!(s.contains("-f dshow -i audio=Line In -t 0.5 -f null -"), "{s}");
    }

    #[test]
    fn a_camera_that_sends_is_found_for_real() {
        let Some(ff) = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).flat_map(|d| [d.join("ffmpeg.exe"), d.join("ffmpeg")]).find(|p| p.is_file())) else { return };
        // FFmpeg's own test picture stands in for a camera on the network (one that never answers: none).
        assert!(probe(&ff, &["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=blue:s=64x36", "-frames:v", "1", "-f", "null", "-"].map(String::from)));
        let began = Instant::now();
        assert!(!probe(&ff, &camera_probe_args(OS, "rtsp://127.0.0.1:1/none")));
        assert!(began.elapsed() < LIMIT + Duration::from_secs(2));
    }
}
