//! Bible references and the bundled King James Version.
//! Copyrighted translations come from licensed online sources (`sources`, spec §8.5).
use crate::model::VerseText;

pub mod api_bible;
pub mod blocklist;
pub mod esv;
pub mod import;
pub mod nlt;
pub mod sources;
pub mod youversion;
use std::{collections::BTreeMap, sync::OnceLock};

/// (code in the eBible KJV source, display name, extra aliases)
pub const BOOKS: &[(&str, &str, &str)] = &[
    ("GEN", "Genesis", "gn ge"), ("EXO", "Exodus", "ex exod"), ("LEV", "Leviticus", "lv lev"), ("NUM", "Numbers", "nm nu num"),
    ("DEU", "Deuteronomy", "dt deut"), ("JOS", "Joshua", "josh jsh"), ("JDG", "Judges", "jdg judg jg"), ("RUT", "Ruth", "ru rth"),
    ("1SA", "1 Samuel", "1sam 1sm 1sa"), ("2SA", "2 Samuel", "2sam 2sm 2sa"), ("1KI", "1 Kings", "1kgs 1ki 1kg"), ("2KI", "2 Kings", "2kgs 2ki 2kg"),
    ("1CH", "1 Chronicles", "1chr 1ch 1chron"), ("2CH", "2 Chronicles", "2chr 2ch 2chron"), ("EZR", "Ezra", "ezr"), ("NEH", "Nehemiah", "neh ne"),
    ("EST", "Esther", "est esth"), ("JOB", "Job", "jb"), ("PSA", "Psalms", "ps psa psalm pss"), ("PRO", "Proverbs", "pr prov prv"),
    ("ECC", "Ecclesiastes", "ecc eccl qoh"), ("SOL", "Song of Solomon", "song sos songofsongs sng canticles"), ("ISA", "Isaiah", "isa is"),
    ("JER", "Jeremiah", "jer jr"), ("LAM", "Lamentations", "lam la"), ("EZE", "Ezekiel", "ezek ezk eze"), ("DAN", "Daniel", "dan dn"),
    ("HOS", "Hosea", "hos ho"), ("JOE", "Joel", "jl joel"), ("AMO", "Amos", "am amos"), ("OBA", "Obadiah", "obad ob"), ("JON", "Jonah", "jon jnh"),
    ("MIC", "Micah", "mic mc"), ("NAH", "Nahum", "nah na"), ("HAB", "Habakkuk", "hab hb"), ("ZEP", "Zephaniah", "zeph zep zp"),
    ("HAG", "Haggai", "hag hg"), ("ZEC", "Zechariah", "zech zec zc"), ("MAL", "Malachi", "mal ml"),
    ("MAT", "Matthew", "mt matt mat"), ("MAR", "Mark", "mk mrk mar"), ("LUK", "Luke", "lk luk"), ("JOH", "John", "jn jhn joh"),
    ("ACT", "Acts", "ac act"), ("ROM", "Romans", "rom ro rm"), ("1CO", "1 Corinthians", "1cor 1co"), ("2CO", "2 Corinthians", "2cor 2co"),
    ("GAL", "Galatians", "gal ga"), ("EPH", "Ephesians", "eph ephes"), ("PHI", "Philippians", "phil php pp"), ("COL", "Colossians", "col"),
    ("1TH", "1 Thessalonians", "1thess 1th 1thes"), ("2TH", "2 Thessalonians", "2thess 2th 2thes"), ("1TI", "1 Timothy", "1tim 1ti 1tm"),
    ("2TI", "2 Timothy", "2tim 2ti 2tm"), ("TIT", "Titus", "tit ti"), ("PHM", "Philemon", "phlm phm philem"), ("HEB", "Hebrews", "heb"),
    ("JAM", "James", "jas jm jam"), ("1PE", "1 Peter", "1pet 1pe 1pt"), ("2PE", "2 Peter", "2pet 2pe 2pt"), ("1JO", "1 John", "1jn 1jo 1jhn"),
    ("2JO", "2 John", "2jn 2jo 2jhn"), ("3JO", "3 John", "3jn 3jo 3jhn"), ("JUD", "Jude", "jude jud"), ("REV", "Revelation", "rev re rv revelations"),
];

