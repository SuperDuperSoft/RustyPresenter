//! Where the singers are in the song: a streaming beam search over the words of the planned slide order.
//!
//! "Plan first, jump when sure": going on in the planned order is cheap, skipping a word or two or repeating a
//! line of the same slide costs a little, and a jump anywhere else costs a penalty and must be confirmed by
//! `jump_words` well-matched words in a row before it is ever reported. Talk that matches nothing leaves the
//! position where it was and reports `off_script`.
use super::lyrics::SongText;
use std::collections::HashMap;

/// A word the ear heard: its sound key (`lyrics::sound_key`, or phonemes later), when, and how sure the ear is.
#[derive(Clone, Debug, PartialEq)]
pub struct Heard { pub key: String, pub at: f64, pub conf: f32 }

/// Where the aligner thinks the singers are.
#[derive(Clone, Debug, PartialEq)]
pub struct Position {
    /// The last word sung (index in `SongText::words`); `None` until a word is heard after a (re)start.
    pub word: Option<usize>,
    /// Index in the plan (the planned slide order).
    pub plan_index: usize,
    /// 0–1: how far ahead this place is of the best place on another slide (a slide with the very same words
    /// is not another place: it shows the same thing).
    pub confidence: f32,
    /// The last words matched nothing here (talk, prayer, an ad-lib).
    pub off_script: bool,
}

/// Well-matched words in a row needed before a jump outside the plan is believed.
pub const JUMP_WORDS: u8 = 4;
const BEAM: usize = 64;
/// A heard word that is not the next one (talk, an ad-lib, a misheard word): the place stays.
const INSERT: f32 = 0.6;
/// Each planned word skipped (the ear missed it, or the singers dropped it).
const SKIP: f32 = 0.4;
/// How many words ahead a heard word may land (so at most two are skipped).
const AHEAD: usize = 3;
/// Back to the start of a line of the same slide (singers repeat a line).
const REPEAT: f32 = 0.5;
/// Just put on this slide by the operator: the first word heard may be anywhere on it. The same cost as a
/// repeated line: cheaper than a jump, dearer than the next word.
const SEAT: f32 = 0.5;
/// Anywhere else: a jump outside the plan.
const JUMP: f32 = 3.0;
/// A match at least this good counts towards a jump and clears misses.
const GOOD: f32 = 0.75;
/// Misses in a row that make the place off-script.
const OFF_SCRIPT: u8 = 3;
/// Beam scores this far behind the best are dropped.
const PRUNE: f32 = 15.0;
/// The score margin that counts as fully sure.
const SURE_MARGIN: f32 = 2.0;

/// One planned word.
#[derive(Clone, Debug)]
struct Spot { plan_index: usize, word: usize, line_start: bool }

/// One hypothesis of where the singers are.
#[derive(Clone, Debug)]
struct Hyp {
    /// The next planned word (index in `track`); the last sung one is `next - 1` when `heard`.
    next: usize,
    /// The plan index before anything is heard.
    plan: usize,
    heard: bool,
    score: f32,
    /// Good words still needed before this jump is believed; 0 = believed.
    pending: u8,
    misses: u8,
}

pub struct Aligner {
    plan: Vec<usize>,
    track: Vec<Spot>,
    /// Per plan index, its first word in `track` (`track.len()` when it has none).
    starts: Vec<usize>,
    /// Per plan index, the first plan index whose slide has exactly the same words (itself when there is none,
    /// or when it has no words). Two slides with the same words show the same thing; which copy it is only
    /// matters for what comes next, and the next words decide that.
    twin: Vec<usize>,
    by_key: HashMap<String, Vec<usize>>,
    keys: Vec<String>,
    beam: Vec<Hyp>,
    jump_words: u8,
    /// The operator just put this slide up (`seat_anywhere`): the first word heard may be anywhere on it.
    seat: bool,
    last: Position,
}

