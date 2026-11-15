//! N22: recording the room screen to an MP4 file. The room's composed picture (video under the slide, overlays and
//! fades included) is read back after each frame it draws (`gl`, or a snapshot in software drawing) into a
//! `FrameSink`; a writer thread feeds it to an `ffmpeg` child at a fixed 30 frames a second (`pump`), which writes
//! H.264 into a Matroska file that survives a crash. Stopping turns it into the MP4 (`args::finish`), adding the
//! app's own sound from the files that played (`sound`), or a microphone recorded alongside.
pub mod args;
pub mod gl;
pub mod pump;
pub mod run;
pub mod sound;

pub use pump::FrameSink;
pub use run::{Ended, Plan, Recording};

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Frames a second in every recording.
pub const FPS: u32 = 30;

/// The picture size of a recording.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Preset {
    #[serde(rename = "720p")]
    P720,
    #[default]
    #[serde(rename = "1080p")]
    P1080,
}

impl Preset {
    pub const ALL: [Preset; 2] = [Preset::P720, Preset::P1080];

    pub fn size(self) -> (u32, u32) {
        match self { Preset::P720 => (1280, 720), Preset::P1080 => (1920, 1080) }
    }

    /// The highest video rate, kbit/s. Slides need far less; a busy video gets this much.
    pub fn max_kbps(self) -> u32 {
        match self { Preset::P720 => 3000, Preset::P1080 => 6000 }
    }

    /// The most one hour can take on disk (video at its highest rate, plus sound).
    pub fn bytes_per_hour(self) -> u64 { (u64::from(self.max_kbps()) + 192) * 1000 / 8 * 3600 }
}

/// Where the recording's sound comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sound {
    None,
    /// The sound of the videos and songs the room plays, taken from their files when the recording stops.
    App,
    /// A microphone or line-in, as `args::list_inputs` names it.
    Input(String),
}

impl Sound {
    /// As saved: `none`, `app` or `input:<device>`.
    pub fn parse(s: &str) -> Sound {
        match s {
            "none" => Sound::None,
            "app" | "" => Sound::App,
            _ => s.strip_prefix("input:").filter(|d| !d.is_empty()).map_or(Sound::App, |d| Sound::Input(d.into())),
        }
    }

    pub fn key(&self) -> String {
        match self { Sound::None => "none".into(), Sound::App => "app".into(), Sound::Input(d) => format!("input:{d}") }
    }
}

/// Settings → Media → Recording (N22).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct RecordPrefs {
    pub size: Preset,
    /// Empty: `default_folder`.
    pub folder: String,
    /// `Sound::key`; empty is the app's sound.
    pub sound: String,
    /// Where Go Live sends the recording as it is made (their keys are not here: `stream::keys`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stream: Vec<crate::stream::Destination>,
    /// The sound output whose sound goes out with a livestream: empty, the one the app plays to; "off", none;
    /// else its name as `Settings::audio_device` has it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub stream_sound: String,
}

impl RecordPrefs {
    /// The output to capture for a livestream (`app_output`: where the app plays its sound); `None`: no PC sound.
    pub fn pc_sound(&self, app_output: &str) -> Option<String> {
        match self.stream_sound.as_str() {
            "off" => None,
            "" => Some(app_output.to_string()),
            d => Some(d.to_string()),
        }
    }
}

impl RecordPrefs {
    pub fn folder(&self) -> PathBuf {
        if self.folder.trim().is_empty() { default_folder() } else { PathBuf::from(self.folder.trim()) }
    }

    pub fn sound(&self) -> Sound { Sound::parse(&self.sound) }
}

/// The computer's Videos folder (else the home folder), in a folder of its own.
pub fn default_folder() -> PathBuf {
    dirs::video_dir().or_else(dirs::home_dir).unwrap_or_else(std::env::temp_dir).join("RustyPresenter recordings")
}

/// The finished file's name, from the time it started: `Recording 2026-11-07 10-30-05.mp4`. Never one that exists.
pub fn file_name(folder: &Path, at: chrono::NaiveDateTime) -> PathBuf {
    let stem = format!("Recording {}", at.format("%Y-%m-%d %H-%M-%S"));
    (1..).map(|i| if i == 1 { stem.clone() } else { format!("{stem} ({i})") })
        .map(|s| folder.join(format!("{s}.mp4")))
        .find(|p| !p.exists() && !run::partial_of(p).exists())
        .expect("some name is free")
}

/// "1:02:03" or "2:03": how long it has recorded, for the Record button.
pub fn clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

