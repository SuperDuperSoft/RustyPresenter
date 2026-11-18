//! Settings → Livestream (its advanced rows) (design `docs/superpowers/specs/2026-11-17-livestream-output-design.md`):
//! what the easy page leaves at its best guess. How the picture is made (pictures a second, video format, how the
//! bitrate is kept, how often a whole picture comes, how hard the encoder works, profile, B-frames), the sound
//! (rate, bitrate, channels), the network (which network card sends, SRT's safer send) and the recording (MP4 or
//! MKV, its name, cutting long ones into pieces, a second recording without the words, keeping the last minutes).
//! Every setting's default is what the app did before, so a church that never opens it sees no change. All pure:
//! FFmpeg's arguments are made here and tested here.
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The picture formats. H.264 plays everywhere; HEVC (H.265) and AV1 give a sharper picture for the same upload, but
/// only some services take them and they need a newer computer or graphics card.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Codec {
    #[default]
    #[serde(rename = "h264")]
    H264,
    #[serde(rename = "hevc")]
    Hevc,
    #[serde(rename = "av1")]
    Av1,
}

impl Codec {
    pub const ALL: [Codec; 3] = [Codec::H264, Codec::Hevc, Codec::Av1];

    /// The encoders for this format, best first: the one from the same maker as `h264` (the H.264 encoder chosen
    /// by Settings or found by itself, `args::choose_encoder`), then the processor's own.
    pub fn encoders(self, h264: &str) -> Vec<&'static str> {
        let same = match (self, h264) {
            (Codec::H264, _) => return args_h264(h264).into_iter().collect(),
            (Codec::Hevc, "libx264") => Some("libx265"),
            (Codec::Hevc, "h264_nvenc") => Some("hevc_nvenc"),
            (Codec::Hevc, "h264_qsv") => Some("hevc_qsv"),
            (Codec::Hevc, "h264_amf") => Some("hevc_amf"),
            (Codec::Hevc, "h264_mf") => Some("hevc_mf"),
            (Codec::Hevc, "h264_videotoolbox") => Some("hevc_videotoolbox"),
            (Codec::Av1, "libx264") => Some("libsvtav1"),
            (Codec::Av1, "h264_nvenc") => Some("av1_nvenc"),
            (Codec::Av1, "h264_qsv") => Some("av1_qsv"),
            (Codec::Av1, "h264_amf") => Some("av1_amf"),
            (Codec::Av1, "h264_mf") => Some("av1_mf"),
            _ => None,
        };
        let own = if self == Codec::Hevc { "libx265" } else { "libsvtav1" };
        let mut v: Vec<&'static str> = same.into_iter().collect();
        if !v.contains(&own) { v.push(own) }
        v
    }

    /// The format `encoder` makes.
    pub fn of(encoder: &str) -> Codec {
        if encoder.starts_with("hevc") || encoder == "libx265" { Codec::Hevc }
        else if encoder.starts_with("av1") || encoder == "libsvtav1" || encoder == "libaom-av1" { Codec::Av1 }
        else { Codec::H264 }
    }

    pub fn label(self) -> String {
        match self { Codec::H264 => "H.264".into(), Codec::Hevc => "HEVC (H.265)".into(), Codec::Av1 => "AV1".into() }
    }
}

/// An H.264 encoder's name as `&'static str`, when it is one the app knows.
fn args_h264(e: &str) -> Option<&'static str> { super::args::CHOOSABLE.into_iter().chain(super::args::ENCODERS).find(|c| *c == e) }

/// The format to use: `chosen`, unless a destination going out does not take it (then H.264 for all, so no
/// destination needs a second encode), or it is AV1 while live (AV1 cannot pass through the stream's local feeds,
/// whose MPEG-TS does not carry it in this FFmpeg: tried November 17, 2026).
pub fn codec_for(chosen: Codec, services: &[crate::stream::Service], live: bool) -> Codec {
    if chosen == Codec::Av1 && live { return Codec::H264 }
    if services.iter().all(|s| s.accepts(chosen)) { chosen } else { Codec::H264 }
}

/// How the bitrate is kept.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Rate {
    /// As before: the processor's encoder spends little on still slides and up to the bitrate on a busy video;
    /// the others keep to the bitrate.
    #[default]
    #[serde(rename = "auto")]
    Auto,
    /// Always the bitrate: what some services and older equipment ask for.
    #[serde(rename = "cbr")]
    Constant,
    /// The bitrate on average, up to half again on a busy picture.
    #[serde(rename = "vbr")]
    Variable,
}

