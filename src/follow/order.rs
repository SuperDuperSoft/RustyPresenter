//! The order a song is really sung in. The slide list has each verse and chorus once; the band sings the chorus
//! after every verse, the bridge twice, the last line again. Left to the list, the follower must be surprised
//! every time (a jump it only believes after `jump_words` good words, about two seconds late). A practice tape
//! (`tape`) knows better: the slides someone put up by hand, in the order they were up, are the order the band
//! sings (`learn`). Kept with the service item (`SungOrder`), that order is what the follower expects on Sunday
//! (`Route`): going on along it is free, and only leaving it is a jump.
//!
//! The tape also knows how long each slide was up. Kept with the order (`SungOrder::secs`), it is what the
//! follower falls back on when the ear loses the singers (`clock`): the band takes about as long over a verse on
//! Sunday as it did on Thursday.
//!
//! The follower itself knows nothing of this. It is handed the song as if it had been written out in the sung
//! order, a slide as often as it is sung (`Route::slides`), and `Route` turns its slide numbers back into the
//! song's own.
use super::review::Take;
use crate::model::{Slide, SlideContent};
use serde::{Deserialize, Serialize};

/// A slide that was up for less than this at practice was on the way to another (Next, Next), or a slip.
pub const MIN_UP_S: f64 = 1.5;
/// The longest order kept: a song rehearsed over and over in one go is not an order to expect.
pub const MAX_LEN: usize = 96;

/// The order a service item's song was sung in at practice.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SungOrder {
    /// The song's slides in the order they were sung.
    pub slides: Vec<usize>,
    /// `mark` of the slides it was learned for: it is void once their words change.
    pub mark: u64,
    /// 1.4.5: per entry of `slides`, the seconds it was up at practice (from when it came up to when the next
    /// one did). Empty in an order learned before.
    #[serde(default)]
    pub secs: Vec<f32>,
}

/// A number for a song's slides (their words, in order) that stays the same from one run and one version of the
/// app to the next.
pub fn mark<S: AsRef<str>>(slides: &[S]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| { h ^= b as u64; h = h.wrapping_mul(0x0000_0100_0000_01b3) };
    for s in slides {
        for b in s.as_ref().bytes() { eat(b) }
        eat(0xff);
    }
    h
}

/// A slide's words as a tape keeps them, and as `mark` takes them.
pub fn words(slide: &Slide) -> String {
    match &slide.content { SlideContent::Text { text, .. } => text.clone(), _ => String::new() }
}

/// `mark` of an item's slides.
pub fn mark_of(slides: &[Slide]) -> u64 { mark(&slides.iter().map(words).collect::<Vec<_>>()) }

/// The order the slides of a take were up in, each for long enough to have been sung. `None` when that is the
/// slide list's own order (nothing to learn), or no order at all (one slide, a song gone through in seconds).
pub fn learn(take: &Take) -> Option<Vec<usize>> {
    let (out, _) = learn_timed(take)?;
    let as_listed = out.windows(2).all(|w| w[1] == w[0] + 1);
    (!as_listed).then_some(out)
}

/// The order the slides of a take were up in, each for long enough to have been sung, and for how many seconds
/// (to when the next one came up; the last one to the end of the take). Also when that is the slide list's own
/// order: how long each slide takes is worth knowing there too. `None` when it is no order at all.
pub fn learn_timed(take: &Take) -> Option<(Vec<usize>, Vec<f32>)> {
    let n = take.slides.len();
    let mut out: Vec<usize> = vec![];
    let mut from: Vec<f64> = vec![];
    for (i, s) in take.screen.iter().enumerate() {
        let until = take.screen.get(i + 1).map_or(take.until, |next| next.at);
        // With its words off the slide is not being sung, however long it stays.
        if !s.words || s.slide >= n || until - s.at < MIN_UP_S { continue }
        if out.last() != Some(&s.slide) { out.push(s.slide); from.push(s.at) }
    }
    let secs = (0..out.len()).map(|i| (from.get(i + 1).copied().unwrap_or(take.until) - from[i]).max(0.0) as f32).collect();
    (out.len() >= 2 && out.len() <= MAX_LEN).then_some((out, secs))
}

