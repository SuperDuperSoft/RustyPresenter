//! The app's own sound in a recording. The room's players never hand out their sound, and capturing the speakers
//! would record everything else the computer plays too. So while recording, the room says four times a second
//! where each audible player is in its file (`Heard`); this turns that into stretches of files played at a time
//! (`Segment`), and `args::finish` mixes those stretches from the files into the MP4 when the recording stops.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How far a player's position may move off the clock between two looks and still count as playing on (seconds).
const DRIFT: f64 = 0.1;
/// Less than this far along since the last look is "not moving" (paused, loading).
const STILL: f64 = 0.02;
/// Shorter stretches are left out.
const SHORTEST: f64 = 0.05;

/// One audible player as the room sees it now.
#[derive(Clone, Debug, PartialEq)]
pub struct Heard {
    /// Tells players apart (`Player::serial`).
    pub key: u64,
    pub path: PathBuf,
    /// Seconds into the file.
    pub pos: f64,
    /// 0 to 1; 0 when muted.
    pub volume: f64,
}

/// `path` from `from` to `to` seconds, heard `at` seconds into the recording, at `volume`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub path: PathBuf,
    pub from: f64,
    pub to: f64,
    pub at: f64,
    pub volume: f64,
}

#[derive(Clone, Debug)]
struct Track {
    key: u64,
    path: PathBuf,
    volume: f64,
    last_t: f64,
    last_pos: f64,
    /// Playing since `from` at `at`; `None` while not seen moving.
    run: Option<(f64, f64)>,
}

/// The stretches heard so far, and the players being followed.
#[derive(Debug, Default)]
pub struct SoundLog {
    tracks: Vec<Track>,
    done: Vec<Segment>,
}

impl SoundLog {
    /// What the room hears `t` seconds into the recording.
    pub fn hear(&mut self, t: f64, now: &[Heard]) {
        let mut next = Vec::with_capacity(now.len());
        for tr in std::mem::take(&mut self.tracks) {
            match now.iter().find(|h| h.key == tr.key && h.path == tr.path) {
                Some(h) => next.push(self.follow(tr, t, h)),
                // Gone (stopped, or another file in the same player): it played on until about now.
                None => self.end(&tr, tr.last_pos + (t - tr.last_t).max(0.0)),
            }
        }
        let new: Vec<&Heard> = now.iter().filter(|h| !next.iter().any(|tr: &Track| tr.key == h.key && tr.path == h.path)).collect();
        for h in new {
            next.push(Track { key: h.key, path: h.path.clone(), volume: h.volume, last_t: t, last_pos: h.pos, run: None });
        }
        self.tracks = next;
    }

    fn follow(&mut self, mut tr: Track, t: f64, h: &Heard) -> Track {
        let (dt, d) = (t - tr.last_t, h.pos - tr.last_pos);
        let steady = (d - dt).abs() <= DRIFT;
        match tr.run {
            Some(_) if steady && h.volume == tr.volume => {}
            Some(_) if steady => {
                self.end(&tr, h.pos);
                tr.run = Some((h.pos, t));
            }
            Some(_) if (-STILL..dt - DRIFT).contains(&d) => {
                // Paused (or stalled) since the last look: it stopped where it is now.
                self.end(&tr, h.pos);
                tr.run = None;
            }
            Some(_) => {
                // A jump (a skip button, a loop): it played on until the jump.
                self.end(&tr, tr.last_pos + dt.max(0.0));
                tr.run = None;
            }
            None if d > STILL => {
                // Started moving since the last look: it set off `d` seconds ago (at most since that look).
                let from = h.pos - d.min(dt.max(0.0));
                tr.run = Some((from, t - (h.pos - from)));
            }
            None => {}
        }
        tr.volume = h.volume;
        tr.last_t = t;
        tr.last_pos = h.pos;
        tr
    }

    fn end(&mut self, tr: &Track, to: f64) {
        let Some((from, at)) = tr.run else { return };
        if to - from >= SHORTEST && tr.volume > 0.0 {
            self.done.push(Segment { path: tr.path.clone(), from, to, at: at.max(0.0), volume: tr.volume });
        }
    }

    /// Everything heard up to `t`, as if every player stopped then. The log goes on as it was.
    pub fn segments(&self, t: f64) -> Vec<Segment> {
        let mut copy = SoundLog { tracks: self.tracks.clone(), done: self.done.clone() };
        copy.hear(t, &[]);
        copy.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(key: u64, file: &str, pos: f64) -> Heard { Heard { key, path: file.into(), pos, volume: 1.0 } }

    fn seg(file: &str, from: f64, to: f64, at: f64) -> Segment { Segment { path: file.into(), from, to, at, volume: 1.0 } }

    fn close(a: &[Segment], b: &[Segment]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.path == b.path && (a.from - b.from).abs() < 1e-6 && (a.to - b.to).abs() < 1e-6 && (a.at - b.at).abs() < 1e-6 && a.volume == b.volume)
    }

    /// Looks four times a second from `t0` to `t1`, with `pos(t)` for one player (`None`: not there).
    fn run(log: &mut SoundLog, t0: f64, t1: f64, pos: impl Fn(f64) -> Option<Heard>) {
        let mut t = t0;
        while t <= t1 + 1e-9 {
            let now: Vec<Heard> = pos(t).into_iter().collect();
            log.hear(t, &now);
            t += 0.25;
        }
    }