/// How hard the encoder works: faster leaves more of the computer free, slower gives a sharper picture.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Speed {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "fastest")]
    Fastest,
    #[serde(rename = "fast")]
    Fast,
    #[serde(rename = "balanced")]
    Balanced,
    #[serde(rename = "best")]
    Best,
}

/// The H.264 profile: Baseline for very old players, High for the sharpest picture.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Profile {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "baseline")]
    Baseline,
    #[serde(rename = "main")]
    Main,
    #[serde(rename = "high")]
    High,
}

impl Rate { pub const ALL: [Rate; 3] = [Rate::Auto, Rate::Constant, Rate::Variable]; }
impl Speed { pub const ALL: [Speed; 5] = [Speed::Auto, Speed::Fastest, Speed::Fast, Speed::Balanced, Speed::Best]; }
impl Profile { pub const ALL: [Profile; 4] = [Profile::Auto, Profile::Baseline, Profile::Main, Profile::High]; }

/// The pictures a second Settings offers.
pub const FRAME_RATES: [u32; 5] = [24, 25, 30, 50, 60];
/// Seconds between whole pictures Settings offers (services ask for 2; 4 at most).
pub const KEYFRAMES: [u32; 4] = [1, 2, 3, 4];
/// B-frames Settings offers, after Automatic.
pub const B_FRAMES: [u8; 4] = [0, 1, 2, 3];
/// Sound: the sample rates, bitrates (kbit/s) and channels Settings offers.
pub const SOUND_RATES: [u32; 2] = [44100, 48000];
pub const SOUND_KBPS: [u32; 6] = [96, 128, 160, 192, 256, 320];

/// How the picture is made.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Video {
    #[serde(skip_serializing_if = "is_default")]
    pub rate: Rate,
    /// Seconds between whole pictures; 0: 2.
    #[serde(skip_serializing_if = "is_zero")]
    pub keyframe_secs: u32,
    #[serde(skip_serializing_if = "is_default")]
    pub speed: Speed,
    #[serde(skip_serializing_if = "is_default")]
    pub profile: Profile,
    /// `None`: the encoder's own choice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b_frames: Option<u8>,
}

/// How the sound is made.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Sound {
    /// Samples a second; 0: as it comes (48 kHz from the app).
    #[serde(skip_serializing_if = "is_zero")]
    pub rate: u32,
    /// kbit/s; 0: 160.
    #[serde(skip_serializing_if = "is_zero")]
    pub kbps: u32,
    /// 1: one channel (mono); 0 or 2: stereo.
    #[serde(skip_serializing_if = "is_zero_u8")]
    pub channels: u8,
}

/// How the stream goes out on the network.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Network {
    /// The address of the network card to send from (`stream::network_card`); empty: the computer chooses.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub bind: String,
    /// SRT's safer send: how long (ms) the far end waits for a lost piece to be sent again; 0: SRT's own (120 ms).
    #[serde(skip_serializing_if = "is_zero")]
    pub srt_latency_ms: u32,
}

/// SRT safer-send waits Settings offers (ms), after SRT's own.
pub const SRT_LATENCIES: [u32; 4] = [500, 1000, 2000, 4000];

/// The finished recording's file type.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Format {
    /// Plays everywhere (made from the MKV written while recording, at the end).
    #[default]
    #[serde(rename = "mp4")]
    Mp4,
    /// Kept as written: nothing to make at the end, and every part of it stays readable.
    #[serde(rename = "mkv")]
    Mkv,
}

impl Format {
    pub fn extension(self) -> &'static str { match self { Format::Mp4 => "mp4", Format::Mkv => "mkv" } }
    /// FFmpeg's name for it.
    pub fn muxer(self) -> &'static str { match self { Format::Mp4 => "mp4", Format::Mkv => "matroska" } }
}

/// What is kept of a recording.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Keep {
    #[serde(skip_serializing_if = "is_default")]
    pub format: Format,
    /// The file's name: `{date}`, `{time}` and `{title}` (the service's name) are filled in; empty: `DEFAULT_NAME`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Long recordings cut into pieces of this many minutes (0: never), or of at most this many GB (0: never).
    #[serde(skip_serializing_if = "is_zero")]
    pub split_minutes: u32,
    #[serde(skip_serializing_if = "is_zero")]
    pub split_gb: u32,
    /// While live: a second recording of the stream without the words over the camera (`stream::clean_file`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub clean: bool,
    /// "Save the last minutes": how many minutes the button keeps (0: no button).
    #[serde(skip_serializing_if = "is_zero")]
    pub replay_minutes: u32,
}

