//! Which downloaded files are SongSelect songs, and how one joins the library without losing the church's edits.
//! Only the files the person downloaded themselves are read (see the November 17 design note).
use super::{import_txt, usr};
use crate::{import::chordpro, model::Song};
use uuid::Uuid;

/// A downloaded file bigger than this is not a song.
pub const MAX_BYTES: u64 = 1_000_000;

/// The file names a SongSelect song download can have.
pub const EXTENSIONS: &[&str] = &["txt", "usr", "chordpro", "cho", "chopro", "pro"];

/// What a downloaded file turned out to be.
#[derive(Debug)]
pub enum Export {
    Song(Song),
    /// Clearly from SongSelect, but it could not be read: why.
    Unreadable(String),
}

fn ext_of(name: &str) -> String {
    std::path::Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// A ChordPro tag that starts a song or names it: `{title: …}`, `{t: …}`, `{start_of_verse}`, `{soc}`.
fn has_chordpro_tags(text: &str) -> bool {
    text.lines().map(str::trim).any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with('{') && ["{title", "{t:", "{start_of_", "{sov", "{soc", "{comment", "{c:", "{ccli"].iter().any(|t| l.starts_with(t))
    })
}

/// A line SongSelect writes under every song.
fn is_ccli_song_line(l: &str) -> bool { l.trim().to_ascii_lowercase().starts_with("ccli song") }

/// The text inside `{comment: …}`, `{c: …}` or `{footer: …}`, or the line itself.
fn unwrap_tag(l: &str) -> &str {
    let t = l.trim();
    if let Some(inner) = t.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
        if let Some((name, value)) = inner.split_once(':') {
            if matches!(name.trim().to_ascii_lowercase().as_str(), "comment" | "c" | "ci" | "footer") { return value.trim() }
        }
    }
    t
}

fn is_copyright(l: &str) -> bool { l.starts_with('©') || l.to_ascii_lowercase().starts_with("copyright") }
fn is_terms(l: &str) -> bool { let l = l.to_ascii_lowercase(); l.contains("songselect") && l.contains("terms") }
fn is_licence(l: &str) -> bool { l.to_ascii_lowercase().starts_with("ccli licen") }

/// A ChordPro song from SongSelect: its credit lines at the end (CCLI Song #, writers, ©, the terms line and the
/// church's licence number) are taken out of the words and kept as the song's credits.
pub fn read_chordpro(text: &str) -> Result<Song, String> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.lines().collect();
    let mut drop = vec![false; lines.len()];
    let (mut ccli, mut copyright, mut writers) = (String::new(), String::new(), String::new());
    let ci = lines.iter().position(|l| is_ccli_song_line(unwrap_tag(l)));
    for (i, l) in lines.iter().enumerate() {
        let t = unwrap_tag(l);
        if is_ccli_song_line(t) { ccli = t.chars().filter(char::is_ascii_digit).collect::<String>().trim_start_matches('0').to_string(); drop[i] = true }
        else if is_copyright(t) && ci.is_some_and(|c| i + 3 >= c) { if copyright.is_empty() { copyright = t.to_string() } drop[i] = true }
        else if is_terms(t) || is_licence(t) { drop[i] = true }
    }
    // The writers: the line in its own block just before "CCLI Song #", else the line just after it.
    if let Some(c) = ci {
        let plain = |i: usize| { let t = unwrap_tag(lines[i]); !t.is_empty() && !drop[i] && !t.starts_with('{') && crate::slides::section_label(t).is_none() };
        let before = (c > 0 && plain(c - 1) && (c < 2 || lines[c - 2].trim().is_empty())).then(|| c - 1);
        let after = (c + 1 < lines.len() && plain(c + 1)).then_some(c + 1);
        if let Some(w) = before.or(after) { writers = unwrap_tag(lines[w]).to_string(); drop[w] = true }
    }
    let kept: Vec<&str> = lines.iter().zip(&drop).filter(|(_, d)| !**d).map(|(l, _)| *l).collect();
    let report = chordpro::parse_str(&kept.join("\n"));
    let mut song = match report.songs.into_iter().next() {
        Some(s) => s,
        None => return Err(report.skipped.into_iter().next().map(|(_, why)| why).unwrap_or_else(|| crate::tr!("No lyrics were found in this file."))),
    };
    if song.ccli.is_empty() { song.ccli = ccli }
    if song.copyright.is_empty() { song.copyright = copyright }
    if song.writers.is_empty() { song.writers = writers.split('|').map(str::trim).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" | ") }
    Ok(song)
}

