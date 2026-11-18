//! Chapters in a recording: one at each service item taken live (a song, a reading, the sermon), named as the
//! service list names it. A video player then jumps straight to it, and YouTube can show them on its timeline.
//! The writer looks at the live item once a second (`Source`) and keeps the list (`Log`); finishing the file
//! writes them in (`metadata`, read by FFmpeg).
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A chapter: from `at` seconds into the recording, called `title`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chapter {
    pub at: f64,
    pub title: String,
}

/// What is live now, by name (`None`: nothing).
#[derive(Clone)]
pub struct Source(pub Arc<dyn Fn() -> Option<String> + Send + Sync>);

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("chapters::Source") }
}

/// The chapters so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Log {
    pub list: Vec<Chapter>,
}

impl Log {
    /// What was live `at` seconds in: a new chapter when it is another item than the last one's.
    pub fn note(&mut self, at: f64, title: Option<String>) {
        let Some(title) = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) else { return };
        if self.list.last().is_some_and(|c| c.title == title) { return }
        self.list.push(Chapter { at, title });
    }
}

/// FFmpeg's metadata file with `chapters`, the last running to `end` seconds.
pub fn metadata(chapters: &[Chapter], end: f64) -> String {
    if chapters.is_empty() { return String::new() }
    let ms = |s: f64| (s.max(0.0) * 1000.0).round() as u64;
    // `=`, `;`, `#`, `\` and line ends are FFmpeg's own in this file: written with a backslash before them.
    let escape = |t: &str| t.chars().fold(String::new(), |mut o, c| {
        if matches!(c, '=' | ';' | '#' | '\\' | '\n') { o.push('\\') }
        o.push(c);
        o
    });
    let mut m = String::from(";FFMETADATA1\n");
    for (i, c) in chapters.iter().enumerate() {
        let start = ms(c.at);
        let until = chapters.get(i + 1).map_or(ms(end), |n| ms(n.at));
        let until = if until > start { until } else { start + 1000 };
        m.push_str(&format!("[CHAPTER]\nTIMEBASE=1/1000\nSTART={start}\nEND={until}\ntitle={}\n", escape(&c.title)));
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(at: f64, title: &str) -> Chapter { Chapter { at, title: title.into() } }

    #[test]
    fn a_chapter_starts_at_each_new_item_taken_live() {
        let mut log = Log::default();
        log.note(0.0, None);
        log.note(1.0, Some("Welcome".into()));
        log.note(2.0, Some("Welcome".into()));
        log.note(30.0, Some("Amazing Grace".into()));
        // Nothing live for a moment (cleared between items): the song's chapter goes on.
        log.note(31.0, None);
        log.note(95.0, Some("John 3:16-21 (KJV)".into()));
        log.note(96.0, Some("  ".into()));
        assert_eq!(log.list, [ch(1.0, "Welcome"), ch(30.0, "Amazing Grace"), ch(95.0, "John 3:16-21 (KJV)")]);
    }

    #[test]
    fn ffmpeg_reads_the_chapters_with_their_names_kept_whole() {
        let m = metadata(&[ch(1.0, "Welcome"), ch(30.25, "Song = praise; #1 \\ done")], 60.0);
        assert_eq!(m, ";FFMETADATA1\n\
            [CHAPTER]\nTIMEBASE=1/1000\nSTART=1000\nEND=30250\ntitle=Welcome\n\
            [CHAPTER]\nTIMEBASE=1/1000\nSTART=30250\nEND=60000\ntitle=Song \\= praise\\; \\#1 \\\\ done\n");
        // A last chapter that would end before it starts runs a second.
        assert!(metadata(&[ch(10.0, "A")], 5.0).contains("START=10000\nEND=11000\n"));
        assert_eq!(metadata(&[], 60.0), "", "no chapters: nothing");
    }
}
