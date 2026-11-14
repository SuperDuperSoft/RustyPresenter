//! Where the singers are in the song: a streaming beam search over the words of the planned slide order.
//!
//! "Plan first, jump when sure": going on in the planned order is cheap, skipping a word or two or repeating a
//! line of the same slide costs a little, and a jump anywhere else costs a penalty and must be confirmed by
//! `jump_words` well-matched words, in order, before it is ever reported. The ear mishears sung words, so a jump
//! being confirmed may fail to explain a word or two on the way (`Hyp::slips`): the leader who goes to another
//! slide is followed after about a line, not only after four words heard perfectly one after the other. Talk
//! that matches nothing leaves the position where it was and reports `off_script`.
//!
//! One jump is cheaper: when the ear misses three to five words in a row, the singers are a little further on
//! along the SAME slide. That place is found again by two exact words in a row, and it never reaches another
//! slide, so the worst a false one can do is move the place a few words on along the slide that is already up.
//! (Not on a slide whose words are in the plan again with a different slide after them: there the copies must
//! stay level, so every copy keeps the full proof.)
use super::g2p;
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
    /// 0–1: how far ahead this place is of the best place on another slide. A slide with the very same words
    /// is not another place (it shows the same thing) until its last line is sung: there the next slide is
    /// about to go up, so a copy that is followed by other words is another place again. 0 while a place the
    /// operator's move seated in mid-slide is still unproved, and while a one-letter word ("I", "Oh") has
    /// carried the place onto a slide's last line and the planned next word has not been heard exactly yet.
    pub confidence: f32,
    /// The last words matched nothing here (talk, prayer, an ad-lib).
    pub off_script: bool,
}

/// Well-matched words, in order, needed before a jump outside the plan is believed.
pub const JUMP_WORDS: u8 = 4;
const BEAM: usize = 128;
/// A heard word that is not the next one (talk, an ad-lib, a misheard word): the place stays.
const INSERT: f32 = 0.6;
/// Each planned word skipped (the ear missed it, or the singers dropped it).
const SKIP: f32 = 0.4;
/// How many words ahead a heard word may land (so at most two are skipped).
const AHEAD: usize = 3;
/// How far ahead, on its own slide, a place that has lost the singers is looked for again (nearer than `AHEAD`
/// is a plain skip): the ear missed three to five words in a row, about a line.
const REFIND: usize = 5;
/// Back to the start of a line of the same slide (singers repeat a line).
const REPEAT: f32 = 0.5;
/// Just put on this slide by the operator: the first word heard may be anywhere on it. Dearer than skipping
/// two words (2 × `SKIP`), so the slide's opening wins whenever it explains the word: a refrain's last line
/// often repeats its first ("it is well … it is well with my soul"), and a place on the last line brings the
/// next slide up. Still far cheaper than a jump.
const SEAT: f32 = 0.9;
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
    /// While the jump is being confirmed: how many more times it may fail to explain what the ear delivers (a
    /// stray or misheard word, or a planned word the ear missed). The words that prove it are as many as ever;
    /// they need not come one right after the other. A cheaper proof (`refind`) has none to spare.
    slips: u8,
    misses: u8,
    /// Seated in mid-slide by the operator's move (`seat_anywhere`), or carried onto a slide's last line by a
    /// one-letter word (`push`): planned next words, heard exactly, still needed before this place is sure;
    /// 0 = proved, or never in doubt.
    seated: u8,
    /// A place found again a little ahead on its own slide, still being confirmed by the cheaper proof (two
    /// exact words in a row instead of `jump_words`): it is only ever finished on that slide (`plan`).
    refind: bool,
}

