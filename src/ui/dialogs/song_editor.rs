//! Song editor logic that needs no window.
use super::strings as tr;
use crate::slides::{Section, parse_sections};

/// Order box text to a song's arrangement: split on commas, trimmed, empties dropped (`operator.js` `save`).
pub fn parse_order(order: &str) -> Vec<String> {
    order.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

/// The `+ <label>` chips: each section once, in written order. Labels differing only in case stay separate, as on
/// the web.
pub fn section_chips(lyrics: &str) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for s in parse_sections(lyrics) {
        if !labels.contains(&s.label) {
            labels.push(s.label);
        }
    }
    labels
}

/// Order names that match no section, each once. Matching is `slides::song_order`'s, so a name listed here is
/// exactly one the core drops.
pub fn order_warnings(lyrics: &str, order: &str) -> Vec<String> {
    let sections: Vec<Section> = parse_sections(lyrics);
    let mut unknown: Vec<String> = Vec::new();
    for name in parse_order(order) {
        let known = sections.iter().any(|s| s.label.eq_ignore_ascii_case(&name));
        if !known && !unknown.iter().any(|u| u.eq_ignore_ascii_case(&name)) {
            unknown.push(name);
        }
    }
    unknown
}

/// The note under Order, or nothing when every name is known.
pub fn order_warning_text(lyrics: &str, order: &str) -> Option<String> {
    let names = order_warnings(lyrics, order);
    (!names.is_empty()).then(|| tr::not_in_lyrics(&names))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::Song, slides::song_order};

    const LYRICS: &str = "Verse 1\nAmazing grace\n\nChorus\nMy chains are gone\n\nVerse 2\nHow sweet";

    #[test]
    fn order_splits_on_commas_and_drops_empties() {
        assert_eq!(parse_order(" Verse 1, Chorus ,, Verse 2,"), ["Verse 1", "Chorus", "Verse 2"]);
        assert!(parse_order("  ").is_empty());
    }

    #[test]
    fn section_chips_are_unique_and_in_written_order() {
        assert_eq!(section_chips("Chorus\na\n\nVerse 1\nb\n\nChorus\nc"), ["Chorus", "Verse 1"]);
    }

    #[test]
    fn order_warnings_lists_unknown_names() {
        assert_eq!(order_warnings(LYRICS, "Verse 1, Bridge, Chorus, Tag"), ["Bridge", "Tag"]);
        assert_eq!(order_warning_text(LYRICS, "Verse 1, Bridge, Tag").as_deref(), Some("Not in the lyrics: Bridge, Tag"));
    }

    #[test]
    fn no_warning_when_every_name_is_known() {
        assert!(order_warnings(LYRICS, "chorus, VERSE 2").is_empty());
        assert_eq!(order_warning_text(LYRICS, ""), None);
    }

    #[test]
    fn an_unknown_name_is_listed_once() {
        assert_eq!(order_warnings(LYRICS, "Bridge, bridge, Bridge"), ["Bridge"]);
    }

    #[test]
    fn unlabelled_lyrics_are_called_parts() {
        // A blank line starts a new slide in the same section, so there is one part here.
        assert_eq!(section_chips("one\n\ntwo"), ["Part 1"]);
        assert!(order_warnings("one\n\ntwo", "Part 1").is_empty());
        assert_eq!(order_warnings("one\n\ntwo", "Part 2"), ["Part 2"]);
    }

    #[test]
    fn warnings_agree_with_what_the_core_keeps() {
        let order = "Chorus, Bridge, verse 2, tag, Verse 1";
        let song = Song { lyrics: LYRICS.into(), arrangement: parse_order(order), ..Song::default() };
        let kept = song_order(&song, &parse_sections(LYRICS)).len();
        assert_eq!(kept + order_warnings(LYRICS, order).len(), song.arrangement.len());
    }
}
