//! What `ffmpeg` is asked to do, as plain argument lists: recording (`record`), making the MP4 (`finish`), and
//! listing the sound inputs (`list_inputs_args`, `parse_inputs`). All pure, so all tested here.
use super::{Preset, sound::Segment};
use crate::platform::tools::Os;
use std::ffi::OsString;
use std::path::Path;

/// The H.264 encoders we can use, best first: x264 (in every full FFmpeg), then Windows' own (Media Foundation),
/// then Cisco's.
pub const ENCODERS: [&str; 3] = ["libx264", "h264_mf", "libopenh264"];

/// The best H.264 encoder in `ffmpeg -encoders`'s list.
pub fn pick_encoder(list: &str) -> Option<&'static str> {
    let has = |name: &str| list.lines().any(|l| l.split_whitespace().nth(1) == Some(name) && l.trim_start().starts_with('V'));
    ENCODERS.into_iter().find(|e| has(e))
}

/// A microphone or line-in: `name` is what FFmpeg opens, `label` what the operator reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub name: String,
    pub label: String,
}

/// The FFmpeg input format for sound inputs on `os`.
pub fn input_format(os: Os) -> &'static str {
    match os { Os::Windows => "dshow", Os::MacOs => "avfoundation", Os::Linux => "pulse" }
}

/// The arguments that make FFmpeg list `os`'s sound inputs (it prints them and stops with an error).
pub fn list_inputs_args(os: Os) -> Vec<&'static str> {
    match os {
        Os::Windows => vec!["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"],
        Os::MacOs => vec!["-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""],
        Os::Linux => vec!["-hide_banner", "-sources", "pulse"],
    }
}

/// The sound inputs in what `list_inputs_args` printed (both streams together).
pub fn parse_inputs(os: Os, out: &str) -> Vec<Input> {
    let mut found: Vec<Input> = Vec::new();
    let mut push = |name: &str, label: &str| {
        if !name.is_empty() && !found.iter().any(|i| i.name == name) { found.push(Input { name: name.into(), label: label.into() }) }
    };
    match os {
        Os::Windows => {
            // New FFmpeg: `[dshow @ 0…] "Microphone (USB)" (audio)`. Old FFmpeg: the names after "DirectShow audio
            // devices", up to the video ones. "Alternative name" lines are the same device again.
            let mut audio_section = false;
            for l in out.lines() {
                if l.contains("DirectShow audio devices") { audio_section = true; continue }
                if l.contains("DirectShow video devices") { audio_section = false; continue }
                if l.contains("Alternative name") { continue }
                let Some(name) = quoted(l) else { continue };
                if l.trim_end().ends_with("(audio)") || (audio_section && !l.contains("(video)") && !l.contains("(none)")) { push(name, name) }
            }
        }
        Os::MacOs => {
            let mut audio_section = false;
            for l in out.lines() {
                if l.contains("AVFoundation audio devices") { audio_section = true; continue }
                if l.contains("AVFoundation video devices") { audio_section = false; continue }
                if !audio_section { continue }
                // `[AVFoundation indev @ 0x…] [0] MacBook Pro Microphone`
                let Some(rest) = l.rsplit_once("] [").map(|(_, r)| r) else { continue };
                if let Some((_, name)) = rest.split_once("] ") { push(name.trim(), name.trim()) }
            }
        }
        Os::Linux => {
            // `* alsa_input.usb-… [USB Microphone]`; a `.monitor` is what a speaker plays, which is not an input.
            for l in out.lines() {
                let l = l.trim_start_matches(['*', ' ']);
                let Some((name, label)) = l.split_once(" [") else { continue };
                if name.contains(' ') || name.ends_with(".monitor") || !label.ends_with(']') { continue }
                push(name, label.trim_end_matches(']'));
            }
        }
    }
    found
}

/// The cameras (and capture cards) in what `list_inputs_args` printed. Windows and macOS; Linux has none yet.
pub fn parse_cameras(os: Os, out: &str) -> Vec<Input> {
    let mut found: Vec<Input> = Vec::new();
    let mut push = |name: &str| {
        if !name.is_empty() && !found.iter().any(|i| i.name == name) { found.push(Input { name: name.into(), label: name.into() }) }
    };
    match os {
        Os::Windows => {
            let mut video_section = false;
            for l in out.lines() {
                if l.contains("DirectShow video devices") { video_section = true; continue }
                if l.contains("DirectShow audio devices") { video_section = false; continue }
                if l.contains("Alternative name") { continue }
                let Some(name) = quoted(l) else { continue };
                if l.trim_end().ends_with("(video)") || (video_section && !l.contains("(audio)") && !l.contains("(none)")) { push(name) }
            }
        }
        Os::MacOs => {
            let mut video_section = false;
            for l in out.lines() {
                if l.contains("AVFoundation video devices") { video_section = true; continue }
                if l.contains("AVFoundation audio devices") { video_section = false; continue }
                if !video_section { continue }
                let Some(rest) = l.rsplit_once("] [").map(|(_, r)| r) else { continue };
                if let Some((_, name)) = rest.split_once("] ") { push(name.trim()) }
            }
        }
        Os::Linux => {}
    }
    found
}

