//! N13: the walk-in music. A playlist of its own, with shuffle and its own volume, that plays apart from the slides:
//! changing slides never touches it. Clear audio and Clear everything stop it, like any other sound.
//!
//! The saved list lives in `Automation::music`; what plays lives in `AutomationRuntime::music` (memory only). The
//! player itself is in `music_out`, on a thread of its own.
use crate::app::{ActionError, App};
use crate::live::PlayState;
use crate::model::{AssetId, MediaKind};
use crate::timers::{MediaClock, display_ms};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

/// Volume is saved this long after the last change, so dragging the slider saves once.
pub const SAVE_VOLUME_AFTER_MS: i64 = 1500;
/// Back within this many seconds of a track's start goes to the one before; later it starts the track again.
const RESTART_AFTER_SECS: f64 = 3.0;
pub const MAX_TRACKS: usize = 500;

/// One song of the walk-in music: a file from the media library.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub id: Uuid,
    pub asset: AssetId,
    pub title: String,
    /// Seconds, as the media library knew it when added.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}

/// The saved walk-in playlist.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicList {
    pub tracks: Vec<Track>,
    pub shuffle: bool,
    /// 0–100.
    pub volume: u8,
    /// Start again from the top after the last song.
    pub repeat: bool,
}

impl Default for MusicList {
    fn default() -> Self { MusicList { tracks: vec![], shuffle: false, volume: 70, repeat: true } }
}

impl MusicList {
    fn track(&self, id: Uuid) -> Option<&Track> { self.tracks.iter().find(|t| t.id == id) }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MusicState { #[default] Stopped, Playing, Paused }

/// What the mini player and the phone show.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MusicReadout {
    pub state: MusicState,
    pub title: String,
    /// Time left in the song, whole seconds (0 while stopped).
    pub left_ms: i64,
    /// 1-based place in the play order; 0 while stopped.
    pub number: usize,
    pub count: usize,
    pub shuffle: bool,
    pub volume: u8,
}

/// What the player should do: play `path` as playback `inst`, paused or not, at `volume`.
#[derive(Clone, Debug, PartialEq)]
pub struct MusicWant { pub inst: u64, pub asset: AssetId, pub paused: bool, pub volume: u8 }

/// The walk-in music's run (memory only).
#[derive(Clone, Debug, Default)]
pub struct MusicRun {
    pub state: MusicState,
    /// Track ids in the order they play.
    pub order: Vec<Uuid>,
    pub at: usize,
    /// Bumped for every song loaded; a report about an older one is ignored.
    pub inst: u64,
    pub position: f64,
    pub duration: f64,
    /// Songs in a row that would not play; when every song fails, the music stops.
    failures: usize,
    /// `Live::audio_clears` when last looked at.
    pub seen_clears: u64,
    /// A volume not saved yet, and when it changed.
    pub volume: Option<(u8, DateTime<Utc>)>,
    rng: u64,
}

/// SplitMix64: enough to shuffle a playlist, and the same order for the same seed in tests.
fn next_rand(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The play order: the list's order, or shuffled. A shuffled order never starts with `not_first` when it has a
/// choice, so the same song never plays twice in a row across a reshuffle.
pub fn play_order(list: &MusicList, rng: &mut u64, not_first: Option<Uuid>) -> Vec<Uuid> {
    let mut ids: Vec<Uuid> = list.tracks.iter().map(|t| t.id).collect();
    if !list.shuffle { return ids }
    for i in (1..ids.len()).rev() {
        let j = (next_rand(rng) % (i as u64 + 1)) as usize;
        ids.swap(i, j);
    }
    if ids.len() > 1 && not_first.is_some_and(|n| ids[0] == n) { ids.swap(0, 1) }
    ids
}

impl MusicRun {
    /// The song playing or paused.
    pub fn current<'a>(&self, list: &'a MusicList) -> Option<&'a Track> {
        if self.state == MusicState::Stopped { return None }
        list.track(*self.order.get(self.at)?)
    }

    pub fn volume(&self, list: &MusicList) -> u8 { self.volume.map_or(list.volume, |(v, _)| v) }

    fn load(&mut self) {
        self.inst += 1;
        self.position = 0.0;
        self.duration = 0.0;
    }

