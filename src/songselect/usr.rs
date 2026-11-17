//! SongSelect's `.usr` song file → Song. An INI-like text: a `[S A<CCLI number>]` heading, then `Title=`,
//! `Author=`, `Copyright=`, `Admin=`, `Keys=`, `Themes=`, `Fields=` (the part names) and `Words=` (each part's words),
//! with `/t` between parts and `/n` between lines. Written from what the file looks like; only read, never written.
use crate::{import::common::{label_line, lyric_line, numbered_label, section_name}, model::Song};
use std::collections::HashMap;
use uuid::Uuid;

/// True when `text` looks like a SongSelect `.usr` file: a `[S …]` song heading, or SongSelect named as its maker.
pub fn looks_like(text: &str) -> bool {
    text.lines().map(str::trim).any(|l| song_heading(l).is_some() || (l.to_ascii_lowercase().starts_with("type=") && l.to_ascii_lowercase().contains("songselect")))
}

/// `[S A4768151]` → "4768151"; `[S]` → "". None when the line is not the song's heading.
fn song_heading(line: &str) -> Option<String> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?.trim();
    let rest = inner.strip_prefix(['S', 's'])?;
    if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) { return None }
    Some(rest.chars().filter(char::is_ascii_digit).collect::<String>().trim_start_matches('0').to_string())
}

/// SongSelect's separators: `/t` between parts, `/n` between lines.
fn parts(v: &str) -> Vec<String> { v.split("/t").map(|p| p.trim().to_string()).collect() }

