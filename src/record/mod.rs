//! N22: recording the audience screen to an MP4 file. The room's composed picture (video under the slide, overlays and
//! fades included) is read back after each frame it draws (`gl`, or a snapshot in software drawing) into a
//! `FrameSink`; a writer thread feeds it to an `ffmpeg` child at a fixed 30 frames a second (`pump`), which writes
//! H.264 into a Matroska file that survives a crash. Stopping turns it into the MP4 (`args::finish`), adding the
//! app's own sound from the files that played (`sound`), or a microphone recorded alongside.
pub mod advanced;
pub mod args;
pub mod chapters;
pub mod clips;
pub mod devices;
pub mod gl;
pub mod peek;
pub mod pump;
pub mod run;
pub mod slow;
pub mod sound;
pub mod setups;
pub mod unfinished;

pub use pump::FrameSink;
pub use run::{Ended, Plan, Recording};

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Frames a second in every recording.
pub const FPS: u32 = 30;

/// The picture size of a recording and of the livestream (November 17, 2026: any size, not only 720p and 1080p).
/// Saved as "480p", "720p", "1080p", "1440p" or "2160p", or "1600x900" for a size typed by hand. Anything else
/// (a newer or broken file) reads as 1080p, so a settings file never fails to load because of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum Preset {
    P480,
    P720,
    #[default]
    P1080,
    P1440,
    /// 4K (3840 × 2160).
    P2160,
    /// A size typed by hand: always even and within `Preset::MIN`–`Preset::MAX` (`Preset::custom` keeps it so).
    Custom(u32, u32),
}

impl Preset {
    /// The sizes offered by name, smallest first ("Custom" comes after them in Settings).
    pub const ALL: [Preset; 5] = [Preset::P480, Preset::P720, Preset::P1080, Preset::P1440, Preset::P2160];
    /// The smallest and largest size that can be typed: smaller is unreadable, larger is past what encoders take.
    pub const MIN: (u32, u32) = (320, 180);
    pub const MAX: (u32, u32) = (7680, 4320);

    /// A size typed by hand, kept within `MIN`–`MAX` and made even (encoders need even sides). A typed size that is
    /// one of the named ones is that one.
    pub fn custom(w: u32, h: u32) -> Preset {
        let even = |v: u32, lo: u32, hi: u32| v.clamp(lo, hi) & !1;
        let size = (even(w, Self::MIN.0, Self::MAX.0), even(h, Self::MIN.1, Self::MAX.1));
        Self::ALL.into_iter().find(|p| p.size() == size).unwrap_or(Preset::Custom(size.0, size.1))
    }

    pub fn size(self) -> (u32, u32) {
        match self {
            Preset::P480 => (854, 480),
            Preset::P720 => (1280, 720),
            Preset::P1080 => (1920, 1080),
            Preset::P1440 => (2560, 1440),
            Preset::P2160 => (3840, 2160),
            Preset::Custom(w, h) => (w, h),
        }
    }

    /// The highest video rate at 30 pictures a second, kbit/s. Slides need far less; a busy video gets this much.
    /// The named sizes have the rates streaming services ask for; a typed size gets one from its number of pixels.
    pub fn max_kbps(self) -> u32 {
        match self {
            Preset::P480 => 1500,
            Preset::P720 => 3000,
            Preset::P1080 => 6000,
            Preset::P1440 => 9000,
            Preset::P2160 => 20000,
            Preset::Custom(w, h) => {
                let share = f64::from(w) * f64::from(h) / (1920.0 * 1080.0);
                ((6000.0 * share.powf(0.8) / 100.0).round() as u32 * 100).max(500)
            }
        }
    }

    /// The highest video rate at `fps` pictures a second: half again over 30 (more pictures need more bits, but
    /// each changes less from the one before).
    pub fn kbps_at(self, fps: u32) -> u32 { if fps > 30 { self.max_kbps() * 3 / 2 } else { self.max_kbps() } }