/// Standard USFM/USX book ids in `BOOKS` order (API.Bible, YouVersion and USX files use them). Several differ from
/// the eBible-style codes above (SOL→SNG, EZE→EZK, JOE→JOL, NAH→NAM, MAR→MRK, JOH→JHN, PHI→PHP, JAM→JAS, 1JO→1JN…).
pub const USFM: [&str; 66] = [
    "GEN", "EXO", "LEV", "NUM", "DEU", "JOS", "JDG", "RUT", "1SA", "2SA", "1KI", "2KI", "1CH", "2CH", "EZR", "NEH",
    "EST", "JOB", "PSA", "PRO", "ECC", "SNG", "ISA", "JER", "LAM", "EZK", "DAN", "HOS", "JOL", "AMO", "OBA", "JON",
    "MIC", "NAM", "HAB", "ZEP", "HAG", "ZEC", "MAL",
    "MAT", "MRK", "LUK", "JHN", "ACT", "ROM", "1CO", "2CO", "GAL", "EPH", "PHP", "COL", "1TH", "2TH", "1TI", "2TI",
    "TIT", "PHM", "HEB", "JAS", "1PE", "2PE", "1JN", "2JN", "3JN", "JUD", "REV",
];

pub const TRANSLATIONS: &[&str] =&["NIV", "ESV", "CSB", "HCSB", "NLT", "KJV", "WEB", "BSB"];

/// Public-domain texts in the app (eBible.org verse-per-line files, see assets/bibles/SOURCE.html): they work offline with no setup.
pub const BUILT_IN: &[(&str, &str)] = &[("KJV", "King James Version"), ("WEB", "World English Bible"), ("BSB", "Berean Standard Bible")];

pub struct Kjv {
    verses: BTreeMap<(u8, u16, u16), String>,
}

static KJV: OnceLock<Kjv> = OnceLock::new();
static WEB: OnceLock<Kjv> = OnceLock::new();
static BSB: OnceLock<Kjv> = OnceLock::new();

pub fn kjv() -> &'static Kjv {
    KJV.get_or_init(|| Kjv::parse(include_str!("../assets/bibles/kjv.txt")))
}

/// A bundled public-domain translation, parsed on first use.
pub fn built_in(translation: &str) -> Option<&'static Kjv> {
    match translation {
        "KJV" => Some(kjv()),
        "WEB" => Some(WEB.get_or_init(|| Kjv::parse(include_str!("../assets/bibles/web.txt")))),
        "BSB" => Some(BSB.get_or_init(|| Kjv::parse(include_str!("../assets/bibles/bsb.txt")))),
        _ => None,
    }
}