    fn begin(&mut self, list: &MusicList, seed: u64, first: Option<Uuid>) -> Result<(), String> {
        if list.tracks.is_empty() { return Err(super::strings::music_empty()) }
        if self.rng == 0 { self.rng = seed | 1 }
        self.order = play_order(list, &mut self.rng, first);
        // A chosen song plays first; then the songs after it in the list (or the shuffled rest).
        if let Some(i) = first.and_then(|f| self.order.iter().position(|id| *id == f)) {
            if list.shuffle { let f = self.order.remove(i); self.order.insert(0, f) } else { self.order.rotate_left(i) }
        }
        self.at = 0;
        self.failures = 0;
        self.state = MusicState::Playing;
        self.load();
        Ok(())
    }

    /// Plays, or goes on after a pause. `seed` shuffles a new run.
    pub fn play(&mut self, list: &MusicList, seed: u64) -> Result<(), String> {
        match self.state {
            MusicState::Paused => { self.state = MusicState::Playing; Ok(()) }
            MusicState::Playing => Ok(()),
            MusicState::Stopped => self.begin(list, seed, None),
        }
    }

    /// Plays song `id` now (the playlist's play button).
    pub fn play_track(&mut self, list: &MusicList, id: Uuid, seed: u64) -> Result<(), String> {
        if list.track(id).is_none() { return Err(super::strings::music_track_gone()) }
        match self.order.iter().position(|t| *t == id) {
            Some(i) if self.state != MusicState::Stopped => {
                self.at = i;
                self.state = MusicState::Playing;
                self.load();
                Ok(())
            }
            _ => self.begin(list, seed, Some(id)),
        }
    }

    pub fn pause(&mut self) { if self.state == MusicState::Playing { self.state = MusicState::Paused } }

    pub fn stop(&mut self) {
        if self.state == MusicState::Stopped { return }
        self.state = MusicState::Stopped;
        self.load();
    }

    /// The next song; after the last one, the top again (a new shuffle) or, without repeat, stop.
    pub fn next(&mut self, list: &MusicList) {
        if self.state == MusicState::Stopped { return }
        if self.at + 1 < self.order.len() {
            self.at += 1;
        } else if list.repeat && !list.tracks.is_empty() {
            let last = self.order.get(self.at).copied();
            self.order = play_order(list, &mut self.rng, last);
            self.at = 0;
        } else {
            return self.stop();
        }
        self.load();
    }

    /// Early in a song: the one before it. Later: this song from its start.
    pub fn prev(&mut self) {
        if self.state == MusicState::Stopped { return }
        if self.position < RESTART_AFTER_SECS { self.at = self.at.saturating_sub(1) }
        self.load();
    }

    /// A report from the player. Returns (changed, gave up): every song failed in a row, so the music stopped.
    pub fn report(&mut self, list: &MusicList, inst: u64, state: &PlayState, position: f64, duration: f64) -> (bool, bool) {
        if inst != self.inst || self.state == MusicState::Stopped { return (false, false) }
        match state {
            PlayState::Playing | PlayState::Paused => {
                self.position = position;
                if duration > 0.0 { self.duration = duration }
                if *state == PlayState::Playing { self.failures = 0 }
                (true, false)
            }
            PlayState::Ended => { self.next(list); (true, false) }
            PlayState::Error => {
                self.failures += 1;
                if self.failures >= self.order.len().max(1) {
                    self.stop();
                    return (true, true);
                }
                self.next(list);
                (true, false)
            }
        }
    }

    /// The saved list changed: songs taken out leave the order, new ones join at its end. When the song playing was
    /// taken out, the next one plays.
    pub fn list_changed(&mut self, list: &MusicList) {
        if self.state == MusicState::Stopped { self.order.clear(); return }
        let current = self.order.get(self.at).copied();
        self.order.retain(|id| list.track(*id).is_some());
        for t in &list.tracks { if !self.order.contains(&t.id) { self.order.push(t.id) } }
        match current.and_then(|c| self.order.iter().position(|id| *id == c)) {
            Some(i) => self.at = i,
            None if self.order.is_empty() => self.stop(),
            None => {
                if self.at >= self.order.len() {
                    if !list.repeat { return self.stop() }
                    self.at = 0;
                }
                self.load();
            }
        }
    }

    /// What the player should do now; `None`: nothing plays.
    pub fn wanted(&self, list: &MusicList) -> Option<MusicWant> {
        let t = self.current(list)?;
        Some(MusicWant { inst: self.inst, asset: t.asset.clone(), paused: self.state == MusicState::Paused, volume: self.volume(list) })
    }

    /// What an "audio time left" timer reads while no audio item plays.
    pub fn clock(&self) -> Option<MediaClock> {
        if self.state == MusicState::Stopped { return None }
        MediaClock::from_secs(self.position, self.duration, self.state == MusicState::Paused)
    }

