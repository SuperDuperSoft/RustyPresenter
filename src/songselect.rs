//! SongSelect lyric `.txt` export → Song. (Not a SongSelect login.)
//! Observed layout: title, blank, section blocks, then credits:
//! writers line, `CCLI Song # N`, `© …`, terms line, `CCLI License # N`.
//!
//! The app never signs in to SongSelect, never reads or fetches its pages and never downloads for the church: it
//! opens SongSelect in the person's own web browser and reads the song files they download themselves (`inbox`,
//! `watch`; see docs/superpowers/specs/2026-11-17-songselect-inbox-design.md).
use crate::{model::Song, slides::section_label};
use uuid::Uuid;

pub mod inbox;
pub mod usr;
pub mod watch;

/// SongSelect's home page, opened in the person's own web browser.
pub const HOME: &str = "https://songselect.ccli.com/";

/// Opens SongSelect in the computer's web browser. Tests open nothing and note it instead.
pub fn open_in_browser() -> Result<(), String> {
    #[cfg(test)]
    { testing::OPENED.with(|o| o.set(o.get() + 1)); Ok(()) }
    #[cfg(not(test))]
    { crate::usage::open::open_file(std::path::Path::new(HOME)) }
}

#[cfg(test)]
pub(crate) mod testing {
    thread_local! {
        /// How many times SongSelect was opened in the browser on this thread.
        pub(crate) static OPENED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
}

fn is_copyright(line: &str) -> bool {
    let line = line.trim();
    line.starts_with('©')
        || line.to_ascii_lowercase().starts_with("copyright")
        || line.strip_prefix("c ").is_some_and(|rest| {
            rest.as_bytes().get(..4).is_some_and(|year| year.iter().all(u8::is_ascii_digit))
        })
}

pub fn import_txt(text: &str) -> Result<Song, String> {
    if text.len() > 256_000 { return Err(crate::tr!("That file is too big to be a song file.")); }
    let normalized = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.lines().map(str::trim_end).collect();
    let title_i = lines.iter().position(|l| !l.trim().is_empty()).ok_or_else(|| crate::tr!("The song file is empty."))?;
    let title = lines[title_i].trim().trim_matches('"').trim().to_string();
    if title.is_empty() { return Err(crate::tr!("Give the song a title.")); }
    let first_section = (title_i + 1..lines.len()).find(|&i| section_label(lines[i]).is_some())
        .ok_or_else(|| crate::tr!("This file has no Verse/Chorus headings. Use SongSelect's \"Lyrics (.txt)\" download."))?;

    let ccli_i = (first_section..lines.len()).find(|&i| lines[i].trim().to_ascii_lowercase().starts_with("ccli song"));
    let copyright_i = (first_section..lines.len()).find(|&i| is_copyright(lines[i]));
    let mut lyrics_end = ccli_i.into_iter().chain(copyright_i).min().unwrap_or(lines.len());
    let mut writers = String::new();
    if ccli_i == Some(lyrics_end) {
        // Writers sit in their own block immediately before the CCLI song number.
        if let Some(wi) = (first_section + 1..lyrics_end).rev().find(|&i| !lines[i].trim().is_empty()) {
            if lines[wi - 1].trim().is_empty() && section_label(lines[wi]).is_none() {
                writers = lines[wi].trim().to_string();
                lyrics_end = wi;
            }
        }
    }
    // Some older exports put writers right under the title.
    if writers.is_empty() {
        if let Some(w) = lines[title_i + 1..first_section].iter().map(|l| l.trim()).find(|l| !l.is_empty()) { writers = w.to_string(); }
    }
    let lyric_lines = &lines[first_section..lyrics_end];
    if !lyric_lines.iter().any(|line| !line.trim().is_empty() && section_label(line).is_none()) {
        return Err(crate::tr!("No song words were found in this file."));
    }
    let lyrics = lyric_lines.join("\n").trim().to_string();
    let ccli = ccli_i.map(|i| lines[i].chars().filter(char::is_ascii_digit).collect()).unwrap_or_default();
    let copyright = copyright_i.map(|i| lines[i].trim().to_string()).unwrap_or_default();
    Ok(Song { id: Uuid::new_v4(), title, writers, ccli, copyright, lyrics, arrangement: vec![], ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    const SONG: &str = "A Test Song\r\n\r\nVerse 1\r\nOriginal test line one\r\nOriginal test line two\r\n\r\nChorus\r\nA repeated test line\r\n\r\nTest Writer | Other Writer\r\nCCLI Song # 12345\r\n© 2026 Test Music\r\nFor use solely with the SongSelect® Terms of Use. All rights reserved. www.ccli.com\r\nCCLI License # 98765";

    #[test]
    fn writers_before_ccli_number() {
        let s = import_txt(SONG).unwrap();
        assert_eq!(s.title, "A Test Song");
        assert_eq!(s.writers, "Test Writer | Other Writer");
        assert_eq!(s.ccli, "12345");
        assert_eq!(s.copyright, "© 2026 Test Music");
        assert!(s.lyrics.starts_with("Verse 1"));
        assert!(!s.lyrics.contains("Writer"));
        assert!(!s.lyrics.contains("98765"));
    }

    #[test]
    fn rejects_non_lyric_files() {
        assert!(import_txt("Song Title\nA writer\nCopyright text").is_err());
        assert!(import_txt("").is_err());
        assert!(import_txt("Song Title\n\nVerse 1\n\nChorus").is_err());
        assert!(import_txt("\"  \"\n\nVerse 1\nWords").is_err());
    }

    #[test]
    fn separates_copyright_before_ccli_from_the_words() {
        let s = import_txt("Song\nOld Writer\n\nVerse 1\ncome and sing\n\n\u{a9} Test Music\nCCLI Song # 42").unwrap();
        assert_eq!(s.lyrics, "Verse 1\ncome and sing");
        assert_eq!(s.writers, "Old Writer");
        assert_eq!(s.copyright, "\u{a9} Test Music");
        assert_eq!(s.ccli, "42");
    }

    #[test]
    fn keeps_words_beginning_with_c_when_there_is_no_ccli_number() {
        let s = import_txt("Song\n\nVerse 1\ncome and sing\n\nc 2026 Test Music").unwrap();
        assert_eq!(s.lyrics, "Verse 1\ncome and sing");
        assert_eq!(s.copyright, "c 2026 Test Music");
    }
}
