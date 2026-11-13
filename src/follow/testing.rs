//! Test helpers shared by the follow tests: a fake live engine and a sung song.
use super::align::Heard;
use super::engine::{Engine, FollowEvent, Host, LiveSong};
use super::lyrics::sound_key;
use super::rules::{Mode, Settings, Voice};
use crate::model::{Slide, SlideContent};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub fn text(t: &str) -> Slide { Slide::new("", SlideContent::Text { text: t.into(), footer: None, reference: None, size: 0, parallel: None }) }

pub const SONG: [&str; 3] = [
    "amazing grace how sweet the sound\nthat saved a wretch like me",
    "I once was lost but now am found\nwas blind but now I see",
    "through many dangers toils and snares\nI have already come",
];

/// A live engine stand-in: records the follower's own changes; `operator` changes as a click would.
pub struct Fake { pub live: Mutex<Option<LiveSong>>, pub shown: Mutex<Vec<String>>, pub notices: Mutex<Vec<String>>, pub modes: Mutex<Vec<(Uuid, Mode)>> }

impl Fake {
    pub fn song(mode: Mode) -> Arc<Fake> {
        let live = LiveSong { item_id: Uuid::new_v4(), is_song: true, slides: SONG.iter().map(|t| text(t)).collect(), slide: 0, words_up: true,
            video_playing: false, version: 1, mode, settings: Settings::default() };
        Arc::new(Fake { live: Mutex::new(Some(live)), shown: Mutex::default(), notices: Mutex::default(), modes: Mutex::default() })
    }
    pub fn change(&self, f: impl FnOnce(&mut LiveSong)) {
        let mut l = self.live.lock().unwrap();
        let l = l.as_mut().unwrap();
        f(l);
        l.version += 1;
    }
    pub fn operator_show(&self, slide: usize) { self.change(|l| { l.slide = slide; l.words_up = true }) }
    pub fn slide(&self) -> usize { self.live.lock().unwrap().as_ref().unwrap().slide }
    pub fn item(&self) -> Uuid { self.live.lock().unwrap().as_ref().unwrap().item_id }
    pub fn shown(&self) -> Vec<String> { self.shown.lock().unwrap().clone() }
}

impl Host for Fake {
    fn live(&self) -> Option<LiveSong> { self.live.lock().unwrap().clone() }
    fn show(&self, item: Uuid, slide: usize, base: u64) -> bool {
        let mut l = self.live.lock().unwrap();
        let Some(l) = l.as_mut().filter(|l| l.version == base && l.item_id == item) else { return false };
        l.slide = slide;
        l.words_up = true;
        l.version += 1;
        self.shown.lock().unwrap().push(format!("show {slide}"));
        true
    }
    fn clear_slide(&self, base: u64) -> bool {
        let mut l = self.live.lock().unwrap();
        let Some(l) = l.as_mut().filter(|l| l.version == base) else { return false };
        l.words_up = false;
        l.version += 1;
        self.shown.lock().unwrap().push("blank".into());
        true
    }
    fn notice(&self, text: &str) { self.notices.lock().unwrap().push(text.into()) }
    fn save_mode(&self, item: Uuid, mode: Mode) { self.modes.lock().unwrap().push((item, mode)) }
}

/// Sings `words` a third of a second apart (each one heard as it is sung); the clock moves on.
pub fn sing(e: &mut Engine, host: &dyn Host, words: &str, t: &mut f64) {
    for w in words.split_whitespace() {
        *t += 1.0 / 3.0;
        e.step(host, *t, Some(FollowEvent::Voice(Voice::Singing, *t)));
        e.step(host, *t, Some(FollowEvent::Heard(Heard { key: sound_key(w), at: *t, conf: 1.0 })));
    }
}

pub const VERSE_1: &str = "amazing grace how sweet the sound that saved a wretch like me";
pub const VERSE_2: &str = "I once was lost but now am found was blind but now I see";