/// A song file picked or dropped by the person (any of SongSelect's song downloads). A CCLI number is not needed.
pub fn read_song_file(name: &str, bytes: &[u8]) -> Result<Song, String> {
    if bytes.len() as u64 > MAX_BYTES { return Err(crate::tr!("That file is too big to be a lyric file.")) }
    let text = chordpro::decode(bytes);
    match ext_of(name).as_str() {
        "usr" => usr::parse(&text),
        "txt" | "" if has_chordpro_tags(&text) => read_chordpro(&text),
        "txt" | "" => import_txt(&text),
        _ => read_chordpro(&text),
    }
}

/// A file that appeared in Downloads: `None` when it is not a SongSelect song (left alone without a word).
pub fn read_export(name: &str, bytes: &[u8]) -> Option<Export> {
    let ext = ext_of(name);
    if !EXTENSIONS.contains(&ext.as_str()) || bytes.len() as u64 > MAX_BYTES { return None }
    let text = chordpro::decode(bytes);
    let from_songselect = text.lines().any(|l| is_ccli_song_line(unwrap_tag(l)) || is_terms(unwrap_tag(l)));
    let read = match ext.as_str() {
        "usr" if usr::looks_like(&text) => usr::parse(&text),
        "usr" => return None,
        "txt" if has_chordpro_tags(&text) => read_chordpro(&text),
        "txt" if from_songselect => import_txt(&text),
        "txt" => return None,
        _ if has_chordpro_tags(&text) || from_songselect => read_chordpro(&text),
        _ => return None,
    };
    match read {
        Ok(s) if !s.ccli.is_empty() => Some(Export::Song(s)),
        // A ChordPro or text file of the church's own, with no CCLI number, is not a SongSelect download.
        Ok(_) if !from_songselect && ext != "usr" => None,
        Ok(_) => Some(Export::Unreadable(crate::tr!("it has no CCLI song number"))),
        Err(_) if !from_songselect && ext != "usr" => None,
        Err(e) => Some(Export::Unreadable(e)),
    }
}

/// What happened to a song brought in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Landed {
    Added,
    /// The library already has this song with these words: nothing changed.
    AlreadyThere,
    /// The library has this CCLI number with other words (the church's edits): this one was added beside it,
    /// under this title.
    KeptBoth(String),
}

/// The words without chords, section names, case, spacing or punctuation, to tell whether two songs say the same.
fn words_key(lyrics: &str) -> String {
    let lyrics = crate::slides::strip_chords(lyrics).unwrap_or_else(|| lyrics.to_string());
    lyrics.lines()
        .filter(|l| crate::slides::section_label(l).is_none())
        .flat_map(|l| l.split(|c: char| !c.is_alphanumeric()))
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>().join(" ")
}

pub fn same_words(a: &str, b: &str) -> bool { words_key(a) == words_key(b) }

/// The title a second copy gets: "<title> (from SongSelect)".
pub fn second_title(title: &str) -> String { crate::tr!("{title} (from SongSelect)", title = title) }

