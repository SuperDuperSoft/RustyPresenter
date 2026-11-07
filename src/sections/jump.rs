//! The jump rule (spec §3.2, §5.2): which slide a section key or chip goes to.
use super::{SectionKind, numbered_verses, occurrences, strings};
use crate::model::Slide;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JumpTarget { pub kind: SectionKind, pub number: Option<u8>, pub occurrence: Option<u16> }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JumpError { NotASong, NoSuchSection(SectionKind, Option<u8>), NoSuchOccurrence, KindMismatch }

impl JumpError {
    pub fn message(&self) -> String {
        match self {
            JumpError::NotASong => strings::keys_songs_only().into(),
            JumpError::NoSuchSection(kind, number) => strings::no_section(&kind.name(), *number),
            JumpError::NoSuchOccurrence | JumpError::KindMismatch => strings::song_changed().into(),
        }
    }
}

/// The slide to jump to from `current`. With an occurrence, its first slide. Otherwise, among the occurrences
/// that match: (1) the start of the one the live slide is in, (2) else the first one ahead, (3) else the last one
/// before. `V` (Verse, no number) tries "ahead" first, so it moves on to the next verse.
pub fn jump_target(slides: &[Slide], current: usize, t: &JumpTarget) -> Result<usize, JumpError> {
    let occ = occurrences(slides);
    if occ.is_empty() { return Err(JumpError::NotASong) }
    if let Some(o) = t.occurrence {
        let o = occ.iter().find(|x| x.occurrence == o).ok_or(JumpError::NoSuchOccurrence)?;
        return if o.kind == t.kind { Ok(o.first_slide) } else { Err(JumpError::KindMismatch) };
    }
    let numbered = numbered_verses(&occ);
    let candidates: Vec<_> = occ.iter().filter(|o| {
        o.kind == t.kind && match (t.kind, t.number) {
            (_, None) => true,
            (SectionKind::Verse, Some(n)) if !numbered => o.verse_nth == Some(n),
            (_, Some(n)) => o.number == Some(n),
        }
    }).collect();
    let inside = candidates.iter().find(|o| (o.first_slide..o.first_slide + o.slides).contains(&current)).map(|o| o.first_slide);
    let ahead = candidates.iter().find(|o| o.first_slide > current).map(|o| o.first_slide);
    let before = candidates.iter().rev().find(|o| o.first_slide + o.slides <= current).map(|o| o.first_slide);
    let next_verse = t.kind == SectionKind::Verse && t.number.is_none();
    let found = if next_verse { ahead.or(inside).or(before) } else { inside.or(ahead).or(before) };
    found.ok_or(JumpError::NoSuchSection(t.kind, t.number))
}

/// A section key: `1`–`9` a verse, a kind's letter (any case) that kind. Not with Ctrl, Cmd or Alt held.
pub fn jump_for_key(key: &str, ctrl_or_cmd: bool, alt: bool) -> Option<(SectionKind, Option<u8>)> {
    if ctrl_or_cmd || alt { return None }
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else { return None };
    if let Some(d) = c.to_digit(10).filter(|d| *d >= 1) { return Some((SectionKind::Verse, Some(d as u8))) }
    let c = c.to_ascii_lowercase();
    SectionKind::ALL.into_iter().find(|k| k.key() == Some(c)).map(|k| (k, None))
}

