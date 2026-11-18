//! Settings → General → Screens and graphics and media (N21, N24): what new pictures and videos do, and where the sound goes. Turned into libmpv
//! options here, so the room and the tests read the same thing.
use crate::model::EndAction;
use crate::video::Role;
use serde::{Deserialize, Serialize};

/// How a picture or video fills the screen when its shape differs from the screen's.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Scale {
    /// All of it shows, with black bars at the sides or top.
    #[default]
    Fit,
    /// The screen is filled; the edges are cut off.
    Fill,
    /// Pulled to the screen's shape.
    Stretch,
}

/// Where the left and right sound go (per-channel routing through libmpv's `af=lavfi`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Channels {
    #[default]
    Stereo,
    /// Both speakers play the same mix.
    Mono,
    /// Left and right swapped.
    Swap,
    /// The mix on the left speaker only.
    Left,
    /// The mix on the right speaker only.
    Right,
}

pub const CHANNELS: [Channels; 5] = [Channels::Stereo, Channels::Mono, Channels::Swap, Channels::Left, Channels::Right];
pub const SCALES: [Scale; 3] = [Scale::Fit, Scale::Fill, Scale::Stretch];
/// "When a video ends", in the order the pickers show them.
pub const ENDS: [EndAction; 4] = [EndAction::Stop, EndAction::Hold, EndAction::Loop, EndAction::Next];
/// The picture slideshow choices in seconds; 0: until Next is pressed.
pub const PICTURE_SECS: [u16; 8] = [0, 5, 8, 10, 15, 20, 30, 60];
/// The longest main output delay.
pub const MAX_DELAY_MS: u32 = 2000;

/// Settings → General → Screens and graphics and media.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct MediaPrefs {
    /// How pictures and videos on the media layer fill the audience screen.
    pub scale: Scale,
    /// Pictures added together as one slideshow turn by themselves after this many seconds; 0: they don't.
    pub picture_secs: u16,
    /// What a newly added video does when it ends.
    pub video_end: EndAction,
    /// The operator's headphones (an mpv device name). Empty: none chosen, so Listen can't be used.
    pub listen_device: String,
    /// The room's sound comes this many milliseconds late (to line up with a slow projector).
    pub delay_ms: u32,
    pub channels: Channels,
}

/// Where the sound of the room's players goes right now.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SoundOut {
    /// mpv device name; empty: the computer's default.
    pub device: String,
    /// `audio-delay`, in seconds.
    pub delay: f64,
    /// `af`; empty: none.
    pub af: String,
}

/// The sound output: the main one with its delay and routing, or the headphones as they are while listening.
pub fn sound_out(main_device: &str, prefs: &MediaPrefs, listening: bool) -> SoundOut {
    if listening && !prefs.listen_device.is_empty() {
        return SoundOut { device: prefs.listen_device.clone(), delay: 0.0, af: String::new() };
    }
    SoundOut { device: main_device.to_string(), delay: f64::from(prefs.delay_ms.min(MAX_DELAY_MS)) / 1000.0, af: af(prefs.channels) }
}

/// The `af` option for a routing. Everything is made stereo first, so a mono or 5.1 file is routed the same way.
pub fn af(channels: Channels) -> String {
    let pan = match channels {
        Channels::Stereo => return String::new(),
        Channels::Mono => "c0=0.5*c0+0.5*c1|c1=0.5*c0+0.5*c1",
        Channels::Swap => "c0=c1|c1=c0",
        Channels::Left => "c0=0.5*c0+0.5*c1|c1=0*c1",
        Channels::Right => "c0=0*c0|c1=0.5*c0+0.5*c1",
    };
    format!("lavfi=[aformat=channel_layouts=stereo,pan=stereo|{pan}]")
}

/// The mpv options a new player of the room gets from these settings (the device is set apart).
pub fn mpv_options(role: Role, scale: Scale, sound: &SoundOut) -> Vec<(&'static str, String)> {
    let mut out = vec![];
    if let Role::Foreground(_) = role {
        match scale {
            Scale::Fit => {}
            Scale::Fill => out.push(("panscan", "1.0".to_string())),
            Scale::Stretch => out.push(("keepaspect", "no".to_string())),
        }
    }
    if role != Role::Background {
        if sound.delay != 0.0 { out.push(("audio-delay", format!("{:.3}", sound.delay))) }
        if !sound.af.is_empty() { out.push(("af", sound.af.clone())) }
    }
    out
}

/// The nearest picture slideshow choice to `secs`.
pub fn picture_index(secs: u16) -> usize {
    (0..PICTURE_SECS.len()).min_by_key(|&i| PICTURE_SECS[i].abs_diff(secs)).unwrap_or(0)
}