    /// The most one hour can take on disk (video at its highest rate, plus sound).
    pub fn bytes_per_hour(self) -> u64 { (u64::from(self.max_kbps()) + 192) * 1000 / 8 * 3600 }

    /// How it is saved: "720p", or "1600x900" for a typed size.
    pub fn key(self) -> String {
        match self {
            Preset::P480 => "480p".into(),
            Preset::P720 => "720p".into(),
            Preset::P1080 => "1080p".into(),
            Preset::P1440 => "1440p".into(),
            Preset::P2160 => "2160p".into(),
            Preset::Custom(w, h) => format!("{w}x{h}"),
        }
    }

    /// A saved size; `None` for anything it cannot be.
    pub fn parse(s: &str) -> Option<Preset> {
        let s = s.trim().to_ascii_lowercase();
        Some(match s.as_str() {
            "480p" => Preset::P480,
            "720p" => Preset::P720,
            "1080p" => Preset::P1080,
            "1440p" => Preset::P1440,
            "2160p" | "4k" => Preset::P2160,
            _ => {
                let (w, h) = s.split_once(['x', '×'])?;
                Preset::custom(w.trim().parse().ok()?, h.trim().parse().ok()?)
            }
        })
    }

    /// What Settings shows: "1080p", "4K", or "1600 × 900".
    pub fn label(self) -> String {
        match self { Preset::P2160 => "4K".into(), Preset::Custom(w, h) => format!("{w} × {h}"), p => p.key() }
    }
}

impl Serialize for Preset {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.serialize_str(&self.key()) }
}

impl<'de> Deserialize<'de> for Preset {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Lenient: whatever is saved, the settings still load (an unknown size is 1080p).
        let v = serde_json::Value::deserialize(d)?;
        Ok(v.as_str().and_then(Preset::parse).unwrap_or_default())
    }
}