/// "Jumped to Verse 2 (2)", "Jumped to Chorus (C)".
pub fn jump_toast(kind: SectionKind, number: Option<u8>, key: char) -> String {
    let name = match number { Some(n) => format!("{} {n}", kind.name()), None => kind.name() };
    strings::jumped(&name, key.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::refs::kit;

    const JS: &str = "Verse 1\na\nVerse 2\nb\nChorus\nx\ny\nBridge\nz";

    fn t(kind: SectionKind, number: Option<u8>) -> JumpTarget { JumpTarget { kind, number, occurrence: None } }
    fn arranged() -> Vec<Slide> { kit::song(JS, &["Verse 1", "Chorus", "Verse 2", "Chorus"], 1) } // V1 C(1,2) V2 C(4,5)

    #[test]
    fn jump_restarts_current_occurrence() { assert_eq!(jump_target(&arranged(), 2, &t(SectionKind::Chorus, None)), Ok(1)) }

    #[test]
    fn jump_goes_to_next_occurrence_ahead() { assert_eq!(jump_target(&arranged(), 3, &t(SectionKind::Chorus, None)), Ok(4)) }

    #[test]
    fn jump_falls_back_to_last_before() {
        assert_eq!(jump_target(&arranged(), 5, &t(SectionKind::Chorus, None)), Ok(4));
        let s = kit::song(JS, &["Verse 1", "Chorus", "Verse 2"], 1);
        assert_eq!(jump_target(&s, 3, &t(SectionKind::Chorus, None)), Ok(1));
    }

    #[test]
    fn jump_digit_selects_verse_number() { assert_eq!(jump_target(&arranged(), 0, &t(SectionKind::Verse, Some(2))), Ok(3)) }

    #[test]
    fn digit_falls_back_to_nth_unnumbered_verse() {
        let s = kit::song("Verse\na\nChorus\nx\nVerse\nb", &[], 1);
        assert_eq!(jump_target(&s, 0, &t(SectionKind::Verse, Some(2))), Ok(2));
    }

    #[test]
    fn jump_v_goes_to_next_verse_any_number() {
        assert_eq!(jump_target(&arranged(), 0, &t(SectionKind::Verse, None)), Ok(3), "inside V1 → the next verse");
        assert_eq!(jump_target(&arranged(), 4, &t(SectionKind::Verse, None)), Ok(3), "none ahead → the last before");
    }

    #[test]
    fn jump_explicit_occurrence() {
        let s = arranged();
        assert_eq!(jump_target(&s, 0, &JumpTarget { kind: SectionKind::Chorus, number: None, occurrence: Some(3) }), Ok(4));
        assert_eq!(jump_target(&s, 0, &JumpTarget { kind: SectionKind::Chorus, number: None, occurrence: Some(9) }), Err(JumpError::NoSuchOccurrence));
    }

    #[test]
    fn jump_occurrence_kind_mismatch_errors() {
        let e = jump_target(&arranged(), 0, &JumpTarget { kind: SectionKind::Chorus, number: None, occurrence: Some(0) }).unwrap_err();
        assert_eq!(e, JumpError::KindMismatch);
        assert_eq!(e.message(), "The song changed — try again.");
    }

    #[test]
    fn jump_missing_kind_errors() {
        let s = kit::song(JS, &["Verse 1", "Chorus"], 1);
        assert_eq!(jump_target(&s, 0, &t(SectionKind::Tag, None)).unwrap_err().message(), "This song has no Tag.");
        assert_eq!(jump_target(&s, 0, &t(SectionKind::Verse, Some(4))).unwrap_err().message(), "This song has no Verse 4.");
    }

    #[test]
    fn jump_on_non_song_is_not_a_song() {
        assert_eq!(jump_target(&kit::plain(&["a"]), 0, &t(SectionKind::Chorus, None)), Err(JumpError::NotASong));
        assert_eq!(JumpError::NotASong.message(), "Section keys work on songs.");
    }

    #[test]
    fn letters_and_digits_map_to_jumps() {
        assert_eq!(jump_for_key("c", false, false), Some((SectionKind::Chorus, None)));
        assert_eq!(jump_for_key("C", false, false), Some((SectionKind::Chorus, None)));
        assert_eq!(jump_for_key("l", false, false), Some((SectionKind::Interlude, None)));
        assert_eq!(jump_for_key("v", false, false), Some((SectionKind::Verse, None)));
        assert_eq!(jump_for_key("7", false, false), Some((SectionKind::Verse, Some(7))));
    }

    #[test]
    fn ctrl_or_alt_letter_is_not_a_jump() {
        assert_eq!(jump_for_key("c", true, false), None);
        assert_eq!(jump_for_key("2", false, true), None);
    }

    #[test]
    fn zero_and_unmapped_letters_do_nothing() {
        for k in ["0", "x", "n", "cc", ""] { assert_eq!(jump_for_key(k, false, false), None, "{k}") }
    }

    #[test]
    fn toast_names_kind_and_key() {
        assert_eq!(jump_toast(SectionKind::Verse, Some(2), '2'), "Jumped to Verse 2 (2)");
        assert_eq!(jump_toast(SectionKind::Chorus, None, 'c'), "Jumped to Chorus (C)");
        assert_eq!(jump_toast(SectionKind::PreChorus, None, 'p'), "Jumped to Pre-chorus (P)");
        assert_eq!(jump_toast(SectionKind::Verse, None, 'v'), "Jumped to Verse (V)");
    }
}
