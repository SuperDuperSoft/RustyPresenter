//! A song's live slides as a list of words, each with a sound-alike key, syllables, slide and line.
use crate::model::{Slide, SlideContent};

/// One sung word.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    /// Sound-alike key (`sound_key`), so "grace" and "grays" match.
    pub key: String,
    pub syllables: u8,
    pub slide: usize,
    /// The line of the slide (counting lines with words only).
    pub line: usize,
}

/// Where a slide's words are in `SongText::words`.
#[derive(Clone, Debug, PartialEq)]
pub struct SlideSpan {
    pub slide: usize,
    pub first_word: usize,
    pub last_line_first_word: usize,
    /// One past the slide's last word.
    pub end_word: usize,
    pub section: Option<String>,
    /// Nothing to sing here (an `{Instrumental}` slide, a picture, an empty slide).
    pub instrumental: bool,
}

impl SlideSpan {
    pub fn words(&self) -> std::ops::Range<usize> { self.first_word..self.end_word }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SongText { pub words: Vec<Word>, pub slides: Vec<SlideSpan> }

impl SongText {
    /// The live item's slides, in the order they are played.
    pub fn from_slides(slides: &[Slide]) -> SongText {
        let mut song = SongText::default();
        for (i, slide) in slides.iter().enumerate() {
            let first_word = song.words.len();
            let mut last_line_first_word = first_word;
            let text = match &slide.content { SlideContent::Text { text, .. } => text.as_str(), _ => "" };
            let mut line = 0;
            for l in text.lines() {
                if crate::sections::label_line(l).is_some() { continue }
                let words = split_words(l);
                if words.is_empty() { continue }
                last_line_first_word = song.words.len();
                for w in words {
                    song.words.push(Word { key: sound_key(&w), syllables: syllables(&w), text: w, slide: i, line });
                }
                line += 1;
            }
            let end_word = song.words.len();
            song.slides.push(SlideSpan {
                slide: i, first_word, last_line_first_word, end_word,
                section: slide.section.as_ref().map(|s| s.label.clone()),
                instrumental: end_word == first_word,
            });
        }
        song
    }

    /// The words of `slide`'s line that holds word `word`, as written.
    pub fn line_text(&self, word: usize) -> Option<String> {
        let w = self.words.get(word)?;
        let words: Vec<&str> = self.words.iter().filter(|x| x.slide == w.slide && x.line == w.line).map(|x| x.text.as_str()).collect();
        Some(words.join(" "))
    }
}

/// A Han or kana character: those scripts write no spaces, so each one is a word of its own.
fn one_char_word(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

/// Repeat marks such as "x2", "2x", "(repeat)" are not sung.
fn is_mark(w: &str) -> bool {
    let l = w.to_lowercase();
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    l.strip_prefix('x').is_some_and(digits) || l.strip_suffix('x').is_some_and(digits) || l == "repeat"
}

fn split_words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in line.split(|c: char| c.is_whitespace() || matches!(c, '-' | '–' | '—' | '/')) {
        let mut cur = String::new();
        let push = |cur: &mut String, out: &mut Vec<String>| {
            if !cur.is_empty() && !is_mark(cur) { out.push(cur.clone()) }
            cur.clear();
        };
        for c in raw.chars() {
            if one_char_word(c) {
                push(&mut cur, &mut out);
                out.push(c.to_string());
            } else if c.is_alphanumeric() {
                cur.push(c);
            } else if matches!(c, '\'' | '’' | 'ʼ') && !cur.is_empty() {
                // Kept inside a word ("don't"); dropped from the key.
                cur.push('\'');
            }
        }
        while cur.ends_with('\'') { cur.pop(); }
        push(&mut cur, &mut out);
    }
    out
}

/// A Latin letter without its accent; `None` for anything else.
fn fold(c: char) -> Option<&'static str> {
    Some(match c {
        'a'..='z' => return None,
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'č' => "c",
        'ď' | 'đ' | 'ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
        'ğ' => "g",
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i",
        'ł' | 'ľ' => "l",
        'ñ' | 'ń' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
        'œ' => "oe",
        'ř' => "r",
        'ś' | 'š' | 'ş' => "s",
        'ß' => "ss",
        'ť' | 'ţ' => "t",
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => "u",
        'ý' | 'ÿ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        'þ' => "th",
        _ => return None,
    })
}

/// The word in plain lowercase a–z and digits, or `None` when it has letters of another script.
fn latin(word: &str) -> Option<String> {
    let mut s = String::new();
    for c in word.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() { s.push(c) }
        else if let Some(f) = fold(c) { s.push_str(f) }
        else if c.is_alphanumeric() { return None }
    }
    Some(s)
}

fn vowel(c: u8) -> bool { matches!(c, b'a' | b'e' | b'i' | b'o' | b'u' | b'y') }

