//! The clock from practice. The tape of a practice knows how long each slide was up (`order::SungOrder::secs`),
//! and a band takes about as long over a verse on Sunday as it did on Thursday. So once the singers have surely
//! been heard on the slide that is up, it is known about when they will be through with it, and when the ear
//! then loses them (a loud band, a line it cannot make out) the next slide
//! still comes up when it did at practice, instead of waiting for words of it to be recognised a line late.
//!
//! It is a fallback, never the first say:
//! - It only counts from words surely heard on the slide that is up (`heard`). A slide nobody was heard singing
//!   is never left by the clock, so the clock cannot run on through a song by itself: every step needs sung
//!   proof of the step before.
//! - Words win. While the singers are surely heard early on the slide (it is taking longer than at practice: a
//!   line again, a slower day), the slide stays (`due`).
//! - Sunday's tempo is measured against practice's from slide to slide (`ratio`) and the practice times are
//!   stretched to it.
//! - It never goes back, never skips, and never takes words off.
use super::lyrics::SongText;
use super::order::Route;

/// The next slide comes up this long before practice says its first word is due. Less than the usual lead: the
/// clock is the rougher guide, and a slide that leaves a moment late is better than one that leaves a line early.
pub const CLOCK_LEAD: f64 = 1.0;
/// A slide up for less than this at practice is no guide.
const MIN_SECS: f64 = 3.0;
/// The singers surely heard this recently, before the slide's last line: they are not through with it.
const RECENT_S: f64 = 3.0;
/// Sunday's tempo is believed between these (against practice); a slide that took less or more was not sung the
/// same way, and says nothing about the tempo.
const RATIO_MIN: f64 = 0.75;
const RATIO_MAX: f64 = 1.35;
/// A breath between lines, when counting back from a word to its slide's first.
const LINE_BREATH: f64 = 0.4;

#[derive(Clone, Debug)]
pub struct Clock {
    /// The place the singers were last surely heard on, and when its first word was sung (counted back from the
    /// first word heard there).
    on: Option<(usize, f64)>,
    /// Sunday's seconds for one of practice's.
    ratio: f64,
    /// The last sure word on `on`: when it was sung, and whether it was on the slide's last line.
    last: Option<(f64, bool)>,
}

impl Default for Clock {
    fn default() -> Self { Clock { on: None, ratio: 1.0, last: None } }
}

impl Clock {
    /// The operator moved, or the song started again: what was counted is void (the tempo is kept).
    pub fn reset(&mut self) { self.on = None; self.last = None }

    pub fn ratio(&self) -> f64 { self.ratio }

    /// A word of `place` (word `w` of `text`), surely sung at `sung`, at `pace` syllables a second.
    pub fn heard(&mut self, route: &Route, text: &SongText, place: usize, w: usize, sung: f64, pace: f64) {
        let Some(span) = text.slides.get(place) else { return };
        if w < span.first_word || w >= span.end_word { return }
        if self.on.is_none_or(|(p, _)| p != place) {
            let before: u32 = text.words[span.first_word..=w].iter().map(|x| x.syllables as u32).sum();
            let lines = (text.words[w].line - text.words[span.first_word].line) as f64;
            let first = sung - before as f64 / pace.max(1.0) - lines * LINE_BREATH;
            // From the slide before to this one: how long that took, against practice.
            if let Some((p, began)) = self.on.filter(|(p, _)| p + 1 == place) {
                if let Some(secs) = route.secs(p).filter(|s| *s >= MIN_SECS) {
                    let r = (first - began) / secs;
                    if (RATIO_MIN..=RATIO_MAX).contains(&r) { self.ratio = (self.ratio + r) / 2.0 }
                }
            }
            self.on = Some((place, first));
        }
        self.last = Some((sung, w >= span.last_line_first_word));
    }

    /// When the slide after `place` is due up, going by practice: only once the singers have been heard on
    /// `place`, and practice knows how long it takes.
    pub fn at(&self, route: &Route, place: usize) -> Option<f64> {
        let (_, began) = self.on.filter(|(p, _)| *p == place)?;
        let secs = route.secs(place).filter(|s| *s >= MIN_SECS)?;
        Some(began + secs * self.ratio - CLOCK_LEAD)
    }

    /// It is time for the slide after `place` (`at`), and nothing heard says otherwise.
    pub fn due(&self, route: &Route, place: usize, now: f64) -> bool {
        let Some(at) = self.at(route, place) else { return false };
        if now < at { return false }
        // Surely heard a moment ago, and not yet on the last line: the slide is taking longer than at practice.
        !self.last.is_some_and(|(sung, last_line)| !last_line && now - sung <= RECENT_S)
    }
}

#[cfg(test)]
#[path = "clock_tests.rs"]
mod tests;
