//! The review log: every change auto-follow made or suggested, when, why ("heard 'grace that saved'") and how
//! sure it was, so the team can look back after a service or a Practice run.
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use uuid::Uuid;

/// What auto-follow did.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogAction {
    /// Took this slide live.
    Show(usize),
    /// Highlighted this slide for the operator.
    Suggest(usize),
    /// Took the words off.
    Blank,
    /// Put the words of this slide back.
    Unblank(usize),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LogEntry {
    /// Seconds on the follower's clock (from the start of the recording in Practice).
    pub at: f64,
    pub item: Uuid,
    pub action: LogAction,
    /// Why, for people: "heard "that saved a wretch"", "no singing for 3 s".
    pub reason: String,
    /// 0–1: how sure the aligner was.
    pub confidence: f32,
}

impl LogEntry {
    /// One line for the log view: `1:02.5  Slide 2  heard "…" (92%)`.
    pub fn line(&self) -> String {
        let what = match self.action {
            LogAction::Show(n) => crate::tr!("Slide {number}", number = n + 1),
            LogAction::Suggest(n) => crate::tr!("Suggested slide {number}", number = n + 1),
            LogAction::Blank => crate::tr!("Words off"),
            LogAction::Unblank(n) => crate::tr!("Words back on slide {number}", number = n + 1),
        };
        let t = self.at.max(0.0);
        let (m, s) = ((t / 60.0).floor() as u64, t % 60.0);
        format!("{m}:{s:04.1}  {what}  {} ({}%)", self.reason, (self.confidence.clamp(0.0, 1.0) * 100.0).round() as u32)
    }
}

/// The last entries, newest last.
#[derive(Clone, Debug, Default)]
pub struct ReviewLog { entries: VecDeque<LogEntry> }

/// A long service keeps at most this many.
const KEEP: usize = 2000;

impl ReviewLog {
    pub fn push(&mut self, e: LogEntry) {
        self.entries.push_back(e);
        if self.entries.len() > KEEP { self.entries.pop_front(); }
    }

    pub fn entries(&self) -> Vec<LogEntry> { self.entries.iter().cloned().collect() }

    pub fn clear(&mut self) { self.entries.clear() }
}
