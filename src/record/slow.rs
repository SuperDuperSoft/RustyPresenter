//! Whether the computer keeps up with the recording (and the stream). The writer owes pictures when encoding is
//! slower than the clock; how far behind it got each second is looked at here. A moment's hiccup (a disk busy,
//! a program starting) is nothing; behind for half of the last `WINDOW` seconds is a computer that cannot keep
//! up, and the picture going out is jerky. Said once, and once again when it is smooth again.
use std::collections::VecDeque;
use std::time::Duration;

/// Further behind than this in a second counts as behind.
pub const BEHIND: Duration = Duration::from_millis(500);
/// The seconds looked at.
pub const WINDOW: usize = 20;
/// Behind in this many of them: too slow.
pub const TOO_MANY: usize = 10;

/// What to say about how the computer keeps up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Say {
    /// It cannot keep up.
    TooSlow,
    /// Said it could not; for a whole `WINDOW` it has.
    Smooth,
}

/// The last `WINDOW` seconds, and whether `TooSlow` was said.
#[derive(Debug, Default)]
pub struct Slow {
    recent: VecDeque<bool>,
    said: bool,
}

impl Slow {
    /// One second's lag (the most the writer was behind in it). What to say now, if anything.
    pub fn note(&mut self, lag: Duration) -> Option<Say> {
        self.recent.push_back(lag > BEHIND);
        if self.recent.len() > WINDOW { self.recent.pop_front(); }
        let behind = self.recent.iter().filter(|b| **b).count();
        if !self.said && behind >= TOO_MANY {
            self.said = true;
            return Some(Say::TooSlow);
        }
        if self.said && self.recent.len() == WINDOW && behind == 0 {
            self.said = false;
            return Some(Say::Smooth);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> Duration { Duration::from_millis(v) }

    #[test]
    fn a_hiccup_is_nothing_but_falling_behind_again_and_again_is_said_once() {
        let mut s = Slow::default();
        // Keeping up, with a few hiccups.
        for i in 0..60 { assert_eq!(s.note(if i % 7 == 0 { ms(1500) } else { ms(30) }), None, "second {i}") }
        // Behind every other second: said when half of the last 20 were behind, and only then.
        let mut said = vec![];
        for i in 0..39 { if let Some(x) = s.note(if i % 2 == 0 { ms(900) } else { ms(100) }) { said.push((i, x)) } }
        assert_eq!(said.len(), 1, "{said:?}");
        assert_eq!(said[0].1, Say::TooSlow);
        assert!(said[0].0 >= 17 && said[0].0 <= 19, "{said:?}");
        // Better, but not for long enough yet.
        for i in 0..WINDOW - 1 { assert_eq!(s.note(ms(20)), None, "second {i}") }
        assert_eq!(s.note(ms(20)), Some(Say::Smooth), "a whole window kept up");
        assert_eq!(s.note(ms(20)), None);
        // Slow again later: said again.
        let again: Vec<Say> = (0..WINDOW).filter_map(|_| s.note(ms(2000))).collect();
        assert_eq!(again, [Say::TooSlow]);
    }

    #[test]
    fn exactly_the_limit_is_not_behind() {
        let mut s = Slow::default();
        assert!((0..100).all(|_| s.note(BEHIND).is_none()));
    }
}
