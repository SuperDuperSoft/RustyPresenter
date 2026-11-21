//! Looking back at a practice tape (`tape`). Each song on it is played through the follower as it is now: from the
//! labels and words the tape holds (`replay`, done in a moment), or from the sound itself, heard again by the ear
//! (`rehear`: slower, and what a change to the ear needs). What Automatic would have put on the screen, left to
//! itself from the song's first slide, is then held against what was on the screen at the time (`score`): for
//! every slide change on the tape, whether Automatic made it too and how much earlier or later, the changes it
//! made that nobody else did, and how much of the time both showed the same slide.
//!
//! The tape's screen is the truth to compare with when the slides were moved by hand at practice (Manual or
//! Suggest); with Automatic on at practice it shows where a hand had to correct it.
use super::align::Heard;
use super::audio::{Ear, Listener};
use super::ear::Lexicon;
use super::engine::{Engine, FollowEvent, Host, LiveSong};
use super::rules::{Mode, Settings, Voice};
use super::tape::{LINES_FILE, SOUND_FILE, heard_of};
use super::voice::{FRAME, RATE};
use crate::model::{Slide, SlideContent};
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

/// The screen at a moment of the tape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    pub at: f64,
    pub slide: usize,
    pub words: bool,
    /// Put there by a hand (else by auto-follow).
    pub by_hand: bool,
}

/// One song on a tape: from when it went live until something else did.
#[derive(Clone, Debug, PartialEq)]
pub struct Take {
    /// The service item (a tape names it; a made-up take has none).
    pub item: Option<Uuid>,
    pub title: String,
    pub from: f64,
    pub until: f64,
    /// Each slide's words.
    pub slides: Vec<String>,
    /// The room's labels: when, what, note onsets a second, and since when a note is being held.
    pub labels: Vec<(f64, Voice, f32, Option<f64>)>,
    /// The ear's words: when each was delivered, and the word (its `at` is when it was sung).
    pub heard: Vec<(f64, Heard)>,
    /// The screen: its state when the song went live, then every change.
    pub screen: Vec<Shown>,
}

fn voice_of(name: &str) -> Voice {
    match name { "singing" => Voice::Singing, "speaking" => Voice::Speaking, "music" => Voice::Music, _ => Voice::Silence }
}

/// The songs on the tape in `dir`, in order.
pub fn read(dir: &Path) -> Result<Vec<Take>, String> {
    let text = std::fs::read_to_string(dir.join(LINES_FILE)).map_err(|e| e.to_string())?;
    Ok(takes(&text))
}