impl Aligner {
    /// `plan`: the slide indices in the order they are planned (for live slides: 0, 1, 2…). Every slide of the
    /// plan is a candidate; slides outside the song are ignored.
    pub fn new(song: &SongText, plan: &[usize]) -> Aligner {
        let mut track = Vec::new();
        let mut starts = Vec::new();
        let mut keys = Vec::new();
        for (p, &slide) in plan.iter().enumerate() {
            starts.push(track.len());
            let Some(span) = song.slides.get(slide) else { continue };
            for w in span.words() {
                let line_start = w == span.first_word || song.words[w].line != song.words[w - 1].line;
                track.push(Spot { plan_index: p, word: w, line_start });
                keys.push(song.words[w].key.clone());
            }
        }
        let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
        for (t, k) in keys.iter().enumerate() { if !k.is_empty() { by_key.entry(k.clone()).or_default().push(t) } }
        let twin: Vec<usize> = (0..starts.len())
            .map(|p| (0..p).find(|&q| !words_of(&keys, &starts, p).is_empty() && words_of(&keys, &starts, q) == words_of(&keys, &starts, p)).unwrap_or(p))
            .collect();
        let mut a = Aligner { plan: plan.to_vec(), track, starts, twin, by_key, keys, beam: vec![], jump_words: JUMP_WORDS, seat: false,
            last: Position { word: None, plan_index: 0, confidence: 0.0, off_script: false } };
        a.restart(0);
        a
    }

    /// Words in a row needed to believe a jump outside the plan (Settings → jump strictness), at least 2.
    pub fn with_jump_words(mut self, n: u8) -> Aligner { self.jump_words = n.max(2); self }

    /// The place as last reported.
    pub fn position(&self) -> &Position { &self.last }

    /// The operator took `slide` live: start again from its beginning, at its planned place nearest ahead (a
    /// slide outside the plan starts from the plan's beginning).
    pub fn reset_to(&mut self, slide: usize) {
        let here = self.last.plan_index;
        let at = |p: &usize| self.plan[*p] == slide;
        let p = (here..self.plan.len()).find(at).or_else(|| (0..self.plan.len()).find(at)).unwrap_or(0);
        self.restart(p)
    }

    /// Call right after `reset_to` when the operator moved by hand in mid-song (Back after an early change): the
    /// singers may be anywhere on that slide, so the first word heard may land on any of its words, not only
    /// its first three. It never reaches another slide, and it ends at the next `reset_to`. A song's start
    /// does not ask for it: there one stray word matching the last line would change the slide early.
    pub fn seat_anywhere(&mut self) { self.seat = true }

    fn restart(&mut self, plan_index: usize) {
        let p = plan_index.min(self.starts.len().saturating_sub(1));
        let next = self.starts.get(p).copied().unwrap_or(0);
        self.beam = vec![Hyp { next, plan: p, heard: false, score: 0.0, pending: 0, misses: 0 }];
        self.last = Position { word: None, plan_index: p, confidence: 0.0, off_script: false };
        self.seat = false;
    }

    /// How well `heard` matches planned word `t`: 1 same key, less for short keys ("the", "a"), 0.5 one sound
    /// off (`fuzzy` only), else 0.
    fn score(&self, heard: &str, t: usize, fuzzy: bool) -> f32 {
        let key = &self.keys[t];
        if key.is_empty() || heard.is_empty() { return 0.0 }
        if key == heard { return if key.chars().count() <= 1 { 0.7 } else { 1.0 } }
        if fuzzy && key.chars().count() >= 4 && heard.chars().count() >= 4 && one_edit(key, heard) { 0.5 } else { 0.0 }
    }

    fn plan_of(&self, h: &Hyp) -> usize { if h.heard { self.track[h.next - 1].plan_index } else { h.plan } }