/// Remove source markup: paragraph marks and the brackets around supplied words.
fn clean(text: &str) -> String {
    let t = text.replace('¶', "").replace(['[', ']'], "");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Kjv {
    pub fn parse(src: &str) -> Self {
        let mut verses = BTreeMap::new();
        for line in src.lines() {
            let Some((code, rest)) = line.split_once(' ') else { continue };
            let Some(book) = BOOKS.iter().position(|b| b.0 == code) else { continue }; // skips the Apocrypha
            let Some((cv, text)) = rest.split_once(' ') else { continue };
            let Some((c, v)) = cv.split_once(':') else { continue };
            if let (Ok(c), Ok(v)) = (c.parse(), v.parse()) { verses.insert((book as u8, c, v), clean(text)); }
        }
        Self { verses }
    }

    pub fn len(&self) -> usize { self.verses.len() }
    pub fn is_empty(&self) -> bool { self.verses.is_empty() }

    /// Verses in a book (KJV versification); used for the "half a book" storage limits.
    pub fn book_len(&self, book: u8) -> usize {
        self.verses.range((book, 0, 0)..=(book, u16::MAX, u16::MAX)).count()
    }

    fn last_verse(&self, book: u8, chapter: u16) -> Option<u16> {
        self.verses.range((book, chapter, 0)..=(book, chapter, u16::MAX)).next_back().map(|(k, _)| k.2)
    }

    pub fn lookup(&self, input: &str) -> Result<(String, Vec<VerseText>), String> {
        let r = parse_reference(input)?;
        let display = display_name(r.book);
        let mut out = Vec::new();
        for part in &r.parts {
            let (sc, sv) = part.start;
            let (ec, ev) = part.end;
            let ev_resolved = match ev { Some(v) => v, None => self.last_verse(r.book, ec).ok_or_else(|| format!("{display} {ec} does not exist."))? };
            if self.last_verse(r.book, sc).is_none() { return Err(format!("{display} {sc} does not exist.")); }
            let mut c = sc;
            let mut v = sv.unwrap_or(1);
            loop {
                // A range can run past the book's last chapter (Jude 1:1-3:1).
                let last = self.last_verse(r.book, c).ok_or_else(|| format!("{display} {c} does not exist."))?;
                match self.verses.get(&(r.book, c, v)) {
                    Some(t) => out.push(VerseText { reference: format!("{display} {c}:{v}"), text: t.clone() }),
                    None if v == sv.unwrap_or(1) && c == sc => return Err(format!("{display} {c}:{v} does not exist.")),
                    None => {}
                }
                if c == ec && v >= ev_resolved { break; }
                if v >= last { c += 1; v = 1; if c > ec { break; } } else { v += 1; }
                if out.len() > 400 { return Err("That passage is too long. Choose up to about 400 verses.".into()); }
            }
            if ev.is_some() && !out.iter().any(|x| x.reference == format!("{display} {ec}:{ev_resolved}")) {
                return Err(format!("{display} {ec}:{ev_resolved} does not exist."));
            }
        }
        if out.is_empty() { return Err("No verses found.".into()); }
        Ok((canonical(&r, display), out))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Part { pub start: (u16, Option<u16>), pub end: (u16, Option<u16>) }

#[derive(Debug, Clone, PartialEq)]
pub struct Reference { pub book: u8, pub parts: Vec<Part> }

fn norm(s: &str) -> String { s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect() }

pub fn find_book(name: &str) -> Option<u8> {
    let n = norm(name);
    if n.is_empty() { return None; }
    if let Some(i) = BOOKS.iter().position(|b| norm(b.1) == n || b.2.split_whitespace().any(|a| a == n)) { return Some(i as u8); }
    // Unique prefix ("gen", "deut", "philipp", "1 thess")
    let hits: Vec<usize> = BOOKS.iter().enumerate().filter(|(_, b)| n.len() >= 3 && norm(b.1).starts_with(&n)).map(|(i, _)| i).collect();
    if hits.len() == 1 { Some(hits[0] as u8) } else { None }
}

/// Parse `John 3:16-18`, `jn 3 16`, `Ps 23`, `Ps 23:1,4-6`, `John 1:1-2:3`, `1 Cor 13`.
pub fn parse_reference(input: &str) -> Result<Reference, String> {
    let s = input.trim().replace(['–', '—'], "-");
    if s.is_empty() { return Err("Type a reference, like John 3:16.".into()); }
    // Book = everything up to the first digit that comes after a letter.
    let chars: Vec<char> = s.chars().collect();
    let mut seen_letter = false;
    let mut split = chars.len();
    for (i, c) in chars.iter().enumerate() {
        if c.is_alphabetic() { seen_letter = true; }
        if c.is_ascii_digit() && seen_letter { split = i; break; }
    }
    let book_part: String = chars[..split].iter().collect();
    let nums: String = chars[split..].iter().collect();
    let book = find_book(&book_part).ok_or_else(|| format!("I don't recognize the book \"{}\".", book_part.trim()))?;
    let nums = nums.trim();
    if nums.is_empty() { return Err("Add a chapter, like John 3 or John 3:16.".into()); }
    // "3 16-18" (no colon) means chapter 3, verses 16-18.
    let nums = if !nums.contains(':') {
        let mut it = nums.splitn(2, |c: char| c == ' ' || c == '.');
        let c = it.next().unwrap_or("");
        match it.next() { Some(rest) if !rest.trim().is_empty() => format!("{c}:{}", rest.trim()), _ => c.to_string() }
    } else { nums.to_string() };
    let nums: String = nums.chars().filter(|c| !c.is_whitespace()).collect();
    let bad = || "That reference doesn't look right. Try something like John 3:16-18.".to_string();
    let num = |t: &str| -> Result<u16, String> { t.parse::<u16>().ok().filter(|n| *n > 0).ok_or_else(bad) };
    let mut parts = Vec::new();
    let mut chapter: Option<u16> = None;
    for piece in nums.split([',', ';']).filter(|p| !p.is_empty()) {
        let (a, b) = match piece.split_once('-') { Some((a, b)) => (a, Some(b)), None => (piece, None) };
        // start
        let start = if let Some((c, v)) = a.split_once(':') { let c = num(c)?; chapter = Some(c); (c, Some(num(v)?)) }
            else if let Some(c) = chapter { (c, Some(num(a)?)) }
            else { let c = num(a)?; (c, None) };
        let end = match b {
            None => start,
            Some(b) => if let Some((c, v)) = b.split_once(':') { let c = num(c)?; chapter = Some(c); (c, Some(num(v)?)) }
                else if start.1.is_none() { (num(b)?, None) } // "Ps 23-24" chapters
                else { (start.0, Some(num(b)?)) },
        };
        if (end.0, end.1.unwrap_or(u16::MAX)) < (start.0, start.1.unwrap_or(0)) { return Err("The end of the range comes before the start.".into()); }
        if start.1.is_none() && chapter.is_none() { /* whole chapter(s) */ }
        parts.push(Part { start, end });
    }
    if parts.is_empty() { return Err(bad()); }
    Ok(Reference { book, parts })
}

/// Verses the operator pasted from a Bible they own (each publisher's free quotation allowance, e.g. Holman's
/// 1,000 HCSB verses). Numbers in the text that continue the count — `16 For God… 17 For God…`, `[16]`, `3:16` —
/// start new verses, as does the first verse of the next requested part (`Ps 23:1,4-6`); `1` moves to the next
/// chapter when the reference reaches it. A leading chapter drop-cap (`3 There was a man`) is verse 1.
/// Other numbers are text. Text with no numbers is one slide.
pub fn pasted(input: &str, text: &str) -> Result<(String, Vec<VerseText>), String> {
    let r = parse_reference(input)?;
    let display = display_name(r.book);
    let first = &r.parts[0];
    let first_v = first.start.1.unwrap_or(1);
    let (mut c, mut v) = (first.start.0, first_v);
    let inside = |cv: (u16, u16)| r.parts.iter().any(|p| (p.start.0, p.start.1.unwrap_or(1)) <= cv && cv <= (p.end.0, p.end.1.unwrap_or(u16::MAX)));
    let mut out: Vec<(u16, u16, String)> = Vec::new();
    let mut started = false;
    for word in text.split_whitespace() {
        let n = word.trim_matches(|ch: char| matches!(ch, '[' | ']' | '(' | ')' | '.' | ','));
        let cv = n.split_once(':').and_then(|(a, b)| Some((a.parse::<u16>().ok()?, b.parse::<u16>().ok()?)))
            .filter(|&cv| inside(cv));
        // Start of the next requested part after where we are.
        let part_start = r.parts.iter().map(|p| (p.start.0, p.start.1.unwrap_or(1))).find(|&p| p > (c, v));
        let next = match (cv, n.parse::<u16>()) {
            (Some(cv), _) => Some(cv),
            (None, Ok(x)) if !started => {
                if x == first_v { Some((c, x)) }
                else if x == c && first_v == 1 { Some((c, 1)) } // chapter drop-cap
                else { Some((c, x)).filter(|&cv| inside(cv)) }
            }
            (None, Ok(1)) if inside((c.saturating_add(1), 1)) => Some((c.saturating_add(1), 1)),
            (None, Ok(x)) if Some(x) == v.checked_add(1) => Some((c, x)),
            (None, Ok(x)) => part_start.filter(|p| p.1 == x),
            _ => None,
        };
        match next {
            Some((nc, nv)) if !started || (nc, nv) > (c, v) => { (c, v) = (nc, nv); started = true; out.push((c, v, String::new())); }
            _ => {
                if out.is_empty() { out.push((c, v, String::new())); started = true; }
                let t = &mut out.last_mut().unwrap().2;
                if !t.is_empty() { t.push(' '); }
                t.push_str(word);
            }
        }
    }
    out.retain(|x| !x.2.is_empty());
    if out.is_empty() { return Err("Paste the verse text first.".into()); }
    if out.len() > 400 { return Err("That passage is too long. Choose up to about 400 verses.".into()); }
    let label = canonical(&r, display);
    // One unnumbered block keeps the whole reference as its label.
    let one = out.len() == 1 && !text.split_whitespace().next().is_some_and(|w| w.trim_matches(['[', ']']).chars().all(|ch| ch.is_ascii_digit() || ch == ':'));
    Ok((label.clone(), out.into_iter().map(|(c, v, text)| VerseText { reference: if one { label.clone() } else { format!("{display} {c}:{v}") }, text }).collect()))
}

/// Book name as shown in a reference ("Psalm 23", not "Psalms 23").
pub fn display_name(book: u8) -> &'static str {
    let name = BOOKS[book as usize].1;
    if name == "Psalms" { "Psalm" } else { name }
}

pub fn canonical(r: &Reference, display: &str) -> String {
    let mut out = String::new();
    let mut chapter = 0;
    for (i, p) in r.parts.iter().enumerate() {
        if i > 0 { out.push_str(", "); }
        match (p.start, p.end) {
            ((c, None), (e, None)) => { out.push_str(&c.to_string()); if e != c { out.push_str(&format!("–{e}")); } }
            ((c, Some(v)), (e, Some(w))) => {
                if c != chapter { out.push_str(&format!("{c}:")); chapter = c; }
                out.push_str(&v.to_string());
                if e != c { out.push_str(&format!("–{e}:{w}")); chapter = e; } else if w != v { out.push_str(&format!("–{w}")); }
            }
            ((c, v), (e, w)) => out.push_str(&format!("{c}:{}–{e}:{}", v.unwrap_or(1), w.map(|x| x.to_string()).unwrap_or_default())),
        }
    }
    format!("{display} {out}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "GEN 1:1 In the beginning God created the heaven and the earth.\n\
JOH 3:15 That whosoever believeth in him should not perish, but have eternal life.\n\
JOH 3:16 ¶ For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.\n\
JOH 3:17 For God sent not his Son into the world to condemn the world; but that the world through him might be saved.\n\
JOH 3:18 He that believeth on him is not condemned.\n\
JOH 1:50 x\nJOH 1:51 a\nJOH 2:1 b\nJOH 2:2 c\n\
PSA 23:1 The LORD [is] my shepherd; I shall not want.\nPSA 23:2 b\nPSA 23:3 c\nPSA 23:4 d\nPSA 23:5 e\nPSA 23:6 f\n\
1JO 2:1 a\n1JO 2:2 b\n1JO 2:3 c\nTOB 1:1 apocrypha\n";

    #[test]
    fn parses_common_forms() {
        let k = Kjv::parse(SAMPLE);
        assert_eq!(k.lookup("John 3:16-18").unwrap().1.len(), 3);
        assert_eq!(k.lookup("jn 3 16-18").unwrap().1.len(), 3);
        assert_eq!(k.lookup("Jn. 3:16–18").unwrap().1.len(), 3);
        assert_eq!(k.lookup("Ps 23").unwrap().1.len(), 6);
        let (label, v) = k.lookup("Ps 23:1,4-6").unwrap();
        assert_eq!(v.len(), 4);
        assert_eq!(label, "Psalm 23:1, 4–6");
        assert_eq!(k.lookup("1 John 2:1-3").unwrap().1.len(), 3);
        assert_eq!(k.lookup("1jn 2:1-3").unwrap().1.len(), 3);
        let (label, v) = k.lookup("John 1:51-2:2").unwrap();
        assert_eq!(v.iter().map(|x| x.reference.as_str()).collect::<Vec<_>>(), vec!["John 1:51", "John 2:1", "John 2:2"]);
        assert_eq!(label, "John 1:51–2:2");
    }

    #[test]
    fn rejects_bad_references() {
        let k = Kjv::parse(SAMPLE);
        assert!(k.lookup("John 3:0").is_err());
        assert!(k.lookup("John 3:18-16").is_err());
        assert!(k.lookup("John 3:17-25").is_err());
        assert!(k.lookup("John 99").is_err());
        assert!(k.lookup("Hezekiah 1:1").is_err());
        assert!(k.lookup("Tobit 1:1").is_err());
    }

    #[test]
    fn range_past_the_last_chapter_of_a_short_book_is_an_error_not_a_hang() {
        let err = kjv().lookup("Jude 1:1-3:1").unwrap_err();
        assert_eq!(err, "Jude 2 does not exist.");
    }

    #[test]
    fn cleans_markup() {
        let k = Kjv::parse(SAMPLE);
        assert_eq!(k.lookup("Ps 23:1").unwrap().1[0].text, "The LORD is my shepherd; I shall not want.");
        assert!(k.lookup("John 3:16").unwrap().1[0].text.starts_with("For God"));
    }

    #[test]
    fn book_names() {
        assert_eq!(find_book("Philippians"), Some(49));
        assert_eq!(find_book("phil"), Some(49));
        assert_eq!(find_book("philem"), Some(56));
        assert_eq!(find_book("jude"), Some(64));
        assert_eq!(find_book("judg"), Some(6));
        assert_eq!(find_book("Song of Solomon"), Some(21));
        assert_eq!(find_book("1 Thess"), Some(51));
        assert_eq!(find_book("Revelation"), Some(65));
        assert_eq!(BOOKS.len(), 66);
    }

    #[test]
    fn full_bundled_kjv_has_the_protestant_canon() {
        let k = kjv();
        if k.len() > 1000 { assert_eq!(k.len(), 31102); }
    }

    #[test]
    fn pasted_verses_split_on_their_numbers() {
        let (label, v) = pasted("John 3:16-17", "16 For God loved the world 3 times. 17 For God did not send").unwrap();
        assert_eq!(label, "John 3:16–17");
        assert_eq!(v.iter().map(|x| (x.reference.as_str(), x.text.as_str())).collect::<Vec<_>>(),
            vec![("John 3:16", "For God loved the world 3 times."), ("John 3:17", "For God did not send")]);
        let (_, v) = pasted("John 1:51-2:2", "[51] a [1] b [2] c").unwrap();
        assert_eq!(v.iter().map(|x| x.reference.as_str()).collect::<Vec<_>>(), vec!["John 1:51", "John 2:1", "John 2:2"]);
        let (_, v) = pasted("Ps 23:1-2", "23:1 The LORD is my shepherd, see 3:1. 23:2 He makes me").unwrap();
        assert_eq!(v.iter().map(|x| x.reference.as_str()).collect::<Vec<_>>(), vec!["Psalm 23:1", "Psalm 23:2"]);
        assert_eq!(v[0].text, "The LORD is my shepherd, see 3:1.");
    }

    fn refs(v: &[VerseText]) -> Vec<&str> { v.iter().map(|x| x.reference.as_str()).collect() }

    #[test]
    fn pasted_parts_after_the_first_start_new_verses() {
        let (_, v) = pasted("Ps 23:1,4-6", "1 The LORD 4 Even though 5 You prepare 6 Only goodness").unwrap();
        assert_eq!(refs(&v), vec!["Psalm 23:1", "Psalm 23:4", "Psalm 23:5", "Psalm 23:6"]);
        assert_eq!(v[1].text, "Even though");
        // A number inside a later range that isn't its start stays text.
        let (_, v) = pasted("Ps 23:1,4-6", "1 The LORD fed 5 people 4 Even though").unwrap();
        assert_eq!(refs(&v), vec!["Psalm 23:1", "Psalm 23:4"]);
        assert_eq!(v[0].text, "The LORD fed 5 people");
        let (_, v) = pasted("Ps 23:6, 25:1", "6 Surely 1 To you").unwrap();
        assert_eq!(refs(&v), vec!["Psalm 23:6", "Psalm 25:1"]);
    }

    #[test]
    fn pasted_chapter_drop_cap_is_verse_one() {
        let (_, v) = pasted("John 3:1-2", "3 There was a man 2 This man came").unwrap();
        assert_eq!(refs(&v), vec!["John 3:1", "John 3:2"]);
        assert_eq!(v[0].text, "There was a man");
        let (_, v) = pasted("Psalm 23", "23 The LORD is my shepherd 2 He makes me").unwrap();
        assert_eq!(refs(&v), vec!["Psalm 23:1", "Psalm 23:2"]);
        // A leading number outside the reference is text.
        let (_, v) = pasted("Gen 7:4", "40 days and 40 nights").unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].text, "40 days and 40 nights");
        let (_, v) = pasted("John 3:16-17", "17 For God did not send").unwrap();
        assert_eq!(refs(&v), vec!["John 3:17"]);
    }

    #[test]
    fn pasted_next_chapter_only_inside_the_reference() {
        let (_, v) = pasted("John 3:16-17", "16 For God so loved 1 world 17 For God").unwrap();
        assert_eq!(refs(&v), vec!["John 3:16", "John 3:17"]);
        assert_eq!(v[0].text, "For God so loved 1 world");
        let (_, v) = pasted("John 3:16-17", "16 For God, see 3:20 and 4:1, 17 For God").unwrap();
        assert_eq!(refs(&v), vec!["John 3:16", "John 3:17"]);
        assert_eq!(v[0].text, "For God, see 3:20 and 4:1,");
        let (_, v) = pasted("Ps 23-24", "23 The LORD 2 He makes 1 The earth").unwrap();
        assert_eq!(refs(&v), vec!["Psalm 23:1", "Psalm 23:2", "Psalm 24:1"]);
    }

    #[test]
    fn pasted_text_without_numbers_is_one_slide() {
        let (_, v) = pasted("Romans 8:28", "And we know that all things work together").unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].reference, "Romans 8:28");
        assert!(pasted("Romans 8:28", "   ").is_err());
    }

    #[test]
    fn web_and_bsb_are_built_in() {
        for t in ["WEB", "BSB"] {
            let b = built_in(t).unwrap();
            assert!(b.len() > 31_000, "{t} has {} verses", b.len());
            let (label, v) = b.lookup("John 3:16").unwrap();
            assert_eq!(label, "John 3:16");
            assert!(v[0].text.starts_with("For God so loved the world"), "{t}: {}", v[0].text);
        }
        assert!(built_in("ESV").is_none());
    }
}
