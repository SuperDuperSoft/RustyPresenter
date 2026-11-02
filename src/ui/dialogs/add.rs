//! Add dialog logic that needs no window.
use super::strings as tr;
use crate::{model::Song, slides};

/// The most characters of a lyric line a song row shows (`operator.js:424`).
const FIRST_LYRIC_CHARS: usize = 80;

/// The line under a song's title: its writers and CCLI number, or else its first lyric (`operator.js:419`).
pub fn song_subtitle(song: &Song) -> String {
    let ccli = (!song.ccli.is_empty()).then(|| tr::ccli_number(&song.ccli));
    let credits: Vec<&str> = [Some(song.writers.as_str()).filter(|w| !w.is_empty()), ccli.as_deref()].into_iter().flatten().collect();
    if credits.is_empty() { first_lyric(&song.lyrics) } else { credits.join(" · ") }
}

fn first_lyric(lyrics: &str) -> String {
    let line = lyrics.lines().find(|l| !l.trim().is_empty() && slides::section_label(l).is_none());
    line.unwrap_or("").chars().take(FIRST_LYRIC_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(writers: &str, ccli: &str, lyrics: &str) -> Song {
        Song { writers: writers.into(), ccli: ccli.into(), lyrics: lyrics.into(), ..Song::default() }
    }

    #[test]
    fn credits_come_first_when_there_are_any() {
        assert_eq!(song_subtitle(&song("John Newton", "22025", "Amazing grace")), "John Newton · CCLI #22025");
        assert_eq!(song_subtitle(&song("John Newton", "", "Amazing grace")), "John Newton");
        assert_eq!(song_subtitle(&song("", "22025", "Amazing grace")), "CCLI #22025");
    }

    #[test]
    fn song_row_subtitle_falls_back_to_first_lyric() {
        assert_eq!(song_subtitle(&song("", "", "Verse 1\n\n  \nAmazing grace\nhow sweet")), "Amazing grace");
        assert_eq!(song_subtitle(&song("", "", "[Chorus]")), "");
    }

    #[test]
    fn long_first_lyric_stops_at_80_characters_not_bytes() {
        let line = "é".repeat(100);
        assert_eq!(song_subtitle(&song("", "", &line)).chars().count(), 80);
    }
}