/// FFmpeg's input arguments for camera `name` on `os`.
pub fn camera_input(os: Os, name: &str) -> Vec<String> {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if is_network_camera(name) {
        let address = name.trim();
        // RTSP over TCP: over UDP a busy network loses pieces of pictures. A camera that does not answer in 5 s
        // is given up on, and the camera feed tries again.
        let rtsp = address.to_ascii_lowercase().starts_with("rtsp");
        let how: &[&str] = if rtsp { &["-rtsp_transport", "tcp", "-timeout", "5000000"] } else { &[] };
        return [v(how), v(&["-thread_queue_size", "64", "-i"]), vec![address.to_string()]].concat();
    }
    match os {
        // A camera's pictures wait in a bigger buffer than DirectShow's own, so a busy moment drops none.
        Os::Windows => [v(&["-f", "dshow", "-rtbufsize", "100M", "-thread_queue_size", "64", "-i"]), vec![format!("video={name}")]].concat(),
        Os::MacOs => [v(&["-f", "avfoundation", "-framerate", "30", "-thread_queue_size", "64", "-i"]), vec![format!("{name}:none")]].concat(),
        Os::Linux => [v(&["-f", "v4l2", "-thread_queue_size", "64", "-i"]), vec![name.to_string()]].concat(),
    }
}

/// The kinds of address a camera on the network is reached at.
const NETWORK: [&str; 10] = ["rtsp://", "rtsps://", "srt://", "rtmp://", "rtmps://", "http://", "https://", "udp://", "rtp://", "tcp://"];

/// Whether camera `name` is a camera on the network, given by its address (`rtsp://…`, `srt://…`).
pub fn is_network_camera(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    NETWORK.iter().any(|s| n.starts_with(s) && n.len() > s.len())
}

/// What camera `name` is called on the scene button: a camera on the network by where it is, never with the
/// password its address may hold.
pub fn camera_label(name: &str) -> String {
    if !is_network_camera(name) { return name.to_string() }
    let rest = name.trim().split_once("://").map_or("", |(_, r)| r);
    let place = &rest[..rest.find(['/', '?']).unwrap_or(rest.len())];
    let place = place.rsplit_once('@').map_or(place, |(_, p)| p);
    let host = if place.starts_with('[') { &place[..place.find(']').map_or(place.len(), |i| i + 1)] } else { place.split(':').next().unwrap_or(place) };
    crate::tr!("Network camera {host}", host = host)
}

fn quoted(l: &str) -> Option<&str> {
    let a = l.find('"')?;
    let b = l[a + 1..].find('"')? + a + 1;
    Some(&l[a + 1..b])
}

/// What FFmpeg's `-i` needs to open sound input `name` with `format` (dshow wants `audio=…`, avfoundation `:…`).
pub fn device_input(format: &str, name: &str) -> String {
    match format { "dshow" => format!("audio={name}"), "avfoundation" => format!(":{name}"), _ => name.to_string() }
}

/// One recording's FFmpeg run: raw RGBA pictures on its input, H.264 (and the input's sound) into Matroska, which
/// is readable even if the app or the computer stops halfway.
pub struct Record<'a> {
    pub size: (u32, u32),
    pub fps: u32,
    pub preset: Preset,
    pub encoder: &'a str,
    /// A sound input to record with the picture: `(format, name)`.
    pub input: Option<(&'a str, &'a str)>,
    /// `input` passed through the app (`stream::board_feed`), read from there instead of from the device.
    pub board: Option<crate::stream::pc_sound::Feed>,
    pub out: &'a Path,
    /// Where to send the same picture and sound as a livestream (`stream::ready`); empty: only the file.
    pub stream: &'a [String],
    /// The PC's own sound, captured as it plays (`stream::pc_sound`), mixed with `input` when there is one.
    pub pc: Option<crate::stream::pc_sound::Feed>,
    /// What is done to the stream's sound (Settings): held back, turned up or down, cleaned, evened out.
    pub fx: SoundFx,
}