/// The pieces Settings offers to cut long recordings into: minutes, then GB.
pub const SPLIT_MINUTES: [u32; 3] = [30, 60, 120];
pub const SPLIT_GB: [u32; 3] = [2, 4, 8];
/// The minutes "Save the last minutes" offers to keep.
pub const REPLAY_MINUTES: [u32; 5] = [1, 2, 5, 10, 15];

/// Everything in More settings (kept in `RecordPrefs::more`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct More {
    /// Pictures a second (one of `FRAME_RATES`); 0: `record::FPS`.
    #[serde(skip_serializing_if = "is_zero")]
    pub fps: u32,
    #[serde(skip_serializing_if = "is_default")]
    pub codec: Codec,
    #[serde(skip_serializing_if = "is_default")]
    pub video: Video,
    #[serde(skip_serializing_if = "is_default")]
    pub sound: Sound,
    #[serde(skip_serializing_if = "is_default")]
    pub network: Network,
    #[serde(skip_serializing_if = "is_default")]
    pub keep: Keep,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool { *v == T::default() }
fn is_zero(v: &u32) -> bool { *v == 0 }
fn is_zero_u8(v: &u8) -> bool { *v == 0 }

impl More {
    pub fn is_default(&self) -> bool { *self == More::default() }

    /// Pictures a second: the setting when it is one offered, else `record::FPS`.
    pub fn fps(&self) -> u32 { if FRAME_RATES.contains(&self.fps) { self.fps } else { super::FPS } }
}

impl Video {
    /// Pictures between whole pictures at `fps`.
    pub fn gop(&self, fps: u32) -> u32 {
        let secs = if KEYFRAMES.contains(&self.keyframe_secs) { self.keyframe_secs } else { 2 };
        fps.max(1) * secs
    }
}