/// Removes `[G]`-style chords (a bracket starting with a note letter, at most 12 characters).
fn strip_chords(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(i) = rest.find('[') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        match tail.find(']') {
            Some(e) if e <= 12 && tail[..e].starts_with(|c: char| ('A'..='G').contains(&c)) && !tail[..e].contains(' ') => rest = &tail[e + 1..],
            _ => { out.push('['); rest = tail; }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Writers as the app keeps them: "A | B | C".
fn writers(v: &str) -> String {
    v.split(['|', ';']).flat_map(|p| p.split("/t")).map(str::trim).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" | ")
}

pub fn parse(text: &str) -> Result<Song, String> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let mut ccli = None;
    let mut keys: HashMap<String, String> = HashMap::new();
    let mut in_song = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            // The first song in the file is the one read; a second heading ends it.
            if let Some(n) = song_heading(line) {
                if ccli.is_some() { break }
                ccli = Some(n);
                in_song = true;
            } else {
                in_song = false;
            }
            continue;
        }
        if !in_song { continue }
        if let Some((k, v)) = line.split_once('=') {
            keys.entry(k.trim().to_ascii_lowercase()).or_insert_with(|| v.trim().to_string());
        }
    }
    let Some(ccli) = ccli else { return Err(crate::tr!("This is not a SongSelect song file.")) };
    let get = |k: &str| keys.get(k).map(String::as_str).unwrap_or("");
    let title = get("title").trim().to_string();
    if title.is_empty() { return Err(crate::tr!("The song file has no title.")) }
    let names = parts(get("fields"));
    let words = parts(get("words"));
    // Section name → its lines; a part named again with the same words is played again (the arrangement), not
    // written twice.
    let mut taken: HashMap<String, Vec<String>> = HashMap::new();
    let mut order = Vec::new();
    let mut replayed = false;
    let mut verses = 0;
    let mut sections = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let lines: Vec<String> = w.split("/n").map(strip_chords).filter(|l| !l.is_empty()).map(|l| lyric_line(&l)).collect();
        if lines.is_empty() { continue }
        let name = names.get(i).map(|n| n.trim()).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| { verses += 1; format!("Verse {verses}") });
        let key = section_name(Some(&name));
        if taken.get(&key) == Some(&lines) { order.push(key); replayed = true; continue }
        let label = if taken.contains_key(&key) { numbered_label(&name, |n| taken.contains_key(n)) } else { name };
        let key = section_name(Some(&label));
        taken.insert(key.clone(), lines.clone());
        order.push(key);
        sections.push(format!("{}\n{}", label_line(&label), lines.join("\n")));
    }
    if sections.is_empty() { return Err(crate::tr!("No lyrics were found in this file.")) }
    let copyright = {
        let c = parts(get("copyright")).into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" | ");
        let admin = parts(get("admin")).into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" | ");
        let c = if c.is_empty() || c.starts_with('©') { c } else { format!("© {c}") };
        match (c.is_empty(), admin.is_empty()) {
            (_, true) => c,
            (true, false) => format!("(Admin. by {admin})"),
            (false, false) => format!("{c} (Admin. by {admin})"),
        }
    };
    Ok(Song { id: Uuid::new_v4(), title, writers: writers(get("author")), ccli, copyright, lyrics: sections.join("\n\n"),
        arrangement: if replayed { order } else { vec![] }, ..Default::default() })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Made up in the style of SongSelect's `.usr` download (not a real song).
    pub(crate) const SAMPLE: &str = "[File]\r\nType=SongSelect Import File\r\nVersion=3.0\r\n\r\n[S A7654321]\r\nTitle=Morning Light Test Song\r\nAuthor=Test Writer | Other Writer\r\nCopyright=2026 Test Music Publishing/tSample Songs\r\nAdmin=Test Admin Co.\r\nThemes=Grace/tMorning\r\nKeys=G\r\nFields=Verse 1/tChorus/tVerse 2/tChorus\r\nWords=Morning [G]light is breaking/nOver every [C]hill/tSing it out/nSing it [D]out again/tSecond verse is here/nStill the light/tSing it out/nSing it out again\r\n";

    #[test]
    fn reads_title_credits_and_parts() {
        assert!(looks_like(SAMPLE));
        let s = parse(SAMPLE).unwrap();
        assert_eq!(s.title, "Morning Light Test Song");
        assert_eq!(s.ccli, "7654321");
        assert_eq!(s.writers, "Test Writer | Other Writer");
        assert_eq!(s.copyright, "© 2026 Test Music Publishing | Sample Songs (Admin. by Test Admin Co.)");
        assert_eq!(s.lyrics, "Verse 1\nMorning light is breaking\nOver every hill\n\nChorus\nSing it out\nSing it out again\n\nVerse 2\nSecond verse is here\nStill the light");
        assert_eq!(s.arrangement, ["Verse 1", "Chorus", "Verse 2", "Chorus"], "the chorus written once and played twice");
        // A part named twice with other words is kept as a second part.
        let two = parse("[S A1]\nTitle=T\nFields=Chorus/tChorus\nWords=one/ttwo").unwrap();
        assert!(two.lyrics.starts_with("Chorus\none\n\n") && two.lyrics.contains("two") && two.arrangement.is_empty(), "{}", two.lyrics);
    }

    #[test]
    fn copes_with_odd_files() {
        assert!(parse("Title=Nothing\nWords=a").is_err(), "no song heading");
        assert!(!looks_like("[Settings]\nTitle=x"));
        assert!(parse("[S A1]\nTitle=\nWords=a").is_err(), "no title");
        assert!(parse("[S A1]\nTitle=T\nWords=").is_err(), "no words");
        // Words with no part names become verses; extra names are fine; a second song is left out.
        let s = parse("[S A0042]\ntitle=Lower Case\nWords=one/ntwo/tthree\n[S A99]\nTitle=Second").unwrap();
        assert_eq!((s.ccli.as_str(), s.title.as_str()), ("42", "Lower Case"));
        assert_eq!(s.lyrics, "Verse 1\none\ntwo\n\nVerse 2\nthree");
        assert_eq!(s.copyright, "");
        // A line that the app would read as a heading stays words.
        assert!(parse("[S A5]\nTitle=T\nFields=Verse 1\nWords=Chorus/nreal words").unwrap().lyrics.contains("(Chorus)"));
        // Brackets that are not chords stay.
        assert_eq!(strip_chords("[Repeat x2] go [Am7]on"), "[Repeat x2] go on");
    }
}