/// What is done to a livestream's sound, from Settings (`RecordPrefs::sound_fx`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SoundFx {
    /// The sound held back this long, so it lines up with a camera whose picture comes late.
    pub delay_ms: u32,
    /// The microphone or line-in turned up (or down, below 0) this many dB.
    pub board_db: i32,
    /// The computer's own sound turned up or down this many dB.
    pub pc_db: i32,
    /// Loud and quiet moments evened out, and nothing ever too loud.
    pub even: bool,
    /// Steady hum and hiss taken out of the microphone or line-in.
    pub quiet_noise: bool,
}

/// Evens the sound out: what is louder than about -18 dB is brought down (3 to 1), all of it brought up 6 dB, and
/// nothing goes over -1 dB.
pub const EVEN_OUT: &str = "acompressor=threshold=0.125:ratio=3:attack=20:release=250:makeup=2,alimiter=limit=0.89:level=disabled";

impl SoundFx {
    /// The filters on the microphone or line-in alone: hum and hiss out, then its level.
    fn board(&self) -> Vec<String> {
        let mut f = vec![];
        if self.quiet_noise { f.push("afftdn=nf=-25".to_string()) }
        if self.board_db != 0 { f.push(format!("volume={}dB", self.board_db)) }
        f
    }

    /// The filters on the computer's own sound alone: its level.
    fn pc(&self) -> Vec<String> { if self.pc_db != 0 { vec![format!("volume={}dB", self.pc_db)] } else { vec![] } }

    /// The filters on all of the sound: held back for the camera, then evened out.
    fn all(&self) -> Vec<String> {
        let mut f = vec![];
        if self.delay_ms > 0 { f.push(format!("adelay={}:all=1", self.delay_ms)) }
        if self.even { f.push(EVEN_OUT.to_string()) }
        f
    }

    /// `-af` with `alone`'s filters and then those on all of the sound; nothing when there are none.
    fn one(&self, alone: Vec<String>) -> Vec<String> {
        let f = [alone, self.all()].concat();
        if f.is_empty() { vec![] } else { vec!["-af".into(), f.join(",")] }
    }

    /// The `-filter_complex` mixing input 1 (the microphone or line-in) and 2 (the computer's sound) into `[a]`.
    fn mix(&self) -> String {
        let (board, pc) = (self.board(), self.pc());
        let mut graph = String::new();
        let mut label = |n: u8, f: Vec<String>, name: &str| if f.is_empty() { format!("[{n}:a]") } else {
            graph.push_str(&format!("[{n}:a]{}[{name}];", f.join(",")));
            format!("[{name}]")
        };
        let (b, p) = (label(1, board, "b"), label(2, pc, "p"));
        let all: String = self.all().iter().map(|f| format!(",{f}")).collect();
        format!("{graph}{b}{p}amix=inputs=2:duration=shortest:normalize=0{all}[a]")
    }
}