    /// The mini player's line; `None` when there is no walk-in music at all.
    pub fn readout(&self, list: &MusicList) -> Option<MusicReadout> {
        let current = self.current(list);
        if list.tracks.is_empty() && current.is_none() { return None }
        let left = self.clock().map_or(0, |c| display_ms(c.left_ms, true));
        Some(MusicReadout {
            state: if current.is_some() { self.state } else { MusicState::Stopped },
            title: current.map(|t| t.title.clone()).unwrap_or_default(),
            left_ms: left,
            number: if current.is_some() { self.at + 1 } else { 0 },
            count: list.tracks.len(),
            shuffle: list.shuffle,
            volume: self.volume(list),
        })
    }

    /// A volume change waiting to be saved, once the slider has rested.
    pub fn volume_to_save(&self, now: DateTime<Utc>) -> Option<u8> {
        self.volume.filter(|(_, at)| now - *at >= TimeDelta::milliseconds(SAVE_VOLUME_AFTER_MS)).map(|(v, _)| v)
    }
}

/// A new seed for a shuffle.
fn seed() -> u64 { Uuid::new_v4().as_u64_pair().0 }

/// What the playlist dialog and the phone can ask for.
#[derive(Clone, Debug, PartialEq)]
pub enum MusicOp { Play, Pause, Toggle, Stop, Next, Prev, Volume(u8), Shuffle(bool), PlayTrack(Uuid) }

impl MusicOp {
    /// From an action's last part and body, as `automation/music/<op>` and the phone send them.
    pub fn parse(op: &str, body: &Value) -> Option<MusicOp> {
        Some(match op {
            "play" => MusicOp::Play,
            "pause" => MusicOp::Pause,
            "toggle" => MusicOp::Toggle,
            "stop" => MusicOp::Stop,
            "next" => MusicOp::Next,
            "prev" => MusicOp::Prev,
            "volume" => MusicOp::Volume(body["volume"].as_u64()?.min(100) as u8),
            "shuffle" => MusicOp::Shuffle(body["on"].as_bool()?),
            "play-track" => MusicOp::PlayTrack(body["id"].as_str().and_then(|s| Uuid::parse_str(s).ok())?),
            _ => return None,
        })
    }
}

/// Library files as songs for the list (audio only), each with a new id.
pub fn tracks_from_library(media: &[crate::model::MediaFile], ids: &[Uuid]) -> Vec<Track> {
    ids.iter().filter_map(|id| media.iter().find(|m| m.id == *id && m.kind == MediaKind::Audio))
        .map(|m| Track { id: Uuid::new_v4(), asset: m.asset.clone(), title: m.name.clone(), duration: m.duration }).collect()
}

/// `automation/music/*`: the controls, and `save` for the playlist dialog.
pub async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    let bad = ActionError::bad;
    let op = action.trim_start_matches("automation/music/");
    let v: Value = if body.is_empty() { Value::Null } else { serde_json::from_slice(body).map_err(|e| bad(crate::tr!("Bad request: {error}", error = e)))? };
    if op == "save" {
        let mut list: MusicList = serde_json::from_value(v.clone()).map_err(|e| bad(crate::tr!("Bad request: {error}", error = e)))?;
        if list.tracks.len() > MAX_TRACKS { return Err(bad(super::strings::music_too_many())) }
        list.volume = list.volume.min(100);
        for t in &mut list.tracks { if t.id.is_nil() { t.id = Uuid::new_v4() } }
        app.music_save(list).map_err(bad)?;
        return Ok(json!({}));
    }
    let op = MusicOp::parse(op, &v).ok_or_else(|| ActionError { status: 404, message: "unknown action".into() })?;
    app.music(op).map_err(bad)?;
    Ok(json!({}))
}

