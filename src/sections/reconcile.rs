//! What to do with the live item after the library changed (spec §5.4, §5.5).
use super::{Mapped, same_on_screen, strings};
use crate::model::{Slide, SlideContent};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Why the live item differs from the library in a way the room would show.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeReason { WordsOnScreen, LookOnScreen, PlaceLost, NoSlides }

impl ChangeReason {
    pub fn banner(self) -> String {
        match self {
            ChangeReason::WordsOnScreen => strings::banner_words(),
            ChangeReason::LookOnScreen => strings::banner_look(),
            ChangeReason::PlaceLost => strings::banner_lost(),
            ChangeReason::NoSlides => strings::banner_no_slides(),
        }
    }

    /// Update screen can apply it (not when the song has no slides now).
    pub fn can_update(self) -> bool { self != ChangeReason::NoSlides }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LiveChange { pub item_id: Uuid, pub reason: ChangeReason }

#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    /// Nothing differs.
    Same,
    /// Only stage notes differ: copy them in with no version change.
    NotesOnly,
    /// The room would look the same: take the new slides, the live slide is now `slide`. `map`: old → new index.
    Adopt { slide: usize, map: Vec<Option<usize>> },
    /// Keep the snapshot and show the banner.
    Banner(ChangeReason),
}

/// `look_equal`: the backgrounds (and anything else outside the slides that the room shows) are the same.
pub fn decide(old: &[Slide], cursor: usize, new: &[Slide], look_equal: bool, mapper: impl Fn(usize) -> Mapped) -> Decision {
    if old == new && look_equal { return Decision::Same }
    if new.is_empty() { return Decision::Banner(ChangeReason::NoSlides) }
    let notes_only = old.len() == new.len() && old.iter().zip(new).all(|(a, b)| a.label == b.label && a.content == b.content && a.section == b.section);
    if notes_only && look_equal { return Decision::NotesOnly }
    let Some(i) = mapper(cursor).index() else { return Decision::Banner(ChangeReason::PlaceLost) };
    let (was, now) = (&old[cursor], &new[i]);
    if same_on_screen(now, was) && look_equal {
        return Decision::Adopt { slide: i, map: (0..old.len()).map(|k| mapper(k).index()).collect() };
    }
    if text_and_reference(was) != text_and_reference(now) && words(old) != words(new) {
        Decision::Banner(ChangeReason::WordsOnScreen)
    } else {
        Decision::Banner(ChangeReason::LookOnScreen)
    }
}

fn text_and_reference(s: &Slide) -> Option<(&str, Option<&str>)> {
    match &s.content { SlideContent::Text { text, reference, .. } => Some((text, reference.as_deref())), _ => None }
}

/// Every text slide's words joined, with the whitespace collapsed: the same words split another way match.
pub fn words(slides: &[Slide]) -> String {
    slides.iter().filter_map(|s| text_and_reference(s).map(|(t, _)| t)).flat_map(str::split_whitespace).collect::<Vec<_>>().join(" ")
}