/// Words for a size this computer or the internet may not keep up with (`None`: nothing to say). `cores` is the
/// computer's processor count; `fps` the pictures a second.
pub fn size_warning(size: Preset, fps: u32, cores: usize) -> Option<String> {
    let (w, h) = size.size();
    // How much work it is, next to 1080p at 30 pictures a second.
    let load = f64::from(w) * f64::from(h) * f64::from(fps.max(1)) / (1920.0 * 1080.0 * 30.0);
    let heavy = (load > 1.01 && cores < 4) || (load > 2.01 && cores < 8) || (load > 4.01 && cores < 12) || load > 8.01;
    let mut words = vec![];
    if heavy {
        words.push(crate::tr!("{size} at {fps} pictures a second may be more than this computer can make while live. If the stream stutters, choose a smaller size.", size = size.label(), fps = fps));
    }
    if w > 4096 || h > 4096 {
        words.push(crate::tr!("Graphics card encoders stop at 4096 wide for H.264, so the processor makes this size: it is slow."));
    }
    if size.kbps_at(fps) > 12000 {
        let mbps = format!("{:.0}", f64::from(size.kbps_at(fps) + 192) * 1.5 / 1000.0);
        words.push(crate::tr!("This size needs about {mbps} Mbit/s of upload for each destination. Test the upload speed before Sunday.", mbps = mbps));
    }
    (!words.is_empty()).then(|| words.join(" "))
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

/// Settings → Livestream → Recording (N22).
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
    /// A third and fourth camera, as the second, each with its PTZ control address.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub camera3: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub camera3_control: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub camera4: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub camera4_control: String,
    /// Livestream scenes chosen by themselves from what is live (`stream::scene::choose`); `None` is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_scenes: Option<bool>,
    /// Presentations go out as the audience screen with the camera in the corner (not the audience screen alone).
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
    /// Livestream: the stream's picture and sound go out over NDI too (`stream::ndi::Out`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ndi_stream: bool,
    /// Livestream: the words over the camera go out over NDI alone, see-through around them, for keying.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ndi_words: bool,
    /// Livestream: the words over the camera are also recorded alone, see-through around them, beside the
    /// recording (`stream::words_file`), for video editing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub words_file: bool,
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
    /// The church's own livestream scenes (`stream::own_scenes`), in the order of the scene button.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub own_scenes: Vec<crate::stream::own_scenes::OwnScene>,
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
    /// Livestream: the sound set up with one choice (`stream::sound_setup`); Custom: as set by hand.
    #[serde(default, skip_serializing_if = "crate::stream::sound_setup::Mode::is_custom")]
    pub stream_sound_mode: crate::stream::sound_setup::Mode,
    /// Livestream: a second and third microphone or line-in (names as `args::list_inputs` gives them; empty:
    /// none), mixed with the recording's sound input, each turned up or down (dB, within `MAX_LEVEL_DB`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stream_input2: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stream_input3: String,
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub stream_input2_db: i32,
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub stream_input3_db: i32,
    /// Livestream: each sound's filters (More sound settings); `None`: as they start (only the limiter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_filters: Option<crate::stream::sound_setup::StreamFilters>,
    /// Livestream: where the stream's sound is played to listen to it (`stream::listen`): empty, nowhere;
    /// `stream::listen::DEFAULT`, the computer's usual output; else an output as `Settings::audio_device` has it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stream_listen: String,
    /// Livestream: live captions and the website player (`stream::extras`).
    #[serde(default, skip_serializing_if = "crate::stream::extras::Extras::is_default")]
    pub extras: crate::stream::extras::Extras,
    /// Livestream → More settings (`advanced`): pictures a second, video format, sound, network, recording.
    #[serde(default, skip_serializing_if = "advanced::More::is_default")]
    pub more: advanced::More,
    /// Whole livestream setups saved by name ("Sunday", "Wedding"), to use again (`setups`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub setups: Vec<setups::Setup>,
    /// The processor's encoder (x264) could not keep up on this computer while live: Automatic now uses the
    /// graphics card's encoder when there is one that works (`run::Plan::make`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub x264_too_slow: bool,
    /// Go Live: what shows in a camera's place while it sends nothing (`stream::camera_lost`).
    #[serde(skip_serializing_if = "crate::stream::camera_lost::CameraLost::is_default")]
    pub camera_lost: crate::stream::camera_lost::CameraLost,
    /// How the stream changes between scenes, unless the church's scene has its own way (`stream::transition`).
    #[serde(default, skip_serializing_if = "crate::stream::transition::Transition::is_fade")]
    pub scene_transition: crate::stream::transition::Transition,
}


fn is_zero_u32(v: &u32) -> bool { *v == 0 }
fn is_zero_i32(v: &i32) -> bool { *v == 0 }

/// The longest sound delay Settings offers (a camera more than a second late is broken, not slow).
pub const MAX_SOUND_DELAY: u32 = 1000;
/// The bitrates Settings offers (kbit/s), after Automatic.
pub const BITRATES: [u32; 9] = [1500, 2500, 4500, 6000, 8000, 12000, 20000, 35000, 50000];

/// The most a sound is turned up or down in Settings, in dB.
pub const MAX_LEVEL_DB: i32 = 20;

impl RecordPrefs {
    /// What a livestream does to its sound.
    /// What goes out over NDI while live.
    pub fn ndi(&self) -> crate::stream::ndi::OutPrefs { crate::stream::ndi::OutPrefs { stream: self.ndi_stream, words: self.ndi_words } }

    pub fn sound_fx(&self) -> args::SoundFx {
        args::SoundFx {
            delay_ms: self.sound_delay_ms.min(MAX_SOUND_DELAY),
            board_db: self.stream_board_db.clamp(-MAX_LEVEL_DB, MAX_LEVEL_DB),
            pc_db: self.stream_pc_db.clamp(-MAX_LEVEL_DB, MAX_LEVEL_DB),
            even: self.stream_even.unwrap_or(true),
            quiet_noise: self.stream_quiet_noise,
            tracks: self.stream_tracks,
            board_filters: self.filters().board,
            pc_filters: self.filters().pc,
        }
    }

