//! What `ffmpeg` is asked to do, as plain argument lists: recording (`record`), making the MP4 (`finish`), and
//! listing the sound inputs (`list_inputs_args`, `parse_inputs`). All pure, so all tested here.
use super::{Preset, sound::Segment, tr};
use crate::platform::tools::Os;
use std::ffi::OsString;
use std::path::Path;

/// The H.264 encoders we can use, best first: x264 (in every full FFmpeg), then the Mac's own (VideoToolbox),
/// Windows' own (Media Foundation), then Cisco's.
pub const ENCODERS: [&str; 4] = ["libx264", "h264_videotoolbox", "h264_mf", "libopenh264"];

/// The encoders Settings offers by name (after Automatic): the processor (x264), NVIDIA, Intel and AMD graphics,
/// Windows' own, Cisco's, the Mac's own. New ones go at the end: Settings keeps the choice by its place here.
pub const CHOOSABLE: [&str; 7] = ["libx264", "h264_nvenc", "h264_qsv", "h264_amf", "h264_mf", "libopenh264", "h264_videotoolbox"];

/// The encoder to use: `wanted` (Settings; empty: automatic) when this FFmpeg has it and `works` says it runs on
/// this computer (a graphics card's needs that card), else the best one (`pick_encoder`).
pub fn choose_encoder(list: &str, wanted: &str, works: impl Fn(&str) -> bool) -> Option<&'static str> {
    let has = |name: &str| list.lines().any(|l| l.split_whitespace().nth(1) == Some(name) && l.trim_start().starts_with('V'));
    CHOOSABLE.into_iter().find(|e| *e == wanted && has(e) && works(e)).or_else(|| pick_encoder(list))
}

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

/// The cameras (and capture cards) in what `list_inputs_args` printed. Windows and macOS; Linux's are read from
/// the system (`devices::cameras`).
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
    // An NDI source is not read by FFmpeg (`stream::camera_feed` receives it).
    if let Some(source) = crate::stream::ndi::source_name(name) { return vec![crate::stream::ndi::INPUT.into(), source.to_string()] }
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
    if let Some(source) = crate::stream::ndi::source_name(name) { return crate::stream::ndi::label(source) }
    // Linux: a camera is saved as its device (`devices::cameras`).
    if name.starts_with("/dev/") { return super::devices::linux_camera_label(name) }
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
    /// Which of `stream` get the tall picture instead (missing: not tall).
    pub tall: &'a [bool],
    /// The video bitrate set by hand (kbit/s); 0: the preset's.
    pub kbps: u32,
    /// The PC's own sound, captured as it plays (`stream::pc_sound`), mixed with `input` when there is one.
    pub pc: Option<crate::stream::pc_sound::Feed>,
    /// What is done to the stream's sound (Settings): held back, turned up or down, cleaned, evened out.
    pub fx: SoundFx,
    /// NDI out (`stream::ndi::Out`): the local port a copy of the stream's sound goes to, raw 48 kHz stereo float.
    /// Only a sound passed through the app has an end FFmpeg waits for; without one, none is sent.
    pub ndi_sound: Option<u16>,
    /// Go Live: a second and third microphone or line-in, passed through the app, mixed with `input` (only with one).
    pub more: &'a [Extra],
    /// Listen (`stream::listen`): the local port a copy of the stream's sound goes to, as for `ndi_sound`.
    pub listen: Option<u16>,
}

/// Another microphone or line-in in the stream's mix: its feed through the app, its level and its filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extra {
    pub feed: crate::stream::pc_sound::Feed,
    pub db: i32,
    pub filters: Filters,
}

/// How strong a gate or an evener is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strength { #[default] Off, Light, Strong }

impl Strength {
    /// Settings' place for it: 0 off, 1 light, 2 strong.
    pub fn index(self) -> i32 { match self { Strength::Off => 0, Strength::Light => 1, Strength::Strong => 2 } }
    pub fn from_index(i: i32) -> Strength { match i { 1 => Strength::Light, 2 => Strength::Strong, _ => Strength::Off } }
}

/// What is done to one sound before it is mixed (Settings → More sound settings). The default does nothing; the
/// stream starts from `Filters::SAFE`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Filters {
    /// Quiets the sound between words (a gate).
    pub gate: Strength,
    /// Brings loud moments down and quiet ones up (a compressor).
    pub even: Strength,
    /// Nothing goes over -1 dB (a limiter).
    pub limit: bool,
    /// Low (100 Hz), middle (1 kHz) and high (8 kHz) turned up or down, dB (within `MAX_EQ_DB`).
    pub low_db: i32,
    pub mid_db: i32,
    pub high_db: i32,
}

/// The most low, middle or high is turned up or down, dB.
pub const MAX_EQ_DB: i32 = 12;
/// Nothing over -1 dB of full scale.
pub const LIMIT: &str = "alimiter=limit=0.89:level=disabled";

impl Filters {
    /// Where every sound starts: only the limiter, so the stream never crackles.
    pub const SAFE: Filters = Filters { gate: Strength::Off, even: Strength::Off, limit: true, low_db: 0, mid_db: 0, high_db: 0 };