impl App {
    /// A walk-in music control from the mini player, the playlist, the phone, a macro or the calendar.
    pub fn music(&self, op: MusicOp) -> Result<(), String> {
        let now = Utc::now();
        let shuffle = {
            let data = self.data();
            let mut rt = self.automation.lock().unwrap();
            let list = &data.automation.music;
            let m = &mut rt.music;
            match op {
                MusicOp::Play => m.play(list, seed())?,
                MusicOp::Pause => m.pause(),
                MusicOp::Toggle => if m.state == super::music::MusicState::Playing { m.pause() } else { m.play(list, seed())? },
                MusicOp::Stop => m.stop(),
                MusicOp::Next => m.next(list),
                MusicOp::Prev => m.prev(),
                MusicOp::Volume(v) => m.volume = Some((v.min(100), now)),
                MusicOp::PlayTrack(id) => m.play_track(list, id, seed())?,
                MusicOp::Shuffle(_) => {}
            }
            match op { MusicOp::Shuffle(on) => Some(on), _ => None }
        };
        if let Some(on) = shuffle {
            // A new order from the next song on: the one playing goes on.
            self.edit_untracked(|d| { d.automation.music.shuffle = on; Ok(()) })?;
            let data = self.data();
            let mut rt = self.automation.lock().unwrap();
            let m = &mut rt.music;
            if m.state != MusicState::Stopped {
                let current = m.order.get(m.at).copied();
                let mut order = play_order(&data.automation.music, &mut m.rng, None);
                if let Some(c) = current { order.retain(|id| *id != c); order.insert(0, c) }
                m.order = order;
                m.at = 0;
            }
        }
        self.music_changed();
        Ok(())
    }

    /// The playlist dialog saved: the list, its shuffle, volume and repeat. What plays follows the new list.
    pub fn music_save(&self, list: MusicList) -> Result<(), String> {
        self.edit(|d| { d.automation.music = list; Ok(()) })?;
        {
            let data = self.data();
            let mut rt = self.automation.lock().unwrap();
            rt.music.volume = None;
            rt.music.list_changed(&data.automation.music);
        }
        self.music_changed();
        Ok(())
    }

    /// A report from the walk-in music's player.
    pub fn music_report(&self, r: crate::video::PlayerReport) {
        let gave_up = {
            let data = self.data();
            let mut rt = self.automation.lock().unwrap();
            let (changed, gave_up) = rt.music.report(&data.automation.music, r.inst, &r.state, r.position, r.duration);
            if !changed { return }
            gave_up
        };
        if gave_up {
            let why = r.error.unwrap_or_else(|| crate::tr!("the file could not be played"));
            self.set_notice(&super::strings::music_failed(&why));
        }
        self.music_sync();
        self.publish_timers(Utc::now(), matches!(r.state, PlayState::Ended | PlayState::Error));
    }

    /// Tells the player what to play, and every screen the new readout.
    pub(crate) fn music_changed(&self) {
        self.music_sync();
        self.publish_timers(Utc::now(), true);
        self.ticker_wake.notify_one();
    }

    /// Sends the player what it should do, when that changed since the last time.
    pub(crate) fn music_sync(&self) {
        let sound = self.sound_out();
        let data = self.data();
        let mut rt = self.automation.lock().unwrap();
        let want = rt.music.wanted(&data.automation.music);
        let msg = want.map(|w| (w, sound));
        if rt.music_sent == msg { return }
        rt.music_sent = msg.clone();
        let path = msg.map(|(w, s)| (self.store.asset_path(&w.asset), w, s));
        if let Some(tx) = &rt.music_out { let _ = tx.send(path.map(|(p, w, s)| super::music_out::Want { path: p, inst: w.inst, paused: w.paused, volume: w.volume, sound: s })); }
    }

    /// After a live change: Clear audio or Clear everything stops the walk-in music too.
    pub(crate) fn music_after_clear(&self, audio_clears: u64) -> bool {
        let mut rt = self.automation.lock().unwrap();
        if rt.music.seen_clears == audio_clears { return false }
        rt.music.seen_clears = audio_clears;
        let was = rt.music.state != MusicState::Stopped;
        rt.music.stop();
        was
    }

    /// Saves a volume that has rested (called from the ticker).
    pub(crate) fn music_save_volume(&self, now: DateTime<Utc>) {
        let v = self.automation.lock().unwrap().music.volume_to_save(now);
        let Some(v) = v else { return };
        if let Err(e) = self.edit_untracked(|d| { d.automation.music.volume = v; Ok(()) }) { self.set_notice(&e) }
        let mut rt = self.automation.lock().unwrap();
        if rt.music.volume.is_some_and(|(x, _)| x == v) { rt.music.volume = None }
    }