    /// Takes one heard word and returns the new place.
    pub fn push(&mut self, heard: &Heard) -> Position {
        if self.track.is_empty() { return self.last.clone() }
        let weight = heard.conf.clamp(0.3, 1.0);
        let key = heard.key.as_str();
        let mut out: Vec<Hyp> = Vec::with_capacity(self.beam.len() * 4);
        for h in &self.beam {
            // The place stays (talk, an ad-lib, a misheard word). A jump still being confirmed ends here.
            if h.pending == 0 { out.push(Hyp { score: h.score - INSERT, misses: h.misses.saturating_add(1), ..h.clone() }) }
            // On in the plan, skipping at most two words.
            for k in 0..AHEAD {
                let t = h.next + k;
                if t >= self.track.len() { break }
                let m = self.score(key, t, k == 0);
                if m <= 0.0 { continue }
                let pending = match h.pending {
                    0 => 0,
                    _ if k > 0 => continue,
                    p if m >= GOOD => p - 1,
                    p => p,
                };
                out.push(Hyp { next: t + 1, plan: h.plan, heard: true, score: h.score + m * weight - SKIP * k as f32, pending,
                    misses: if m >= GOOD { 0 } else { h.misses } });
            }
            // Seated by the operator and nothing heard yet: any later word of this slide, when it sounds exactly the same.
            if self.seat && !h.heard {
                let end = self.starts.get(h.plan + 1).copied().unwrap_or(self.track.len());
                for t in (h.next + AHEAD)..end {
                    let m = self.score(key, t, false);
                    if m >= GOOD { out.push(Hyp { next: t + 1, plan: h.plan, heard: true, score: h.score + m * weight - SEAT, pending: 0, misses: 0 }) }
                }
            }
            // A line of the same slide again.
            if h.heard && h.pending == 0 {
                let p = self.track[h.next - 1].plan_index;
                let first = self.starts[p];
                for t in (first..h.next).filter(|&t| self.track[t].line_start) {
                    for (k, t) in [(0, t), (1, t + 1)] {
                        if t >= h.next { continue }
                        let m = self.score(key, t, false);
                        if m > 0.0 {
                            out.push(Hyp { next: t + 1, plan: h.plan, heard: true, score: h.score + m * weight - REPEAT - SKIP * k as f32, pending: 0,
                                misses: if m >= GOOD { 0 } else { h.misses } });
                        }
                    }
                }
            }
        }
        // A jump outside the plan, from the best believed place, to a word that sounds exactly the same.
        if let Some(best) = self.beam.iter().filter(|h| h.pending == 0).max_by(|a, b| a.score.total_cmp(&b.score)).cloned() {
            let here = self.plan_of(&best);
            let near = |t: usize| {
                let s = &self.track[t];
                (t >= best.next && t < best.next + AHEAD) || (s.plan_index == here && t < best.next)
            };
            for &t in self.by_key.get(key).into_iter().flatten() {
                let m = self.score(key, t, false);
                if m < GOOD || near(t) { continue }
                let to = self.track[t].plan_index;
                // Nearer places first, ahead before behind.
                let far = if to >= here { (to - here) as f32 * 0.05 } else { 0.05 + (here - to) as f32 * 0.1 };
                out.push(Hyp { next: t + 1, plan: to, heard: true, score: best.score + m * weight - JUMP - far, pending: self.jump_words - 1, misses: 0 });
            }
        }
        // Keep the best per place; a jump being confirmed is kept apart from a believed place.
        out.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut seen = std::collections::HashSet::new();
        out.retain(|h| seen.insert((h.next, h.heard, h.pending > 0)));
        let top = out.first().map(|h| h.score).unwrap_or(0.0);
        out.retain(|h| h.score >= top - PRUNE);
        out.truncate(BEAM);
        for h in &mut out { h.score -= top }
        if !out.is_empty() { self.beam = out }
        self.last = self.report();
        self.last.clone()
    }

    fn report(&self) -> Position {
        let Some(best) = self.beam.iter().filter(|h| h.pending == 0).max_by(|a, b| a.score.total_cmp(&b.score)) else { return self.last.clone() };
        let here = self.plan_of(best);
        // Anything else on another slide, a jump being confirmed too, makes it less sure. A slide with the same
        // words (a chorus that is in the plan more than once) is not a rival: the screen shows the same either way.
        let rival = self.beam.iter().filter(|h| self.twin.get(self.plan_of(h)) != self.twin.get(here)).map(|h| h.score).fold(f32::NEG_INFINITY, f32::max);
        let off_script = best.misses >= OFF_SCRIPT;
        let confidence = if !best.heard || off_script { 0.0 } else { ((best.score - rival) / SURE_MARGIN).clamp(0.0, 1.0) };
        Position { word: best.heard.then(|| self.track[best.next - 1].word), plan_index: here, confidence, off_script }
    }
}

/// The word keys of plan index `p`: from its first word to the next plan index's first.
fn words_of<'a>(keys: &'a [String], starts: &[usize], p: usize) -> &'a [String] {
    &keys[starts[p]..starts.get(p + 1).copied().unwrap_or(keys.len())]
}

/// True when `a` and `b` differ by one letter added, removed or changed.
fn one_edit(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (short, long) = if a.len() <= b.len() { (&a, &b) } else { (&b, &a) };
    match long.len() - short.len() {
        0 => short.iter().zip(long.iter()).filter(|(x, y)| x != y).count() == 1,
        1 => (0..long.len()).any(|skip| long.iter().enumerate().filter(|(i, _)| *i != skip).map(|(_, c)| c).eq(short.iter())),
        _ => false,
    }
}

#[cfg(test)]
#[path = "align_tests.rs"]
mod tests;