pub mod tr {
    use crate::tr;
    pub fn no_ffmpeg(hint: &str) -> String { tr!("Recording needs FFmpeg, a free program. {hint}", hint = hint) }
    pub fn no_h264() -> String { tr!("This copy of FFmpeg cannot make H.264 video. Install the full FFmpeg and try again.") }
    pub fn no_room() -> String { tr!("Turn the room screen on first: the recording is what the room shows.") }
    pub fn no_folder(path: &str, why: &str) -> String { tr!("Could not use the recordings folder {path}: {why}", path = path, why = why) }
    pub fn low_disk(free: &str, need: &str) -> String { tr!("The recordings folder's disk has only {free} free. An hour of recording needs about {need}. Choose another folder in Settings → Media.", free = free, need = need) }
    pub fn disk_full() -> String { tr!("The disk is almost full, so the recording stopped.") }
    pub fn not_started(why: &str) -> String { tr!("The recording could not start: {why}", why = why) }
    pub fn failed(why: &str) -> String { tr!("The recording stopped: {why}", why = why) }
    pub fn recording() -> String { tr!("Recording the room screen.") }
    pub fn finishing() -> String { tr!("Saving the recording…") }
    pub fn saved(path: &str) -> String { tr!("Recording saved: {path}", path = path) }
    pub fn saved_mkv(path: &str) -> String { tr!("Recording saved as {path} (it could not be turned into MP4).", path = path) }
    pub fn no_app_sound() -> String { tr!("The recording was saved without the app's sound, which could not be added.") }
    pub fn ffmpeg_ended() -> String { tr!("FFmpeg stopped unexpectedly") }
    pub fn none() -> String { tr!("No sound") }
    pub fn app_sound() -> String { tr!("The sound the app plays (videos and songs)") }
    pub fn input(name: &str) -> String { tr!("Microphone or line-in: {name}", name = name) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_their_sizes_and_save_by_name() {
        assert_eq!(Preset::P720.size(), (1280, 720));
        assert_eq!(Preset::P1080.size(), (1920, 1080));
        assert_eq!(serde_json::to_string(&Preset::P720).unwrap(), "\"720p\"");
        let p: RecordPrefs = serde_json::from_str(r#"{"size":"720p"}"#).unwrap();
        assert_eq!((p.size, p.sound()), (Preset::P720, Sound::App));
        assert_eq!(RecordPrefs::default().size, Preset::P1080);
    }

    #[test]
    fn an_hour_at_1080p_is_under_three_gigabytes() {
        let gb = Preset::P1080.bytes_per_hour() as f64 / 1e9;
        assert!((2.0..3.0).contains(&gb), "{gb}");
        assert!(Preset::P720.bytes_per_hour() < Preset::P1080.bytes_per_hour());
    }

    #[test]
    fn sound_choices_round_trip() {
        for s in [Sound::None, Sound::App, Sound::Input("Microphone (USB)".into())] { assert_eq!(Sound::parse(&s.key()), s) }
        assert_eq!(Sound::parse(""), Sound::App);
        assert_eq!(Sound::parse("input:"), Sound::App);
        assert_eq!(Sound::parse("input:a:b"), Sound::Input("a:b".into()));
    }

    #[test]
    fn the_folder_defaults_to_a_recordings_folder() {
        assert!(RecordPrefs::default().folder().ends_with("RustyPresenter recordings"));
        assert_eq!(RecordPrefs { folder: " /x/y ".into(), ..Default::default() }.folder(), PathBuf::from("/x/y"));
    }

    #[test]
    fn file_names_come_from_the_time_and_never_clash() {
        let dir = std::env::temp_dir().join(format!("rp-record-names-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let at = chrono::NaiveDate::from_ymd_opt(2026, 11, 7).unwrap().and_hms_opt(10, 30, 5).unwrap();
        let first = file_name(&dir, at);
        assert_eq!(first, dir.join("Recording 2026-11-07 10-30-05.mp4"));
        std::fs::write(&first, b"").unwrap();
        assert_eq!(file_name(&dir, at), dir.join("Recording 2026-11-07 10-30-05 (2).mp4"));
        std::fs::write(run::partial_of(&dir.join("Recording 2026-11-07 10-30-05 (2).mp4")), b"").unwrap();
        assert_eq!(file_name(&dir, at), dir.join("Recording 2026-11-07 10-30-05 (3).mp4"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_clock_shows_hours_only_when_needed() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(125), "2:05");
        assert_eq!(clock(3723), "1:02:03");
    }
}
