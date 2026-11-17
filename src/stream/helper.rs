//! The first-time Go Live helper (livestreaming design §4 item 9, "first-time wizard"): when Go Live is pressed
//! with nowhere to stream to, a few plain steps instead of a message: where to stream (paste the key), which
//! camera, which sound, a check, done. The choices are the same ones Settings → Media → Livestream keeps, so the
//! church can change them there later. Pure: the dialog is `ui::dialogs::go_live_helper`.
use super::{Destination, Service, built_in_id};
use crate::record::RecordPrefs;
use crate::tr;

/// Whether Go Live should open the helper: no destination is switched on and NDI out is off (nothing would go
/// anywhere).
pub fn needed(prefs: &RecordPrefs) -> bool {
    !prefs.stream.iter().any(|d| d.on) && !prefs.ndi().any()
}

/// What the helper's steps chose.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Choice {
    /// Settings' row: 0 YouTube, 1 Facebook, 2 Twitch, 3 another server.
    pub service: usize,
    /// The other server's address (row 3 only).
    pub url: String,
    /// The camera's name as FFmpeg opens it; empty: no camera (the room screen goes out).
    pub camera: String,
    /// The sound's saved key (`record::Sound::key`).
    pub sound: String,
}

/// Puts the helper's choices into the settings: that destination switched on (the others left as they were), the
/// camera first, the sound chosen. The key itself goes to the computer's password store, not here.
pub fn apply(prefs: &mut RecordPrefs, c: &Choice) {
    let i = c.service.min(3);
    let id = built_in_id(i);
    let service = super::BUILT_IN[i].0;
    match prefs.stream.iter_mut().find(|d| d.id == id) {
        Some(d) => { d.on = true; d.service = service; if i == 3 { d.url = c.url.trim().to_string() } }
        None => prefs.stream.push(Destination { id, service, on: true, url: if i == 3 { c.url.trim().to_string() } else { String::new() }, ..Destination::default() }),
    }
    if c.camera.trim().is_empty() {
        prefs.camera.clear();
    } else if prefs.camera != c.camera {
        prefs.camera = c.camera.clone();
        // The new first camera may have been another row: it is not kept twice.
        for other in [&mut prefs.camera2, &mut prefs.camera3, &mut prefs.camera4] { if *other == c.camera { other.clear() } }
    }
    if !c.sound.is_empty() { prefs.sound = c.sound.clone() }
}

/// Whether the first step is done: a key typed (or one already kept), and for another server a good address.
pub fn where_ready(c: &Choice, key: &str, key_kept: bool) -> Result<(), String> {
    let has_key = !key.trim().is_empty() || key_kept;
    let d = Destination { service: super::BUILT_IN[c.service.min(3)].0, url: c.url.clone(), key: if key.trim().is_empty() && key_kept { "kept".into() } else { key.trim().to_string() }, on: true, ..Destination::default() };
    match d.address() {
        Ok(_) => Ok(()),
        Err(super::Problem::NoKey) if !has_key => Err(tr!("Paste the stream key first.")),
        Err(super::Problem::NoAddress) => Err(tr!("Type the server's address first.")),
        Err(p) => Err(super::tr::not_ready(d.title(), &p.words())),
    }
}

/// Where the service shows its stream key, step by step.
pub fn key_help(service: usize) -> String {
    match super::BUILT_IN[service.min(3)].0 {
        Service::YouTube => tr!("On YouTube: open YouTube Studio, choose Create → Go live → Stream, and copy the Stream key."),
        Service::Facebook => tr!("On Facebook: open your Page, choose Live video → Streaming software, and copy the Stream key (turn on \"Use a persistent stream key\" so it stays the same every week)."),
        Service::Twitch => tr!("On Twitch: open the Creator Dashboard, choose Settings → Stream, and copy the Primary stream key."),
        Service::Custom => tr!("Your streaming service gives you a server address (starting with rtmp://, rtmps:// or srt://) and usually a stream key."),
        Service::Instagram => tr!("On Instagram: in Live Producer (on a computer, Create → Live video → Streaming software), copy the Stream URL and Stream key. Only some accounts are given a stream key."),
        Service::X => tr!("On X: in Media Studio, choose Producer → Sources, and copy the RTMP URL and Stream key. Only some accounts can stream from software."),
        Service::TikTok => tr!("On TikTok: in LIVE Studio or the LIVE Center, copy the Server URL and Stream key. Only some accounts are given a stream key."),
    }
}

/// What the test stream does and who can see it, for the last step.
pub fn test_help(service: usize) -> String {
    match super::BUILT_IN[service.min(3)].0 {
        Service::YouTube => tr!("The test sends 20 seconds to YouTube. Anyone who can see your stream sees it: in YouTube Studio, set the stream to Private or Unlisted first if nobody should."),
        Service::Facebook => tr!("The test sends 20 seconds to Facebook. With a persistent key Facebook waits for you to press Go live there, so nobody sees it."),
        _ => tr!("The test sends 20 seconds to the service, then stops by itself."),
    }
}

