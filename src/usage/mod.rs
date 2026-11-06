//! The song use log for CCLI reporting (FEAT-7 §3.3, §5.4): which songs were shown live, when and in which
//! service, and the report made from it. Kept in `usage.json` beside `data.json` and written by its own thread, so
//! a song going live never waits on a save.
#[path = "../strings/usage.rs"]
pub mod strings;
mod actions;
pub mod export;
mod open;
mod serde_dates;

use chrono::{DateTime, Datelike, Days, Months, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, atomic::{AtomicBool, Ordering}, mpsc},
};
use uuid::Uuid;

pub use actions::{DetailRow, ReportView, hit_for};

/// A song counts once per usage session: a run of a service's song uses with no gap over this (FEAT-7 §5.4).
pub const SESSION_GAP_MINUTES: i64 = 45;
/// Entries older than this are dropped when the log is opened (decision 20).
pub const KEEP_YEARS: u32 = 3;

/// Everything in `usage.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct UsageLog {
    /// "Keep a song use log for CCLI reporting" (on by default, decision 15).
    pub enabled: bool,
    pub entries: Vec<UsageEntry>,
    /// The last day already reported to CCLI; the report starts the day after.
    #[serde(with = "serde_dates::opt_date")]
    pub reported_through: Option<NaiveDate>,
}

impl Default for UsageLog {
    fn default() -> Self { Self { enabled: true, entries: vec![], reported_through: None } }
}

/// One song shown live. The song's details are copies: the song may be edited or deleted later.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UsageEntry {
    pub id: Uuid,
    #[serde(with = "serde_dates::instant")]
    pub at: DateTime<Utc>,
    /// The day on this computer when it was shown.
    #[serde(with = "serde_dates::date")]
    pub local_date: NaiveDate,
    pub song_id: Uuid,
    pub title: String,
    #[serde(default)]
    pub ccli: String,
    #[serde(default)]
    pub writers: String,
    #[serde(default)]
    pub copyright: String,
    pub service_id: Uuid,
    #[serde(default)]
    pub service_name: String,
    /// A livestream page or window was open and showing the words (not that the church was on air).
    #[serde(default)]
    pub streamed: bool,
}

/// One line of the report: a song (by CCLI number, else by song) and how often it was used.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ReportRow {
    pub title: String,
    pub ccli: String,
    pub writers: String,
    pub shown: u32,
    pub streamed: u32,
    #[serde(with = "serde_dates::date")]
    pub last_used: NaiveDate,
}

/// Is this song new in the service's current usage session? The session is the run of the service's entries,
/// newest first, each within 45 minutes of the next newer one, the newest within 45 minutes of `now`. Pure over
/// the log, so a restart or a crash does not change it.
pub fn should_log(entries: &[UsageEntry], song_id: Uuid, service_id: Uuid, now: DateTime<Utc>) -> bool {
    let mut mine: Vec<&UsageEntry> = entries.iter().filter(|e| e.service_id == service_id).collect();
    mine.sort_by(|a, b| b.at.cmp(&a.at));
    let gap = chrono::Duration::minutes(SESSION_GAP_MINUTES);
    let mut newer = now;
    for e in mine {
        if newer - e.at > gap { break }
        if e.song_id == song_id { return false }
        newer = newer.min(e.at);
    }
    true
}

/// The report for `from..=to` (local days), one row per CCLI number (else per song), sorted by title. Songs with
/// no CCLI number are left out unless `include_no_ccli`.
pub fn report(log: &[UsageEntry], from: NaiveDate, to: NaiveDate, include_no_ccli: bool) -> Vec<ReportRow> {
    let mut rows: Vec<(String, ReportRow, DateTime<Utc>)> = vec![];
    for e in log.iter().filter(|e| e.local_date >= from && e.local_date <= to) {
        let ccli = e.ccli.trim();
        if ccli.is_empty() && !include_no_ccli { continue }
        let key = if ccli.is_empty() { e.song_id.to_string() } else { format!("ccli:{ccli}") };
        match rows.iter_mut().find(|(k, ..)| *k == key) {
            Some((_, row, newest)) => {
                row.shown += 1;
                row.streamed += u32::from(e.streamed);
                row.last_used = row.last_used.max(e.local_date);
                // The newest entry's copy names the row, as the song may have been edited since.
                if e.at > *newest {
                    *newest = e.at;
                    row.title = e.title.clone();
                    row.writers = e.writers.clone();
                }
            }
            None => rows.push((key, ReportRow {
                title: e.title.clone(), ccli: ccli.to_string(), writers: e.writers.clone(), shown: 1,
                streamed: u32::from(e.streamed), last_used: e.local_date,
            }, e.at)),
        }
    }
    let mut rows: Vec<ReportRow> = rows.into_iter().map(|(_, r, _)| r).collect();
    rows.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()).then_with(|| a.ccli.cmp(&b.ccli)));
    rows
}

/// The report's first day when it opens: the day after "reported through", else three months ago.
pub fn default_from(reported_through: Option<NaiveDate>, today: NaiveDate) -> NaiveDate {
    match reported_through {
        Some(d) => d.checked_add_days(Days::new(1)).unwrap_or(d),
        None => months_ago(today, 3),
    }
}

