//! N22: recording the room screen to an MP4 file. The room's composed picture (video under the slide, overlays and
//! fades included) is read back after each frame it draws (`gl`, or a snapshot in software drawing) into a
//! `FrameSink`; a writer thread feeds it to an `ffmpeg` child at a fixed 30 frames a second (`pump`), which writes
//! H.264 into a Matroska file that survives a crash. Stopping turns it into the MP4 (`args::finish`), adding the
//! app's own sound from the files that played (`sound`), or a microphone recorded alongside.
pub mod args;
pub mod chapters;
pub mod clips;
pub mod gl;
pub mod pump;
pub mod run;
pub mod slow;
pub mod sound;
pub mod unfinished;

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
    /// A camera in the livestream: its name as FFmpeg lists it; empty: none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub camera: String,
    /// A second camera to switch to while live; empty: none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub camera2: String,
    /// Each camera's PTZ control address (VISCA over IP, `stream::ptz`); empty: it cannot be moved.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub camera_control: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub camera2_control: String,
    /// Livestream scenes chosen by themselves from what is live (`stream::scene::choose`); `None` is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_scenes: Option<bool>,
    /// Presentations go out as the room screen with the camera in the corner (not the room screen alone).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub presentation_camera: bool,
    /// Livestream: the sound held back this many milliseconds (0 to `MAX_SOUND_DELAY`), so it lines up with a
    /// camera whose picture comes late.
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub sound_delay_ms: u32,
    /// Livestream: what goes out is held back this many seconds (one of `stream::delay::DELAYS`), so Hold or
    /// Mute can stop something before anyone watching sees it; 0: none. The recording is not held back.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub stream_delay_secs: u32,
    /// Livestream: a destination whose internet cannot keep up gets a lighter stream, and the full one again later
    /// (`stream::send::Ladder`); `None` is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_lighter: Option<bool>,
    /// Livestream with both sounds: the recording also keeps each on a track of its own (`args::SoundFx::tracks`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stream_tracks: bool,
    /// The video encoder chosen by hand (one of `args::CHOOSABLE`); empty: automatic.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub encoder: String,
    /// The video bitrate chosen by hand (kbit/s, one of `BITRATES`); 0: the size's own.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub bitrate_kbps: u32,
    /// Where the corner camera and the words over the camera go.
    #[serde(default, skip_serializing_if = "crate::stream::compose::Layout::is_default")]
    pub stream_layout: crate::stream::compose::Layout,
    /// Livestream: the microphone or line-in, and the computer's own sound, turned up or down (dB, within
    /// `MAX_LEVEL_DB`).
    #[serde(skip_serializing_if = "is_zero_i32")]
    pub stream_board_db: i32,
    #[serde(skip_serializing_if = "is_zero_i32")]
    pub stream_pc_db: i32,
    /// Livestream: loud and quiet moments evened out (`None`: yes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_even: Option<bool>,
    /// Livestream: hum and hiss taken out of the microphone or line-in.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stream_quiet_noise: bool,
    /// The Starting soon, Ending and Hold cards.
    #[serde(skip_serializing_if = "crate::stream::pictures::Cards::is_default")]
    pub cards: crate::stream::pictures::Cards,
    /// Signed in: what a broadcast is called (`{date}`, `{service}`; empty: `accounts::default_title`), its
    /// description, and who can watch on YouTube.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub stream_title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub stream_description: String,
    /// Signed in to YouTube: the broadcast made ahead for the next service (its id and link); used by Go Live once.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stream_scheduled: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stream_scheduled_link: String,
    /// Signed in to YouTube: a picture file for each broadcast's thumbnail; empty: none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stream_thumbnail: String,
    #[serde(skip_serializing_if = "crate::stream::accounts::Privacy::is_public")]
    pub stream_privacy: crate::stream::accounts::Privacy,
    /// The note about streaming licences was shown (once, the first time Go Live started).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub licence_noted: bool,
}