/// How long the test stream lasts.
pub const TEST_SECS: u64 = 20;

/// The loudest sound FFmpeg's `volumedetect` heard, in dB (0 is full scale), from what it printed.
pub fn loudest(said: &str) -> Option<f64> {
    said.lines().find_map(|l| l.split("max_volume:").nth(1)).and_then(|v| v.trim().trim_end_matches("dB").trim().parse().ok())
}

/// Quieter than this at its loudest, over a few seconds, is nothing (a send turned off, a cable out).
pub const QUIET_DB: f64 = -50.0;

/// What the check says about the sound input `name`, from its loudest sound (`None`: it could not be opened).
pub fn sound_words(name: &str, db: Option<f64>) -> String {
    match db {
        None => tr!("Sound: {name} could not be opened. Is it plugged in?", name = name),
        Some(d) if d < QUIET_DB => tr!("Sound: almost nothing from {name}. Is the sound board sending to this computer, and is someone talking?", name = name),
        Some(_) => tr!("Sound: {name} is coming in.", name = name),
    }
}

/// What the check says about the camera.
pub fn camera_words(name: &str, picture: bool) -> String {
    if picture { tr!("Camera: {name} gives a picture.", name = name) }
    else { tr!("Camera: no picture from {name}. Is it plugged in, and closed in other programs (Zoom, Teams, OBS)?", name = name) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helper_opens_only_when_nothing_would_go_out() {
        let mut p = RecordPrefs::default();
        assert!(needed(&p));
        p.ndi_stream = true;
        assert!(!needed(&p), "NDI out alone is enough to go live");
        let mut p = RecordPrefs::default();
        p.stream.push(Destination { id: built_in_id(0), on: false, ..Destination::default() });
        assert!(needed(&p), "a destination that is off");
        p.stream[0].on = true;
        assert!(!needed(&p));
    }

    #[test]
    fn its_choices_become_the_livestream_settings() {
        let mut p = RecordPrefs { camera2: "USB Camera".into(), ..RecordPrefs::default() };
        p.stream.push(Destination { id: built_in_id(0), service: Service::YouTube, on: true, ..Destination::default() });
        apply(&mut p, &Choice { service: 3, url: " rtmp://live.example.org/app ".into(), camera: "USB Camera".into(), sound: "input:Line In (USB)".into() });
        let custom = p.stream.iter().find(|d| d.id == built_in_id(3)).unwrap();
        assert!(custom.on && custom.service == Service::Custom && custom.url == "rtmp://live.example.org/app");
        assert!(p.stream.iter().find(|d| d.id == built_in_id(0)).unwrap().on, "YouTube stays as it was");
        assert_eq!((p.camera.as_str(), p.camera2.as_str()), ("USB Camera", ""), "the camera once, first");
        assert_eq!(p.sound, "input:Line In (USB)");
        assert!(!needed(&p));
        apply(&mut p, &Choice { service: 1, camera: String::new(), ..Choice::default() });
        assert!(p.camera.is_empty(), "no camera: the room screen");
        assert_eq!(p.sound, "input:Line In (USB)", "an empty sound choice leaves it");
    }

    #[test]
    fn the_first_step_needs_a_key_or_an_address() {
        let yt = Choice { service: 0, ..Choice::default() };
        assert!(where_ready(&yt, "", false).is_err());
        assert!(where_ready(&yt, "abcd-1234", false).is_ok());
        assert!(where_ready(&yt, "", true).is_ok(), "a key already kept");
        assert!(where_ready(&yt, "two words", false).is_err());
        let custom = Choice { service: 3, ..Choice::default() };
        assert!(where_ready(&custom, "", false).is_err());
        assert!(where_ready(&Choice { url: "rtmp://live.example.org/app".into(), ..custom.clone() }, "", false).is_ok(), "some servers have no key");
        for i in 0..4 { assert!(!key_help(i).is_empty() && !test_help(i).is_empty()) }
    }

    #[test]
    fn the_check_reads_how_loud_the_sound_was() {
        let said = "[Parsed_volumedetect_0 @ 0x1] n_samples: 96000
[Parsed_volumedetect_0 @ 0x1] mean_volume: -41.2 dB
[Parsed_volumedetect_0 @ 0x1] max_volume: -18.5 dB
";
        assert_eq!(loudest(said), Some(-18.5));
        assert_eq!(loudest("nothing"), None);
        assert!(sound_words("Line In", Some(-18.5)).contains("coming in"));
        assert!(sound_words("Line In", Some(-91.0)).contains("almost nothing"));
        assert!(sound_words("Line In", None).contains("could not be opened"));
    }
}
