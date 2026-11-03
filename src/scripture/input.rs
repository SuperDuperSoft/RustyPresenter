//! Verse identities from verse labels, and what the operator typed in the Bible box (spec §5.6).
use super::VerseId;
use crate::bible::{self, Reference, find_book, parse_reference};

/// Split a verse label (`"1 Kings 22:3"`) into its book part, chapter and verse.
/// `None` unless it ends in ` c:v` with both numbers above zero.
pub fn parse_verse_label(label: &str) -> Option<(&str, u16, u16)> {
    let (book, cv) = label.trim().rsplit_once(' ')?;
    let (c, v) = cv.split_once(':')?;
    let (c, v) = (c.parse::<u16>().ok()?, v.parse::<u16>().ok()?);
    (c > 0 && v > 0 && !book.trim().is_empty()).then_some((book.trim(), c, v))
}

/// The verse a label names; accepts "Psalm" and "Psalms" and any name `find_book` knows.
pub fn verse_id_of(label: &str) -> Option<VerseId> {
    let (book, c, v) = parse_verse_label(label)?;
    Some(VerseId(find_book(book)?, c, v))
}

/// The book name verse labels use: `BOOKS` display name, with "Psalms" shown as "Psalm"
/// (the same names as `Kjv::lookup`).
pub fn display_name(book: u8) -> &'static str { bible::display_name(book) }

/// `"John 3:16"` for a verse id.
pub fn label_of(id: VerseId) -> String { format!("{} {}:{}", display_name(id.book()), id.chapter(), id.verse()) }

/// `"Psalm 46"`: a whole chapter as a reference the Bible readers accept.
pub fn chapter_reference(book: u8, chapter: u16) -> String { format!("{} {chapter}", display_name(book)) }

/// What the operator typed in the Bible box.
#[derive(Debug, Clone, PartialEq)]
pub enum BibleInput {
    /// A reference the readers can look up (`jn 3 16`).
    Reference(Reference),
    /// Just a book name (`Job`): ask for a chapter or offer a word search.
    BookOnly { book: u8 },
    /// Has digits but no known book (`Hezekiah 3:1`): a mistyped reference.
    UnknownWithDigits,
    /// Anything else: a keyword search.
    Words,
}

/// The four rules of spec §5.6, in order.
pub fn classify_bible_input(text: &str) -> BibleInput {
    if let Ok(r) = parse_reference(text) { return BibleInput::Reference(r); }
    if let Some(book) = find_book(text.trim()) { return BibleInput::BookOnly { book }; }
    if text.chars().any(|c| c.is_ascii_digit()) { return BibleInput::UnknownWithDigits; }
    BibleInput::Words
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bible::BOOKS;

    #[test]
    fn verse_id_of_round_trips_display_names() {
        for (i, _) in BOOKS.iter().enumerate() {
            let label = format!("{} 1:1", display_name(i as u8));
            assert_eq!(verse_id_of(&label), Some(VerseId(i as u8, 1, 1)), "{label}");
            assert_eq!(label_of(VerseId(i as u8, 1, 1)), label);
        }
        assert_eq!(display_name(18), "Psalm");
    }
    #[test]
    fn verse_id_of_parses_labels_from_kjv_and_imported_readers() {
        let (_, vs) = crate::bible::kjv().lookup("John 3:16-17").unwrap();
        assert_eq!(vs.iter().map(|v| verse_id_of(&v.reference)).collect::<Vec<_>>(), vec![Some(VerseId(42, 3, 16)), Some(VerseId(42, 3, 17))]);
        assert_eq!(verse_id_of("Psalms 23:1"), Some(VerseId(18, 23, 1)));
        assert_eq!(verse_id_of("Song of Solomon 2:1"), Some(VerseId(21, 2, 1)));
        assert_eq!(verse_id_of("1 Kings 22:3"), Some(VerseId(10, 22, 3)));
    }
    #[test]
    fn unparseable_label_gives_none() {
        for l in ["Psalm 23", "Hezekiah 3:1", "John 3:0", "", "John :16", "John 3:16a"] { assert_eq!(verse_id_of(l), None, "{l}"); }
    }
    #[test]
    fn classify_reference() { assert!(matches!(classify_bible_input("jn 3 16"), BibleInput::Reference(_))); }
    #[test]
    fn classify_book_only_job_mark_judge_numbers() {
        for (t, b) in [("Job", 17), ("Mark", 40), ("judge", 6), ("numbers", 3)] { assert_eq!(classify_bible_input(t), BibleInput::BookOnly { book: b }, "{t}"); }
    }
    #[test]
    fn classify_digits_unknown_book() {
        for t in ["Hezekiah 3:1", "40 days", "seventy times 7"] { assert_eq!(classify_bible_input(t), BibleInput::UnknownWithDigits, "{t}"); }
    }
    #[test]
    fn classify_words() {
        for t in ["be still and know", "Phillipians", "love one another"] { assert_eq!(classify_bible_input(t), BibleInput::Words, "{t}"); }
    }
    #[test]
    fn chapter_reference_uses_display_name() { assert_eq!(chapter_reference(18, 46), "Psalm 46"); }
}
