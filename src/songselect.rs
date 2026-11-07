//! SongSelect lyric `.txt` export → Song. (Not a SongSelect login.)
//! Observed layout: title, blank, section blocks, then credits:
//! writers line, `CCLI Song # N`, `© …`, terms line, `CCLI License # N`.
use crate::{model::Song, slides::section_label};
use uuid::Uuid;

pub fn import_txt(text: &str) -> Result<Song, String> {
    if text.len() > 256_000 { return Err(crate::tr!("That file is too big to be a lyric file.")); }
    let normalized = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.lines().map(str::trim_end).collect();
    let title_i = lines.iter().position(|l| !l.trim().is_empty()).ok_or_else(|| crate::tr!("The lyric file is empty."))?;
    let title = lines[title_i].trim().trim_matches('"').to_string();
    let first_section = (title_i + 1..lines.len()).find(|&i| section_label(lines[i]).is_some())
        .ok_or_else(|| crate::tr!("This file has no Verse/Chorus headings. Use SongSelect's \"Lyrics (.txt)\" download."))?;

    let ccli_i = (first_section..lines.len()).find(|&i| lines[i].trim().to_ascii_lowercase().starts_with("ccli song"));
    let mut writers = String::new();
    let mut lyrics_end = lines.len();
    if let Some(ci) = ccli_i {
        lyrics_end = ci;
        // Writers: the single non-empty line right before "CCLI Song #", sitting in its own block.
        if let Some(wi) = (first_section + 1..ci).rev().find(|&i| !lines[i].trim().is_empty()) {
            let own_block = wi == 0 || lines[wi - 1].trim().is_empty();
            if own_block && section_label(lines[wi]).is_none() { writers = lines[wi].trim().to_string(); lyrics_end = wi; }
        }
    } else if let Some(fi) = (first_section..lines.len()).find(|&i| { let l = lines[i].trim(); l.starts_with('©') || l.to_ascii_lowercase().starts_with("copyright") }) {
        lyrics_end = fi;
    }
    // Some older exports put writers right under the title.
    if writers.is_empty() {
        if let Some(w) = lines[title_i + 1..first_section].iter().map(|l| l.trim()).find(|l| !l.is_empty()) { writers = w.to_string(); }
    }
    let mut ccli = String::new();
    let mut copyright = String::new();
    for l in &lines[lyrics_end..] {
        let t = l.trim();
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("ccli song") { ccli = t.chars().filter(|c| c.is_ascii_digit()).collect(); }
        else if t.starts_with('©') || lower.starts_with("copyright") { copyright = t.to_string(); }
    }
    let lyrics = lines[first_section..lyrics_end].join("\n").trim().to_string();
    if lyrics.is_empty() { return Err(crate::tr!("No lyrics were found in this file.")); }
    Ok(Song { id: Uuid::new_v4(), title, writers, ccli, copyright, lyrics, arrangement: vec![] })
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
    }
}