fn is_zero_u32(v: &u32) -> bool { *v == 0 }
fn is_zero_i32(v: &i32) -> bool { *v == 0 }

/// The longest sound delay Settings offers (a camera more than a second late is broken, not slow).
pub const MAX_SOUND_DELAY: u32 = 1000;
/// The bitrates Settings offers (kbit/s), after Automatic.
pub const BITRATES: [u32; 5] = [2500, 4500, 6000, 8000, 12000];

/// The most a sound is turned up or down in Settings, in dB.
pub const MAX_LEVEL_DB: i32 = 20;

impl RecordPrefs {
    /// What a livestream does to its sound.
    pub fn sound_fx(&self) -> args::SoundFx {
        args::SoundFx {
            delay_ms: self.sound_delay_ms.min(MAX_SOUND_DELAY),
            board_db: self.stream_board_db.clamp(-MAX_LEVEL_DB, MAX_LEVEL_DB),
            pc_db: self.stream_pc_db.clamp(-MAX_LEVEL_DB, MAX_LEVEL_DB),
            even: self.stream_even.unwrap_or(true),
            quiet_noise: self.stream_quiet_noise,
            tracks: self.stream_tracks,
        }
    }

    /// The automatic scene choice's settings.
    pub fn auto(&self) -> crate::stream::scene::AutoPrefs {
        crate::stream::scene::AutoPrefs { on: self.auto_scenes.unwrap_or(true), presentation_camera: self.presentation_camera }
    }

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
    /// The recording's sound tracks, as players list them.
    pub fn track_mix() -> String { tr!("Everything mixed") }
    pub fn track_board() -> String { tr!("Microphone or line-in") }
    pub fn track_pc() -> String { tr!("This computer") }
    pub fn no_h264() -> String { tr!("This copy of FFmpeg cannot make H.264 video. Install the full FFmpeg and try again.") }
    pub fn clip_started() -> String { tr!("Clip started. Press Clip again to end it; it is saved as its own video when the recording stops.") }
    pub fn clip_marked() -> String { tr!("Clip marked. It is saved as its own video when the recording stops.") }
    pub fn clip_saved(path: &str) -> String { tr!("Clip saved: {path}", path = path) }
    pub fn clip_failed(path: &str) -> String { tr!("The clip could not be saved: {path}", path = path) }
    pub fn was_live() -> String { tr!("RustyPresenter stopped in the middle of a livestream. The recording up to then has been kept. Go live again?") }
    pub fn go_live_again() -> String { tr!("Go Live") }
    pub fn too_slow() -> String { tr!("This computer is not keeping up, so the recording and stream are jerky. Choose 720p under Picture size in Settings → Media, or close other programs.") }
    pub fn smooth_again() -> String { tr!("The computer is keeping up again: the picture is smooth.") }
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
    pub fn no_sound(name: &str) -> String { tr!("No sound has come from {name} for 10 seconds. Check the cable, and that the sound board is sending to this computer.", name = name) }
    pub fn sound_back(name: &str) -> String { tr!("Sound from {name} is coming in again.", name = name) }
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
    fn the_streams_sound_settings_are_kept_and_given_to_the_recorder() {
        let p = RecordPrefs::default();
        assert_eq!(p.sound_fx(), args::SoundFx { delay_ms: 0, board_db: 0, pc_db: 0, even: true, quiet_noise: false, tracks: false }, "evened out unless turned off");
        assert_eq!(serde_json::to_value(&p).unwrap().get("stream_even"), None);
        let p = RecordPrefs { sound_delay_ms: 5000, stream_board_db: 45, stream_pc_db: -45, stream_even: Some(false), stream_quiet_noise: true, ..RecordPrefs::default() };
        assert_eq!(p.sound_fx(), args::SoundFx { delay_ms: MAX_SOUND_DELAY, board_db: MAX_LEVEL_DB, pc_db: -MAX_LEVEL_DB, even: false, quiet_noise: true, tracks: false });
        let back: RecordPrefs = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
        assert_eq!(back, p);
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
