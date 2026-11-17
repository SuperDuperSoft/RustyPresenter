//! Spec §6.4: a chosen sound output that goes away pauses what can be heard and tells the operator, so a video
//! never plays on in silence or falls back to the laptop's speakers. Settings → Video → Sound goes to is the way
//! out: with the Windows default chosen there is nothing to watch.
use crate::{live::{Command, Frame, MediaLayer}, video::ffi::AudioDevice};

/// Is `chosen` missing from `found`? Empty `found` means the list could not be read, which says nothing.
pub fn missing(chosen: &str, found: &[AudioDevice]) -> bool {
    !chosen.is_empty() && !found.is_empty() && !found.iter().any(|d| d.name == chosen)
}

/// What pauses everything that can be heard. Background video is always muted, so it plays on.
pub fn pauses(frame: &Frame) -> Vec<Command> {
    let media = frame.media.iter().filter_map(|m| match m.v {
        MediaLayer::Video { paused: false, .. } => Some(Command::MediaPause { inst: m.inst, paused: true }),
        _ => None,
    });
    let audio = frame.audio.iter().filter(|a| !a.v.paused).map(|a| Command::AudioPause { inst: a.inst, paused: true });
    media.chain(audio).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::{test_app, App, Monitor, Screens}, live::*, model::*, preflight::strings as s};
    use std::sync::{atomic::{AtomicUsize, Ordering}, Arc, Mutex};
    use uuid::Uuid;

    fn dev(name: &str) -> AudioDevice { AudioDevice { name: name.into(), label: name.into() } }

    #[test]
    fn a_device_in_the_list_is_there() { assert!(!missing("wasapi/{a}", &[dev("wasapi/{a}")])); }

    #[test]
    fn a_device_not_in_the_list_is_missing() { assert!(missing("wasapi/{a}", &[dev("wasapi/{b}")])); }

    #[test]
    fn the_system_default_is_never_missing() { assert!(!missing("", &[dev("wasapi/{b}")])); }

    #[test]
    fn an_unread_list_says_nothing() { assert!(!missing("wasapi/{a}", &[])); }

    fn video(inst: u64, paused: bool) -> Option<Inst<MediaLayer>> {
        Some(Inst { inst, v: MediaLayer::Video { asset: "v.mp4".into(), end: EndAction::Stop, paused } })
    }

    fn sound(inst: u64, paused: bool) -> Option<Inst<AudioLayer>> {
        Some(Inst { inst, v: AudioLayer { asset: "a.mp3".into(), title: "a".into(), looped: false, paused } })
    }

    fn names(frame: &Frame) -> Vec<(u64, bool)> {
        pauses(frame).iter().map(|c| match c {
            Command::MediaPause { inst, .. } => (*inst, true),
            Command::AudioPause { inst, .. } => (*inst, false),
            c => panic!("{c:?}"),
        }).collect()
    }

    #[test]
    fn video_and_audio_both_pause() {
        assert_eq!(names(&Frame { media: video(1, false), audio: sound(2, false), ..Frame::default() }), [(1, true), (2, false)]);
    }

    #[test]
    fn what_is_already_paused_is_left() {
        assert_eq!(names(&Frame { media: video(1, true), audio: sound(2, true), ..Frame::default() }), []);
    }

    #[test]
    fn a_background_video_is_muted_so_it_plays_on() {
        let background = Some(Inst { inst: 3, v: Background::Video { asset: "b.mp4".into(), dim: 0 } });
        assert_eq!(names(&Frame { background, ..Frame::default() }), []);
    }

    /// Counts the times the system is asked, as that is slow.
    struct Outputs { found: Mutex<Vec<AudioDevice>>, asked: AtomicUsize }

    impl Screens for Outputs {
        fn open(&self, _: &str, _: Option<&str>) -> Result<(), String> { Ok(()) }
        fn close(&self, _: &str) {}
        fn open_roles(&self) -> Vec<String> { vec![] }
        fn monitors(&self) -> Vec<Monitor> { vec![] }
        fn audio_devices(&self) -> Vec<AudioDevice> {
            self.asked.fetch_add(1, Ordering::Relaxed);
            self.found.lock().unwrap().clone()
        }
    }

    fn app_with(chosen: &str, found: &[&str]) -> (Arc<App>, Arc<Outputs>) {
        let app = test_app();
        app.data.lock().unwrap().settings.audio_device = chosen.into();
        let outputs = Arc::new(Outputs { found: Mutex::new(found.iter().map(|f| dev(f)).collect()), asked: AtomicUsize::new(0) });
        app.set_screens(outputs.clone());
        (app, outputs)
    }

    fn play_audio(app: &App) -> u64 {
        let media_id = Uuid::new_v4();
        let file = MediaFile { id: media_id, name: "a".into(), kind: MediaKind::Audio, asset: "a.mp3".into(), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() };
        let item_id = Uuid::new_v4();
        let kind = ItemKind::Media { media_id, autoplay: true, end: EndAction::Stop, looped: false };
        {
            let mut d = app.data.lock().unwrap();
            d.media.push(file);
            d.services[0].items.push(ServiceItem { id: item_id, kind, background: None, notes: String::new(), theme: None });
        }
        let out = app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::Show { item_id, slide: 0 } });
        assert!(out.ok, "{out:?}");
        app.live.lock().unwrap().frame.audio.as_ref().expect("audio on air").inst
    }

    fn audio_paused(app: &App) -> bool { app.live.lock().unwrap().frame.audio.as_ref().unwrap().v.paused }

    #[test]
    fn a_lost_output_pauses_and_tells_the_operator() {
        let (app, _) = app_with("wasapi/{board}", &["wasapi/{laptop}"]);
        play_audio(&app);
        app.watch_sound();
        assert!(audio_paused(&app));
        assert_eq!(app.live.lock().unwrap().notice.as_deref(), Some(s::sound_gone().as_str()));
    }

    #[test]
    fn an_output_that_is_there_changes_nothing() {
        let (app, _) = app_with("wasapi/{board}", &["wasapi/{board}"]);
        play_audio(&app);
        app.watch_sound();
        assert!(!audio_paused(&app));
        assert_eq!(app.live.lock().unwrap().notice, None);
    }

    #[test]
    fn pressing_play_with_the_output_still_gone_pauses_again() {
        let (app, _) = app_with("wasapi/{board}", &["wasapi/{laptop}"]);
        let inst = play_audio(&app);
        app.watch_sound();
        assert!(app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::AudioPause { inst, paused: false } }).ok);
        app.watch_sound();
        assert!(audio_paused(&app));
    }

    #[test]
    fn the_notice_goes_when_the_output_is_back() {
        let (app, outputs) = app_with("wasapi/{board}", &["wasapi/{laptop}"]);
        play_audio(&app);
        app.watch_sound();
        *outputs.found.lock().unwrap() = vec![dev("wasapi/{board}")];
        app.watch_sound();
        assert_eq!(app.live.lock().unwrap().notice, None);
        assert!(audio_paused(&app), "pressing play is the operator's");
    }

    #[test]
    fn the_notice_goes_when_the_default_output_is_chosen() {
        let (app, _) = app_with("wasapi/{board}", &["wasapi/{laptop}"]);
        play_audio(&app);
        app.watch_sound();
        app.data.lock().unwrap().settings.audio_device.clear();
        app.watch_sound();
        assert_eq!(app.live.lock().unwrap().notice, None);
    }

    #[test]
    fn the_notice_stays_while_the_list_cannot_be_read() {
        let (app, outputs) = app_with("wasapi/{board}", &["wasapi/{laptop}"]);
        play_audio(&app);
        app.watch_sound();
        outputs.found.lock().unwrap().clear();
        app.watch_sound();
        assert_eq!(app.live.lock().unwrap().notice.as_deref(), Some(s::sound_gone().as_str()));
    }

    #[test]
    fn another_notice_is_left_alone() {
        let (app, outputs) = app_with("wasapi/{board}", &["wasapi/{board}"]);
        play_audio(&app);
        app.set_notice("The audience screen restarted.");
        app.watch_sound();
        assert_eq!(app.live.lock().unwrap().notice.as_deref(), Some("The audience screen restarted."));
        assert_eq!(outputs.asked.load(Ordering::Relaxed), 1, "something can be heard");
    }

    #[test]
    fn the_system_is_not_asked_while_nothing_can_be_heard() {
        let (app, outputs) = app_with("wasapi/{board}", &["wasapi/{laptop}"]);
        app.watch_sound();
        assert_eq!(outputs.asked.load(Ordering::Relaxed), 0);
        assert_eq!(app.live.lock().unwrap().notice, None);
    }

    #[test]
    fn the_system_is_not_asked_with_the_default_output() {
        let (app, outputs) = app_with("", &["wasapi/{laptop}"]);
        play_audio(&app);
        app.watch_sound();
        assert_eq!(outputs.asked.load(Ordering::Relaxed), 0);
        assert!(!audio_paused(&app));
    }

    #[test]
    fn nothing_is_paused_once_the_operator_has_paused_it() {
        let (app, outputs) = app_with("wasapi/{board}", &["wasapi/{laptop}"]);
        let inst = play_audio(&app);
        assert!(app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::AudioPause { inst, paused: true } }).ok);
        app.watch_sound();
        assert_eq!(outputs.asked.load(Ordering::Relaxed), 0);
        assert_eq!(app.live.lock().unwrap().notice, None);
    }
}