    /// The filters for one sound, in order: hum and hiss out (`quiet_noise`), the gate, low/middle/high, the evener,
    /// its level (`db`), the limiter.
    pub fn chain(&self, quiet_noise: bool, db: i32) -> Vec<String> {
        let mut f = vec![];
        if quiet_noise { f.push("afftdn=nf=-25".to_string()) }
        match self.gate {
            Strength::Off => {}
            // About -45 dB and quieter is turned down 12 dB; -35 dB and quieter, 24 dB.
            Strength::Light => f.push("agate=threshold=0.0056:range=0.25:ratio=2:attack=5:release=300".into()),
            Strength::Strong => f.push("agate=threshold=0.0178:range=0.063:ratio=4:attack=5:release=300".into()),
        }
        let eq = |db: i32| db.clamp(-MAX_EQ_DB, MAX_EQ_DB);
        if self.low_db != 0 { f.push(format!("bass=g={}:f=100", eq(self.low_db))) }
        if self.mid_db != 0 { f.push(format!("equalizer=f=1000:t=q:w=1:g={}", eq(self.mid_db))) }
        if self.high_db != 0 { f.push(format!("treble=g={}:f=8000", eq(self.high_db))) }
        match self.even {
            Strength::Off => {}
            // Over -18 dB brought down 2 to 1, all of it up 3 dB; over -24 dB 4 to 1, up 8 dB.
            Strength::Light => f.push("acompressor=threshold=0.125:ratio=2:attack=20:release=250:makeup=1.4".into()),
            Strength::Strong => f.push("acompressor=threshold=0.063:ratio=4:attack=10:release=200:makeup=2.5".into()),
        }
        if db != 0 { f.push(format!("volume={db}dB")) }
        if self.limit { f.push(LIMIT.to_string()) }
        f
    }
}

/// One sound in the mix: its FFmpeg input, its filters, its name in the graph and its track's title.
struct Source { input: usize, chain: Vec<String>, name: String, title: String, limit: bool }

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
    /// With both sounds: the recording also keeps each on a track of its own after the mix (the stream gets only
    /// the mix), so they can be balanced again afterwards.
    pub tracks: bool,
    /// The microphone or line-in's own filters, and the computer's sound's (More sound settings).
    pub board_filters: Filters,
    pub pc_filters: Filters,
}

/// Evens the sound out: what is louder than about -18 dB is brought down (3 to 1), all of it brought up 6 dB, and
/// nothing goes over -1 dB.
pub const EVEN_OUT: &str = "acompressor=threshold=0.125:ratio=3:attack=20:release=250:makeup=2,alimiter=limit=0.89:level=disabled";

impl SoundFx {
    /// The filters on the microphone or line-in alone: hum and hiss out, its own filters and its level.
    fn board(&self) -> Vec<String> { self.board_filters.chain(self.quiet_noise, self.board_db) }

    /// The filters on the computer's own sound alone (never hum and hiss: it has none, and music would dull).
    fn pc(&self) -> Vec<String> { self.pc_filters.chain(false, self.pc_db) }

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

    /// The `-filter_complex` mixing `sources` (the microphone or line-in `[b]`, the computer's sound `[p]`, more
    /// inputs `[b2]`, `[b3]`) into `[a]`. With `tracks`, each also comes out alone, after its own filters and held
    /// back as the mix is: `[bt]`, `[pt]`, … With two or more sounds and a limiter on any, a last one after the mix
    /// keeps their sum from crackling (evening out ends with one already).
    fn mix(&self, sources: &[Source]) -> String {
        let mut graph = String::new();
        let held = if self.delay_ms > 0 { format!("adelay={}:all=1", self.delay_ms) } else { "anull".to_string() };
        let mut labels = String::new();
        for s in sources {
            let (n, name, f) = (s.input, &s.name, &s.chain);
            labels.push_str(&if self.tracks {
                let before: String = f.iter().map(|x| format!("{x},")).collect();
                graph.push_str(&format!("[{n}:a]{before}asplit=2[{name}][{name}s];[{name}s]{held}[{name}t];"));
                format!("[{name}]")
            } else if f.is_empty() { format!("[{n}:a]") } else {
                graph.push_str(&format!("[{n}:a]{}[{name}];", f.join(",")));
                format!("[{name}]")
            });
        }
        let mut all = self.all();
        if !self.even && sources.iter().any(|s| s.limit) { all.push(LIMIT.to_string()) }
        let all: String = all.iter().map(|f| format!(",{f}")).collect();
        format!("{graph}{labels}amix=inputs={}:duration=shortest:normalize=0{all}[a]", sources.len())
    }
}