    /// Where the music player's file is, for tests and the dialog.
    pub fn music_path(&self, asset: &str) -> PathBuf { self.store.asset_path(asset) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(n: u128, shuffle: bool) -> MusicList {
        MusicList { tracks: (1..=n).map(|i| Track { id: Uuid::from_u128(i), asset: format!("{i}.mp3"), title: format!("Song {i}"), duration: None }).collect(), shuffle, ..MusicList::default() }
    }
    fn id(i: u128) -> Uuid { Uuid::from_u128(i) }
    fn playing_title(r: &MusicRun, l: &MusicList) -> String { r.current(l).map(|t| t.title.clone()).unwrap_or_default() }

    #[test]
    fn plays_in_order_pauses_and_goes_on() {
        let l = list(3, false);
        let mut r = MusicRun::default();
        assert_eq!(r.wanted(&l), None);
        r.play(&l, 7).unwrap();
        assert_eq!(r.wanted(&l), Some(MusicWant { inst: 1, asset: "1.mp3".into(), paused: false, volume: 70 }));
        r.pause();
        assert_eq!(r.wanted(&l).unwrap().paused, true);
        assert_eq!(r.wanted(&l).unwrap().inst, 1, "a pause keeps the same song loaded");
        r.play(&l, 7).unwrap();
        assert_eq!(r.state, MusicState::Playing);
        r.next(&l);
        assert_eq!(playing_title(&r, &l), "Song 2");
        assert_eq!(r.inst, 2);
        r.stop();
        assert_eq!(r.wanted(&l), None);
    }

    #[test]
    fn an_empty_list_cannot_play() {
        assert_eq!(MusicRun::default().play(&MusicList::default(), 1), Err(super::super::strings::music_empty()));
    }

    #[test]
    fn shuffle_plays_every_song_once_and_is_the_same_for_a_seed() {
        let l = list(8, true);
        let (mut a, mut b) = (1u64, 1u64);
        let one = play_order(&l, &mut a, None);
        assert_eq!(one, play_order(&l, &mut b, None));
        let mut sorted = one.clone();
        sorted.sort();
        assert_eq!(sorted, (1..=8).map(id).collect::<Vec<_>>());
        assert_ne!(one, (1..=8).map(id).collect::<Vec<_>>(), "it is shuffled");
        assert_eq!(play_order(&list(3, false), &mut a, None), [id(1), id(2), id(3)]);
    }

    #[test]
    fn a_reshuffle_never_repeats_the_last_song_first() {
        let l = list(2, true);
        for seed in 1..50u64 {
            let mut rng = seed;
            for _ in 0..5 { assert_ne!(play_order(&l, &mut rng, Some(id(2)))[0], id(2), "seed {seed}") }
        }
    }

    #[test]
    fn after_the_last_song_it_starts_again_or_stops() {
        let mut l = list(2, false);
        let mut r = MusicRun::default();
        r.play(&l, 1).unwrap();
        r.next(&l);
        r.next(&l);
        assert_eq!(playing_title(&r, &l), "Song 1", "repeat goes back to the top");
        l.repeat = false;
        r.next(&l);
        r.next(&l);
        assert_eq!(r.state, MusicState::Stopped);
    }

    #[test]
    fn a_finished_song_goes_to_the_next_and_old_reports_are_ignored() {
        let l = list(3, false);
        let mut r = MusicRun::default();
        r.play(&l, 1).unwrap();
        assert_eq!(r.report(&l, 1, &PlayState::Playing, 10.0, 200.0), (true, false));
        assert_eq!(r.clock(), Some(MediaClock { left_ms: 190_000, paused: false }));
        assert_eq!(r.report(&l, 1, &PlayState::Ended, 200.0, 200.0), (true, false));
        assert_eq!(playing_title(&r, &l), "Song 2");
        assert_eq!(r.report(&l, 1, &PlayState::Playing, 50.0, 200.0), (false, false), "the old song's player");
        assert_eq!(r.position, 0.0);
    }

    #[test]
    fn a_song_that_fails_is_skipped_and_all_failing_stops() {
        let l = list(2, false);
        let mut r = MusicRun::default();
        r.play(&l, 1).unwrap();
        assert_eq!(r.report(&l, 1, &PlayState::Error, 0.0, 0.0), (true, false));
        assert_eq!(playing_title(&r, &l), "Song 2");
        assert_eq!(r.report(&l, 2, &PlayState::Error, 0.0, 0.0), (true, true));
        assert_eq!(r.state, MusicState::Stopped);
    }

    #[test]
    fn back_restarts_a_song_or_goes_to_the_one_before() {
        let l = list(3, false);
        let mut r = MusicRun::default();
        r.play(&l, 1).unwrap();
        r.next(&l);
        r.report(&l, r.inst, &PlayState::Playing, 30.0, 100.0);
        r.prev();
        assert_eq!(playing_title(&r, &l), "Song 2", "late in a song: from its start");
        assert_eq!(r.position, 0.0);
        r.prev();
        assert_eq!(playing_title(&r, &l), "Song 1");
        r.prev();
        assert_eq!(playing_title(&r, &l), "Song 1");
    }

    #[test]
    fn playing_a_chosen_song() {
        let l = list(3, true);
        let mut r = MusicRun::default();
        r.play_track(&l, id(3), 5).unwrap();
        assert_eq!(playing_title(&r, &l), "Song 3");
        assert_eq!(r.at, 0, "a chosen song starts the run");
        let mut plain = MusicRun::default();
        plain.play_track(&list(3, false), id(2), 5).unwrap();
        assert_eq!(plain.order, [id(2), id(3), id(1)], "then the songs after it");
        r.play_track(&l, id(1), 5).unwrap();
        assert_eq!(playing_title(&r, &l), "Song 1");
        assert!(r.play_track(&l, id(9), 5).is_err());
    }

    #[test]
    fn editing_the_list_while_it_plays() {
        let mut l = list(3, false);
        let mut r = MusicRun::default();
        r.play(&l, 1).unwrap();
        r.next(&l); // Song 2
        let inst = r.inst;
        l.tracks.push(Track { id: id(4), asset: "4.mp3".into(), title: "Song 4".into(), duration: None });
        l.tracks.retain(|t| t.id != id(1));
        r.list_changed(&l);
        assert_eq!(playing_title(&r, &l), "Song 2");
        assert_eq!(r.inst, inst, "the song playing goes on");
        assert_eq!(r.order, [id(2), id(3), id(4)]);
        l.tracks.retain(|t| t.id != id(2));
        r.list_changed(&l);
        assert_eq!(playing_title(&r, &l), "Song 3", "the song playing was taken out: the next one plays");
        assert!(r.inst > inst);
        l.tracks.clear();
        r.list_changed(&l);
        assert_eq!(r.state, MusicState::Stopped);
    }

    #[test]
    fn volume_is_its_own_and_saved_once_it_rests() {
        let l = list(1, false);
        let mut r = MusicRun::default();
        let t0 = Utc::now();
        r.volume = Some((40, t0));
        assert_eq!(r.volume(&l), 40);
        assert_eq!(r.volume_to_save(t0 + TimeDelta::milliseconds(500)), None);
        assert_eq!(r.volume_to_save(t0 + TimeDelta::milliseconds(SAVE_VOLUME_AFTER_MS)), Some(40));
        r.play(&l, 1).unwrap();
        assert_eq!(r.wanted(&l).unwrap().volume, 40);
    }

    #[test]
    fn readout_for_the_mini_player() {
        let l = list(2, true);
        let mut r = MusicRun::default();
        assert_eq!(r.readout(&MusicList::default()), None, "no walk-in music: nothing to show");
        let stopped = r.readout(&l).unwrap();
        assert_eq!((stopped.state, stopped.number, stopped.count, stopped.shuffle), (MusicState::Stopped, 0, 2, true));
        r.play_track(&l, id(2), 3).unwrap();
        r.report(&l, r.inst, &PlayState::Playing, 10.4, 70.0);
        let on = r.readout(&l).unwrap();
        assert_eq!((on.state, on.title.as_str(), on.left_ms, on.number), (MusicState::Playing, "Song 2", 60_000, 1));
    }

    #[test]
    fn ops_parse_from_actions() {
        assert_eq!(MusicOp::parse("volume", &json!({ "volume": 150 })), Some(MusicOp::Volume(100)));
        assert_eq!(MusicOp::parse("shuffle", &json!({ "on": true })), Some(MusicOp::Shuffle(true)));
        assert_eq!(MusicOp::parse("next", &Value::Null), Some(MusicOp::Next));
        assert_eq!(MusicOp::parse("volume", &Value::Null), None);
        assert_eq!(MusicOp::parse("dance", &Value::Null), None);
    }

    #[test]
    fn saved_list_loads_leniently() {
        let l: MusicList = serde_json::from_str("{}").unwrap();
        assert_eq!(l, MusicList::default());
        assert_eq!((l.volume, l.repeat), (70, true));
        let l: MusicList = serde_json::from_str(r#"{"tracks":[{"asset":"a.mp3","title":"A"}],"shuffle":true}"#).unwrap();
        assert_eq!(l.tracks[0].title, "A");
        assert!(l.shuffle);
    }
}