/// The main output delay typed in the form: whole milliseconds, 0–2000. `None` when it isn't a number.
pub fn parse_delay(text: &str) -> Option<u32> {
    let t = text.trim().trim_end_matches("ms").trim();
    if t.is_empty() { return Some(0) }
    t.parse::<u32>().ok().map(|n| n.min(MAX_DELAY_MS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_load_with_fit_and_no_delay() {
        let s: crate::model::Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.media, MediaPrefs::default());
        assert_eq!((s.media.scale, s.media.video_end, s.media.picture_secs, s.media.delay_ms), (Scale::Fit, EndAction::Stop, 0, 0));
        let p: MediaPrefs = serde_json::from_str(r#"{"scale":"fill","video_end":"next","channels":"swap"}"#).unwrap();
        assert_eq!((p.scale, p.video_end, p.channels), (Scale::Fill, EndAction::Next, Channels::Swap));
    }

    #[test]
    fn a_media_value_this_version_cant_read_loads_as_the_defaults() {
        let s: crate::model::Settings = serde_json::from_str(r#"{"media":{"scale":"zoom"}}"#).unwrap();
        assert_eq!(s.media, MediaPrefs::default());
    }

    #[test]
    fn the_main_output_has_its_delay_and_routing() {
        let p = MediaPrefs { delay_ms: 120, channels: Channels::Swap, listen_device: "wasapi/{phones}".into(), ..MediaPrefs::default() };
        let out = sound_out("wasapi/{board}", &p, false);
        assert_eq!(out, SoundOut { device: "wasapi/{board}".into(), delay: 0.12, af: af(Channels::Swap) });
    }

    #[test]
    fn listening_sends_the_sound_to_the_headphones_as_it_is() {
        let p = MediaPrefs { delay_ms: 120, channels: Channels::Left, listen_device: "wasapi/{phones}".into(), ..MediaPrefs::default() };
        assert_eq!(sound_out("wasapi/{board}", &p, true), SoundOut { device: "wasapi/{phones}".into(), delay: 0.0, af: String::new() });
    }

    #[test]
    fn listening_with_no_headphones_chosen_changes_nothing() {
        let p = MediaPrefs { delay_ms: 50, ..MediaPrefs::default() };
        assert_eq!(sound_out("", &p, true), sound_out("", &p, false));
    }

    #[test]
    fn a_delay_is_kept_to_two_seconds() {
        let p = MediaPrefs { delay_ms: 90_000, ..MediaPrefs::default() };
        assert_eq!(sound_out("", &p, false).delay, 2.0);
    }

    #[test]
    fn routing_makes_stereo_first() {
        assert_eq!(af(Channels::Stereo), "");
        assert_eq!(af(Channels::Swap), "lavfi=[aformat=channel_layouts=stereo,pan=stereo|c0=c1|c1=c0]");
        for c in CHANNELS.into_iter().skip(1) {
            let f = af(c);
            assert!(f.starts_with("lavfi=[aformat=channel_layouts=stereo,pan=stereo|") && f.ends_with(']'), "{f}");
            assert!(!f.contains(':'), "a colon would end the filter's options in mpv's list: {f}");
        }
    }

    #[test]
    fn a_foreground_video_is_scaled_and_heard_through_the_settings() {
        let sound = SoundOut { device: String::new(), delay: 0.25, af: af(Channels::Mono) };
        let fg = Role::Foreground(EndAction::Stop);
        assert_eq!(mpv_options(fg, Scale::Fit, &sound), [("audio-delay", "0.250".to_string()), ("af", af(Channels::Mono))]);
        assert_eq!(mpv_options(fg, Scale::Fill, &SoundOut::default()), [("panscan", "1.0".to_string())]);
        assert_eq!(mpv_options(fg, Scale::Stretch, &SoundOut::default()), [("keepaspect", "no".to_string())]);
    }

    #[test]
    fn audio_is_delayed_but_not_scaled_and_a_background_is_left_alone() {
        let sound = SoundOut { device: String::new(), delay: 0.1, af: String::new() };
        assert_eq!(mpv_options(Role::Audio { looped: false }, Scale::Fill, &sound), [("audio-delay", "0.100".to_string())]);
        assert!(mpv_options(Role::Background, Scale::Stretch, &sound).is_empty(), "the room crops it itself, and it is muted");
    }

    #[test]
    fn picture_choices_round_to_the_nearest() {
        assert_eq!(picture_index(0), 0);
        assert_eq!(picture_index(10), 3);
        assert_eq!(picture_index(11), 3);
        assert_eq!(picture_index(600), PICTURE_SECS.len() - 1);
    }

    mod in_the_app {
        use crate::app::{App, test_app};
        use crate::live::{Command, Envelope, MediaLayer, PlayState, RoomEvent};
        use crate::model::*;
        use serde_json::json;
        use uuid::Uuid;

        fn video(app: &App) -> Uuid {
            let id = Uuid::new_v4();
            app.data.lock().unwrap().media.push(MediaFile { id, name: "Intro".into(), kind: MediaKind::Video, asset: "v.mp4".into(), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() });
            id
        }

        async fn add(app: &std::sync::Arc<App>, item: serde_json::Value) -> Uuid {
            let v = app.do_action("service/add", json!({ "item": item }).to_string().as_bytes()).await.unwrap();
            serde_json::from_value(v["id"].clone()).unwrap()
        }

        fn kind(app: &App, id: Uuid) -> ItemKind { app.data.lock().unwrap().services[0].items.iter().find(|i| i.id == id).unwrap().kind.clone() }

        #[tokio::test]
        async fn a_video_added_without_a_choice_ends_as_settings_say() {
            let app = test_app();
            let m = video(&app);
            app.data.lock().unwrap().settings.media.video_end = EndAction::Next;
            let id = add(&app, json!({ "kind": "media", "media_id": m, "autoplay": true })).await;
            assert!(matches!(kind(&app, id), ItemKind::Media { end: EndAction::Next, .. }));
            let id = add(&app, json!({ "kind": "media", "media_id": m, "end": "loop" })).await;
            assert!(matches!(kind(&app, id), ItemKind::Media { end: EndAction::Loop, .. }), "a choice made is kept");
        }

        #[tokio::test]
        async fn pictures_added_as_a_slideshow_turn_by_themselves() {
            let app = test_app();
            let deck = Uuid::new_v4();
            app.data.lock().unwrap().decks.push(Deck { id: deck, title: "Notices".into(), slides: vec!["a.png".into(), "b.png".into()], notes: vec![], transitions: vec![], advance_ms: vec![], show: None });
            app.data.lock().unwrap().settings.media.picture_secs = 8;
            let shows = add(&app, json!({ "kind": "deck", "deck_id": deck, "slideshow": true })).await;
            let talk = add(&app, json!({ "kind": "deck", "deck_id": deck })).await;
            let advance = app.data.lock().unwrap().automation.advance.clone();
            assert_eq!(advance.get(&shows).map(|a| (a.secs, a.loop_item)), Some((8, true)));
            assert!(!advance.contains_key(&talk), "PowerPoint slides wait for Next");
            app.data.lock().unwrap().settings.media.picture_secs = 0;
            let off = add(&app, json!({ "kind": "deck", "deck_id": deck, "slideshow": true })).await;
            assert!(!app.data.lock().unwrap().automation.advance.contains_key(&off), "0: until Next is pressed");
        }

        fn ended(app: &App) {
            let inst = app.live.lock().unwrap().frame.media.as_ref().expect("a video is up").inst;
            app.room_event(RoomEvent::Media { inst, state: PlayState::Ended, position: 10.0, duration: 10.0, error: None });
        }

        #[tokio::test]
        async fn a_video_that_goes_on_shows_the_next_slide_when_it_ends() {
            let app = test_app();
            let m = video(&app);
            let clip = add(&app, json!({ "kind": "media", "media_id": m, "end": "next" })).await;
            let words = add(&app, json!({ "kind": "text", "title": "Welcome", "body": "Hello" })).await;
            assert!(app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::Show { item_id: clip, slide: 0 } }).ok);
            assert!(matches!(app.live.lock().unwrap().frame.media.as_ref().map(|m| &m.v), Some(MediaLayer::Video { end: EndAction::Next, .. })));
            ended(&app);
            assert_eq!(app.live.lock().unwrap().cursor.as_ref().map(|c| c.item_id), Some(words));
        }

        #[tokio::test]
        async fn at_the_end_of_the_service_it_is_taken_down() {
            let app = test_app();
            let m = video(&app);
            let clip = add(&app, json!({ "kind": "media", "media_id": m, "end": "next" })).await;
            assert!(app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::Show { item_id: clip, slide: 0 } }).ok);
            ended(&app);
            assert!(app.live.lock().unwrap().frame.media.is_none());
        }

        #[tokio::test]
        async fn a_held_video_stays_up() {
            let app = test_app();
            let m = video(&app);
            let clip = add(&app, json!({ "kind": "media", "media_id": m, "end": "hold" })).await;
            add(&app, json!({ "kind": "text", "title": "Welcome", "body": "Hello" })).await;
            assert!(app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::Show { item_id: clip, slide: 0 } }).ok);
            ended(&app);
            assert_eq!(app.live.lock().unwrap().cursor.as_ref().map(|c| c.item_id), Some(clip));
            assert!(app.live.lock().unwrap().frame.media.is_some());
        }
    }

    #[test]
    fn a_typed_delay_is_read() {
        assert_eq!(parse_delay("120"), Some(120));
        assert_eq!(parse_delay(" 80 ms"), Some(80));
        assert_eq!(parse_delay(""), Some(0));
        assert_eq!(parse_delay("5000"), Some(MAX_DELAY_MS));
        assert_eq!(parse_delay("soon"), None);
        assert_eq!(parse_delay("-5"), None);
    }
}