/// Puts `song` in `songs` without touching the church's own: see `Landed`. Gives the id to add to a service.
pub fn place(songs: &mut Vec<Song>, mut song: Song) -> (Uuid, Landed) {
    let ccli = song.ccli.trim().to_string();
    let same_number: Vec<&Song> = songs.iter().filter(|s| !ccli.is_empty() && s.ccli.trim() == ccli).collect();
    if let Some(s) = same_number.iter().find(|s| same_words(&s.lyrics, &song.lyrics)) { return (s.id, Landed::AlreadyThere) }
    if same_number.is_empty() {
        let id = song.id;
        songs.push(song);
        return (id, Landed::Added);
    }
    song.title = second_title(&song.title);
    let (id, title) = (song.id, song.title.clone());
    songs.push(song);
    (id, Landed::KeptBoth(title))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Made up in the style of SongSelect's "Lyrics" download (not a real song).
    pub(crate) const LYRICS_TXT: &str = "River Of Test Songs\r\n\r\nVerse 1\r\nWe are walking by the river\r\nSinging all the way\r\n\r\nChorus\r\nOh the river runs\r\nOh the river sings\r\n\r\nVerse 2\r\nEvery morning by the water\r\nNew songs every day\r\n\r\nSample Writer | Another Writer\r\nCCLI Song # 1234567\r\n© 2026 Example Worship Music (Admin. by Example Publishing)\r\nFor use solely with the SongSelect® Terms of Use. All rights reserved. www.ccli.com\r\nCCLI License # 11223344\r\n";

    /// Made up in the style of SongSelect's ChordPro download (not a real song).
    pub(crate) const CHORDPRO: &str = "{title: Hills Of Test}\n{key: D}\n{tempo: 72}\n{time: 4/4}\n\n{comment: Verse 1}\n[D]Over the hills the [G]morning comes\n[A]Light on the [D]fields\n\n{comment: Chorus}\n[G]Sing, sing, [D]sing\n[A]All the hills will [D]ring\n\nCCLI Song # 7000001\nSample Writer\n© 2025 Example Hymns\nFor use solely with the SongSelect® Terms of Use.  All rights reserved. www.ccli.com\nCCLI License # 11223344\n";

    #[test]
    fn songselect_lyrics_txt_is_taken() {
        let Some(Export::Song(s)) = read_export("River Of Test Songs.txt", LYRICS_TXT.as_bytes()) else { panic!("a song") };
        assert_eq!((s.title.as_str(), s.ccli.as_str()), ("River Of Test Songs", "1234567"));
        assert_eq!(s.writers, "Sample Writer | Another Writer");
        assert_eq!(s.copyright, "© 2026 Example Worship Music (Admin. by Example Publishing)");
        assert!(s.lyrics.starts_with("Verse 1\nWe are walking") && s.lyrics.ends_with("New songs every day"), "{}", s.lyrics);
    }

    #[test]
    fn songselect_chordpro_is_taken_with_its_chords_and_without_credits_in_the_words() {
        for name in ["hills-of-test.chordpro", "hills.cho", "hills.pro", "hills.txt"] {
            let Some(Export::Song(s)) = read_export(name, CHORDPRO.as_bytes()) else { panic!("{name}: a song") };
            assert_eq!((s.title.as_str(), s.ccli.as_str()), ("Hills Of Test", "7000001"), "{name}");
            assert_eq!(s.writers, "Sample Writer");
            assert_eq!(s.copyright, "© 2025 Example Hymns");
            // The chords stay for the stage; the room's words are without them (`slides::strip_chords`).
            assert_eq!(s.lyrics, "Verse 1\n[D]Over the hills the [G]morning comes\n[A]Light on the [D]fields\n\nChorus\n[G]Sing, sing, [D]sing\n[A]All the hills will [D]ring");
            assert_eq!(crate::slides::strip_chords(&s.lyrics).unwrap(), "Verse 1\nOver the hills the morning comes\nLight on the fields\n\nChorus\nSing, sing, sing\nAll the hills will ring");
        }
        // The same song as lyrics alone has the same words.
        assert!(same_words("Verse 1\n[D]Over the hills", "Verse 1\nOver the hills"));
        // The credits in `{comment}` tags and the writers before the number, as some exports have them.
        let tagged = "{t: Tagged}\n{c: Verse}\nWords here\n\n{c: Writer One | Writer Two}\n{c: CCLI Song # 0042}\n{c: © 2020 Someone}\n";
        let s = read_chordpro(tagged).unwrap();
        assert_eq!((s.ccli.as_str(), s.writers.as_str(), s.copyright.as_str()), ("42", "Writer One | Writer Two", "© 2020 Someone"));
        assert_eq!(s.lyrics, "Verse\nWords here");
        // A `{ccli}` tag is read by the ChordPro reader itself.
        assert_eq!(read_chordpro("{title: X}\n{ccli: 555}\nla la").unwrap().ccli, "555");
    }

    #[test]
    fn songselect_usr_is_taken() {
        let Some(Export::Song(s)) = read_export("song.usr", usr::tests::SAMPLE.as_bytes()) else { panic!("a song") };
        assert_eq!(s.ccli, "7654321");
        assert!(matches!(read_export("broken.usr", b"[S A12]\nTitle=Only a title\n"), Some(Export::Unreadable(_))));
        assert!(read_export("other.usr", b"[User]\nName=Someone").is_none(), "another program's .usr is left alone");
    }

    #[test]
    fn everything_else_is_left_alone() {
        assert!(read_export("notes.txt", b"Shopping list\nmilk\neggs").is_none());
        assert!(read_export("song.txt", b"My Song\n\nVerse 1\nOur own words").is_none(), "a church's own lyrics have no CCLI number");
        assert!(read_export("own.cho", b"{title: Our Own}\n[G]la la").is_none(), "a ChordPro file with no CCLI number");
        assert!(read_export("project.pro", b"QT += core gui\nTARGET = app").is_none(), "a Qt project file");
        assert!(read_export("photo.jpg", LYRICS_TXT.as_bytes()).is_none());
        assert!(read_export("big.txt", &vec![b'a'; MAX_BYTES as usize + 1]).is_none());
        // From SongSelect by its credit lines, but with no words: said so.
        assert!(matches!(read_export("odd.txt", b"Title only\nCCLI Song # 1\n"), Some(Export::Unreadable(_))));
    }

    #[test]
    fn picked_files_of_every_kind_read() {
        assert_eq!(read_song_file("a.txt", LYRICS_TXT.as_bytes()).unwrap().ccli, "1234567");
        assert_eq!(read_song_file("a.chordpro", CHORDPRO.as_bytes()).unwrap().ccli, "7000001");
        assert_eq!(read_song_file("a.usr", usr::tests::SAMPLE.as_bytes()).unwrap().ccli, "7654321");
        assert_eq!(read_song_file("own.cho", b"{title: Our Own}\n[G]la la").unwrap().title, "Our Own");
        // UTF-16, as Windows Notepad saves it.
        let wide: Vec<u8> = [0xFF, 0xFE].into_iter().chain(LYRICS_TXT.encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(read_song_file("wide.txt", &wide).unwrap().title, "River Of Test Songs");
    }

    fn song(title: &str, ccli: &str, lyrics: &str) -> Song { Song { id: Uuid::new_v4(), title: title.into(), ccli: ccli.into(), lyrics: lyrics.into(), ..Default::default() } }

    #[test]
    fn duplicates_never_lose_the_churchs_edits() {
        let mine = song("River", "1234567", "Verse 1\nWe are walking by the river\nSinging all the way");
        let mut songs = vec![mine.clone()];
        // New number: added.
        let (id, landed) = place(&mut songs, song("Other", "9", "Verse 1\nx"));
        assert_eq!(landed, Landed::Added);
        assert_eq!(songs.last().unwrap().id, id);
        // Same number, same words (spacing, case and section names aside): nothing changes.
        let (id, landed) = place(&mut songs, song("River (new)", "1234567", "VERSE 1\nwe are  walking by the river,\n\nsinging all the way!"));
        assert_eq!((id, landed), (mine.id, Landed::AlreadyThere));
        assert_eq!(songs.len(), 2);
        // Same number, other words: the church's song stays as it was, the new one sits beside it.
        let (id, landed) = place(&mut songs, song("River", "1234567", "Verse 1\nA different first line"));
        assert_eq!(landed, Landed::KeptBoth("River (from SongSelect)".into()));
        assert_eq!(songs[0], mine, "the church's own song is untouched");
        assert_eq!(songs.iter().find(|s| s.id == id).unwrap().title, "River (from SongSelect)");
        // The same download again: that copy is already there.
        assert_eq!(place(&mut songs, song("River", "1234567", "Verse 1\nA different first line")), (id, Landed::AlreadyThere));
        assert_eq!(songs.len(), 3);
    }
}