/// A sound-alike key (a small Metaphone): words that sound the same get the same key ("grace" / "grays",
/// "night" / "knight"). Vowel runs become one `A`, silent letters go. Words in other scripts keep their lowercase form.
pub fn sound_key(word: &str) -> String {
    let Some(w) = latin(word) else { return word.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect() };
    let mut b: &[u8] = w.as_bytes();
    // Silent first letters.
    for (from, skip) in [("kn", 1), ("gn", 1), ("pn", 1), ("wr", 1), ("ps", 1)] {
        if w.starts_with(from) { b = &b[skip..]; break }
    }
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    let mut out: Vec<u8> = Vec::new();
    let emit = |out: &mut Vec<u8>, c: u8| { if out.last() != Some(&c) { out.push(c) } };
    let mut i = 0;
    while i < b.len() {
        let (c, n) = (b[i], at(i + 1));
        let mut step = 1;
        match c {
            // "y" before a vowel ("yes") is a consonant.
            b'y' if vowel(n) && n != b'y' => emit(&mut out, b'Y'),
            c if vowel(c) => emit(&mut out, b'A'),
            b'b' => if !(i + 1 == b.len() && i > 0 && b[i - 1] == b'm') { emit(&mut out, b'B') },
            b'c' => match n {
                b'e' | b'i' | b'y' => emit(&mut out, b'S'),
                b'h' => { emit(&mut out, b'X'); step = 2 }
                _ => emit(&mut out, b'K'),
            },
            b'd' => if n == b'g' && matches!(at(i + 2), b'e' | b'i' | b'y') { emit(&mut out, b'J'); step = 2 } else { emit(&mut out, b'T') },
            b'g' => match n {
                b'h' if i == 0 => { emit(&mut out, b'K'); step = 2 }
                b'h' => step = 2,
                b'n' if i + 2 == b.len() => {}
                b'e' | b'i' | b'y' => emit(&mut out, b'J'),
                _ => emit(&mut out, b'K'),
            },
            b'h' => if (i == 0 || vowel(b[i - 1])) && vowel(n) { emit(&mut out, b'H') },
            b'p' => if n == b'h' { emit(&mut out, b'F'); step = 2 } else { emit(&mut out, b'P') },
            b'q' => emit(&mut out, b'K'),
            b's' => if n == b'h' { emit(&mut out, b'X'); step = 2 }
                else if n == b'c' && at(i + 2) == b'h' { emit(&mut out, b'S'); emit(&mut out, b'K'); step = 3 }
                else if n == b'i' && matches!(at(i + 2), b'o' | b'a') { emit(&mut out, b'X'); step = 2 }
                else { emit(&mut out, b'S') },
            b't' => if n == b'h' { emit(&mut out, b'0'); step = 2 }
                else if n == b'c' && at(i + 2) == b'h' {}
                else if n == b'i' && matches!(at(i + 2), b'o' | b'a') { emit(&mut out, b'X'); step = 2 }
                else { emit(&mut out, b'T') },
            b'v' => emit(&mut out, b'F'),
            b'w' => if vowel(n) { emit(&mut out, b'W') },
            b'x' => if i == 0 { emit(&mut out, b'S') } else { emit(&mut out, b'K'); emit(&mut out, b'S') },
            b'z' => emit(&mut out, b'S'),
            c => emit(&mut out, c.to_ascii_uppercase()),
        }
        i += step;
    }
    // A silent final "e" ("grace", "love"), not in short words ("be") or after a vowel ("free").
    if b.len() > 3 && b.ends_with(b"e") && !vowel(b[b.len() - 2]) && out.last() == Some(&b'A') { out.pop(); }
    String::from_utf8(out).unwrap_or_default()
}

/// About how many syllables the word is sung on (vowel groups, less a silent final "e"); at least 1.
pub fn syllables(word: &str) -> u8 {
    let Some(w) = latin(word) else {
        let n = word.chars().filter(|c| c.is_alphanumeric()).count();
        return if word.chars().any(one_char_word) { n.max(1) as u8 } else { n.div_ceil(3).clamp(1, 255) as u8 };
    };
    let b = w.as_bytes();
    let mut groups = 0usize;
    let mut prev = false;
    for (i, &c) in b.iter().enumerate() {
        let v = vowel(c);
        if v && !prev { groups += 1 }
        // "glorious", "radiant": an "i" before another vowel starts a syllable of its own ("nation", "special" don't).
        if v && prev && c != b'e' && b[i - 1] == b'i' && !(i >= 2 && matches!(b[i - 2], b't' | b's' | b'c' | b'g')) { groups += 1 }
        prev = v;
    }
    // "grace", "love": the final "e" is silent; "the", "be", "able" keep theirs.
    let n = b.len();
    if groups > 1 && n > 2 && b[n - 1] == b'e' && !vowel(b[n - 2]) && !(b[n - 2] == b'l' && !vowel(b[n - 3])) { groups -= 1 }
    groups.clamp(1, 255) as u8
}

#[cfg(test)]
#[path = "lyrics_tests.rs"]
mod tests;