pub fn record(r: &Record) -> Vec<OsString> {
    let mut a: Vec<OsString> = Vec::new();
    let mut push = |v: &[&str]| a.extend(v.iter().map(OsString::from));
    let (w, h) = r.size;
    let (size, fps, kbps) = (format!("{w}x{h}"), r.fps.to_string(), if r.kbps > 0 { r.kbps } else { r.preset.max_kbps() });
    // The destinations that get the tall picture (`stream::Destination::tall`), and the others.
    let is_tall = |i: usize| r.tall.get(i).copied().unwrap_or(false);
    let wide: Vec<String> = r.stream.iter().enumerate().filter(|(i, _)| !is_tall(*i)).map(|(_, s)| s.clone()).collect();
    let talls: Vec<String> = r.stream.iter().enumerate().filter(|(i, _)| is_tall(*i)).map(|(_, s)| s.clone()).collect();
    push(&["-hide_banner", "-loglevel", "warning", "-y"]);
    push(&["-f", "rawvideo", "-pix_fmt", "rgba", "-video_size", &size, "-framerate", &fps, "-thread_queue_size", "8", "-i", "pipe:0"]);
    // The sound: its inputs, a graph when it is mixed, and what each output maps (`map`, then its own options).
    let mut graph: Option<String> = None;
    let mut sound: Vec<String> = vec![];
    let mut tall_sound: Vec<String> = vec![];
    // The file has the sounds on tracks of their own after the mix: the destinations get the mix only.
    let mut tracks = false;
    // NDI out: what its copy of the sound maps, empty: none.
    let through_app = r.board.is_some() || r.pc.is_some();
    let ndi = r.ndi_sound.filter(|_| through_app);
    let mut ndi_sound: Vec<String> = vec![];
    // Listen: the same copy of the stream's sound, for headphones.
    let listen = r.listen.filter(|_| through_app);
    let mut listen_sound: Vec<String> = vec![];
    let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<String>>();
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
        // Every sound in the mix, in the order of FFmpeg's inputs: the input, the PC's sound, the others.
        let mut sources = vec![Source { input: 1, chain: r.fx.board(), name: "b".into(), title: tr::track_board(), limit: r.fx.board_filters.limit }];
        if let Some(pc) = r.pc {
            let pc = pc.input_args();
            push(&pc.iter().map(String::as_str).collect::<Vec<_>>());
            sources.push(Source { input: 2, chain: r.fx.pc(), name: "p".into(), title: tr::track_pc(), limit: r.fx.pc_filters.limit });
        }
        for (i, e) in r.more.iter().enumerate() {
            let feed = e.feed.input_args();
            push(&feed.iter().map(String::as_str).collect::<Vec<_>>());
            let n = sources.len() + 1;
            sources.push(Source { input: n, chain: e.filters.chain(r.fx.quiet_noise, e.db), name: format!("b{}", i + 2), title: crate::stream::sound_setup::tr::track_input(i + 2), limit: e.filters.limit });
        }
        if sources.len() > 1 {
            // The sounds together, none turned down. The output ends with the picture: the PC's sound goes on
            // until it is stopped. Split for each output that has it: the wide one, the tall one, NDI, Listen.
            let mix = r.fx.mix(&sources);
            let mut outs = vec!["[a]"];
            if !talls.is_empty() { outs.push("[a2]") }
            if ndi.is_some() { outs.push("[a3]"); ndi_sound = v(&["-map", "[a3]"]) }
            if listen.is_some() { outs.push("[a4]"); listen_sound = v(&["-map", "[a4]"]) }
            graph = Some(if outs.len() == 1 { mix } else { format!("{},asplit={}{}", mix.trim_end_matches("[a]"), outs.len(), outs.concat()) });
            sound = v(&["-map", "[a]", "-shortest", "-shortest_buf_duration", "0.5"]);
            tall_sound = v(&["-map", "[a2]", "-shortest", "-shortest_buf_duration", "0.5"]);
            if r.fx.tracks {
                // The mix first (what players play), then each sound alone, named.
                for s in &sources { sound.extend(["-map".to_string(), format!("[{}t]", s.name)]) }
                let titles = std::iter::once(tr::track_mix()).chain(sources.iter().map(|s| s.title.clone()));
                for (i, title) in titles.enumerate() {
                    sound.extend([format!("-metadata:s:a:{i}"), format!("title={title}")]);
                }
                tracks = true;
            }
        } else {
            sound = [v(&["-map", "1:a"]), r.fx.one(r.fx.board())].concat();
            tall_sound = sound.clone();
            if ndi.is_some() { ndi_sound = sound.clone() }
            if listen.is_some() { listen_sound = sound.clone() }
        }
    } else if let Some(pc) = r.pc {
        let pc = pc.input_args();
        push(&pc.iter().map(String::as_str).collect::<Vec<_>>());
        // `-shortest` alone holds up to ten seconds of sound back to line it up: FFmpeg stopped reading the PC's
        // sound after 0.7 s, its capture blocked, and what played then was lost (tried November 14, 2026).
        sound = [v(&["-map", "1:a", "-shortest", "-shortest_buf_duration", "0.5"]), r.fx.one(r.fx.pc())].concat();
        tall_sound = sound.clone();
        if ndi.is_some() { ndi_sound = sound.clone() }
        if listen.is_some() { listen_sound = sound.clone() }
    } else if !r.stream.is_empty() {
        // Streaming services refuse a stream with no sound: silence stands in. It has no end of its own, so
        // the output ends with the picture (`-shortest`), and it may run only half a second ahead of the
        // picture. Left alone it ran ten seconds ahead, and a sender that joined the feed in the middle (after
        // a lost connection) got nothing out of it. (Made at the clock's pace with `-re` instead, FFmpeg
        // stalled for eleven seconds at the start: tried on November 14, 2026.)
        push(&["-f", "lavfi", "-i", "anullsrc=channel_layout=stereo:sample_rate=48000"]);
        sound = v(&["-map", "1:a", "-shortest", "-shortest_buf_duration", "0.5"]);
        tall_sound = sound.clone();
    }
    if let Some(g) = &graph { push(&["-filter_complex", g]) }
    // The tall picture (phones): the wide one fitted across, black above and below, in a graph of its own.
    let (tw, th) = tall_size(r.size);
    if !talls.is_empty() {
        push(&["-filter_complex", &format!("[0:v]scale={tw}:-2,pad={tw}:{th}:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1[tall]")]);
    }
    let has_sound = r.input.is_some() || r.pc.is_some() || !r.stream.is_empty();
    let (rate, buf, gop) = (format!("{kbps}k"), format!("{}k", kbps * 2), (r.fps * 2).to_string());
    let encode = |a: &mut Vec<OsString>| {
        let mut push = |v: &[&str]| a.extend(v.iter().map(OsString::from));
        match r.encoder {
            // veryfast: little CPU; slides cost almost nothing to encode, a video gets up to the preset's rate.
            "libx264" => push(&["-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-maxrate", &rate, "-bufsize", &buf]),
            e => push(&["-c:v", e, "-b:v", &rate]),
        }
        // A keyframe every 2 s, so a file cut short by a crash loses at most that much.
        push(&["-g", &gop, "-pix_fmt", "yuv420p"]);
        if has_sound { push(&["-c:a", "aac", "-b:a", "160k"]) }
    };
    // The wide output: the file, and every destination that is not tall.
    push(&["-map", "0:v"]);
    push(&sound.iter().map(String::as_str).collect::<Vec<_>>());
    encode(&mut a);
    let mut push = |v: &[&str]| a.extend(v.iter().map(OsString::from));
    if r.stream.is_empty() {
        push(&["-f", "matroska"]);
        a.push(r.out.into());
    } else {
        // One encode, many outputs: the file and every destination (`stream::tee`). The outputs that are sent
        // need the encoder's headers up front.
        push(&["-flags", "+global_header", "-f", "tee"]);
        a.push(crate::stream::tee(r.out, &wide, tracks).into());
    }
    // The tall output: its own encode, to the tall destinations only.
    if !talls.is_empty() {
        a.extend(["-map", "[tall]"].map(OsString::from));
        a.extend(tall_sound.iter().map(OsString::from));
        encode(&mut a);
        a.extend(["-flags", "+global_header", "-f", "tee"].map(OsString::from));
        a.push(crate::stream::tee_only(&talls).into());
    }
    // NDI out: the sound, raw, to the app (`stream::ndi::Out`), which listens before FFmpeg starts.
    if let Some(port) = ndi.filter(|_| !ndi_sound.is_empty()) {
        a.extend(ndi_sound.iter().map(OsString::from));
        let (rate, channels) = (crate::stream::ndi::SOUND_RATE.to_string(), crate::stream::ndi::SOUND_CHANNELS.to_string());
        a.extend(["-c:a", "pcm_f32le", "-ar", &rate, "-ac", &channels, "-f", "f32le"].map(OsString::from));
        a.push(format!("tcp://127.0.0.1:{port}").into());
    }
    // Listen: the same, to the app, which plays it on the headphones' output.
    if let Some(port) = listen.filter(|_| !listen_sound.is_empty()) {
        a.extend(listen_sound.iter().map(OsString::from));
        let (rate, channels) = (crate::stream::listen::RATE.to_string(), crate::stream::listen::CHANNELS.to_string());
        a.extend(["-c:a", "pcm_f32le", "-ar", &rate, "-ac", &channels, "-f", "f32le"].map(OsString::from));
        a.push(format!("tcp://127.0.0.1:{port}").into());
    }
    a
}

