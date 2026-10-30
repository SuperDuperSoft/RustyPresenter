use crate::{model::{ServiceItem, Slide}, songselect::paginate};
use serde::Serialize;
use std::collections::BTreeMap;
use uuid::Uuid;

// Canonical book order and common reference aliases, matching the source's USFM IDs.
const BOOKS: &[(&str, &str, &str)] = &[
    ("GEN","Genesis","ge gn"), ("EXO","Exodus","ex exod"), ("LEV","Leviticus","le lev"),
    ("NUM","Numbers","nu num"), ("DEU","Deuteronomy","de deut dt"), ("JOS","Joshua","jos josh"),
    ("JDG","Judges","jdg judg"), ("RUT","Ruth","ru rut"), ("1SA","1 Samuel","1sam 1sa"),
    ("2SA","2 Samuel","2sam 2sa"), ("1KI","1 Kings","1ki 1kgs"), ("2KI","2 Kings","2ki 2kgs"),
    ("1CH","1 Chronicles","1ch 1chr"), ("2CH","2 Chronicles","2ch 2chr"), ("EZR","Ezra","ezr"),
    ("NEH","Nehemiah","ne neh"), ("EST","Esther","es est"), ("JOB","Job","jb"),
    ("PSA","Psalms","ps psa psalm"), ("PRO","Proverbs","pr pro prov"), ("ECC","Ecclesiastes","ec ecc eccl"),
    ("SNG","Song of Solomon","songof songs songofsongs sos sng"), ("ISA","Isaiah","is isa"),
    ("JER","Jeremiah","je jer"), ("LAM","Lamentations","la lam"), ("EZK","Ezekiel","ezek ezk"),
    ("DAN","Daniel","da dan"), ("HOS","Hosea","ho hos"), ("JOL","Joel","joe jol"),
    ("AMO","Amos","am amo"), ("OBA","Obadiah","ob oba"), ("JON","Jonah","jon"),
    ("MIC","Micah","mi mic"), ("NAM","Nahum","na nah nam"), ("HAB","Habakkuk","hab"),
    ("ZEP","Zephaniah","zep zeph"), ("HAG","Haggai","hag"), ("ZEC","Zechariah","zec zech"),
    ("MAL","Malachi","mal"), ("MAT","Matthew","mt mat matt"), ("MRK","Mark","mk mrk"),
    ("LUK","Luke","lk luk"), ("JHN","John","jn joh jhn"), ("ACT","Acts","ac act"),
    ("ROM","Romans","ro rom"), ("1CO","1 Corinthians","1co 1cor"), ("2CO","2 Corinthians","2co 2cor"),
    ("GAL","Galatians","ga gal"), ("EPH","Ephesians","ep eph"), ("PHP","Philippians","php phil"),
    ("COL","Colossians","col"), ("1TH","1 Thessalonians","1th 1thess"), ("2TH","2 Thessalonians","2th 2thess"),
    ("1TI","1 Timothy","1ti 1tim"), ("2TI","2 Timothy","2ti 2tim"), ("TIT","Titus","ti tit"),
    ("PHM","Philemon","phm philem"), ("HEB","Hebrews","heb"), ("JAS","James","jas jam"),
    ("1PE","1 Peter","1pe 1pet"), ("2PE","2 Peter","2pe 2pet"), ("1JN","1 John","1jn 1jhn"),
    ("2JN","2 John","2jn 2jhn"), ("3JN","3 John","3jn 3jhn"), ("JUD","Jude","jud"),
    ("REV","Revelation","re rev revelationsof"),
];

#[derive(Clone, Debug, Serialize)]
pub struct Verse {
    pub reference: String,
    pub text: String,
}

pub struct Bible {
    verses: BTreeMap<(usize, u16, u16), String>,
}

fn normalize(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace() && *c != '.').flat_map(char::to_lowercase).collect()
}

impl Default for Bible {
    fn default() -> Self { Self::kjv() }
}