pub struct Aligner {
    plan: Vec<usize>,
    track: Vec<Spot>,
    /// Per plan index, its first word in `track` (`track.len()` when it has none).
    starts: Vec<usize>,
    /// Per plan index, the first plan index whose slide has exactly the same words (itself when there is none,
    /// or when it has no words). Two slides with the same words show the same thing; which copy it is only
    /// matters for what comes next, and the next words decide that (see `report`: on the last line, copies
    /// that are followed by different words are rivals until then).
    twin: Vec<usize>,
    by_key: HashMap<String, Vec<usize>>,
    keys: Vec<String>,
    /// Per planned word (`track`), whether the phoneme ear never delivers it (`undelivered`: "a").
    silent: Vec<bool>,
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
        let mut silent = Vec::new();
        for (p, &slide) in plan.iter().enumerate() {
            starts.push(track.len());
            let Some(span) = song.slides.get(slide) else { continue };
            for w in span.words() {
                let line_start = w == span.first_word || song.words[w].line != song.words[w - 1].line;
                track.push(Spot { plan_index: p, word: w, line_start });
                keys.push(song.words[w].key.clone());
                silent.push(undelivered(&song.words[w].text));
            }
        }
        let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
        for (t, k) in keys.iter().enumerate() { if !k.is_empty() { by_key.entry(k.clone()).or_default().push(t) } }
        let twin: Vec<usize> = (0..starts.len())
            .map(|p| (0..p).find(|&q| !words_of(&keys, &starts, p).is_empty() && words_of(&keys, &starts, q) == words_of(&keys, &starts, p)).unwrap_or(p))
            .collect();
        let mut a = Aligner { plan: plan.to_vec(), track, starts, twin, by_key, keys, silent, beam: vec![], jump_words: JUMP_WORDS, seat: false,
            last: Position { word: None, plan_index: 0, confidence: 0.0, off_script: false } };
        a.restart(0);
        a
    }

    /// Words needed to believe a jump outside the plan (Settings → jump strictness), at least 2.
    pub fn with_jump_words(mut self, n: u8) -> Aligner { self.jump_words = n.max(2); self }

    /// How often a jump being confirmed may fail to explain a word (`Hyp::slips`): one for every two words of
    /// its proof. Four words asked for: four good ones among six at most.
    fn slips(&self) -> u8 { self.jump_words / 2 }

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
    ///
    /// A place found this way is reported at once but stays unsure (confidence 0) until `jump_words` exact
    /// words in a row have been heard from it, the same proof as a jump: one or two common words must never
    /// put the singers on the slide's last line, where the next slide would come up. The slide's opening wins
    /// when it explains the word (`SEAT`), and of equally good places the earliest one (`best`).
    pub fn seat_anywhere(&mut self) { self.seat = true }

    fn restart(&mut self, plan_index: usize) {
        let p = plan_index.min(self.starts.len().saturating_sub(1));
        let next = self.starts.get(p).copied().unwrap_or(0);
        self.beam = vec![Hyp { next, plan: p, heard: false, score: 0.0, pending: 0, slips: 0, misses: 0, seated: 0, refind: false }];
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

    /// A step from a place whose next word is `from` to word `t` comes onto the last line of `t`'s slide from
    /// outside it: a line starts on the way, and none starts after `t` on that slide.
    fn onto_last_line(&self, from: usize, t: usize) -> bool {
        let end = self.starts.get(self.track[t].plan_index + 1).copied().unwrap_or(self.track.len());
        (from..=t).any(|s| self.track[s].line_start) && !(t + 1..end).any(|s| self.track[s].line_start)
    }

    /// The best believed place. Of equally good ones the earliest: when two lines of a slide hold the same
    /// words ("with my soul" in the middle and at the end), what was heard fits both, and taking the later one
    /// would bring the next slide up while the earlier line is sung. The earlier one is late at worst; the
    /// words that follow tell them apart.
    fn best(&self) -> Option<&Hyp> {
        self.beam.iter().filter(|h| h.pending == 0).max_by(|a, b| a.score.total_cmp(&b.score).then(b.next.cmp(&a.next)))
    }

    /// Takes one heard word and returns the new place.
    pub fn push(&mut self, heard: &Heard) -> Position {
        if self.track.is_empty() { return self.last.clone() }
        let weight = heard.conf.clamp(0.3, 1.0);
        let key = heard.key.as_str();
        // A key of one plain letter ("I", "Oh" and "a" all sound "A"). A Han or kana word is one character too,
        // but it is a whole word: those are longer than one byte and are not held to what follows.
        let short = key.len() == 1;
        let mut out: Vec<Hyp> = Vec::with_capacity(self.beam.len() * 4);
        for h in &self.beam {
            // The place stays (talk, an ad-lib, a misheard word). A jump still being confirmed has a few such
            // words to spare and then ends here; a cheaper proof ends at the first.
            if h.pending == 0 {
                out.push(Hyp { score: h.score - INSERT, misses: h.misses.saturating_add(1), ..h.clone() })
            } else if !h.refind && h.slips > 0 {
                out.push(Hyp { score: h.score - INSERT, slips: h.slips - 1, ..h.clone() })
            }
            // On in the plan, skipping at most two words.
            for k in 0..AHEAD {
                let t = h.next + k;
                if t >= self.track.len() { break }
                let m = self.score(key, t, k == 0);
                if m <= 0.0 { continue }
                // Planned words stepped over that the ear does deliver. (Some it never delivers, "a": stepping
                // over those is free, and no proof either.) A jump being confirmed has a few to spare (`slips`),
                // a cheaper proof none. Only the word heard counts, so the proof is as long as before.
                let missed = (h.next..t).filter(|&s| !self.silent[s]).count() as u8;
                let mut slips = h.slips;
                let pending = match h.pending {
                    0 => 0,
                    _ if missed > 0 && (h.refind || missed > slips) => continue,
                    // The cheaper proof of a re-find is for its own slide: it is never finished on the next one.
                    _ if h.refind && self.track[t].plan_index != h.plan => continue,
                    p => {
                        slips -= missed;
                        if m >= GOOD { p - 1 } else { p }
                    }
                };
                // A seated place is proved by the planned next word, heard exactly (as a jump is).
                let mut seated = if k == 0 && m >= GOOD { h.seated.saturating_sub(1) } else { h.seated };
                // A one-letter word that carries the place onto a slide's last line proves nothing either: there the
                // next slide would come up, and the vowel of a note held at the end of the line before ("to
                // you——") is heard as just such a word ("Oh", "I"). The place moves on, but it is not sure until
                // the planned next word is heard exactly: a word late at worst, never the next slide a line early.
                if short && self.onto_last_line(h.next, t) { seated = seated.max(1) }
                out.push(Hyp { next: t + 1, plan: h.plan, heard: true, score: h.score + m * weight - SKIP * k as f32, pending, slips,
                    misses: if m >= GOOD { 0 } else { h.misses }, seated, refind: h.refind && pending > 0 });
            }
            // Seated by the operator and nothing heard yet: any later word of this slide, when it sounds exactly the
            // same. Shown as the place at once, but not sure until `jump_words` exact words in a row are heard.
            if self.seat && !h.heard {
                let end = self.starts.get(h.plan + 1).copied().unwrap_or(self.track.len());
                for t in (h.next + AHEAD)..end {
                    let m = self.score(key, t, false);
                    if m >= GOOD {
                        out.push(Hyp { next: t + 1, plan: h.plan, heard: true, score: h.score + m * weight - SEAT, pending: 0, slips: 0, misses: 0,
                            seated: self.jump_words - 1, refind: false });
                    }
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
                                slips: 0, misses: if m >= GOOD { 0 } else { h.misses }, seated: h.seated, refind: false });
                        }
                    }
                }
            }
        }
        // A jump outside the plan, from the best believed place, to a word that sounds exactly the same.
        if let Some(best) = self.best().cloned() {
            let here = self.plan_of(&best);
            let near = |t: usize| {
                let s = &self.track[t];
                (t >= best.next && t < best.next + AHEAD) || (s.plan_index == here && t < best.next)
            };
            // The place was heard on this slide, is proved, and has just failed to explain a word: the ear may have
            // missed three words in a row or more, and then the singers are a little further on along this slide.
            // (A place nothing was heard from, or one still unproved (`Hyp::seated`), keeps the full proof.)
            let lost = best.heard && best.seated == 0 && best.misses >= 1;
            // This slide's words are in the plan again, and that copy goes on to a different slide (a chorus that is
            // in the list twice). Found again cheaply, this copy alone would move on and leave the other behind;
            // on the last line that makes a guessed copy sure (`report`) and brings its next slide up. Such a
            // slide keeps the full proof, which every copy gets alike, so the copies stay level.
            let split = (0..self.twin.len())
                .any(|p| p != here && self.twin.get(p) == self.twin.get(here) && self.twin.get(p + 1) != self.twin.get(here + 1));
            for &t in self.by_key.get(key).into_iter().flatten() {
                let m = self.score(key, t, false);
                if m < GOOD || near(t) { continue }
                let to = self.track[t].plan_index;
                // Nearer places first, ahead before behind.
                let far = if to >= here { (to - here) as f32 * 0.05 } else { 0.05 + (here - to) as f32 * 0.1 };
                // Found again a little ahead on the SAME slide: two exact words in a row are enough (this one and
                // the next), at the price of the words stepped over instead of a jump's. So the place found is
                // sure at once and the next slide still comes up in time, where the full proof left the old slide
                // up for a line of the next one. Wrong, it only moves the place a few words on along the slide
                // that is already up. Never for another slide: the second word must be on this slide too (the
                // slide's last word keeps the full proof), and the proof is held to this slide (`Hyp::refind`). And
                // never on a slide with a copy that goes on to a different slide (`split`).
                let ahead = (best.next + AHEAD..=best.next + REFIND).contains(&t);
                let second = (t + 1..self.track.len()).find(|&s| !self.silent[s]);
                let refind = lost && !split && to == here && ahead && second.is_some_and(|s| self.track[s].plan_index == here);
                let (cost, pending) = if refind { ((SKIP * (t - best.next) as f32).min(JUMP), 1) } else { (JUMP, self.jump_words - 1) };
                // (`far` is 0 for a re-find: it is on this slide.)
                out.push(Hyp { next: t + 1, plan: to, heard: true, score: best.score + m * weight - cost - far, pending,
                    slips: if refind { 0 } else { self.slips() }, misses: 0, seated: 0, refind });
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
        let Some(best) = self.best() else { return self.last.clone() };
        let here = self.plan_of(best);
        // The slide's last line is being sung (no line of it starts after this place): from here on the rules
        // put the NEXT slide up.
        let end = self.starts.get(here + 1).copied().unwrap_or(self.track.len());
        let last_line = best.heard && !(best.next..end).any(|t| self.track[t].line_start);
        // A slide with the same words (a chorus that is in the plan more than once) shows the same either way.
        // But on its last line the copy decides which slide goes up next, before a word of it is sung: there
        // only a copy that is followed by the same words too is the same place. (Unsure then means the next
        // slide waits for its own first word: a word late, never the wrong slide.)
        let next_same = |p: usize| self.twin.get(p + 1) == self.twin.get(here + 1);
        let same = |p: usize| self.twin.get(p) == self.twin.get(here) && (!last_line || next_same(p));
        // Anything else, a jump being confirmed too, makes it less sure.
        let rival = self.beam.iter().filter(|h| !same(self.plan_of(h))).map(|h| h.score).fold(f32::NEG_INFINITY, f32::max);
        let off_script = best.misses >= OFF_SCRIPT;
        // A seated place still to be proved is not sure, however alone it stands.
        let unproved = !best.heard || off_script || best.seated > 0;
        let confidence = if unproved { 0.0 } else { ((best.score - rival) / SURE_MARGIN).clamp(0.0, 1.0) };
        Position { word: best.heard.then(|| self.track[best.next - 1].word), plan_index: here, confidence, off_script }
    }
}

/// The word keys of plan index `p`: from its first word to the next plan index's first.
fn words_of<'a>(keys: &'a [String], starts: &[usize], p: usize) -> &'a [String] {
    &keys[starts[p]..starts.get(p + 1).copied().unwrap_or(keys.len())]
}

/// A word the phoneme ear never delivers: it has phonemes, and none of the ways it is said or sung is listened
/// for (`g2p::listened`; one neutral sound, "a"). A word with no phonemes at all (another script, no letters) is
/// not one of these: the ear that hears it writes it out in full, so it is never stepped over.
fn undelivered(word: &str) -> bool {
    let ways = g2p::variants(word);
    !ways.is_empty() && ways.iter().all(|p| {
        let sounds = g2p::coarse(p);
        !sounds.is_empty() && !g2p::listened(&sounds)
    })
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