/// Where Update screen puts the live slide: the mapped place, or the first slide when it was lost.
pub fn refresh_slide(m: Mapped) -> usize { m.index().unwrap_or(0) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::{map_position, refs::kit};

    const L: &str = "Verse 1\nv1\nChorus\nc\nVerse 2\nv2\nBridge\nb";

    fn run(old: &[Slide], cursor: usize, new: &[Slide]) -> Decision { decide(old, cursor, new, true, |i| map_position(old, i, new)) }

    #[test]
    fn identical_is_same() {
        let s = kit::song(L, &[], 1);
        assert_eq!(run(&s, 0, &s), Decision::Same);
    }

    #[test]
    fn identical_slides_but_look_differs_gives_look_reason() {
        let s = kit::song(L, &[], 1);
        assert_eq!(decide(&s, 0, &s, false, |i| map_position(&s, i, &s)), Decision::Banner(ChangeReason::LookOnScreen));
    }

    #[test]
    fn empty_new_gives_no_slides() {
        assert_eq!(run(&kit::song(L, &[], 1), 0, &[]), Decision::Banner(ChangeReason::NoSlides));
        assert!(!ChangeReason::NoSlides.can_update());
    }

    #[test]
    fn notes_only_change_is_notes_only() {
        let old = kit::song(L, &[], 1);
        let mut new = old.clone();
        new[2].notes = "softly".into();
        assert_eq!(run(&old, 0, &new), Decision::NotesOnly);
    }

    #[test]
    fn edit_other_section_adopts() {
        let old = kit::song(L, &["Verse 1", "Chorus", "Verse 2"], 1);
        let new = kit::song(&L.replace("v2", "v2 fixed"), &["Verse 1", "Chorus", "Verse 2"], 1);
        assert_eq!(run(&old, 0, &new), Decision::Adopt { slide: 0, map: vec![Some(0), Some(1), Some(2)] });
    }

    #[test]
    fn label_only_change_adopts_quietly() {
        let old = kit::song(L, &[], 1);
        let new = kit::song(&L.replace("Chorus", "Refrain"), &[], 1);
        assert!(matches!(run(&old, 1, &new), Decision::Adopt { slide: 1, .. }));
    }

    #[test]
    fn occurrence_shift_adopts_quietly() {
        let old = kit::song(L, &["Verse 1", "Verse 2"], 1);
        let new = kit::song(L, &["Verse 1", "Chorus", "Verse 2"], 1);
        assert!(matches!(run(&old, 1, &new), Decision::Adopt { slide: 2, .. }));
    }

    #[test]
    fn edit_live_slide_gives_words_on_screen() {
        let old = kit::song(L, &[], 1);
        let new = kit::song(&L.replace("v1", "v1 fixed"), &[], 1);
        assert_eq!(run(&old, 0, &new), Decision::Banner(ChangeReason::WordsOnScreen));
    }

    #[test]
    fn lines_per_slide_change_gives_look_reason() {
        let lyrics = "Verse 1\na\nb\nc\nd";
        let old = kit::song(lyrics, &[], 2);
        let new = kit::song(lyrics, &[], 1);
        assert_eq!(run(&old, 0, &new), Decision::Banner(ChangeReason::LookOnScreen));
    }

    #[test]
    fn footer_change_gives_look_reason() {
        let old = kit::song(L, &[], 1);
        let mut new = old.clone();
        if let SlideContent::Text { footer, .. } = &mut new[0].content { *footer = Some("Amazing Grace · CCLI 1".into()) }
        assert_eq!(run(&old, 0, &new), Decision::Banner(ChangeReason::LookOnScreen));
    }

    #[test]
    fn section_deleted_gives_place_lost() {
        let old = kit::song(L, &[], 1);
        let new = kit::song("Verse 1\nv1\nChorus\nc\nVerse 2\nv2", &[], 1);
        assert_eq!(run(&old, 3, &new), Decision::Banner(ChangeReason::PlaceLost));
    }

    #[test]
    fn adopt_map_marks_lost_slides_none() {
        let old = kit::song(L, &[], 1);
        let new = kit::song("Verse 1\nv1\nChorus\nc\nVerse 2\nv2", &[], 1);
        assert_eq!(run(&old, 0, &new), Decision::Adopt { slide: 0, map: vec![Some(0), Some(1), Some(2), None] });
    }

    #[test]
    fn refresh_slide_lost_is_zero() {
        assert_eq!(refresh_slide(Mapped::Lost), 0);
        assert_eq!(refresh_slide(Mapped::Moved(3)), 3);
    }

    #[test]
    fn change_reason_json_and_banner_text() {
        let c = LiveChange { item_id: Uuid::nil(), reason: ChangeReason::WordsOnScreen };
        assert_eq!(serde_json::to_value(&c).unwrap()["reason"], "words_on_screen");
        assert_eq!(ChangeReason::WordsOnScreen.banner(), "You changed the words that are on screen. Update screen shows them now. Clicking a slide also shows the new version.");
    }
}