/// The songs in a tape's lines.
pub fn takes(text: &str) -> Vec<Take> {
    let mut out: Vec<Take> = vec![];
    let mut cur: Option<Take> = None;
    let mut last = 0.0f64;
    let close = |cur: &mut Option<Take>, at: f64, out: &mut Vec<Take>| {
        if let Some(mut t) = cur.take() {
            t.until = at.max(t.from);
            if !t.screen.is_empty() { out.push(t) }
        }
    };
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let Some(at) = v["a"].as_f64() else { continue };
        last = last.max(at);
        if let Some(song) = v.get("song") {
            close(&mut cur, at, &mut out);
            let slides = song["slides"].as_array().map(|a| a.iter().map(|s| s.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
            cur = Some(Take { item: song["item"].as_str().and_then(|s| Uuid::parse_str(s).ok()), title: song["title"].as_str().unwrap_or("").to_string(), from: at, until: at, slides, labels: vec![], heard: vec![], screen: vec![] });
        } else if v.get("other").is_some() || v.get("end").is_some() {
            close(&mut cur, at, &mut out);
        } else if let Some(t) = cur.as_mut() {
            if let Some(l) = v.get("live") {
                let slide = l["slide"].as_u64().unwrap_or(0) as usize;
                t.screen.push(Shown { at, slide, words: l["words"].as_bool().unwrap_or(true), by_hand: l["by"].as_str() != Some("auto") });
            } else if let Some(name) = v["voice"].as_str() {
                t.labels.push((at, voice_of(name), v["onsets"].as_f64().unwrap_or(0.0) as f32, v["held"].as_f64()));
            } else if let Some(h) = heard_of(&v) {
                t.heard.push(h);
            }
        }
    }
    close(&mut cur, last, &mut out);
    out
}

fn slide(text: &str) -> Slide { Slide::new("", SlideContent::Text { text: text.into(), footer: None, reference: None, size: 0, parallel: None }) }

/// The song on a pretend screen that only auto-follow changes.
struct Stage(Mutex<LiveSong>);

impl Stage {
    fn of(take: &Take, settings: &Settings, order: &[usize], secs: &[f32]) -> Stage {
        let first = take.screen.first().copied().unwrap_or(Shown { at: take.from, slide: 0, words: true, by_hand: true });
        Stage(Mutex::new(LiveSong { item_id: Uuid::new_v4(), is_song: true, slides: take.slides.iter().map(|t| slide(t)).collect(),
            slide: first.slide.min(take.slides.len().saturating_sub(1)), words_up: first.words, video_playing: false, version: 1, mode: Mode::Auto, order: order.to_vec(), secs: secs.to_vec(),
            settings: Settings { mode: Mode::Auto, ..settings.clone() } }))
    }
    fn state(&self) -> (usize, bool) { let l = self.0.lock().unwrap(); (l.slide, l.words_up) }
}

impl Host for Stage {
    fn live(&self) -> Option<LiveSong> { Some(self.0.lock().unwrap().clone()) }
    fn show(&self, _item: Uuid, slide: usize, _base: u64) -> bool {
        let mut l = self.0.lock().unwrap();
        l.slide = slide;
        l.words_up = true;
        l.version += 1;
        true
    }
    fn clear_slide(&self, _base: u64) -> bool {
        let mut l = self.0.lock().unwrap();
        l.words_up = false;
        l.version += 1;
        true
    }
    fn notice(&self, _text: &str) {}
    fn save_mode(&self, _item: Uuid, _mode: Mode) {}
}

/// Runs the follower over a take's events and notes what it does to the screen.
struct Run { stage: Stage, engine: Engine, shown: Vec<Shown>, trace: Vec<String> }

impl Run {
    fn new(take: &Take, settings: &Settings, order: &[usize], secs: &[f32]) -> Run {
        let stage = Stage::of(take, settings, order, secs);
        let (slide, words) = stage.state();
        let mut run = Run { stage, engine: Engine::new(), shown: vec![Shown { at: take.from, slide, words, by_hand: true }], trace: vec![] };
        run.step(take.from, FollowEvent::Listening { words: true });
        run
    }

    fn step(&mut self, at: f64, ev: FollowEvent) {
        self.engine.step(&self.stage, at, Some(ev));
        self.trace.extend(self.engine.take_trace());
        let (slide, words) = self.stage.state();
        if self.shown.last().is_none_or(|s| (s.slide, s.words) != (slide, words)) { self.shown.push(Shown { at, slide, words, by_hand: false }) }
    }
}

/// What Automatic, as it is now, does with the labels and words the tape holds: the screen from the song's first
/// slide on, and the follower's listening log.
pub fn replay(take: &Take, settings: &Settings) -> (Vec<Shown>, Vec<String>) { replay_expecting(take, settings, &[]) }

/// `replay` with the follower expecting the song in `order` (`order::SungOrder`; empty: as the slides are listed).
pub fn replay_expecting(take: &Take, settings: &Settings, order: &[usize]) -> (Vec<Shown>, Vec<String>) { replay_timed(take, settings, order, &[]) }

/// `replay_expecting`, with how long each entry of `order` was up at practice (`order::SungOrder::secs`).
pub fn replay_timed(take: &Take, settings: &Settings, order: &[usize], secs: &[f32]) -> (Vec<Shown>, Vec<String>) {
    let mut events: Vec<(f64, FollowEvent)> = take.labels.iter()
        .map(|&(at, voice, onsets, held_since)| (at, FollowEvent::Audio { voice, onsets, at, held_since }))
        .chain(take.heard.iter().map(|(at, h)| (*at, FollowEvent::Heard(h.clone())))).collect();
    events.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut run = Run::new(take, settings, order, secs);
    for (at, ev) in events { run.step(at, ev) }
    (run.shown, run.trace)
}

/// The same from the tape's sound, heard again by `ear` (the speech model, or a stand-in in tests).
pub fn rehear(dir: &Path, take: &Take, settings: &Settings, ear: impl FnOnce(Lexicon) -> Box<dyn Ear>) -> Result<(Vec<Shown>, Vec<String>), String> {
    let mut file = std::fs::File::open(dir.join(SOUND_FILE)).map_err(|e| e.to_string())?;
    let first = (take.from * RATE as f64) as u64;
    file.seek(SeekFrom::Start(44 + first * 2)).map_err(|e| e.to_string())?;
    let mut run = Run::new(take, settings, &[], &[]);
    let mut listener = Listener::new(ear(run.engine.lexicon()));
    let mut bytes = vec![0u8; FRAME * 2];
    let mut pcm = vec![0i16; FRAME];
    let mut n = 0u64;
    loop {
        let at = take.from + (n * FRAME as u64) as f64 / RATE as f64;
        if at >= take.until || file.read_exact(&mut bytes).is_err() { break }
        for (s, b) in pcm.iter_mut().zip(bytes.chunks_exact(2)) { *s = i16::from_le_bytes([b[0], b[1]]) }
        for ev in listener.frame(&pcm, at) { run.step(at, ev) }
        n += 1;
    }
    for ev in listener.finish() { run.step(take.until, ev) }
    Ok((run.shown, run.trace))
}

/// A slide change on the tape, and when Automatic made the same one (if it did).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Change { pub at: f64, pub slide: usize, pub by_hand: bool, pub auto_at: Option<f64> }

/// How Automatic compares with the tape for one song.
#[derive(Clone, Debug, PartialEq)]
pub struct Score {
    pub changes: Vec<Change>,
    /// Changes Automatic made that the tape has not.
    pub extra: Vec<Shown>,
    /// The share of the song (0–1) both showed the same slide.
    pub same: f64,
}

/// A change Automatic makes within this many seconds of the tape's is the same change.
pub const MATCH_S: f64 = 8.0;
/// Within this many seconds of the tape's change it is on time.
pub const ON_TIME_S: f64 = 2.0;

/// The changes to another slide (with its words up) in `screen`, after its first entry.
fn changes(screen: &[Shown]) -> Vec<Shown> {
    let mut out = vec![];
    let mut last = screen.first().map(|s| s.slide);
    for s in screen.iter().skip(1) {
        if s.words && Some(s.slide) != last { out.push(*s) }
        if s.words { last = Some(s.slide) }
    }
    out
}

fn at_time(screen: &[Shown], t: f64) -> Option<(usize, bool)> {
    screen.iter().take_while(|s| s.at <= t).last().map(|s| (s.slide, s.words))
}

pub fn score(take: &Take, auto: &[Shown]) -> Score {
    let auto_changes = changes(auto);
    let mut used = vec![false; auto_changes.len()];
    let mut out = vec![];
    for c in changes(&take.screen) {
        let best = auto_changes.iter().enumerate()
            .filter(|(i, a)| !used[*i] && a.slide == c.slide && (a.at - c.at).abs() <= MATCH_S)
            .min_by(|a, b| (a.1.at - c.at).abs().total_cmp(&(b.1.at - c.at).abs())).map(|(i, a)| (i, a.at));
        if let Some((i, _)) = best { used[i] = true }
        out.push(Change { at: c.at, slide: c.slide, by_hand: c.by_hand, auto_at: best.map(|(_, at)| at) });
    }
    let extra = auto_changes.iter().zip(&used).filter(|(_, u)| !**u).map(|(a, _)| *a).collect();
    let steps = ((take.until - take.from) * 10.0).floor().max(1.0) as usize;
    let same = (0..steps).filter(|i| { let t = take.from + *i as f64 / 10.0; at_time(&take.screen, t) == at_time(auto, t) }).count();
    Score { changes: out, extra, same: same as f64 / steps as f64 }
}

/// Minutes and seconds into the tape: `3:07.5`.
fn clock(t: f64) -> String { format!("{}:{:04.1}", (t / 60.0) as u64, t % 60.0) }

impl Score {
    pub fn on_time(&self) -> usize { self.changes.iter().filter(|c| c.auto_at.is_some_and(|a| (a - c.at).abs() <= ON_TIME_S)).count() }
    pub fn late(&self) -> usize { self.changes.iter().filter(|c| c.auto_at.is_some_and(|a| a - c.at > ON_TIME_S)).count() }
    pub fn early(&self) -> usize { self.changes.iter().filter(|c| c.auto_at.is_some_and(|a| c.at - a > ON_TIME_S)).count() }
    pub fn missed(&self) -> usize { self.changes.iter().filter(|c| c.auto_at.is_none()).count() }

    /// The song's lines of the report.
    pub fn lines(&self, take: &Take) -> Vec<String> {
        let by_hand = self.changes.iter().filter(|c| c.by_hand).count();
        let mut out = vec![format!("{} — at {} on the tape, {} long: {} slide changes ({} by hand), {} words heard", if take.title.is_empty() { "Song" } else { &take.title },
            clock(take.from), clock(take.until - take.from), self.changes.len(), by_hand, take.heard.len())];
        out.push(format!("  Auto-follow now: {} on time, {} early, {} late, {} missed, {} of its own; same slide {:.0}% of the time", self.on_time(), self.early(),
            self.late(), self.missed(), self.extra.len(), self.same * 100.0));
        for c in &self.changes {
            let what = match c.auto_at {
                Some(a) if (a - c.at).abs() < 0.05 => "auto at the same moment".to_string(),
                Some(a) if a < c.at => format!("auto {:.1} s earlier", c.at - a),
                Some(a) => format!("auto {:.1} s later", a - c.at),
                None => "auto never".to_string(),
            };
            out.push(format!("    {}  slide {:<3} {}  {what}", clock(c.at), c.slide + 1, if c.by_hand { "hand" } else { "auto" }));
        }
        for e in &self.extra { out.push(format!("    {}  slide {:<3} only auto", clock(e.at), e.slide + 1)) }
        out
    }
}

/// The report on the tape in `dir`: every song on it, Automatic as it is now against the tape's screen.
/// `ear`: hear the sound again with this ear instead of using the words on the tape.
pub fn report(dir: &Path, settings: &Settings, mut ear: Option<&mut dyn FnMut(Lexicon) -> Box<dyn Ear>>) -> Result<Vec<String>, String> {
    let takes = read(dir)?;
    let mut out = vec![format!("Practice tape {}", dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())];
    if takes.is_empty() { out.push("No song was live while this tape ran.".into()) }
    let (mut all, mut good) = (0, 0);
    for take in &takes {
        let (auto, _) = match ear.as_mut() {
            Some(ear) => rehear(dir, take, settings, |l| ear(l))?,
            None => replay(take, settings),
        };
        let s = score(take, &auto);
        all += s.changes.len();
        good += s.on_time();
        out.extend(s.lines(take));
    }
    if all > 0 { out.insert(1, format!("{good} of {all} slide changes made on time by auto-follow as it is now.")) }
    Ok(out)
}

#[cfg(test)]
#[path = "review_tests.rs"]
mod tests;