/// What a tape teaches about each song on it: the item, its title, the mark of its slides and the order sung.
/// A song that was live more than once counts where it was sung the longest (a run-through, not the bridge
/// gone over again afterwards).
pub fn lessons(takes: &[Take]) -> Vec<(uuid::Uuid, String, SungOrder)> {
    let mut out: Vec<(uuid::Uuid, String, SungOrder)> = vec![];
    for take in takes {
        let (Some(item), Some((slides, secs))) = (take.item, learn_timed(take)) else { continue };
        let lesson = (item, take.title.clone(), SungOrder { slides, mark: mark(&take.slides), secs });
        match out.iter_mut().find(|l| l.0 == item) {
            Some(old) => if lesson.2.slides.len() >= old.2.slides.len() { *old = lesson },
            None => out.push(lesson),
        }
    }
    out
}

/// The order as an operator reads it: "1 2 3 2 2 4".
pub fn spoken(order: &[usize]) -> String { order.iter().map(|s| (s + 1).to_string()).collect::<Vec<_>>().join(" ") }

/// The song laid out in the order it is expected to be sung: each place on the way is one of the song's slides.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    /// Per place, the song's slide.
    slides: Vec<usize>,
    /// The first place of the learned order (slides it leaves out stand before it).
    first: usize,
    /// Per place, the seconds it was up at practice (`timed`); 0: not known.
    secs: Vec<f32>,
}

impl Route {
    /// `order`: the learned order, or nothing (then the route is the slide list). Slides the order leaves out
    /// stand before it, in the list's order: they can still be sung (a jump, as before), and the song still
    /// ends where it ended at practice. An order that names a slide the song has not got is void.
    pub fn new(count: usize, order: &[usize]) -> Route {
        if order.len() < 2 || order.len() > MAX_LEN || order.iter().any(|&s| s >= count) { return Route { slides: (0..count).collect(), first: 0, secs: vec![] } }
        let mut slides: Vec<usize> = (0..count).filter(|s| !order.contains(s)).collect();
        let first = slides.len();
        slides.extend_from_slice(order);
        Route { slides, first, secs: vec![] }
    }

    /// With how long each entry of `order` was up at practice (`SungOrder::secs`). `order` is the one the route
    /// was made from; times that do not fit it are left out.
    pub fn timed(mut self, order: &[usize], secs: &[f32]) -> Route {
        if secs.len() != order.len() || order.len() < 2 || order.len() > MAX_LEN || order.iter().any(|&s| s >= self.len()) { return self }
        let mut per = vec![0.0; self.len()];
        if self.plain() {
            // As listed (from any slide on): a place is its slide.
            if !order.windows(2).all(|w| w[1] == w[0] + 1) { return self }
            for (&s, &t) in order.iter().zip(secs) { per[s] = t }
        } else {
            for (i, &t) in secs.iter().enumerate() { per[self.first + i] = t }
        }
        self.secs = per;
        self
    }

    /// How many places have a time from practice.
    pub fn timed_places(&self) -> usize { (0..self.len()).filter(|&p| self.secs(p).is_some()).count() }

    /// The seconds `place` was up at practice before the next place came up, when that is known.
    pub fn secs(&self, place: usize) -> Option<f64> {
        if place + 1 >= self.len() { return None }
        self.secs.get(place).filter(|t| t.is_finite() && **t > 0.0).map(|&t| t as f64)
    }

    /// The route is the slide list itself.
    pub fn plain(&self) -> bool { self.slides.iter().enumerate().all(|(i, &s)| i == s) }

    pub fn len(&self) -> usize { self.slides.len() }

    /// The song's slide at `place`.
    pub fn slide(&self, place: usize) -> usize { self.slides.get(place).copied().unwrap_or(place) }

    /// The learned order (empty for a plain route).
    pub fn learned(&self) -> &[usize] { if self.plain() { &[] } else { &self.slides[self.first..] } }

    /// The song as the follower is to see it: a slide for every place.
    pub fn laid_out(&self, slides: &[Slide]) -> Vec<Slide> { self.slides.iter().filter_map(|&s| slides.get(s).cloned()).collect() }

    /// The place of `slide` when the song goes live there: in the learned order when it is in it.
    pub fn start(&self, slide: usize) -> usize {
        (self.first..self.len()).find(|&p| self.slides[p] == slide).or_else(|| self.slides.iter().position(|&s| s == slide)).unwrap_or(0)
    }

    /// The place of `slide` now that it is up, having stood at `place`: `place` itself when that is the slide,
    /// else the nearest one further on, else the first one anywhere.
    pub fn place(&self, place: usize, slide: usize) -> usize {
        if self.slides.get(place) == Some(&slide) { return place }
        (place..self.len()).find(|&p| self.slides[p] == slide).or_else(|| self.slides.iter().position(|&s| s == slide)).unwrap_or(place.min(self.len().saturating_sub(1)))
    }
}

#[cfg(test)]
#[path = "order_tests.rs"]
mod tests;