impl Bible {
    pub fn kjv() -> Self {
        let mut verses = BTreeMap::new();
        for line in include_str!("../assets/bibles/kjv.txt").lines() {
            if let Some((code, rest)) = line.split_once(' ')
                && let Some(book) = BOOKS.iter().position(|b| b.0 == code)
                && let Some((numbers, text)) = rest.split_once(' ')
                && let Some((chapter, verse)) = numbers.split_once(':')
                && let (Ok(chapter), Ok(verse)) = (chapter.parse(), verse.parse()) {
                    verses.insert((book, chapter, verse), text.to_owned());
                }
        }
        Self { verses }
    }

    pub fn lookup(&self, reference: &str) -> Result<Vec<Verse>, String> {
        let (book_name, numbers) = reference.trim().rsplit_once(' ').ok_or("Enter a reference such as John 3:16–18 or Psalm 23.")?;
        let name = normalize(book_name);
        let book = BOOKS.iter().position(|b| normalize(b.1) == name || normalize(b.0) == name || b.2.split_whitespace().any(|alias| alias == name)).ok_or("That Bible book was not recognized.")?;
        let numbers = numbers.replace(['–','—'], "-");
        let (chapter, range) = numbers.split_once(':').map_or((numbers.as_str(), None), |(c, r)| (c, Some(r)));
        let chapter: u16 = chapter.parse().map_err(|_| "Enter a valid chapter number.")?;
        let (start, end) = if let Some(range) = range {
            let (start, end) = range.split_once('-').unwrap_or((range, range));
            let start: u16 = start.parse().map_err(|_| "Enter a valid verse number.")?;
            let end: u16 = end.parse().map_err(|_| "Use a verse range within one chapter, such as John 3:16-18.")?;
            if start == 0 || end < start || end - start > 149 { return Err("Select 1–150 verses in order.".into()); }
            (start, end)
        } else {
            let end = self.verses.range((book, chapter, 1)..=(book, chapter, u16::MAX)).next_back().map(|(key, _)| key.2).ok_or("That chapter does not exist.")?;
            (1, end)
        };
        let mut result = Vec::new();
        for verse in start..=end {
            let text = self.verses.get(&(book, chapter, verse)).ok_or_else(|| format!("{} {chapter}:{verse} does not exist in KJV.", BOOKS[book].1))?;
            result.push(Verse { reference: format!("{} {chapter}:{verse}", BOOKS[book].1), text: text.clone() });
        }
        Ok(result)
    }

    pub fn slides(&self, reference: &str) -> Result<ServiceItem, String> {
        let verses = self.lookup(reference)?;
        let mut slides = Vec::new();
        for verse in verses {
            let pages = paginate(&verse.text, 4, 320);
            let count = pages.len();
            for (index, text) in pages.into_iter().enumerate() {
                let label = if count > 1 { format!("{} · {}/{}", verse.reference, index + 1, count) } else { verse.reference.clone() };
                slides.push(Slide::new(label, text));
            }
        }
        Ok(ServiceItem { id: Uuid::new_v4(), title: format!("{} · KJV", reference.trim()), kind: "scripture".into(), credits: "King James Version (KJV)".into(), slides })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_canon_has_all_31102_verses() {
        assert_eq!(Bible::kjv().verses.len(), 31102);
    }
    #[test]
    fn finds_numbered_books_chapters_and_ranges() {
        let bible = Bible::kjv();
        assert_eq!(bible.lookup("1 John 2:1-3").unwrap().len(), 3);
        assert_eq!(bible.lookup("Psalm 23").unwrap().len(), 6);
        assert_eq!(bible.lookup("Jn. 3:16–18").unwrap().len(), 3);
        assert!(bible.lookup("John 3:0").is_err());
        assert!(bible.lookup("John 3:36-38").is_err());
        assert!(bible.lookup("John 99").is_err());
        assert!(bible.lookup("John 3:18-16").is_err());
    }
    #[test]
    fn verse_slides_keep_reference_and_exact_source_words() {
        let bible = Bible::kjv();
        let verses = bible.lookup("John 3:16").unwrap();
        let item = bible.slides("John 3:16").unwrap();
        assert_eq!(item.slides[0].label, "John 3:16");
        assert_eq!(item.slides[0].text, verses[0].text);
    }
}
