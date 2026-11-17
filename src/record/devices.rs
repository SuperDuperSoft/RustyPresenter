//! The cameras and sound inputs of this computer, and how a sound input is read. Windows and macOS: FFmpeg's own
//! lists (`args::parse_inputs`, `args::parse_cameras`). Linux:
//! - cameras from `/sys/class/video4linux`. A camera there often has a second device for information about its
//!   pictures, which FFmpeg cannot open; only the one with the pictures is listed, by its lasting name in
//!   `/dev/v4l/by-id` when it has one (`/dev/video0` can be another camera after a restart);
//! - sound inputs from FFmpeg's PulseAudio list, else from `pactl` (an FFmpeg built without PulseAudio lists none);
//! - a sound input is read by FFmpeg's PulseAudio input, else by `parec`. Both work with PipeWire too.
//!
//! macOS: the loopback device that stands in for the computer's own sound (`stream::pc_sound`).
use super::args::{self, Input};
use super::run::ask_ffmpeg;
use crate::platform::tools::{OS, Os};
use std::path::{Path, PathBuf};

/// The cameras and capture cards this computer can take pictures from.
pub fn cameras(ffmpeg: &Path) -> Vec<Input> {
    if OS == Os::Linux { return linux_cameras() }
    ask_ffmpeg(ffmpeg, &args::list_inputs_args(OS)).map(|out| args::parse_cameras(OS, &out)).unwrap_or_default()
}

/// The microphones and line-ins this computer can record from.
pub fn inputs(ffmpeg: &Path) -> Vec<Input> {
    let found = ask_ffmpeg(ffmpeg, &args::list_inputs_args(OS)).map(|out| args::parse_inputs(OS, &out)).unwrap_or_default();
    if !found.is_empty() || OS != Os::Linux { return found }
    let mut cmd = std::process::Command::new("pactl");
    // English words, whatever the computer's language: they are what is read.
    cmd.args(["list", "sources"]).env("LC_ALL", "C").stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    cmd.output().map(|o| parse_pactl_sources(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default()
}

/// What reads sound input `name` (FFmpeg input `format`) as `stream::board_feed::FORMAT` on its output: the
/// program and its arguments. FFmpeg, except on Linux when this FFmpeg has no PulseAudio and `parec` is there.
pub fn sound_reader(ffmpeg: &Path, format: &str, name: &str) -> (PathBuf, Vec<String>) {
    let ffmpegs = || (ffmpeg.to_path_buf(), crate::stream::board_feed::input_args(format, name));
    if format != "pulse" { return ffmpegs() }
    if ask_ffmpeg(ffmpeg, &["-hide_banner", "-devices"]).is_some_and(|out| has_input(&out, "pulse")) { return ffmpegs() }
    match on_path("parec") {
        Some(parec) => (parec, parec_args(name)),
        None => ffmpegs(),
    }
}

/// Whether `ffmpeg -devices` lists input `name` (` D  pulse   Pulse audio input`).
pub fn has_input(devices: &str, name: &str) -> bool {
    devices.lines().any(|l| {
        let mut words = l.split_whitespace();
        matches!((words.next(), words.next()), (Some(flags), Some(n)) if n == name && flags.starts_with('D') && flags.len() <= 3)
    })
}

/// `parec`'s arguments for source `name`: raw 16-bit stereo at 48 kHz, 20 ms at a time.
pub fn parec_args(name: &str) -> Vec<String> {
    let f = crate::stream::board_feed::FORMAT;
    vec![format!("--device={name}"), "--format=s16le".into(), format!("--rate={}", f.rate), format!("--channels={}", f.channels),
        "--raw".into(), "--latency-msec=20".into()]
}

/// The sound inputs in `LC_ALL=C pactl list sources`: each source's name and description. A source whose name ends
/// in `.monitor` is what an output plays, not an input.
pub fn parse_pactl_sources(out: &str) -> Vec<Input> {
    let mut found: Vec<Input> = vec![];
    let mut current: Option<Input> = None;
    let done = |i: Option<Input>, found: &mut Vec<Input>| {
        if let Some(i) = i.filter(|i| !i.name.is_empty() && !i.name.ends_with(".monitor")) {
            let label = if i.label.is_empty() { i.name.clone() } else { i.label };
            found.push(Input { name: i.name, label });
        }
    };
    for l in out.lines() {
        let t = l.trim();
        if t.starts_with("Source #") {
            done(current.take(), &mut found);
            current = Some(Input { name: String::new(), label: String::new() });
        } else if let Some(c) = current.as_mut() {
            if let Some(n) = t.strip_prefix("Name:") { c.name = n.trim().to_string() }
            else if let Some(d) = t.strip_prefix("Description:") { c.label = d.trim().to_string() }
        }
    }
    done(current, &mut found);
    found
}

/// A video device in `/sys/class/video4linux`.
#[derive(Clone, Debug, PartialEq)]
pub struct V4l {
    /// `video0`.
    pub node: String,
    /// What the device calls itself.
    pub name: String,
    /// Which of the camera's devices this is: 0 has the pictures. Missing: 0.
    pub index: Option<u32>,
    /// Its lasting name in `/dev/v4l/by-id`, when it has one.
    pub by_id: Option<String>,
}

/// The cameras among Linux's video devices, in the order of their nodes: the device with the pictures of each,
/// opened by its lasting name when it has one. Two cameras with the same name are told apart by their node.
pub fn v4l_cameras(mut all: Vec<V4l>) -> Vec<Input> {
    all.sort_by_key(|d| (d.node.trim_start_matches("video").parse::<u32>().unwrap_or(u32::MAX), d.node.clone()));
    all.retain(|d| d.index.unwrap_or(0) == 0);
    let label = |d: &V4l| if d.name.trim().is_empty() { d.node.clone() } else { d.name.trim().to_string() };
    all.iter().map(|d| {
        let same = all.iter().filter(|o| label(o) == label(d)).count() > 1;
        Input {
            name: d.by_id.clone().unwrap_or_else(|| format!("/dev/{}", d.node)),
            label: if same { format!("{} ({})", label(d), d.node) } else { label(d) },
        }
    }).collect()
}

/// What a Linux camera saved as `/dev/…` is called on the scene buttons: `/dev/v4l/by-id/usb-Logitech_C920_AB12-video-index0`
/// is "Logitech C920 AB12", `/dev/video2` "video2".
pub fn linux_camera_label(name: &str) -> String {
    let file = name.rsplit('/').next().unwrap_or(name);
    let file = file.strip_prefix("usb-").or_else(|| file.strip_prefix("pci-")).unwrap_or(file);
    let file = file.rsplit_once("-video-index").map_or(file, |(f, _)| f);
    file.replace('_', " ")
}

/// Linux's cameras, read from `/sys/class/video4linux` and `/dev/v4l/by-id`.
fn linux_cameras() -> Vec<Input> {
    let read = |p: PathBuf| std::fs::read_to_string(p).ok().map(|s| s.trim().to_string());
    let by_id: Vec<(PathBuf, String)> = std::fs::read_dir("/dev/v4l/by-id").map(|d| d.flatten().filter_map(|e| {
        Some((std::fs::canonicalize(e.path()).ok()?, e.path().to_string_lossy().into_owned()))
    }).collect()).unwrap_or_default();
    let all = std::fs::read_dir("/sys/class/video4linux").map(|d| d.flatten().map(|e| {
        let node = e.file_name().to_string_lossy().into_owned();
        let dev = PathBuf::from("/dev").join(&node);
        V4l {
            name: read(e.path().join("name")).unwrap_or_default(),
            index: read(e.path().join("index")).and_then(|i| i.parse().ok()),
            by_id: by_id.iter().find(|(target, _)| *target == dev).map(|(_, p)| p.clone()),
            node,
        }
    }).collect()).unwrap_or_default();
    v4l_cameras(all)
}

/// Where `program` is on the `PATH`.
fn on_path(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(program)).find(|p| p.is_file()))
}