/// The tall (9:16) picture for a wide stream of `size`: as wide as the wide one is high.
pub fn tall_size((_, h): (u32, u32)) -> (u32, u32) {
    let w = h & !1;
    (w, ((u64::from(w) * 16 / 9) as u32) & !1)
}

/// Turns the Matroska file `part` into the MP4 `out`, the picture copied as it is. `sound` is the app's sound to
/// mix in from its files; without it the part's own sound (a microphone, or none) is copied.
pub fn finish(part: &Path, sound: &[Segment], chapters: Option<&Path>, out: &Path) -> Vec<OsString> {
    let mut a: Vec<OsString> = ["-hide_banner", "-loglevel", "warning", "-y", "-i"].map(OsString::from).to_vec();
    a.push(part.into());
    for s in sound {
        a.extend(["-ss".into(), secs(s.from).into(), "-t".into(), secs(s.to - s.from).into(), OsString::from("-i"), s.path.clone().into_os_string()]);
    }
    // The chapters (`chapters::metadata`): the input after the sound files, giving only its chapters.
    if let Some(c) = chapters {
        let n = (sound.len() + 1).to_string();
        a.extend([OsString::from("-i"), c.into(), "-map_metadata".into(), n.clone().into(), "-map_chapters".into(), n.into()]);
    }
    if sound.is_empty() {
        a.extend(["-map", "0", "-c", "copy"].map(OsString::from));
    } else {
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
    fn a_chosen_encoder_is_used_only_when_it_works_here() {
        let list = " V....D libx264  x\n V....D h264_nvenc  NVIDIA\n V....D h264_qsv  Intel\n V....D h264_mf  Windows";
        assert_eq!(choose_encoder(list, "h264_nvenc", |_| true), Some("h264_nvenc"));
        assert_eq!(choose_encoder(list, "h264_nvenc", |e| e != "h264_nvenc"), Some("libx264"), "no NVIDIA card: automatic");
        assert_eq!(choose_encoder(list, "h264_amf", |_| true), Some("libx264"), "not in this FFmpeg: automatic");
        assert_eq!(choose_encoder(list, "", |_| panic!("automatic is not tried")), Some("libx264"));
        assert_eq!(choose_encoder(list, "rm -rf", |_| true), Some("libx264"), "only the encoders offered");
    }

    #[test]
    fn a_bitrate_set_by_hand_replaces_the_presets() {
        let out = PathBuf::from("/r/Recording.recording.mkv");
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: None, board: None, out: &out, stream: &[], tall: &[], kbps: 4500, pc: None, fx: SoundFx::default(), ndi_sound: None, more: &[], listen: None }));
        assert_eq!((after(&a, "-maxrate"), after(&a, "-bufsize")), ("4500k", "9000k"));
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "h264_nvenc", input: None, board: None, out: &out, stream: &[], tall: &[], kbps: 0, pc: None, fx: SoundFx::default(), ndi_sound: None, more: &[], listen: None }));
        assert_eq!(after(&a, "-b:v"), "3000k", "0: the preset's");
    }

    #[test]
    fn x264_is_preferred_then_windows_own() {
        assert_eq!(pick_encoder(ENCODERS_OUT), Some("libx264"));
        assert_eq!(pick_encoder(" V....D h264_mf  H264 via MediaFoundation\n V....D libx264rgb  x"), Some("h264_mf"));
        assert_eq!(pick_encoder(" V....D libx264rgb  x\n A....D libx264  not a video one"), None);
        // A Mac's FFmpeg without x264: the Mac's own.
        assert_eq!(pick_encoder(" V....D h264_videotoolbox  VideoToolbox H.264 Encoder\n V....D libopenh264  x"), Some("h264_videotoolbox"));
        assert_eq!(choose_encoder(" V....D libx264  x\n V....D h264_videotoolbox  VT", "h264_videotoolbox", |_| true), Some("h264_videotoolbox"));
    }

    #[test]
    fn streaming_sends_the_recording_to_every_destination_too() {
        let out = PathBuf::from("/r/Recording.recording.mkv");
        let to = ["rtmp://a.rtmp.youtube.com/live2/key".to_string(), "rtmps://live-api-s.facebook.com:443/rtmp/key2".to_string()];
        // With a sound input: one encode, the file and both destinations.
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: Some(("dshow", "Line In")), board: None, out: &out, stream: &to, tall: &[], kbps: 0, pc: None, fx: SoundFx::default(), ndi_sound: None, more: &[], listen: None }));
        assert_eq!(a[a.len() - 5..a.len() - 1], ["-flags", "+global_header", "-f", "tee"].map(String::from));
        assert_eq!(a.last().unwrap(), "[f=matroska]/r/Recording.recording.mkv|[f=flv:onfail=ignore]rtmp://a.rtmp.youtube.com/live2/key|[f=flv:onfail=ignore]rtmps://live-api-s.facebook.com:443/rtmp/key2");
        assert_eq!(after(&a, "-c:a"), "aac");
        assert!(!a.contains(&"anullsrc=channel_layout=stereo:sample_rate=48000".to_string()));
        // Without one: silence stands in (a service refuses a stream with no sound), and it ends with the picture.
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: None, board: None, out: &out, stream: &to[..1], tall: &[], kbps: 0, pc: None, fx: SoundFx::default(), ndi_sound: None, more: &[], listen: None }));
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
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: None, board: None, out: &out, stream: &to, tall: &[], kbps: 0, pc, fx: SoundFx::default(), ndi_sound: None, more: &[], listen: None }));
        assert!(a.windows(2).any(|w| w == ["-i", "tcp://127.0.0.1:40500?listen=1"]) && after(&a, "-ar") == "48000", "{a:?}");
        assert!(a.windows(4).any(|w| w == ["-map", "0:v", "-map", "1:a"]) && after(&a, "-shortest_buf_duration") == "0.5");
        assert!(!a.iter().any(|x| x.starts_with("anullsrc")), "{a:?}");
        // With the sound board: both, mixed.
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: Some(("dshow", "Line In")), board: None, out: &out, stream: &to, tall: &[], kbps: 0, pc, fx: SoundFx::default(), ndi_sound: None, more: &[], listen: None }));
        assert_eq!(after(&a, "-filter_complex"), "[1:a][2:a]amix=inputs=2:duration=shortest:normalize=0[a]");
        assert!(a.windows(2).any(|w| w == ["-map", "[a]"]) && after(&a, "-c:a") == "aac");
    }

    #[test]
    fn the_sound_can_wait_for_the_camera() {
        use crate::stream::pc_sound::{Feed, Format};
        let out = PathBuf::from("/r/Recording.recording.mkv");
        let to = ["rtmp://a.rtmp.youtube.com/live2/key".to_string()];
        let rec = |input, pc, delay_ms| strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input, board: None, out: &out, stream: &to, tall: &[], kbps: 0, pc, fx: SoundFx { delay_ms, ..SoundFx::default() }, ndi_sound: None, more: &[], listen: None }));
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
        let rec = |input, pc, fx| strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input, board: None, out: &out, stream: &to, tall: &[], kbps: 0, pc, fx, ndi_sound: None, more: &[], listen: None }));
        let pc = Some(Feed { format: Format { samples: "f32le", rate: 48000, channels: 2 }, port: 40500 });
        let line = Some(("dshow", "Line In"));
        let fx = SoundFx { delay_ms: 0, board_db: 6, pc_db: -4, even: true, quiet_noise: true, tracks: false, ..SoundFx::default() };
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
    fn the_recording_can_keep_each_sound_on_a_track_of_its_own() {
        use crate::stream::pc_sound::{Feed, Format};
        let out = PathBuf::from("/r/Recording.recording.mkv");
        let to = ["udp://127.0.0.1:20001?pkt_size=1316".to_string()];
        let pc = Some(Feed { format: Format { samples: "f32le", rate: 48000, channels: 2 }, port: 40500 });
        let line = Some(("dshow", "Line In"));
        let rec = |input, pc, fx| strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input, board: None, out: &out, stream: &to, tall: &[], kbps: 0, pc, fx, ndi_sound: None, more: &[], listen: None }));
        let fx = SoundFx { tracks: true, board_db: 6, delay_ms: 300, ..SoundFx::default() };
        let a = rec(line, pc, fx);
        // Each sound split after its own filters; the copies held back as the mix is.
        assert_eq!(after(&a, "-filter_complex"),
            "[1:a]volume=6dB,asplit=2[b][bs];[bs]adelay=300:all=1[bt];[2:a]asplit=2[p][ps];[ps]adelay=300:all=1[pt];[b][p]amix=inputs=2:duration=shortest:normalize=0,adelay=300:all=1[a]");
        let j = a.join(" ");
        assert!(j.contains("-map [a] -shortest -shortest_buf_duration 0.5 -map [bt] -map [pt]"), "{j}");
        assert!(a.windows(2).any(|w| w[0] == "-metadata:s:a:1" && w[1] == format!("title={}", tr::track_board())), "{j}");
        // The destinations get the picture and the mix only.
        assert!(a.last().unwrap().contains(":select=\\'v,a:0\\':"), "{j}");
        // Without a delay the copies pass as they are.
        assert!(after(&rec(line, pc, SoundFx { tracks: true, ..SoundFx::default() }), "-filter_complex").starts_with("[1:a]asplit=2[b][bs];[bs]anull[bt];"));
        // One sound only: nothing to keep apart.
        let one = rec(line, None, fx).join(" ");
        assert!(!one.contains("[bt]") && !one.contains("select="), "{one}");
        // Off: as before.
        let off = rec(line, pc, SoundFx::default()).join(" ");
        assert!(!off.contains("[bt]") && !off.contains("select=") && !off.contains("-metadata:s:a"), "{off}");
    }

    #[test]
    fn ndi_out_gets_a_copy_of_the_streams_sound() {
        use crate::stream::pc_sound::{Feed, Format};
        let out = PathBuf::from("/r/Recording.recording.mkv");
        let to = ["udp://127.0.0.1:20001?pkt_size=1316".to_string(), "udp://127.0.0.1:20002?pkt_size=1316".to_string()];
        let pc = Some(Feed { format: Format { samples: "f32le", rate: 48000, channels: 2 }, port: 40500 });
        let board = Some(Feed { format: Format { samples: "s16le", rate: 48000, channels: 2 }, port: 40600 });
        let rec = |board: Option<Feed>, pc, tall: &[bool], ndi| strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: board.map(|_| ("dshow", "Line In")), board, out: &out, stream: &to, tall, kbps: 0, pc, fx: SoundFx { board_db: 6, ..SoundFx::default() }, ndi_sound: ndi, more: &[], listen: None }));
        let ndi_out = "-c:a pcm_f32le -ar 48000 -ac 2 -f f32le tcp://127.0.0.1:41000";
        // Both sounds: the mix split for it (and for a tall output too).
        let a = rec(board, pc, &[], Some(41000));
        assert!(after(&a, "-filter_complex").ends_with(",asplit=2[a][a3]"), "{a:?}");
        assert!(a.join(" ").ends_with(&format!("-map [a3] {ndi_out}")), "{a:?}");
        let t = rec(board, pc, &[false, true], Some(41000));
        assert!(after(&t, "-filter_complex").ends_with(",asplit=3[a][a2][a3]"), "{t:?}");
        // One sound: the same input again, with its own filters.
        let one = rec(board, None, &[], Some(41000)).join(" ");
        assert!(one.ends_with(&format!("-map 1:a -af volume=6dB {ndi_out}")), "{one}");
        // No sound through the app, or NDI off: nothing extra.
        assert!(!rec(None, None, &[], Some(41000)).join(" ").contains("f32le"));
        assert!(!rec(board, pc, &[], None).join(" ").contains("[a3]"));
    }

    #[test]
    fn each_sounds_filters_come_in_order_and_do_nothing_until_asked() {
        assert!(Filters::default().chain(false, 0).is_empty());
        assert_eq!(Filters::SAFE.chain(false, 0), [LIMIT]);
        let all = Filters { gate: Strength::Light, even: Strength::Strong, limit: true, low_db: -3, mid_db: 2, high_db: 40 };
        assert_eq!(all.chain(true, 6).join(","), "afftdn=nf=-25,agate=threshold=0.0056:range=0.25:ratio=2:attack=5:release=300,bass=g=-3:f=100,\
            equalizer=f=1000:t=q:w=1:g=2,treble=g=12:f=8000,acompressor=threshold=0.063:ratio=4:attack=10:release=200:makeup=2.5,volume=6dB,{LIMIT}"
            .replace("{LIMIT}", LIMIT));
        let strong = Filters { gate: Strength::Strong, even: Strength::Light, ..Filters::default() }.chain(false, 0);
        assert_eq!(strong, ["agate=threshold=0.0178:range=0.063:ratio=4:attack=5:release=300", "acompressor=threshold=0.125:ratio=2:attack=20:release=250:makeup=1.4"]);
        assert_eq!((Strength::from_index(2), Strength::from_index(9), Strength::Light.index()), (Strength::Strong, Strength::Off, 1));
    }

    #[test]
    fn the_mix_has_a_last_limiter_and_the_other_inputs() {
        use crate::stream::pc_sound::{Feed, Format};
        let out = PathBuf::from("/r/Recording.recording.mkv");
        let to = ["udp://127.0.0.1:20001?pkt_size=1316".to_string()];
        let f = |port| Feed { format: Format { samples: "s16le", rate: 48000, channels: 2 }, port };
        let pc = Some(Feed { format: Format { samples: "f32le", rate: 48000, channels: 2 }, port: 40500 });
        let line = Some(("dshow", "Line In"));
        let safe = SoundFx { board_filters: Filters::SAFE, pc_filters: Filters::SAFE, ..SoundFx::default() };
        let rec = |pc, fx, more: &[Extra], listen| strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: line, board: Some(f(40600)), out: &out, stream: &to, tall: &[], kbps: 0, pc, fx, ndi_sound: None, more, listen }));
        // Two sounds, each limited, then the sum.
        assert_eq!(after(&rec(pc, safe, &[], None), "-filter_complex"), format!("[1:a]{LIMIT}[b];[2:a]{LIMIT}[p];[b][p]amix=inputs=2:duration=shortest:normalize=0,{LIMIT}[a]"));
        // Evened out already ends with a limiter: not twice.
        assert_eq!(after(&rec(pc, SoundFx { even: true, ..safe }, &[], None), "-filter_complex"), format!("[1:a]{LIMIT}[b];[2:a]{LIMIT}[p];[b][p]amix=inputs=2:duration=shortest:normalize=0,{EVEN_OUT}[a]"));
        // One sound: its own limiter is enough.
        assert_eq!(after(&rec(None, safe, &[], None), "-af"), LIMIT);
        // A second and third input after the PC's sound, each with its level and filters.
        let more = [Extra { feed: f(40700), db: 4, filters: Filters::default() }, Extra { feed: f(40800), db: 0, filters: Filters { gate: Strength::Light, ..Filters::default() } }];
        let a = rec(pc, SoundFx::default(), &more, None);
        let j = a.join(" ");
        assert!(j.contains("-i tcp://127.0.0.1:40500?listen=1 -f s16le -ar 48000 -ac 2 -thread_queue_size 1024 -i tcp://127.0.0.1:40700?listen=1 -f s16le -ar 48000 -ac 2 -thread_queue_size 1024 -i tcp://127.0.0.1:40800?listen=1"), "{j}");
        assert_eq!(after(&a, "-filter_complex"), "[3:a]volume=4dB[b2];[4:a]agate=threshold=0.0056:range=0.25:ratio=2:attack=5:release=300[b3];[1:a][2:a][b2][b3]amix=inputs=4:duration=shortest:normalize=0[a]");
        // Without the PC's sound the others come straight after the first.
        assert_eq!(after(&rec(None, SoundFx::default(), &more[..1], None), "-filter_complex"), "[2:a]volume=4dB[b2];[1:a][b2]amix=inputs=2:duration=shortest:normalize=0[a]");
        // Each on a track of its own, named.
        let a = rec(pc, SoundFx { tracks: true, ..SoundFx::default() }, &more, None);
        let j = a.join(" ");
        assert!(j.contains("-map [a] -shortest -shortest_buf_duration 0.5 -map [bt] -map [pt] -map [b2t] -map [b3t]"), "{j}");
        assert!(a.windows(2).any(|w| w[0] == "-metadata:s:a:3" && w[1] == format!("title={}", crate::stream::sound_setup::tr::track_input(2))), "{j}");
        // Listen gets a copy of the mix, as NDI does.
        let a = rec(pc, SoundFx::default(), &[], Some(42000));
        assert!(after(&a, "-filter_complex").ends_with(",asplit=2[a][a4]"), "{a:?}");
        assert!(a.join(" ").ends_with("-map [a4] -c:a pcm_f32le -ar 48000 -ac 2 -f f32le tcp://127.0.0.1:42000"), "{a:?}");
        assert!(rec(None, safe, &[], Some(42000)).join(" ").ends_with(&format!("-map 1:a -af {LIMIT} -c:a pcm_f32le -ar 48000 -ac 2 -f f32le tcp://127.0.0.1:42000")));
    }

    /// The whole chain of filters, as Settings can set it, run by a real FFmpeg over a tone.
    #[test]
    fn a_real_ffmpeg_takes_every_filter() {
        let Some(ffmpeg) = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).flat_map(|d| [d.join("ffmpeg"), d.join("ffmpeg.exe")]).find(|p| p.is_file())) else { return };
        let all = Filters { gate: Strength::Strong, even: Strength::Light, limit: true, low_db: -12, mid_db: 3, high_db: 12 };
        let fx = SoundFx { delay_ms: 200, even: true, ..SoundFx::default() };
        let chain = [all.chain(true, 20), fx.all()].concat().join(",");
        let out = std::process::Command::new(ffmpeg).args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-af", &chain, "-f", "null", "-"]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn a_tall_stream_goes_out_beside_the_wide_one() {
        use crate::stream::pc_sound::{Feed, Format};
        let out = PathBuf::from("/r/Recording.recording.mkv");
        let to = ["udp://127.0.0.1:20001?pkt_size=1316".to_string(), "udp://127.0.0.1:20002?pkt_size=1316".to_string()];
        let pc = Some(Feed { format: Format { samples: "f32le", rate: 48000, channels: 2 }, port: 40500 });
        let rec = |input, pc, fx| strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input, board: None, out: &out, stream: &to, tall: &[false, true], kbps: 0, pc, fx, ndi_sound: None, more: &[], listen: None }));
        // The PC's sound alone: the picture made tall in a graph of its own, and a second output with it.
        let a = rec(None, pc, SoundFx::default());
        let j = a.join(" ");
        assert!(j.contains("-filter_complex [0:v]scale=720:-2,pad=720:1280:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1[tall]"), "{j}");
        // The wide output: the file and the first destination; then the tall one: the second destination only.
        let tees: Vec<&String> = a.iter().filter(|x| x.starts_with("[f=")).collect();
        assert_eq!(tees, ["[f=matroska]/r/Recording.recording.mkv|[f=mpegts:bsfs/v=dump_extra:onfail=ignore]udp://127.0.0.1:20001?pkt_size=1316", "[f=mpegts:bsfs/v=dump_extra:onfail=ignore]udp://127.0.0.1:20002?pkt_size=1316"]);
        let tall_at = j.find("-map [tall]").expect("the tall output");
        assert!(j[tall_at..].contains("-map 1:a") && j[tall_at..].contains("-c:a aac") && j[tall_at..].contains("-g 60"), "{j}");
        assert!(j.find("-map 0:v").unwrap() < tall_at, "the wide output first");
        // Sound mixed in a graph: split for both outputs.
        let a = rec(Some(("dshow", "Line In")), pc, SoundFx { even: true, ..SoundFx::default() });
        let j = a.join(" ");
        assert!(j.contains("asplit=2[a][a2]"), "{j}");
        assert!(j[j.find("-map [tall]").unwrap()..].contains("-map [a2]"), "{j}");
        // One sound with filters on it: the tall output has the same filters.
        let a = rec(Some(("dshow", "Line In")), None, SoundFx { even: true, ..SoundFx::default() });
        assert_eq!(a.iter().filter(|x| *x == "-af").count(), 2, "{a:?}");
        // No tall destination: no tall output.
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: None, board: None, out: &out, stream: &to, tall: &[false, false], kbps: 0, pc: None, fx: SoundFx::default(), ndi_sound: None, more: &[], listen: None }));
        assert!(!a.join(" ").contains("[tall]"));
    }

    #[test]
    fn the_sound_input_comes_through_the_app_when_it_can() {
        use crate::stream::pc_sound::{Feed, Format};
        let out = PathBuf::from("/r/Recording.recording.mkv");
        let board = Some(Feed { format: Format { samples: "s16le", rate: 48000, channels: 2 }, port: 40700 });
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: Some(("dshow", "Line In")), board, out: &out, stream: &[], tall: &[], kbps: 0, pc: None, fx: SoundFx::default(), ndi_sound: None, more: &[], listen: None }));
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
        // An NDI source: by its name, and not read by FFmpeg.
        assert_eq!(camera_label("ndi://STUDIO (Cam 1)"), "NDI: STUDIO (Cam 1)");
        // Linux: a camera by its device, named readably.
        assert_eq!(camera_label("/dev/v4l/by-id/usb-046d_C920_AB-video-index0"), "046d C920 AB");
        assert_eq!(camera_input(Os::Linux, "/dev/v4l/by-id/usb-046d_C920_AB-video-index0").join(" "), "-f v4l2 -thread_queue_size 64 -i /dev/v4l/by-id/usb-046d_C920_AB-video-index0");
        assert_eq!(camera_input(Os::Windows, "ndi://STUDIO (Cam 1)"), vec![crate::stream::ndi::INPUT.to_string(), "STUDIO (Cam 1)".to_string()]);
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
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "libx264", input: None, board: None, out: &out, stream: &[], tall: &[], kbps: 0, pc: None, fx: SoundFx::default(), ndi_sound: None, more: &[], listen: None }));
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
        let with = |format, name| strs(&record(&Record { size: (1920, 1080), fps: 30, preset: Preset::P1080, encoder: "libx264", input: Some((format, name)), board: None, out: &out, stream: &[], tall: &[], kbps: 0, pc: None, fx: SoundFx::default(), ndi_sound: None, more: &[], listen: None }));
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
        let a = strs(&record(&Record { size: (1280, 720), fps: 30, preset: Preset::P720, encoder: "h264_mf", input: None, board: None, out: &out, stream: &[], tall: &[], kbps: 0, pc: None, fx: SoundFx::default(), ndi_sound: None, more: &[], listen: None }));
        assert_eq!(after(&a, "-c:v"), "h264_mf");
        assert_eq!(after(&a, "-b:v"), "3000k");
        assert!(!a.contains(&"-crf".to_string()));
    }

    #[test]
    fn finishing_without_app_sound_copies_everything() {
        let a = strs(&finish(Path::new("p.mkv"), &[], None, Path::new("o.mp4.tmp")));
        assert_eq!(after(&a, "-i"), "p.mkv");
        assert_eq!(after(&a, "-c"), "copy");
        assert_eq!(after(&a, "-movflags"), "+faststart");
        assert_eq!(after(&a, "-f"), "mp4");
        assert_eq!(a.last().unwrap(), "o.mp4.tmp");
    }

    #[test]
    fn finishing_puts_the_chapters_in() {
        let meta = Path::new("o.chapters.txt");
        // The chapters file is the input after the picture and any sound files; its chapters and nothing else.
        let a = strs(&finish(Path::new("p.mkv"), &[], Some(meta), Path::new("o.mp4.tmp")));
        assert!(a.windows(6).any(|w| w == ["-i", "o.chapters.txt", "-map_metadata", "1", "-map_chapters", "1"]), "{a:?}");
        assert!(a.windows(4).any(|w| w == ["-map", "0", "-c", "copy"]), "{a:?}");
        let s = vec![Segment { path: "/m/song.mp3".into(), from: 0.0, to: 10.0, at: 2.5, volume: 1.0 }];
        let a = strs(&finish(Path::new("p.mkv"), &s, Some(meta), Path::new("o.mp4.tmp")));
        assert!(a.windows(6).any(|w| w == ["-i", "o.chapters.txt", "-map_metadata", "2", "-map_chapters", "2"]), "{a:?}");
        assert_eq!(after(&a, "-filter_complex").matches("[1:a]").count(), 1, "the chapters are no sound");
        assert!(!strs(&finish(Path::new("p.mkv"), &[], None, Path::new("o.mp4.tmp"))).iter().any(|x| x == "-map_chapters"));
    }

    #[test]
    fn finishing_with_app_sound_cuts_each_stretch_from_its_file() {
        let s = vec![
            Segment { path: "/m/song.mp3".into(), from: 0.0, to: 10.0, at: 2.5, volume: 1.0 },
            Segment { path: "/m/clip.mp4".into(), from: 31.25, to: 40.0, at: 20.0, volume: 0.5 },
        ];
        let a = strs(&finish(Path::new("p.mkv"), &s, None, Path::new("o.mp4.tmp")));
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