    /// Each sound's filters for the stream.
    pub fn filters(&self) -> crate::stream::sound_setup::StreamFilters { self.stream_filters.unwrap_or_default() }

    /// The stream's second and third microphone or line-in: `(name, dB, filters)`, only with a first one, and each
    /// only after the one before, never one already used.
    pub fn more_inputs(&self) -> Vec<(String, i32, args::Filters)> {
        let Sound::Input(first) = self.sound() else { return vec![] };
        let f = self.filters();
        let mut out: Vec<(String, i32, args::Filters)> = vec![];
        for (name, db, filters) in [(&self.stream_input2, self.stream_input2_db, f.input2), (&self.stream_input3, self.stream_input3_db, f.input3)] {
            if name.is_empty() || *name == first || out.iter().any(|(o, _, _)| o == name) { break }
            out.push((name.clone(), db.clamp(-MAX_LEVEL_DB, MAX_LEVEL_DB), filters));
        }
        out
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

    /// The livestream's cameras in Settings' order, each with its PTZ control address: up to `MAX_CAMERAS`, ending
    /// at the first row with none (a camera with none before it is not used), each camera once.
    pub fn cameras(&self) -> Vec<(String, String)> {
        let rows = [(&self.camera, &self.camera_control), (&self.camera2, &self.camera2_control), (&self.camera3, &self.camera3_control), (&self.camera4, &self.camera4_control)];
        let mut out: Vec<(String, String)> = vec![];
        for (c, ctl) in rows {
            if c.is_empty() || out.iter().any(|(o, _)| o == c) { break }
            out.push((c.clone(), ctl.clone()));
        }
        out
    }

    /// Camera `n`'s control address (1 is the first), as Settings has it.
    pub fn camera_control_of(&self, n: u8) -> &str {
        match n { 1 => &self.camera_control, 2 => &self.camera2_control, 3 => &self.camera3_control, 4 => &self.camera4_control, _ => "" }
    }
}

/// The most cameras a livestream switches between.
pub const MAX_CAMERAS: usize = 4;

/// The computer's Videos folder (else the home folder), in a folder of its own.
pub fn default_folder() -> PathBuf {
    dirs::video_dir().or_else(dirs::home_dir).unwrap_or_else(std::env::temp_dir).join("RustyPresenter recordings")
}

/// The finished file's name, from the time it started: `Recording 2026-11-07 10-30-05.mp4`. Never one that exists.
pub fn file_name(folder: &Path, at: chrono::NaiveDateTime) -> PathBuf { named(folder, &advanced::file_stem("", at, ""), "mp4") }

/// `<stem>.<ext>` in `folder` (More settings name it: `advanced::file_stem`), `<stem> (2).<ext>` and on when that
/// is taken. Never one that exists, nor one being recorded to.
pub fn named(folder: &Path, stem: &str, ext: &str) -> PathBuf {
    (1..).map(|i| if i == 1 { stem.to_string() } else { format!("{stem} ({i})") })
        .map(|s| folder.join(format!("{s}.{ext}")))
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
    pub fn too_slow() -> String { tr!("This computer is not keeping up, so the recording and stream are jerky. Choose a smaller picture size in Settings → Livestream → Picture and quality, or the graphics card under Video encoder, and close other programs.") }
    pub fn smooth_again() -> String { tr!("The computer is keeping up again: the picture is smooth.") }
    pub fn no_room() -> String { tr!("Turn the audience screen on first: the recording is what the room shows.") }
    pub fn no_folder(path: &str, why: &str) -> String { tr!("Could not use the recordings folder {path}: {why}", path = path, why = why) }
    pub fn low_disk(free: &str, need: &str) -> String { tr!("The recordings folder's disk has only {free} free. An hour of recording needs about {need}. Choose another folder in Settings → Livestream → Recording.", free = free, need = need) }
    pub fn disk_full() -> String { tr!("The disk is almost full, so the recording stopped.") }
    pub fn not_started(why: &str) -> String { tr!("The recording could not start: {why}", why = why) }
    pub fn failed(why: &str) -> String { tr!("The recording stopped: {why}", why = why) }
    pub fn recording() -> String { tr!("Recording the audience screen.") }
    pub fn finishing() -> String { tr!("Saving the recording…") }
    pub fn saved(path: &str) -> String { tr!("Recording saved: {path}", path = path) }
    pub fn saved_mkv(path: &str) -> String { tr!("Recording saved as {path} (it could not be turned into MP4).", path = path) }
    /// Cut into pieces (More settings): the first, and how many there are.
    pub fn saved_pieces(path: &str, n: usize) -> String { tr!("Recording saved in {n} pieces, the first: {path}", n = n, path = path) }
    /// "Save the last minutes" (More settings).
    pub fn saving_last(minutes: u32) -> String { tr!("Saving the last {minutes} minutes…", minutes = minutes) }
    pub fn saved_last(path: &str) -> String { tr!("The last minutes are saved: {path}", path = path) }
    pub fn save_last_failed(path: &str) -> String { tr!("The last minutes could not be saved: {path}", path = path) }
    /// The processor's encoder could not keep up, and the encoder is left to the app.
    pub fn too_slow_next_time() -> String { tr!("Next time, the graphics card's encoder is used by itself, if this computer has one that works.") }
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
    fn any_size_can_be_saved_and_old_settings_still_load() {
        // Old files: "720p" and "1080p" as before; the new names; a typed size; anything else is 1080p.
        for (saved, size) in [("720p", (1280, 720)), ("1080p", (1920, 1080)), ("480p", (854, 480)), ("1440p", (2560, 1440)), ("2160p", (3840, 2160)), ("4K", (3840, 2160)), ("1600x900", (1600, 900)), ("1601×901", (1600, 900))] {
            let p: RecordPrefs = serde_json::from_value(serde_json::json!({ "size": saved })).unwrap();
            assert_eq!(p.size.size(), size, "{saved}");
        }
        for odd in [serde_json::json!("8K please"), serde_json::json!(1080), serde_json::json!(null), serde_json::json!({"w": 1})] {
            let p: RecordPrefs = serde_json::from_value(serde_json::json!({ "size": odd, "folder": "x" })).unwrap();
            assert_eq!((p.size, p.folder.as_str()), (Preset::P1080, "x"), "{odd}");
        }
        // Kept within the limits, even, and a typed named size is that name.
        assert_eq!(Preset::custom(10, 10).size(), Preset::MIN);
        assert_eq!(Preset::custom(99999, 99999).size(), Preset::MAX);
        assert_eq!(Preset::custom(1280, 720), Preset::P720);
        let c = Preset::custom(1001, 563);
        assert_eq!((c.size(), c.key().as_str()), ((1000, 562), "1000x562"));
        assert_eq!(serde_json::from_str::<Preset>(&serde_json::to_string(&c).unwrap()).unwrap(), c);
        assert_eq!(Preset::P2160.label(), "4K");
        // The bitrate grows with the size and with the frame rate.
        let rates: Vec<u32> = Preset::ALL.iter().map(|p| p.max_kbps()).collect();
        assert!(rates.windows(2).all(|w| w[0] < w[1]), "{rates:?}");
        let between = Preset::custom(1600, 900).max_kbps();
        assert!(between > 3000 && between < 6000, "{between}");
        assert_eq!(Preset::P1080.kbps_at(60), 9000);
        assert_eq!(Preset::P1080.kbps_at(30), 6000);
    }

    #[test]
    fn big_sizes_are_warned_about_but_allowed() {
        assert_eq!(size_warning(Preset::P1080, 30, 4), None);
        assert_eq!(size_warning(Preset::P720, 60, 2), None);
        let w = size_warning(Preset::P2160, 30, 4).expect("4K on a 4-core computer");
        assert!(w.contains("4K at 30") && w.contains("Mbit/s"), "{w}");
        assert!(size_warning(Preset::custom(7680, 4320), 30, 32).unwrap().contains("4096"));
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
        assert_eq!(p.sound_fx(), args::SoundFx { delay_ms: 0, board_db: 0, pc_db: 0, even: true, quiet_noise: false, tracks: false, board_filters: args::Filters::SAFE, pc_filters: args::Filters::SAFE }, "evened out unless turned off");
        assert_eq!(serde_json::to_value(&p).unwrap().get("stream_even"), None);
        let p = RecordPrefs { sound_delay_ms: 5000, stream_board_db: 45, stream_pc_db: -45, stream_even: Some(false), stream_quiet_noise: true, ..RecordPrefs::default() };
        assert_eq!(p.sound_fx(), args::SoundFx { delay_ms: MAX_SOUND_DELAY, board_db: MAX_LEVEL_DB, pc_db: -MAX_LEVEL_DB, even: false, quiet_noise: true, tracks: false, board_filters: args::Filters::SAFE, pc_filters: args::Filters::SAFE });
        let back: RecordPrefs = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn the_other_inputs_filters_and_listen_are_kept() {
        use crate::stream::sound_setup::{Mode, StreamFilters};
        // Nothing new saved for settings as they start.
        let v = serde_json::to_value(RecordPrefs::default()).unwrap();
        for k in ["stream_sound_mode", "stream_input2", "stream_filters", "stream_listen"] { assert_eq!(v.get(k), None, "{k}") }
        let gate = args::Filters { gate: args::Strength::Light, ..args::Filters::SAFE };
        let p = RecordPrefs {
            sound: "input:Line In".into(), stream_input2: "USB Mic".into(), stream_input2_db: 30, stream_input3: "Phone".into(), stream_input3_db: -3,
            stream_filters: Some(StreamFilters { input2: gate, ..StreamFilters::default() }), stream_listen: "default".into(), stream_sound_mode: Mode::UsbMic,
            ..RecordPrefs::default()
        };
        let back: RecordPrefs = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        assert_eq!(p.more_inputs(), [("USB Mic".to_string(), MAX_LEVEL_DB, gate), ("Phone".to_string(), -3, args::Filters::SAFE)]);
        // Only with a first input, each after the one before, and never the same twice.
        assert!(RecordPrefs { sound: "app".into(), ..p.clone() }.more_inputs().is_empty());
        assert_eq!(RecordPrefs { stream_input2: String::new(), ..p.clone() }.more_inputs(), []);
        assert_eq!(RecordPrefs { stream_input3: "USB Mic".into(), ..p.clone() }.more_inputs().len(), 1);
        assert_eq!(RecordPrefs { stream_input2: "Line In".into(), ..p.clone() }.more_inputs(), []);
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

    #[test]
    fn the_cameras_are_taken_in_order_up_to_four_each_once() {
        let p = RecordPrefs { camera: "A".into(), camera2: "B".into(), camera3: "C".into(), camera3_control: "192.168.1.63".into(), camera4: "D".into(), ..Default::default() };
        let names: Vec<String> = p.cameras().into_iter().map(|(c, _)| c).collect();
        assert_eq!(names, ["A", "B", "C", "D"]);
        assert_eq!(p.cameras()[2].1, "192.168.1.63");
        assert_eq!(p.camera_control_of(3), "192.168.1.63");
        // None before it, or the same camera again: it and those after are not used.
        let gap = RecordPrefs { camera: "A".into(), camera3: "C".into(), ..Default::default() };
        assert_eq!(gap.cameras().len(), 1);
        let twice = RecordPrefs { camera: "A".into(), camera2: "A".into(), camera3: "C".into(), ..Default::default() };
        assert_eq!(twice.cameras().len(), 1);
        assert!(RecordPrefs::default().cameras().is_empty());
        let saved = serde_json::to_value(RecordPrefs::default()).unwrap();
        assert!(saved.get("camera3").is_none() && saved.get("camera4_control").is_none(), "none: nothing saved");
    }
}