/// FFmpeg's picture options for `encoder` at `kbps` and `fps` (after `-c:v` is chosen: they include it).
pub fn video_args(encoder: &str, kbps: u32, fps: u32, v: &Video) -> Vec<String> {
    let (rate, peak, buf) = (format!("{kbps}k"), format!("{}k", kbps * 3 / 2), format!("{}k", kbps * 2));
    let mut a: Vec<String> = vec!["-c:v".into(), encoder.into()];
    let mut push = |x: &[&str]| a.extend(x.iter().map(|s| s.to_string()));
    let family = family(encoder);
    // The encoder's speed (`Speed::Auto`: what the app always used: x264's veryfast, the others their own).
    let speed = match (family, v.speed) {
        (Family::X26x, Speed::Auto | Speed::Balanced) => Some("veryfast"),
        (Family::X26x, Speed::Fastest) => Some("ultrafast"),
        (Family::X26x, Speed::Fast) => Some("superfast"),
        (Family::X26x, Speed::Best) => Some("fast"),
        (Family::Nvenc, Speed::Fastest) => Some("p1"),
        (Family::Nvenc, Speed::Fast) => Some("p2"),
        (Family::Nvenc, Speed::Balanced) => Some("p4"),
        (Family::Nvenc, Speed::Best) => Some("p6"),
        (Family::Qsv, Speed::Fastest) => Some("veryfast"),
        (Family::Qsv, Speed::Fast) => Some("faster"),
        (Family::Qsv, Speed::Balanced) => Some("medium"),
        (Family::Qsv, Speed::Best) => Some("slow"),
        (Family::SvtAv1, Speed::Fastest) => Some("12"),
        (Family::SvtAv1, Speed::Fast) => Some("11"),
        (Family::SvtAv1, Speed::Auto | Speed::Balanced) => Some("10"),
        (Family::SvtAv1, Speed::Best) => Some("8"),
        _ => None,
    };
    if let Some(s) = speed { push(&["-preset", s]) }
    if family == Family::Amf {
        let q = match v.speed { Speed::Fastest | Speed::Fast => Some("speed"), Speed::Balanced => Some("balanced"), Speed::Best => Some("quality"), Speed::Auto => None };
        if let Some(q) = q { push(&["-quality", q]) }
    }
    match (v.rate, family) {
        // As before: x264 and x265 spend little on still slides, and up to the bitrate on a busy video.
        (Rate::Auto, Family::X26x) => push(&["-crf", if encoder == "libx265" { "28" } else { "23" }, "-maxrate", &rate, "-bufsize", &buf]),
        (Rate::Auto, _) => push(&["-b:v", &rate]),
        (Rate::Constant, f) => {
            push(&["-b:v", &rate, "-maxrate", &rate, "-bufsize", &buf]);
            match f {
                Family::X26x => { push(&["-minrate", &rate]); if encoder == "libx264" { push(&["-x264-params", "nal-hrd=cbr"]) } }
                Family::Nvenc => push(&["-rc", "cbr"]),
                Family::Amf => push(&["-rc", "cbr"]),
                Family::Mf => push(&["-rate_control", "cbr"]),
                _ => {}
            }
        }
        (Rate::Variable, f) => {
            push(&["-b:v", &rate, "-maxrate", &peak, "-bufsize", &buf]);
            match f {
                Family::Nvenc => push(&["-rc", "vbr"]),
                Family::Amf => push(&["-rc", "vbr_peak"]),
                Family::Mf => push(&["-rate_control", "pc_vbr"]),
                _ => {}
            }
        }
    }
    // The profile: H.264's three; HEVC has only Main for 8-bit pictures; AV1 none to choose.
    let codec = Codec::of(encoder);
    let profile = match (codec, v.profile, family) {
        (Codec::H264, Profile::Baseline, Family::Amf) => Some("constrained_baseline"),
        (Codec::H264, Profile::Baseline, Family::Mf | Family::Other) => None,
        (Codec::H264, Profile::Baseline, _) => Some("baseline"),
        (Codec::H264, Profile::Main, Family::Mf | Family::Other) => None,
        (Codec::H264, Profile::Main, _) => Some("main"),
        (Codec::H264, Profile::High, Family::Mf | Family::Other) => None,
        (Codec::H264, Profile::High, _) => Some("high"),
        (Codec::Hevc, Profile::Auto, _) | (Codec::Av1, _, _) | (_, Profile::Auto, _) => None,
        (Codec::Hevc, _, Family::Mf | Family::Other) => None,
        (Codec::Hevc, _, _) => Some("main"),
    };
    if let Some(p) = profile { push(&["-profile:v", p]) }
    // B-frames: Baseline has none.
    let b = if codec == Codec::H264 && v.profile == Profile::Baseline { Some(0) } else { v.b_frames.filter(|b| B_FRAMES.contains(b)) };
    if let Some(b) = b && !matches!(family, Family::Mf | Family::Other) { push(&["-bf", &b.to_string()]) }
    push(&["-g", &v.gop(fps).to_string(), "-pix_fmt", "yuv420p"]);
    a
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family { X26x, Nvenc, Qsv, Amf, Mf, SvtAv1, Other }

fn family(encoder: &str) -> Family {
    match encoder {
        "libx264" | "libx265" => Family::X26x,
        "libsvtav1" => Family::SvtAv1,
        e if e.ends_with("_nvenc") => Family::Nvenc,
        e if e.ends_with("_qsv") => Family::Qsv,
        e if e.ends_with("_amf") => Family::Amf,
        e if e.ends_with("_mf") => Family::Mf,
        _ => Family::Other,
    }
}

/// FFmpeg's sound options for a recording or stream that has sound.
pub fn sound_args(s: &Sound) -> Vec<String> {
    let kbps = if SOUND_KBPS.contains(&s.kbps) { s.kbps } else { 160 };
    let mut a: Vec<String> = ["-c:a", "aac", "-b:a"].map(String::from).into();
    a.push(format!("{kbps}k"));
    if SOUND_RATES.contains(&s.rate) { a.extend(["-ar".to_string(), s.rate.to_string()]) }
    if s.channels == 1 { a.extend(["-ac".to_string(), "1".to_string()]) }
    a
}

/// An SRT address with SRT's safer send: the far end waits `ms` for a lost piece to come again. One that already
/// says how long is left as it is; so are other addresses, and 0.
pub fn srt_latency(address: &str, ms: u32) -> String {
    if ms == 0 || !address.trim().to_ascii_lowercase().starts_with("srt://") || address.contains("latency=") { return address.to_string() }
    let join = if address.contains('?') { '&' } else { '?' };
    // SRT counts in microseconds.
    format!("{address}{join}latency={}", u64::from(ms) * 1000)
}

/// The sender's options that send from network card `bind` (its address): RTMP and RTMPS only (FFmpeg's SRT
/// cannot choose: the computer chooses for it).
pub fn bind_args(address: &str, bind: &str) -> Vec<String> {
    let bind = bind.trim();
    let lower = address.trim().to_ascii_lowercase();
    if bind.is_empty() || bind.parse::<std::net::IpAddr>().is_err() || !(lower.starts_with("rtmp://") || lower.starts_with("rtmps://")) { return vec![] }
    vec!["-local_addr".into(), bind.into()]
}

/// The file name a recording gets when Settings has none.
pub const DEFAULT_NAME: &str = "Recording {date} {time}";

/// A recording's name (without its extension) from `pattern`: `{date}` is the day (2026-11-07), `{time}` the time
/// (10-30-05), `{title}` the service's name. Characters a file name cannot hold are left out; an empty result is
/// `DEFAULT_NAME`'s.
pub fn file_stem(pattern: &str, at: chrono::NaiveDateTime, title: &str) -> String {
    let fill = |p: &str| p.replace("{date}", &at.format("%Y-%m-%d").to_string()).replace("{time}", &at.format("%H-%M-%S").to_string()).replace("{title}", title.trim());
    let clean = |s: String| -> String {
        let s: String = s.chars().filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') && !c.is_control()).collect();
        s.split_whitespace().collect::<Vec<_>>().join(" ").trim_matches(['.', ' ']).to_string()
    };
    let named = clean(fill(pattern));
    if named.is_empty() { clean(fill(DEFAULT_NAME)) } else { named }
}

/// How long each piece of a recording of `bytes` that lasts `secs` is when it is cut by `keep` (0: not cut): by
/// minutes, or by size (each piece a little under the size, as a whole picture only comes every two seconds).
pub fn piece_secs(keep: &Keep, bytes: u64, secs: f64) -> u32 {
    let by_time = (SPLIT_MINUTES.contains(&keep.split_minutes)).then(|| keep.split_minutes * 60);
    let by_size = (SPLIT_GB.contains(&keep.split_gb) && bytes > 0 && secs > 0.0).then(|| {
        let per_sec = bytes as f64 / secs;
        ((keep.split_gb as f64 * 1e9 * 0.95) / per_sec).max(60.0) as u32
    });
    let piece = match (by_time, by_size) { (Some(a), Some(b)) => a.min(b), (a, b) => a.or(b).unwrap_or(0) };
    if piece > 0 && f64::from(piece) < secs { piece } else { 0 }
}

/// The name of piece `n` (from 1) of `file`: `Recording … part 2.mp4`.
pub fn piece_name(file: &Path, n: u32) -> PathBuf {
    let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = file.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    file.with_file_name(format!("{stem} part {n}.{ext}"))
}

/// FFmpeg cutting `file` into pieces of `secs` (at whole pictures), copying: `piece_name`'s names, each starting
/// at 0.
pub fn split_args(file: &Path, secs: u32, format: Format) -> Vec<OsString> {
    let mut a: Vec<OsString> = ["-hide_banner", "-loglevel", "warning", "-y", "-i"].map(OsString::from).into();
    a.push(file.into());
    a.extend(["-map", "0", "-c", "copy", "-f", "segment", "-segment_time"].map(OsString::from));
    a.push(secs.to_string().into());
    a.extend(["-reset_timestamps", "1", "-segment_start_number", "1", "-segment_format", format.muxer()].map(OsString::from));
    if format == Format::Mp4 { a.extend(["-segment_format_options", "movflags=+faststart"].map(OsString::from)) }
    // `%d` in the name is the piece's number; a `%` of the name itself is doubled.
    let stem = file.file_stem().map(|s| s.to_string_lossy().replace('%', "%%")).unwrap_or_default();
    a.push(file.with_file_name(format!("{stem} part %d.{}", format.extension())).into());
    a
}

/// FFmpeg saving the last `secs` of the part being written (`part`, `elapsed` seconds long so far) into `out`,
/// copying (it starts at the whole picture before).
pub fn replay_args(part: &Path, elapsed: f64, secs: u32, out: &Path) -> Vec<OsString> {
    let from = (elapsed - f64::from(secs)).max(0.0);
    let mut a: Vec<OsString> = ["-hide_banner", "-loglevel", "warning", "-y", "-ss"].map(OsString::from).into();
    a.push(format!("{from:.3}").into());
    a.push("-i".into());
    a.push(part.into());
    a.extend(["-t".into(), format!("{secs}").into()]);
    a.extend(["-map", "0", "-c", "copy"].map(OsString::from));
    let mp4 = out.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp4"));
    if mp4 { a.extend(["-movflags", "+faststart", "-f", "mp4"].map(OsString::from)) } else { a.extend(["-f", "matroska"].map(OsString::from)) }
    a.push(out.into());
    a
}

/// "<recording name> last 5 minutes.mp4" beside the recording `out`, never over another file.
pub fn replay_name(out: &Path, minutes: u32, format: Format) -> PathBuf {
    let stem = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let base = crate::tr!("{name} last {minutes} minutes", name = stem, minutes = minutes);
    (1..).map(|i| if i == 1 { format!("{base}.{}", format.extension()) } else { format!("{base} ({i}).{}", format.extension()) })
        .map(|n| out.with_file_name(n)).find(|p| !p.exists()).expect("a free name")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::Service;

    fn has(a: &[String], pair: [&str; 2]) -> bool { a.windows(2).any(|w| w == pair) }

    #[test]
    fn untouched_more_settings_make_what_the_app_always_made() {
        let v = video_args("libx264", 3000, 30, &Video::default());
        assert_eq!(v.join(" "), "-c:v libx264 -preset veryfast -crf 23 -maxrate 3000k -bufsize 6000k -g 60 -pix_fmt yuv420p");
        assert_eq!(video_args("h264_nvenc", 6000, 30, &Video::default()).join(" "), "-c:v h264_nvenc -b:v 6000k -g 60 -pix_fmt yuv420p");
        assert_eq!(sound_args(&Sound::default()).join(" "), "-c:a aac -b:a 160k");
        assert_eq!(More::default().fps(), 30);
        assert_eq!(serde_json::to_string(&More::default()).unwrap(), "{}", "nothing saved");
    }

    #[test]
    fn constant_and_variable_bitrates_for_each_encoder() {
        let cbr = Video { rate: Rate::Constant, ..Video::default() };
        let x = video_args("libx264", 4500, 30, &cbr);
        for p in [["-b:v", "4500k"], ["-minrate", "4500k"], ["-maxrate", "4500k"], ["-bufsize", "9000k"], ["-x264-params", "nal-hrd=cbr"]] { assert!(has(&x, p), "{p:?} {x:?}") }
        assert!(!x.contains(&"-crf".to_string()));
        assert!(has(&video_args("h264_nvenc", 4500, 30, &cbr), ["-rc", "cbr"]));
        assert!(has(&video_args("h264_amf", 4500, 30, &cbr), ["-rc", "cbr"]));
        assert!(has(&video_args("h264_mf", 4500, 30, &cbr), ["-rate_control", "cbr"]));
        let q = video_args("h264_qsv", 4500, 30, &cbr);
        assert!(has(&q, ["-maxrate", "4500k"]) && !q.contains(&"-rc".to_string()), "Intel keeps a constant rate when the most is the rate: {q:?}");
        let vbr = Video { rate: Rate::Variable, ..Video::default() };
        let x = video_args("libx264", 4000, 30, &vbr);
        assert!(has(&x, ["-b:v", "4000k"]) && has(&x, ["-maxrate", "6000k"]), "{x:?}");
        assert!(has(&video_args("h264_nvenc", 4000, 30, &vbr), ["-rc", "vbr"]));
        assert!(has(&video_args("h264_amf", 4000, 30, &vbr), ["-rc", "vbr_peak"]));
    }

    #[test]
    fn speed_profile_b_frames_and_keyframes() {
        let v = Video { speed: Speed::Best, profile: Profile::High, b_frames: Some(2), keyframe_secs: 4, ..Video::default() };
        let x = video_args("libx264", 3000, 25, &v);
        for p in [["-preset", "fast"], ["-profile:v", "high"], ["-bf", "2"], ["-g", "100"]] { assert!(has(&x, p), "{p:?} {x:?}") }
        let n = video_args("h264_nvenc", 3000, 60, &Video { speed: Speed::Fastest, ..v });
        assert!(has(&n, ["-preset", "p1"]) && has(&n, ["-g", "240"]) && has(&n, ["-profile:v", "high"]), "{n:?}");
        assert!(has(&video_args("h264_amf", 3000, 30, &Video { speed: Speed::Balanced, profile: Profile::Baseline, ..Video::default() }), ["-profile:v", "constrained_baseline"]));
        assert!(has(&video_args("h264_amf", 3000, 30, &Video { speed: Speed::Balanced, ..Video::default() }), ["-quality", "balanced"]));
        // Baseline has no B-frames, whatever is chosen.
        let b = video_args("libx264", 3000, 30, &Video { profile: Profile::Baseline, b_frames: Some(3), ..Video::default() });
        assert!(has(&b, ["-bf", "0"]) && has(&b, ["-profile:v", "baseline"]), "{b:?}");
        // Windows' own and Cisco's take neither a profile nor B-frames by these names.
        let mf = video_args("h264_mf", 3000, 30, &v);
        assert!(!mf.contains(&"-profile:v".to_string()) && !mf.contains(&"-bf".to_string()), "{mf:?}");
        // HEVC: Main only; AV1: none.
        assert!(has(&video_args("libx265", 3000, 30, &v), ["-profile:v", "main"]));
        assert!(!video_args("libsvtav1", 3000, 30, &v).contains(&"-profile:v".to_string()));
        // A setting not offered is taken as the default.
        assert!(has(&video_args("libx264", 3000, 30, &Video { keyframe_secs: 99, b_frames: Some(9), ..Video::default() }), ["-g", "60"]));
        assert!(!video_args("libx264", 3000, 30, &Video { b_frames: Some(9), ..Video::default() }).contains(&"-bf".to_string()));
    }

    #[test]
    fn hevc_and_av1_use_the_same_maker_then_the_processor() {
        assert_eq!(Codec::Hevc.encoders("h264_nvenc"), ["hevc_nvenc", "libx265"]);
        assert_eq!(Codec::Av1.encoders("libx264"), ["libsvtav1"]);
        assert_eq!(Codec::Av1.encoders("h264_videotoolbox"), ["libsvtav1"], "no AV1 of the Mac's own");
        assert_eq!(Codec::H264.encoders("h264_qsv"), ["h264_qsv"]);
        assert_eq!([Codec::of("hevc_amf"), Codec::of("libsvtav1"), Codec::of("libx264")], [Codec::Hevc, Codec::Av1, Codec::H264]);
        let av1 = video_args("libsvtav1", 3000, 30, &Video::default());
        assert!(has(&av1, ["-preset", "10"]) && has(&av1, ["-b:v", "3000k"]), "{av1:?}");
        assert!(has(&video_args("libx265", 3000, 30, &Video::default()), ["-crf", "28"]));
    }

    #[test]
    fn hevc_only_when_every_destination_takes_it_and_av1_only_for_recordings() {
        assert_eq!(codec_for(Codec::Hevc, &[Service::YouTube, Service::Custom], true), Codec::Hevc);
        assert_eq!(codec_for(Codec::Hevc, &[Service::YouTube, Service::Facebook], true), Codec::H264, "Facebook does not take it");
        assert_eq!(codec_for(Codec::Av1, &[Service::YouTube], true), Codec::H264, "AV1 cannot go through the stream's feeds");
        assert_eq!(codec_for(Codec::Av1, &[], false), Codec::Av1, "a recording alone");
        assert_eq!(codec_for(Codec::H264, &[Service::TikTok], true), Codec::H264);
    }

    #[test]
    fn sound_settings() {
        assert_eq!(sound_args(&Sound { rate: 44100, kbps: 256, channels: 1 }).join(" "), "-c:a aac -b:a 256k -ar 44100 -ac 1");
        assert_eq!(sound_args(&Sound { rate: 12345, kbps: 7, channels: 2 }).join(" "), "-c:a aac -b:a 160k", "not offered: the default");
    }

    #[test]
    fn srt_safer_send_and_the_network_card() {
        assert_eq!(srt_latency("srt://h.example.org:9000", 2000), "srt://h.example.org:9000?latency=2000000");
        assert_eq!(srt_latency("srt://h:9000?streamid=abc", 500), "srt://h:9000?streamid=abc&latency=500000");
        assert_eq!(srt_latency("srt://h:9000?latency=100000", 500), "srt://h:9000?latency=100000", "the address says already");
        assert_eq!(srt_latency("rtmp://a/live/k", 500), "rtmp://a/live/k");
        assert_eq!(srt_latency("srt://h:9000", 0), "srt://h:9000");
        assert_eq!(bind_args("rtmps://live-api-s.facebook.com:443/rtmp/k", "192.168.1.20"), ["-local_addr", "192.168.1.20"]);
        assert!(bind_args("srt://h:9000", "192.168.1.20").is_empty(), "SRT cannot choose");
        assert!(bind_args("rtmp://a/live/k", "").is_empty() && bind_args("rtmp://a/live/k", "not an address").is_empty());
    }

    #[test]
    fn names_from_a_pattern() {
        let at = chrono::NaiveDate::from_ymd_opt(2026, 11, 7).unwrap().and_hms_opt(10, 30, 5).unwrap();
        assert_eq!(file_stem("", at, "x"), "Recording 2026-11-07 10-30-05", "as before");
        assert_eq!(file_stem("{date} {title}", at, "Sunday Morning"), "2026-11-07 Sunday Morning");
        assert_eq!(file_stem("{title}: part/one?", at, "Easter"), "Easter partone");
        assert_eq!(file_stem("{title}", at, ""), "Recording 2026-11-07 10-30-05", "nothing left: the default");
        assert_eq!(file_stem("  {date}   {time}. ", at, ""), "2026-11-07 10-30-05");
    }

    #[test]
    fn long_recordings_are_cut_by_time_or_size() {
        let hour = 3600.0;
        assert_eq!(piece_secs(&Keep::default(), 1_000_000_000, 2.0 * hour), 0, "not asked");
        assert_eq!(piece_secs(&Keep { split_minutes: 60, ..Keep::default() }, 1, 2.5 * hour), 3600);
        assert_eq!(piece_secs(&Keep { split_minutes: 60, ..Keep::default() }, 1, 0.5 * hour), 0, "shorter than one piece");
        // 6 GB over two hours with 2 GB pieces: under 40 minutes each.
        let s = piece_secs(&Keep { split_gb: 2, ..Keep::default() }, 6_000_000_000, 2.0 * hour);
        assert!((2200..2400).contains(&s), "{s}");
        assert_eq!(piece_secs(&Keep { split_gb: 2, split_minutes: 30, ..Keep::default() }, 6_000_000_000, 2.0 * hour), 1800, "the shorter of the two");
        assert_eq!(piece_secs(&Keep { split_minutes: 7, ..Keep::default() }, 1, 2.0 * hour), 0, "not offered");
        let f = Path::new("/r/Sunday 100%.mp4");
        assert_eq!(piece_name(f, 2), Path::new("/r/Sunday 100% part 2.mp4"));
        let a: Vec<String> = split_args(f, 3600, Format::Mp4).iter().map(|s| s.to_string_lossy().into_owned()).collect();
        let j = a.join(" ");
        assert!(j.contains("-c copy -f segment -segment_time 3600 -reset_timestamps 1 -segment_start_number 1 -segment_format mp4 -segment_format_options movflags=+faststart"), "{j}");
        assert!(a.last().unwrap().ends_with("Sunday 100%% part %d.mp4"), "{j}");
    }

    #[test]
    fn the_last_minutes_are_copied_from_the_part_being_written() {
        let a: Vec<String> = replay_args(Path::new("/r/x.recording.mkv"), 1000.0, 300, Path::new("/r/x last 5 minutes.mp4")).iter().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(a.join(" "), "-hide_banner -loglevel warning -y -ss 700.000 -i /r/x.recording.mkv -t 300 -map 0 -c copy -movflags +faststart -f mp4 /r/x last 5 minutes.mp4");
        let a: Vec<String> = replay_args(Path::new("p.mkv"), 60.0, 300, Path::new("o.mkv")).iter().map(|s| s.to_string_lossy().into_owned()).collect();
        assert!(a.join(" ").contains("-ss 0.000") && a.join(" ").ends_with("-f matroska o.mkv"), "{a:?}");
        let dir = std::env::temp_dir().join(format!("rp-replay-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("Recording a.mp4");
        assert_eq!(replay_name(&out, 5, Format::Mp4), dir.join("Recording a last 5 minutes.mp4"));
        std::fs::write(dir.join("Recording a last 5 minutes.mp4"), b"x").unwrap();
        assert_eq!(replay_name(&out, 5, Format::Mp4), dir.join("Recording a last 5 minutes (2).mp4"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn more_settings_are_kept_and_read_back() {
        let m = More {
            fps: 60, codec: Codec::Hevc,
            video: Video { rate: Rate::Constant, keyframe_secs: 1, speed: Speed::Fast, profile: Profile::Main, b_frames: Some(0) },
            sound: Sound { rate: 44100, kbps: 128, channels: 1 },
            network: Network { bind: "10.0.0.5".into(), srt_latency_ms: 2000 },
            keep: Keep { format: Format::Mkv, name: "{date} {title}".into(), split_minutes: 60, split_gb: 4, clean: true, replay_minutes: 5 },
        };
        let back: More = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.fps(), 60);
        assert_eq!(More { fps: 31, ..More::default() }.fps(), 30, "not offered: 30");
    }
}