pub fn record(r: &Record) -> Vec<OsString> {
    let mut a: Vec<OsString> = Vec::new();
    let mut push = |v: &[&str]| a.extend(v.iter().map(OsString::from));
    let (w, h) = r.size;
    let (size, fps, kbps) = (format!("{w}x{h}"), r.fps.to_string(), r.preset.max_kbps());
    push(&["-hide_banner", "-loglevel", "warning", "-y"]);
    push(&["-f", "rawvideo", "-pix_fmt", "rgba", "-video_size", &size, "-framerate", &fps, "-thread_queue_size", "8", "-i", "pipe:0"]);
    if let Some((format, name)) = r.input {
        match r.board {
            Some(board) => { let b = board.input_args(); push(&b.iter().map(String::as_str).collect::<Vec<_>>()) }
            None => {
                let device = device_input(format, name);
                // dshow holds half a second of sound by default; 50 ms keeps it close to the picture.
                if format == "dshow" { push(&["-audio_buffer_size", "50"]) }
                push(&["-thread_queue_size", "1024", "-f", format, "-i", &device]);
            }
        }
        match r.pc {
            Some(pc) => {
                let pc = pc.input_args();
                push(&pc.iter().map(String::as_str).collect::<Vec<_>>());
                // The sound board and the PC's sound together, neither turned down. The output ends with the
                // picture: the PC's sound goes on until it is stopped.
                let mix = r.fx.mix();
                push(&["-filter_complex", &mix, "-map", "0:v", "-map", "[a]", "-shortest", "-shortest_buf_duration", "0.5"]);
            }
            None => {
                push(&["-map", "0:v", "-map", "1:a"]);
                push(&r.fx.one(r.fx.board()).iter().map(String::as_str).collect::<Vec<_>>());
            }
        }
    } else if let Some(pc) = r.pc {
        let pc = pc.input_args();
        push(&pc.iter().map(String::as_str).collect::<Vec<_>>());
        // `-shortest` alone holds up to ten seconds of sound back to line it up: FFmpeg stopped reading the PC's
        // sound after 0.7 s, its capture blocked, and what played then was lost (tried November 14, 2026).
        push(&["-map", "0:v", "-map", "1:a", "-shortest", "-shortest_buf_duration", "0.5"]);
        push(&r.fx.one(r.fx.pc()).iter().map(String::as_str).collect::<Vec<_>>());
    } else if !r.stream.is_empty() {
        // Streaming services refuse a stream with no sound: silence stands in. It has no end of its own, so
        // the output ends with the picture (`-shortest`), and it may run only half a second ahead of the
        // picture. Left alone it ran ten seconds ahead, and a sender that joined the feed in the middle (after
        // a lost connection) got nothing out of it. (Made at the clock's pace with `-re` instead, FFmpeg
        // stalled for eleven seconds at the start: tried on November 14, 2026.)
        push(&["-f", "lavfi", "-i", "anullsrc=channel_layout=stereo:sample_rate=48000"]);
        push(&["-map", "0:v", "-map", "1:a", "-shortest", "-shortest_buf_duration", "0.5"]);
    } else {
        push(&["-map", "0:v"]);
    }
    let (rate, buf, gop) = (format!("{kbps}k"), format!("{}k", kbps * 2), (r.fps * 2).to_string());
    match r.encoder {
        // veryfast: little CPU; slides cost almost nothing to encode, a video gets up to the preset's rate.
        "libx264" => push(&["-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-maxrate", &rate, "-bufsize", &buf]),
        e => push(&["-c:v", e, "-b:v", &rate]),
    }
    // A keyframe every 2 s, so a file cut short by a crash loses at most that much.
    push(&["-g", &gop, "-pix_fmt", "yuv420p"]);
    if r.input.is_some() || r.pc.is_some() || !r.stream.is_empty() { push(&["-c:a", "aac", "-b:a", "160k"]) }
    if r.stream.is_empty() {
        push(&["-f", "matroska"]);
        a.push(r.out.into());
    } else {
        // One encode, many outputs: the file and every destination (`stream::tee`). The outputs that are sent
        // need the encoder's headers up front.
        push(&["-flags", "+global_header", "-f", "tee"]);
        a.push(crate::stream::tee(r.out, r.stream).into());
    }
    a
}

/// Turns the Matroska file `part` into the MP4 `out`, the picture copied as it is. `sound` is the app's sound to
/// mix in from its files; without it the part's own sound (a microphone, or none) is copied.
pub fn finish(part: &Path, sound: &[Segment], out: &Path) -> Vec<OsString> {
    let mut a: Vec<OsString> = ["-hide_banner", "-loglevel", "warning", "-y", "-i"].map(OsString::from).to_vec();
    a.push(part.into());
    if sound.is_empty() {
        a.extend(["-map", "0", "-c", "copy"].map(OsString::from));
    } else {
        for s in sound {
            a.extend(["-ss".into(), secs(s.from).into(), "-t".into(), secs(s.to - s.from).into(), OsString::from("-i"), s.path.clone().into_os_string()]);
        }
        a.extend(["-filter_complex".into(), mix(sound).into(), OsString::from("-map"), "0:v".into(), "-map".into(), "[aout]".into()]);
        a.extend(["-c:v", "copy", "-c:a", "aac", "-b:a", "160k", "-shortest"].map(OsString::from));
    }
    a.extend(["-movflags", "+faststart", "-f", "mp4"].map(OsString::from));
    a.push(out.into());
    a
}

/// The filter that puts each stretch at its time and adds them up, padded with silence to the picture's end.
pub fn mix(sound: &[Segment]) -> String {
    let mut f: Vec<String> = sound.iter().enumerate().map(|(i, s)| {
        let delay = (s.at * 1000.0).round().max(0.0) as u64;
        format!("[{}:a]aresample=48000,volume={:.3},adelay={delay}:all=1[s{i}]", i + 1, s.volume)
    }).collect();
    let ins: String = (0..sound.len()).map(|i| format!("[s{i}]")).collect();
    f.push(if sound.len() == 1 { format!("{ins}apad[aout]") } else { format!("{ins}amix=inputs={}:duration=longest:dropout_transition=0:normalize=0,apad[aout]", sound.len()) });
    f.join(";")
}