/// Sound devices that pass what is played to them on as a sound input (lower case, letters and digits only).
const LOOPBACKS: [&str; 4] = ["blackhole", "soundflower", "loopback", "vbcable"];

fn squash(s: &str) -> String { s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect() }

/// macOS: the loopback device among sound `inputs` that carries what output `output` plays (mpv's name for it:
/// `coreaudio/BlackHole2ch_UID`; empty: the app's own output). The loopback device whose name is in the output's,
/// else the first loopback device. `None`: no loopback device is installed.
pub fn mac_loopback<'a>(output: &str, inputs: &'a [Input]) -> Option<&'a Input> {
    let out = squash(output.strip_prefix("coreaudio/").unwrap_or(output));
    let loopbacks: Vec<&Input> = inputs.iter().filter(|i| LOOPBACKS.iter().any(|l| squash(&i.name).contains(l))).collect();
    loopbacks.iter().find(|i| !out.is_empty() && out.contains(&squash(&i.name))).or(loopbacks.first()).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4l(node: &str, name: &str, index: Option<u32>, by_id: Option<&str>) -> V4l {
        V4l { node: node.into(), name: name.into(), index, by_id: by_id.map(String::from) }
    }

    #[test]
    fn linux_cameras_are_the_devices_with_pictures_by_their_lasting_names() {
        let all = vec![
            v4l("video10", "USB Capture HDMI", Some(0), None),
            v4l("video1", "Integrated Camera: Integrated C", Some(1), Some("/dev/v4l/by-id/usb-Chicony_Integrated_Camera-video-index1")),
            v4l("video0", "Integrated Camera: Integrated C", Some(0), Some("/dev/v4l/by-id/usb-Chicony_Integrated_Camera-video-index0")),
            v4l("video2", "HD Pro Webcam C920", None, None),
        ];
        let got = v4l_cameras(all);
        let names: Vec<&str> = got.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["/dev/v4l/by-id/usb-Chicony_Integrated_Camera-video-index0", "/dev/video2", "/dev/video10"]);
        assert_eq!(got[0].label, "Integrated Camera: Integrated C");
        // Two of the same camera: told apart.
        let twins = v4l_cameras(vec![v4l("video0", "C920", Some(0), None), v4l("video2", "C920", Some(0), None), v4l("video4", "", None, None)]);
        let labels: Vec<&str> = twins.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["C920 (video0)", "C920 (video2)", "video4"]);
    }

    #[test]
    fn a_linux_camera_has_a_readable_name_on_the_buttons() {
        assert_eq!(linux_camera_label("/dev/v4l/by-id/usb-046d_HD_Pro_Webcam_C920_AB12CD-video-index0"), "046d HD Pro Webcam C920 AB12CD");
        assert_eq!(linux_camera_label("/dev/video2"), "video2");
    }

    #[test]
    fn pactl_lists_the_inputs_without_the_monitors() {
        let out = "Source #0\n\tState: SUSPENDED\n\tName: alsa_output.pci-0000_00_1f.3.analog-stereo.monitor\n\tDescription: Monitor of Built-in Audio\n\
            \tProperties:\n\t\tdevice.description = \"Monitor of Built-in Audio\"\n\n\
            Source #1\n\tName: alsa_input.usb-Behringer_UMC202HD-00.analog-stereo\n\tDescription: UMC202HD 192k Analog Stereo\n\n\
            Source #2\n\tName: alsa_input.pci-0000_00_1f.3.analog-stereo\n";
        let got = parse_pactl_sources(out);
        assert_eq!(got, [
            Input { name: "alsa_input.usb-Behringer_UMC202HD-00.analog-stereo".into(), label: "UMC202HD 192k Analog Stereo".into() },
            Input { name: "alsa_input.pci-0000_00_1f.3.analog-stereo".into(), label: "alsa_input.pci-0000_00_1f.3.analog-stereo".into() },
        ]);
        assert!(parse_pactl_sources("").is_empty());
    }

    #[test]
    fn whether_ffmpeg_has_pulseaudio_and_how_parec_reads_instead() {
        let with = "Devices:\n D. = Demuxing supported\n .E = Muxing supported\n ---\n DE alsa            ALSA audio output\n D  lavfi           Libavfilter virtual input device\n D  pulse           Pulse audio input\n  E sdl,sdl2        SDL2 output device\n D  v4l2            Video4Linux2 device grab\n";
        assert!(has_input(with, "pulse") && has_input(with, "v4l2") && has_input(with, "alsa"));
        assert!(!has_input(with, "sdl,sdl2"), "output only");
        let without = "Devices:\n D. = Demuxing supported\n ---\n D  lavfi           Libavfilter virtual input device\n D  v4l2            Video4Linux2\n";
        assert!(!has_input(without, "pulse"));
        assert_eq!(parec_args("x.monitor").join(" "), "--device=x.monitor --format=s16le --rate=48000 --channels=2 --raw --latency-msec=20");
        // Not PulseAudio: FFmpeg, as always, without asking it anything.
        let (program, a) = sound_reader(Path::new("no-such-ffmpeg"), "dshow", "Line In");
        assert_eq!((program, a), (PathBuf::from("no-such-ffmpeg"), crate::stream::board_feed::input_args("dshow", "Line In")));
    }

    #[test]
    fn a_mac_reads_the_loopback_device_that_carries_the_output() {
        let i = |n: &str| Input { name: n.into(), label: n.into() };
        let inputs = [i("MacBook Pro Microphone"), i("BlackHole 16ch"), i("BlackHole 2ch"), i("UMC202HD")];
        assert_eq!(mac_loopback("coreaudio/BlackHole2ch_UID", &inputs).map(|i| i.name.as_str()), Some("BlackHole 2ch"));
        // A Multi-Output Device, or the app's own output: the first loopback device.
        assert_eq!(mac_loopback("coreaudio/~:AMS2_StackedOutput:0", &inputs).map(|i| i.name.as_str()), Some("BlackHole 16ch"));
        assert_eq!(mac_loopback("", &inputs).map(|i| i.name.as_str()), Some("BlackHole 16ch"));
        // A real input whose name is in the output's is never taken for the computer's sound.
        assert_eq!(mac_loopback("coreaudio/AppleUSBAudioEngine:UMC202HD", &inputs[3..]), None);
        assert_eq!(mac_loopback("", &[i("Soundflower (2ch)")]).map(|i| i.name.as_str()), Some("Soundflower (2ch)"));
    }
}