    #[test]
    fn a_song_played_through_is_one_stretch() {
        let mut log = SoundLog::default();
        // Starts 2 s into the recording, from the top, and plays 10 s; stopped at 12 s.
        run(&mut log, 0.0, 14.0, |t| (2.0..12.0).contains(&t).then(|| h(1, "song.mp3", t - 2.0)));
        let s = log.segments(14.0);
        assert!(close(&s, &[seg("song.mp3", 0.0, 10.0, 2.0)]), "{s:?}");
    }

    #[test]
    fn a_start_between_looks_is_placed_by_how_far_it_got() {
        let mut log = SoundLog::default();
        log.hear(0.0, &[h(1, "a.mp4", 0.0)]); // loaded, not moving yet
        log.hear(0.25, &[h(1, "a.mp4", 0.0)]);
        log.hear(0.5, &[h(1, "a.mp4", 0.1)]); // set off at 0.4 s
        log.hear(0.75, &[h(1, "a.mp4", 0.35)]);
        let s = log.segments(0.75);
        assert!(close(&s, &[seg("a.mp4", 0.0, 0.35, 0.4)]), "{s:?}");
    }

    #[test]
    fn a_pause_ends_the_stretch_where_it_stopped_and_play_starts_another() {
        let mut log = SoundLog::default();
        // Plays 0–5 s, paused at file 5.0 for 3 s, plays on from 8 s to 10 s.
        run(&mut log, 0.0, 10.0, |t| Some(h(1, "v.mp4", if t < 5.0 { t } else if t < 8.0 { 5.0 } else { t - 3.0 })));
        let s = log.segments(10.0);
        assert!(close(&s, &[seg("v.mp4", 0.0, 5.0, 0.0), seg("v.mp4", 5.0, 7.0, 8.0)]), "{s:?}");
    }

    #[test]
    fn a_skip_ends_the_stretch_and_the_new_place_goes_on() {
        let mut log = SoundLog::default();
        // Plays 0–4 s, then jumps 30 s ahead at 4 s.
        run(&mut log, 0.0, 6.0, |t| Some(h(1, "v.mp4", if t < 4.0 { t } else { t + 30.0 })));
        let s = log.segments(6.0);
        assert_eq!(s.len(), 2, "{s:?}");
        assert!(close(&s[..1], &[seg("v.mp4", 0.0, 4.0, 0.0)]), "{s:?}");
        assert!((s[1].from - 34.25).abs() < 0.3 && (s[1].at - 4.25).abs() < 0.3 && (s[1].to - 36.0).abs() < 1e-6, "{s:?}");
    }

    #[test]
    fn two_players_at_once_are_two_stretches() {
        let mut log = SoundLog::default();
        let mut t = 0.0;
        while t <= 4.0 {
            log.hear(t, &[h(1, "video.mp4", t), h(2, "music.mp3", t + 10.0)]);
            t += 0.25;
        }
        let s = log.segments(4.0);
        assert!(close(&s, &[seg("video.mp4", 0.0, 4.0, 0.0), seg("music.mp3", 10.0, 14.0, 0.0)]), "{s:?}");
    }

    #[test]
    fn muted_players_are_left_out_and_a_volume_change_starts_a_stretch() {
        let mut log = SoundLog::default();
        let mut t = 0.0;
        while t <= 4.0 {
            let vol = if t < 2.0 { 1.0 } else { 0.5 };
            log.hear(t, &[Heard { volume: vol, ..h(1, "a.mp3", t) }, Heard { volume: 0.0, ..h(2, "bg.mp4", t) }]);
            t += 0.25;
        }
        let s = log.segments(4.0);
        assert!(close(&s, &[seg("a.mp3", 0.0, 2.0, 0.0), Segment { volume: 0.5, ..seg("a.mp3", 2.0, 4.0, 2.0) }]), "{s:?}");
    }

    #[test]
    fn a_new_file_in_the_same_player_ends_the_old_one() {
        let mut log = SoundLog::default();
        run(&mut log, 0.0, 4.0, |t| Some(if t < 2.0 { h(1, "a.mp3", t) } else { h(1, "b.mp3", t - 2.0) }));
        let s = log.segments(4.0);
        assert!(close(&s, &[seg("a.mp3", 0.0, 2.0, 0.0), seg("b.mp3", 0.0, 2.0, 2.0)]), "{s:?}");
    }

    #[test]
    fn segments_can_be_read_while_it_goes_on() {
        let mut log = SoundLog::default();
        run(&mut log, 0.0, 2.0, |t| Some(h(1, "a.mp3", t)));
        assert!(close(&log.segments(2.0), &[seg("a.mp3", 0.0, 2.0, 0.0)]));
        run(&mut log, 2.25, 3.0, |t| Some(h(1, "a.mp3", t)));
        assert!(close(&log.segments(3.0), &[seg("a.mp3", 0.0, 3.0, 0.0)]), "still one stretch");
    }

    #[test]
    fn a_player_that_never_moves_is_not_heard() {
        let mut log = SoundLog::default();
        run(&mut log, 0.0, 3.0, |_| Some(h(1, "paused.mp4", 12.0)));
        assert!(log.segments(3.0).is_empty());
    }
}
