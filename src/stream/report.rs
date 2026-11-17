//! After each stream: a short report of how it went. The operator gets how long it ran and how each destination did
//! (`tr::summary`), then, when there is more to say, the pictures the computer left out, the destinations that got
//! a lighter stream and how light, and the most viewers at once (signed in). All of it, with every destination's
//! numbers, is kept in the app's logs folder (`streams.log`), for whoever looks into a bad Sunday afterwards.
use std::io::Write;
use std::path::{Path, PathBuf};

/// One destination's stream.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    /// Times it lost its connection and came back.
    pub drops: u32,
    /// It connected at least once.
    pub ever: bool,
    /// The lightest stream it got because its internet could not keep up (kbit/s; 0: always the full one).
    pub lowest_kbps: u32,
    /// Times its sender fell behind and left part of the stream out.
    pub skips: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub secs: u64,
    pub rows: Vec<Row>,
    /// Pictures the computer could not make in time (left out of the recording and the stream).
    pub dropped: u64,
    /// Seconds in which the computer was behind (`record::slow::BEHIND`).
    pub slow_secs: u32,
    /// The most viewers at once, signed in (`None`: not known).
    pub viewers: Option<u64>,
    /// The video encoder used.
    pub encoder: String,
}

impl Report {
    /// The short summary, as before (`tr::summary`).
    pub fn summary(&self) -> String {
        let rows: Vec<(String, u32, bool)> = self.rows.iter().map(|r| (r.name.clone(), r.drops, r.ever)).collect();
        super::tr::summary(self.secs, &rows)
    }

    /// Whether anything went wrong.
    pub fn trouble(&self) -> bool { self.rows.iter().any(|r| r.drops > 0 || !r.ever) || self.dropped > 0 || self.slow_secs > 0 }

    /// What there is to say beyond the summary; `None` when nothing.
    pub fn more(&self) -> Option<String> {
        use crate::tr;
        let mut parts = vec![];
        if self.dropped > 0 || self.slow_secs > 0 {
            parts.push(tr!("The computer could not keep up for {secs} seconds and left out {frames} pictures: the stream was jerky then. Next time, try a smaller picture size or the graphics card's encoder (Settings → Livestream → Picture and quality).",
                secs = self.slow_secs, frames = self.dropped));
        }
        for r in self.rows.iter().filter(|r| r.lowest_kbps > 0) {
            parts.push(tr!("{name} got a lighter stream (down to {kbps} kbit/s) because the internet could not keep up.", name = r.name, kbps = r.lowest_kbps));
        }
        if let Some(v) = self.viewers.filter(|v| *v > 0) { parts.push(tr!("At most {viewers} watching at once.", viewers = v)) }
        (!parts.is_empty()).then(|| parts.join(" "))
    }

    /// The report as kept in the log: one line for the stream, one for each destination. Never a key: only names.
    pub fn log_text(&self, at: chrono::NaiveDateTime) -> String {
        let mut s = format!("{} stream of {} ({} s): encoder {}, {} pictures left out, {} s behind, most viewers {}\n",
            at.format("%Y-%m-%d %H:%M"), super::super::record::clock(self.secs), self.secs, if self.encoder.is_empty() { "?" } else { &self.encoder },
            self.dropped, self.slow_secs, self.viewers.map_or_else(|| "unknown".to_string(), |v| v.to_string()));
        for r in &self.rows {
            s.push_str(&format!("  {}: {}, lost {} times, behind {} times, lightest {}\n", r.name, if r.ever { "connected" } else { "never connected" },
                r.drops, r.skips, if r.lowest_kbps > 0 { format!("{} kbit/s", r.lowest_kbps) } else { "full".to_string() }));
        }
        s
    }
}

/// The log file in the logs folder `dir`.
pub fn log_file(dir: &Path) -> PathBuf { dir.join("streams.log") }

/// Adds the report to the log in `dir` (made if need be). A log that cannot be written only loses the report.
pub fn keep(dir: &Path, report: &Report, at: chrono::NaiveDateTime) {
    let written = std::fs::create_dir_all(dir).and_then(|_| {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(log_file(dir))?;
        f.write_all(report.log_text(at).as_bytes())
    });
    if let Err(e) = written { eprintln!("stream report: could not be kept: {e}") }
    eprintln!("stream report: {}", report.log_text(at).trim_end());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> chrono::NaiveDateTime { chrono::NaiveDate::from_ymd_opt(2026, 11, 22).unwrap().and_hms_opt(11, 45, 0).unwrap() }

    #[test]
    fn a_good_stream_says_little_and_a_bad_one_says_what_happened() {
        let good = Report { secs: 3600, rows: vec![Row { name: "YouTube".into(), ever: true, ..Row::default() }], encoder: "libx264".into(), ..Report::default() };
        assert_eq!(good.summary(), "The stream ran 1:00:00. YouTube: connected the whole time.");
        assert!(!good.trouble() && good.more().is_none());
        let bad = Report {
            secs: 4530, dropped: 240, slow_secs: 35, viewers: Some(57), encoder: "libx264".into(),
            rows: vec![Row { name: "YouTube".into(), ever: true, ..Row::default() }, Row { name: "Facebook".into(), drops: 2, ever: true, lowest_kbps: 2100, skips: 9 }],
        };
        assert!(bad.trouble());
        let more = bad.more().unwrap();
        assert!(more.contains("35 seconds") && more.contains("240 pictures") && more.contains("smaller picture size"), "{more}");
        assert!(more.contains("Facebook got a lighter stream (down to 2100 kbit/s)") && more.contains("At most 57 watching"), "{more}");
        assert!(!more.contains("YouTube got"), "{more}");
    }

    #[test]
    fn the_report_is_kept_in_the_logs() {
        let dir = std::env::temp_dir().join(format!("rp-stream-report-{}", uuid::Uuid::new_v4())).join("logs");
        let r = Report { secs: 65, rows: vec![Row { name: "Twitch".into(), drops: 1, ever: true, lowest_kbps: 0, skips: 3 }], ..Report::default() };
        keep(&dir, &r, at());
        keep(&dir, &r, at());
        let text = std::fs::read_to_string(log_file(&dir)).unwrap();
        assert_eq!(text.matches("2026-11-22 11:45 stream of 1:05 (65 s)").count(), 2, "{text}");
        assert!(text.contains("  Twitch: connected, lost 1 times, behind 3 times, lightest full\n"), "{text}");
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }
}