fn secs(s: f64) -> String { format!("{:.3}", s.max(0.0)) }

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn strs(a: &[OsString]) -> Vec<String> { a.iter().map(|s| s.to_string_lossy().into_owned()).collect() }

    fn after<'a>(a: &'a [String], flag: &str) -> &'a str {
        let i = a.iter().position(|x| x == flag).unwrap_or_else(|| panic!("{flag} not in {a:?}"));
        &a[i + 1]
    }

    const ENCODERS_OUT: &str = " V....D libx264rgb           libx264 H.264 RGB\n V....D h264_mf              H264 via MediaFoundation (codec h264)\n V....D libx264              libx264 H.264 / AVC\n A....D aac                  AAC";

    #[test]
    fn x264_is_preferred_then_windows_own() {
        assert_eq!(pick_encoder(ENCODERS_OUT), Some("libx264"));
        assert_eq!(pick_encoder(" V....D h264_mf  H264 via MediaFoundation\n V....D libx264rgb  x"), Some("h264_mf"));
        assert_eq!(pick_encoder(" V....D libx264rgb  x\n A....D libx264  not a video one"), None);
    }

    #[test]
    fn streaming_sends_the_recording_to_every_destination_too() {
        let out = PathBuf::from("/r/Recording.recording.mkv");
        let to = ["rtmp://a.rtmp.youtube.com/live2/key".to_string(), "rtmps://live-api-s.facebook.com:443/rtmp/key2".to_string()];
        // With a sound input: one encode, the file and both destinations.
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: Some(("dshow", "Line In")), board: None, out: &out, stream: &to, pc: None, fx: SoundFx::default() }));
        assert_eq!(a[a.len() - 5..a.len() - 1], ["-flags", "+global_header", "-f", "tee"].map(String::from));
        assert_eq!(a.last().unwrap(), "[f=matroska]/r/Recording.recording.mkv|[f=flv:onfail=ignore]rtmp://a.rtmp.youtube.com/live2/key|[f=flv:onfail=ignore]rtmps://live-api-s.facebook.com:443/rtmp/key2");
        assert_eq!(after(&a, "-c:a"), "aac");
        assert!(!a.contains(&"anullsrc=channel_layout=stereo:sample_rate=48000".to_string()));
        // Without one: silence stands in (a service refuses a stream with no sound), and it ends with the picture.
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: None, board: None, out: &out, stream: &to[..1], pc: None, fx: SoundFx::default() }));
        assert!(a.windows(4).any(|w| w == ["-f", "lavfi", "-i", "anullsrc=channel_layout=stereo:sample_rate=48000"]), "{a:?}");
        assert_eq!(after(&a, "-shortest_buf_duration"), "0.5");
        assert!(a.contains(&"-shortest".to_string()) && after(&a, "-c:a") == "aac");
        assert_eq!(after(&a, "-g"), "60", "a keyframe every 2 s, as the services ask");
    }

    #[test]
    fn the_pcs_sound_goes_in_alone_or_mixed_with_the_sound_board() {
        use crate::stream::pc_sound::{Feed, Format};
        let out = PathBuf::from("/r/Recording.recording.mkv");
        let to = ["rtmp://a.rtmp.youtube.com/live2/key".to_string()];
        let pc = Some(Feed { format: Format { samples: "f32le", rate: 48000, channels: 2 }, port: 40500 });
        // Alone: it is the sound, and there is no stand-in silence.
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: None, board: None, out: &out, stream: &to, pc, fx: SoundFx::default() }));
        assert!(a.windows(2).any(|w| w == ["-i", "tcp://127.0.0.1:40500?listen=1"]) && after(&a, "-ar") == "48000", "{a:?}");
        assert!(a.windows(4).any(|w| w == ["-map", "0:v", "-map", "1:a"]) && after(&a, "-shortest_buf_duration") == "0.5");
        assert!(!a.iter().any(|x| x.starts_with("anullsrc")), "{a:?}");
        // With the sound board: both, mixed.
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: Some(("dshow", "Line In")), board: None, out: &out, stream: &to, pc, fx: SoundFx::default() }));
        assert_eq!(after(&a, "-filter_complex"), "[1:a][2:a]amix=inputs=2:duration=shortest:normalize=0[a]");
        assert!(a.windows(2).any(|w| w == ["-map", "[a]"]) && after(&a, "-c:a") == "aac");
    }

    #[test]
    fn the_sound_can_wait_for_the_camera() {
        use crate::stream::pc_sound::{Feed, Format};
        let out = PathBuf::from("/r/Recording.recording.mkv");
        let to = ["rtmp://a.rtmp.youtube.com/live2/key".to_string()];
        let rec = |input, pc, delay_ms| strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input, board: None, out: &out, stream: &to, pc, fx: SoundFx { delay_ms, ..SoundFx::default() } }));
        let pc = Some(Feed { format: Format { samples: "f32le", rate: 48000, channels: 2 }, port: 40500 });
        // One sound: a filter on it.
        assert_eq!(after(&rec(Some(("dshow", "Line In")), None, 300), "-af"), "adelay=300:all=1");
        assert_eq!(after(&rec(None, pc, 300), "-af"), "adelay=300:all=1");
        // Two, mixed: the mix is delayed.
        assert_eq!(after(&rec(Some(("dshow", "Line In")), pc, 300), "-filter_complex"), "[1:a][2:a]amix=inputs=2:duration=shortest:normalize=0,adelay=300:all=1[a]");
        // No delay, or only stand-in silence: nothing added.
        assert!(!rec(Some(("dshow", "Line In")), None, 0).iter().any(|x| x.contains("adelay")));
        assert!(!rec(None, None, 300).iter().any(|x| x.contains("adelay")));
    }

    #[test]
    fn the_sound_can_be_turned_up_cleaned_and_evened_out() {
        use crate::stream::pc_sound::{Feed, Format};
        let out = PathBuf::from("/r/Recording.recording.mkv");
        let to = ["rtmp://a.rtmp.youtube.com/live2/key".to_string()];
        let rec = |input, pc, fx| strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input, board: None, out: &out, stream: &to, pc, fx }));
        let pc = Some(Feed { format: Format { samples: "f32le", rate: 48000, channels: 2 }, port: 40500 });
        let line = Some(("dshow", "Line In"));
        let fx = SoundFx { delay_ms: 0, board_db: 6, pc_db: -4, even: true, quiet_noise: true };
        // One sound: its level, the noise taken out of the microphone (never out of the PC's own sound), evened out.
        assert_eq!(after(&rec(line, None, fx), "-af"), format!("afftdn=nf=-25,volume=6dB,{EVEN_OUT}"));
        assert_eq!(after(&rec(None, pc, fx), "-af"), format!("volume=-4dB,{EVEN_OUT}"));
        // Two: each its own level before the mix; the mix waits for the camera, then is evened out.
        assert_eq!(after(&rec(line, pc, SoundFx { delay_ms: 300, ..fx }), "-filter_complex"),
            format!("[1:a]afftdn=nf=-25,volume=6dB[b];[2:a]volume=-4dB[p];[b][p]amix=inputs=2:duration=shortest:normalize=0,adelay=300:all=1,{EVEN_OUT}[a]"));
        assert_eq!(after(&rec(line, pc, SoundFx { even: true, ..SoundFx::default() }), "-filter_complex"),
            format!("[1:a][2:a]amix=inputs=2:duration=shortest:normalize=0,{EVEN_OUT}[a]"));
        // Nothing asked for: nothing added. Stand-in silence is left alone.
        let plain = rec(line, pc, SoundFx::default()).join(" ");
        assert!(!plain.contains("volume") && !plain.contains("acompressor") && !plain.contains("afftdn"), "{plain}");
        assert!(!rec(None, None, fx).iter().any(|x| x.contains("volume") || x.contains("acompressor")));
    }

    #[test]
    fn the_sound_input_comes_through_the_app_when_it_can() {
        use crate::stream::pc_sound::{Feed, Format};
        let out = PathBuf::from("/r/Recording.recording.mkv");
        let board = Some(Feed { format: Format { samples: "s16le", rate: 48000, channels: 2 }, port: 40700 });
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: Some(("dshow", "Line In")), board, out: &out, stream: &[], pc: None, fx: SoundFx::default() }));
        assert!(a.join(" ").contains("-f s16le -ar 48000 -ac 2 -thread_queue_size 1024 -i tcp://127.0.0.1:40700?listen=1 -map 0:v -map 1:a"), "{a:?}");
        assert!(!a.iter().any(|x| x.starts_with("audio=")), "not the device itself");
    }

    #[test]
    fn a_network_camera_is_opened_by_its_address() {
        let address = "rtsp://admin:secret@192.168.1.50:554/stream1";
        let a = camera_input(Os::Windows, address);
        assert!(a.join(" ").contains("-rtsp_transport tcp"), "{a:?}");
        assert_eq!(a.last().unwrap(), address);
        assert!(!a.iter().any(|x| x == "dshow" || x.starts_with("video=")), "{a:?}");
        let a = camera_input(Os::Linux, "srt://192.168.1.60:9000");
        assert_eq!(a.last().unwrap(), "srt://192.168.1.60:9000");
        assert!(!a.iter().any(|x| x == "v4l2" || x == "-rtsp_transport"), "{a:?}");
        assert!(is_network_camera("RTSP://cam") && is_network_camera("http://10.0.0.2/video"));
        assert!(!is_network_camera("Integrated Camera") && !is_network_camera(""));
        // Named by where it is, never with its password.
        assert_eq!(camera_label(address), "Network camera 192.168.1.50");
        assert_eq!(camera_label("srt://cam.local:9000?streamid=x"), "Network camera cam.local");
        assert_eq!(camera_label("Integrated Camera"), "Integrated Camera");
    }

    #[test]
    fn cameras_are_listed() {
        let out = "[dshow @ 0] \"Integrated Camera\" (video)\n[dshow @ 0]   Alternative name \"@device_pnp_x\"\n[dshow @ 0] \"Microphone (USB)\" (audio)\n[dshow @ 0] \"OBS Virtual Camera\" (video)";
        let cams: Vec<String> = parse_cameras(Os::Windows, out).into_iter().map(|c| c.name).collect();
        assert_eq!(cams, ["Integrated Camera", "OBS Virtual Camera"]);
        assert_eq!(parse_inputs(Os::Windows, out).len(), 1, "a camera is no sound input");
        assert_eq!(camera_input(Os::Windows, "Integrated Camera").last().unwrap(), "video=Integrated Camera");
    }

    #[test]
    fn recording_reads_raw_frames_and_writes_h264_into_matroska() {
        let out = PathBuf::from("/r/Recording.recording.mkv");
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: None, board: None, out: &out, stream: &[], pc: None, fx: SoundFx::default() }));
        assert_eq!(after(&a, "-f"), "rawvideo");
        assert_eq!(after(&a, "-pix_fmt"), "rgba");
        assert_eq!(after(&a, "-video_size"), "1280x720");
        assert_eq!(after(&a, "-framerate"), "30");
        assert_eq!(after(&a, "-i"), "pipe:0");
        assert_eq!(after(&a, "-c:v"), "libx264");
        assert_eq!(after(&a, "-preset"), "veryfast");
        assert_eq!(after(&a, "-maxrate"), "3000k");
        assert_eq!(after(&a, "-g"), "60");
        assert_eq!(a.iter().filter(|x| *x == "-i").count(), 1, "no sound input");
        assert!(!a.contains(&"-c:a".to_string()));
        assert_eq!(a[a.len() - 2], "matroska");
        assert_eq!(a.last().unwrap(), "/r/Recording.recording.mkv");
    }

    #[test]
    fn a_microphone_is_a_second_input_per_system() {
        let out = PathBuf::from("o.mkv");
        let with = |format, name| strs(&record(&Record { size: (1920, 1080), fps: 30, preset: Preset::P1080, encoder: "libx264", input: Some((format, name)), board: None, out: &out, stream: &[], pc: None, fx: SoundFx::default() }));
        let win = with("dshow", "Microphone (USB Audio)");
        assert!(win.windows(4).any(|w| w == ["-f", "dshow", "-i", "audio=Microphone (USB Audio)"]), "{win:?}");
        assert!(win.windows(4).any(|w| w == ["-map", "0:v", "-map", "1:a"]), "{win:?}");
        assert_eq!(after(&win, "-c:a"), "aac");
        assert_eq!(after(&win, "-maxrate"), "6000k");
        assert!(with("avfoundation", "MacBook Pro Microphone").contains(&":MacBook Pro Microphone".to_string()));
        assert!(with("pulse", "alsa_input.usb").windows(2).any(|w| w == ["pulse", "-i"]));
    }

    #[test]
    fn other_encoders_get_a_plain_rate() {
        let out = PathBuf::from("o.mkv");
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "h264_mf", input: None, board: None, out: &out, stream: &[], pc: None, fx: SoundFx::default() }));
        assert_eq!(after(&a, "-c:v"), "h264_mf");
        assert_eq!(after(&a, "-b:v"), "3000k");
        assert!(!a.contains(&"-crf".to_string()));
    }

    #[test]
    fn finishing_without_app_sound_copies_everything() {
        let a = strs(&finish(Path::new("p.mkv"), &[], Path::new("o.mp4.tmp")));
        assert_eq!(after(&a, "-i"), "p.mkv");
        assert_eq!(after(&a, "-c"), "copy");
        assert_eq!(after(&a, "-movflags"), "+faststart");
        assert_eq!(after(&a, "-f"), "mp4");
        assert_eq!(a.last().unwrap(), "o.mp4.tmp");
    }

    #[test]
    fn finishing_with_app_sound_cuts_each_stretch_from_its_file() {
        let s = vec![
            Segment { path: "/m/song.mp3".into(), from: 0.0, to: 10.0, at: 2.5, volume: 1.0 },
            Segment { path: "/m/clip.mp4".into(), from: 31.25, to: 40.0, at: 20.0, volume: 0.5 },
        ];
        let a = strs(&finish(Path::new("p.mkv"), &s, Path::new("o.mp4.tmp")));
        assert!(a.windows(6).any(|w| w == ["-ss", "0.000", "-t", "10.000", "-i", "/m/song.mp3"]), "{a:?}");
        assert!(a.windows(6).any(|w| w == ["-ss", "31.250", "-t", "8.750", "-i", "/m/clip.mp4"]), "{a:?}");
        let f = after(&a, "-filter_complex");
        assert_eq!(f, "[1:a]aresample=48000,volume=1.000,adelay=2500:all=1[s0];[2:a]aresample=48000,volume=0.500,adelay=20000:all=1[s1];[s0][s1]amix=inputs=2:duration=longest:dropout_transition=0:normalize=0,apad[aout]");
        assert!(a.windows(4).any(|w| w == ["-map", "0:v", "-map", "[aout]"]), "{a:?}");
        assert_eq!(after(&a, "-c:v"), "copy");
        assert!(a.contains(&"-shortest".to_string()));
    }

    #[test]
    fn one_stretch_needs_no_mixing() {
        let s = [Segment { path: "a".into(), from: 0.0, to: 1.0, at: 0.0, volume: 1.0 }];
        assert_eq!(mix(&s), "[1:a]aresample=48000,volume=1.000,adelay=0:all=1[s0];[s0]apad[aout]");
    }

    #[test]
    fn windows_inputs_from_new_and_old_ffmpeg() {
        let new = r#"[dshow @ 000001] "Integrated Camera" (video)
[dshow @ 000001]   Alternative name "@device_pnp_\\?\usb#vid"
[dshow @ 000001] "Microphone (Realtek(R) Audio)" (audio)
[dshow @ 000001]   Alternative name "@device_cm_{33D9A762}\wave_{1}"
[dshow @ 000001] "Line In (USB Audio CODEC )" (audio)
[dshow @ 000001] "OBS Virtual Camera" (none)
dummy: Immediate exit requested"#;
        let names: Vec<String> = parse_inputs(Os::Windows, new).into_iter().map(|i| i.name).collect();
        assert_eq!(names, ["Microphone (Realtek(R) Audio)", "Line In (USB Audio CODEC )"]);
        let old = r#"[dshow @ 0] DirectShow video devices (some may be both video and audio devices)
[dshow @ 0]  "Integrated Camera"
[dshow @ 0] DirectShow audio devices
[dshow @ 0]  "Microphone (Realtek High Definition Audio)"
[dshow @ 0]     Alternative name "@device_cm_{x}""#;
        assert_eq!(parse_inputs(Os::Windows, old), [Input { name: "Microphone (Realtek High Definition Audio)".into(), label: "Microphone (Realtek High Definition Audio)".into() }]);
    }

    #[test]
    fn mac_inputs_are_the_audio_section() {
        let out = "[AVFoundation indev @ 0x1] AVFoundation video devices:\n[AVFoundation indev @ 0x1] [0] FaceTime HD Camera\n[AVFoundation indev @ 0x1] AVFoundation audio devices:\n[AVFoundation indev @ 0x1] [0] MacBook Pro Microphone\n[AVFoundation indev @ 0x1] [1] USB Audio CODEC \n: Input/output error";
        let names: Vec<String> = parse_inputs(Os::MacOs, out).into_iter().map(|i| i.name).collect();
        assert_eq!(names, ["MacBook Pro Microphone", "USB Audio CODEC"]);
    }

    #[test]
    fn linux_inputs_leave_out_speaker_monitors() {
        let out = "Auto-detected sources for pulse:\n  alsa_output.pci-0000_00_1f.3.analog-stereo.monitor [Monitor of Built-in Audio Analog Stereo]\n* alsa_input.pci-0000_00_1f.3.analog-stereo [Built-in Audio Analog Stereo]\n  alsa_input.usb-Blue_Yeti-00.analog-stereo [Yeti Stereo Microphone Analog Stereo]\n";
        let found = parse_inputs(Os::Linux, out);
        assert_eq!(found, [
            Input { name: "alsa_input.pci-0000_00_1f.3.analog-stereo".into(), label: "Built-in Audio Analog Stereo".into() },
            Input { name: "alsa_input.usb-Blue_Yeti-00.analog-stereo".into(), label: "Yeti Stereo Microphone Analog Stereo".into() },
        ]);
    }
}