fn months_ago(today: NaiveDate, n: u32) -> NaiveDate { today.checked_sub_months(Months::new(n)).unwrap_or(today) }

/// The dialog's quick buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quick { SinceLastReport, Last3Months, Last6Months, ThisYear }

impl Quick {
    pub fn from_index(i: usize) -> Option<Self> {
        [Quick::SinceLastReport, Quick::Last3Months, Quick::Last6Months, Quick::ThisYear].get(i).copied()
    }

    /// The From and To dates the button sets.
    pub fn range(self, reported_through: Option<NaiveDate>, today: NaiveDate) -> (NaiveDate, NaiveDate) {
        let from = match self {
            Quick::SinceLastReport => default_from(reported_through, today),
            Quick::Last3Months => months_ago(today, 3),
            Quick::Last6Months => months_ago(today, 6),
            Quick::ThisYear => NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap_or(today),
        };
        (from, today)
    }
}

/// Drops entries older than three years. True when something was dropped (then the log is saved).
pub fn after_load(log: &mut UsageLog, today: NaiveDate) -> bool {
    let Some(cut) = today.checked_sub_months(Months::new(12 * KEEP_YEARS)) else { return false };
    let before = log.entries.len();
    log.entries.retain(|e| e.local_date >= cut);
    log.entries.len() != before
}

/// Reads a date typed as `YYYY-MM-DD`.
pub fn parse_date(text: &str) -> Option<NaiveDate> { NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok() }

/// Today on this computer.
pub fn today() -> NaiveDate { chrono::Local::now().date_naive() }

/// What the writer thread is asked to do.
enum Msg {
    Hit(UsageEntry),
    /// Answered once every message sent before it is done (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    Sync(mpsc::Sender<()>),
}

struct Shared {
    path: PathBuf,
    log: Mutex<UsageLog>,
    /// `log.enabled`, readable on the command path without waiting for the writer's lock (held during a save).
    enabled: AtomicBool,
}

impl Shared {
    fn save(&self, log: &UsageLog) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(log).map_err(|e| e.to_string())?;
        crate::store::write_atomic(&self.path, &json).map_err(|e| e.to_string())
    }

    /// Re-checks the session under the lock and appends. A failed save is only logged: the hit stays in memory and
    /// goes out with the next save (FEAT-7 §8).
    fn append(&self, mut hit: UsageEntry) {
        let mut log = self.log.lock().unwrap();
        if !log.enabled || !should_log(&log.entries, hit.song_id, hit.service_id, hit.at) { return }
        if hit.id.is_nil() { hit.id = Uuid::new_v4(); }
        log.entries.push(hit);
        if let Err(e) = self.save(&log) { eprintln!("usage log: could not save: {e}"); }
    }
}

/// The song use log of one data folder and its writer thread (`usage-log`), which ends when this is dropped.
pub struct Usage {
    shared: Arc<Shared>,
    tx: mpsc::Sender<Msg>,
}

impl Usage {
    /// Reads `usage.json` in `root` (a missing or broken file is an empty log) and starts the writer.
    pub fn open(root: &Path) -> Self {
        let path = root.join("usage.json");
        let mut log: UsageLog = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let pruned = after_load(&mut log, today());
        let enabled = AtomicBool::new(log.enabled);
        let shared = Arc::new(Shared { path, log: Mutex::new(log), enabled });
        if pruned && let Err(e) = shared.save(&shared.log.lock().unwrap()) { eprintln!("usage log: could not save: {e}"); }
        let (tx, rx) = mpsc::channel::<Msg>();
        let writer = shared.clone();
        let started = std::thread::Builder::new().name("usage-log".into()).spawn(move || {
            while let Ok(msg) = rx.recv() {
                match msg {
                    Msg::Hit(hit) => writer.append(hit),
                    Msg::Sync(done) => { let _ = done.send(()); }
                }
            }
        });
        if let Err(e) = started { eprintln!("usage log: no writer thread: {e}"); }
        Self { shared, tx }
    }

    /// Hands a song use to the writer thread; returns at once.
    pub fn hit(&self, hit: UsageEntry) { let _ = self.tx.send(Msg::Hit(hit)); }

    /// Never waits on the log's lock, so a song going live never waits on a save.
    pub fn enabled(&self) -> bool { self.shared.enabled.load(Ordering::Relaxed) }

    /// The log, for reading.
    pub fn log(&self) -> MutexGuard<'_, UsageLog> { self.shared.log.lock().unwrap() }

    /// Changes the log and saves it.
    pub fn edit<T>(&self, f: impl FnOnce(&mut UsageLog) -> T) -> Result<T, String> {
        let mut log = self.shared.log.lock().unwrap();
        let r = f(&mut log);
        self.shared.enabled.store(log.enabled, Ordering::Relaxed);
        self.shared.save(&log)?;
        Ok(r)
    }

    /// Waits until the writer has handled every hit sent so far.
    #[cfg(test)]
    pub fn flush(&self) {
        let (done, wait) = mpsc::channel();
        if self.tx.send(Msg::Sync(done)).is_ok() { let _ = wait.recv(); }
    }
}

#[cfg(test)]
mod tests;
