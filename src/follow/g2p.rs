//! Words to the sounds the ear hears. The phoneme model (`wav2vec2-xlsr-53-espeak-cv-ft`) writes speech as
//! espeak-style IPA, so a song's words are turned into the same (English rules plus a table of common words that
//! break them). The letter model (`wav2vec2-base-960h`) writes capital letters, so its words only need folding to
//! A–Z. Words in other scripts have no phonemes here: they keep their letters.
use super::lyrics::latin;

/// Common words the rules get wrong, as espeak (en-us) says them.
const EXCEPTIONS: &[(&str, &str)] = &[
    ("a", "ɐ"), ("above", "ɐbʌv"), ("again", "ɐɡɛn"), ("all", "ɔːl"), ("alleluia", "ælɪluːjə"), ("already", "ɔːlɹɛdi"),
    ("amazing", "ɐmeɪzɪŋ"), ("amen", "ɑːmɛn"), ("an", "æn"), ("and", "ænd"), ("any", "ɛni"), ("are", "ɑːɹ"), ("as", "æz"),
    ("be", "biː"), ("been", "bɪn"), ("begin", "bɪɡɪn"), ("below", "bɪloʊ"), ("break", "bɹeɪk"), ("brother", "bɹʌðɚ"),
    ("by", "baɪ"), ("christ", "kɹaɪst"), ("come", "kʌm"), ("could", "kʊd"), ("do", "duː"), ("does", "dʌz"),
    ("done", "dʌn"), ("earth", "ɜːθ"), ("father", "fɑːðɚ"), ("flow", "floʊ"), ("forgive", "fɚɡɪv"), ("four", "foːɹ"),
    ("from", "fɹʌm"), ("full", "fʊl"), ("get", "ɡɛt"), ("give", "ɡɪv"), ("glory", "ɡloːɹi"), ("go", "ɡoʊ"), ("god", "ɡɑːd"),
    ("great", "ɡɹeɪt"), ("grow", "ɡɹoʊ"), ("hallelujah", "hælɪluːjə"), ("has", "hæz"), ("have", "hæv"), ("he", "hiː"),
    ("heart", "hɑːɹt"), ("heaven", "hɛvən"), ("here", "hɪɹ"), ("his", "hɪz"), ("holy", "hoʊli"), ("how", "haʊ"),
    ("i", "aɪ"), ("into", "ɪntuː"), ("is", "ɪz"), ("jesus", "dʒiːzəs"), ("know", "noʊ"), ("live", "lɪv"), ("lord", "lɔːɹd"),
    ("love", "lʌv"), ("low", "loʊ"), ("many", "mɛni"), ("me", "miː"), ("mother", "mʌðɚ"), ("my", "maɪ"), ("no", "noʊ"),
    ("now", "naʊ"), ("o", "oʊ"), ("of", "ʌv"), ("oh", "oʊ"), ("once", "wʌns"), ("one", "wʌn"), ("only", "oʊnli"),
    ("other", "ʌðɚ"), ("our", "aʊɚ"), ("own", "oʊn"), ("put", "pʊt"), ("said", "sɛd"), ("says", "sɛz"), ("shall", "ʃæl"),
    ("she", "ʃiː"), ("should", "ʃʊd"), ("show", "ʃoʊ"), ("slow", "sloʊ"), ("so", "soʊ"), ("some", "sʌm"), ("son", "sʌn"),
    ("sorrow", "sɑːɹoʊ"), ("spirit", "spɪɹɪt"), ("sure", "ʃʊɹ"), ("than", "ðæn"), ("that", "ðæt"), ("the", "ðə"),
    ("thee", "ðiː"), ("their", "ðɛɹ"), ("them", "ðɛm"), ("then", "ðɛn"), ("there", "ðɛɹ"), ("these", "ðiːz"),
    ("they", "ðeɪ"), ("thine", "ðaɪn"), ("this", "ðɪs"), ("those", "ðoʊz"), ("thou", "ðaʊ"), ("though", "ðoʊ"),
    ("through", "θɹuː"), ("thy", "ðaɪ"), ("to", "tuː"), ("unto", "ʌntuː"), ("was", "wʌz"), ("we", "wiː"), ("were", "wɜː"),
    ("what", "wʌt"), ("where", "wɛɹ"), ("who", "huː"), ("whom", "huːm"), ("whose", "huːz"), ("with", "wɪð"),
    ("won", "wʌn"), ("word", "wɜːd"), ("work", "wɜːk"), ("world", "wɜːld"), ("worship", "wɜːʃɪp"), ("worthy", "wɜːði"),
    ("would", "wʊd"), ("you", "juː"), ("your", "jʊɹ"),
];

/// The word as espeak-style IPA (en-us, no stress marks), e.g. "grace" → "ɡɹeɪs". `None` for a word that isn't
/// written in Latin letters (it keeps its letters, `key`).
pub fn phonemes(word: &str) -> Option<String> {
    let w = latin(word)?;
    let w: String = w.chars().filter(|c| c.is_ascii_lowercase()).collect();
    if w.is_empty() { return None }
    Some(english(&w))
}

/// What the ear compares for a word: its phonemes, or for other scripts its letters.
pub fn key(word: &str) -> String { phonemes(word).unwrap_or_else(|| letters(word)) }

/// The word as the letter model writes it: capitals A–Z (accents dropped). Other scripts keep their own letters.
pub fn letters(word: &str) -> String {
    match latin(word) {
        Some(w) => w.chars().filter(|c| c.is_ascii_lowercase()).map(|c| c.to_ascii_uppercase()).collect(),
        None => word.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect(),
    }
}

fn exception(w: &str) -> Option<&'static str> { EXCEPTIONS.iter().find(|(k, _)| *k == w).map(|(_, v)| *v) }

fn vowel(c: u8) -> bool { matches!(c, b'a' | b'e' | b'i' | b'o' | b'u' | b'y') }

/// One vowel group: a word of one syllable.
fn one_syllable(w: &[u8]) -> bool {
    let mut groups = 0;
    let mut prev = false;
    for &c in w {
        let v = vowel(c);
        if v && !prev { groups += 1 }
        prev = v;
    }
    groups == 1
}

/// The stem of "saving" / "saved": "save" when the stem is one syllable ending in vowel + consonant, "run" for
/// "running".
fn stem(s: &str) -> String {
    let b = s.as_bytes();
    let n = b.len();
    if n >= 2 && b[n - 1] == b[n - 2] && !vowel(b[n - 1]) && !matches!(b[n - 1], b'l' | b's') { return s[..n - 1].to_string() }
    if n >= 2 && one_syllable(b) && vowel(b[n - 2]) && !vowel(b[n - 1]) && !matches!(b[n - 1], b'w' | b'x' | b'y') && !(n >= 3 && vowel(b[n - 3])) {
        return format!("{s}e");
    }
    s.to_string()
}

/// The last sound of `p` (an IPA string).
fn ends_with_any(p: &str, ends: &[&str]) -> bool { ends.iter().any(|e| p.ends_with(e)) }

fn english(w: &str) -> String {
    if let Some(p) = exception(w) { return p.to_string() }
    let n = w.len();
    // Endings: the stem's sounds and the ending's.
    if n > 4 && w.ends_with("ing") {
        return format!("{}ɪŋ", english(&stem(&w[..n - 3])));
    }
    if n > 3 && w.ends_with("ed") && !w.ends_with("eed") {
        let p = english(&stem(&w[..n - 2]));
        let end = if ends_with_any(&p, &["t", "d"]) { "ᵻd" } else if ends_with_any(&p, &["p", "k", "f", "s", "ʃ", "tʃ", "θ"]) { "t" } else { "d" };
        return format!("{p}{end}");
    }
    if n > 3 && w.ends_with('s') && !w.ends_with("ss") && !w.ends_with("us") && !w.ends_with("is") {
        let p = english(&w[..n - 1]);
        let end = if ends_with_any(&p, &["s", "z", "ʃ", "ʒ"]) { "ᵻz" } else if ends_with_any(&p, &["p", "t", "k", "f", "θ"]) { "s" } else { "z" };
        return format!("{p}{end}");
    }
    if n > 4 && w.ends_with("ly") { return format!("{}li", english(&w[..n - 2])) }
    if n > 5 && w.ends_with("ness") { return format!("{}nəs", english(&w[..n - 4])) }
    if n > 4 && w.ends_with("ful") { return format!("{}fəl", english(&w[..n - 3])) }
    rules(w.as_bytes())
}

/// Letter-to-sound rules for one stem.
fn rules(b: &[u8]) -> String {
    let n = b.len();
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    let rest = |i: usize, s: &str| b[i..].starts_with(s.as_bytes());
    // A final "e" after a consonant is silent (not in "be", handled as an exception) and makes the vowel before
    // it long ("grace", "shine").
    let silent_e = n > 2 && b[n - 1] == b'e' && !vowel(b[n - 2]) && b[..n - 1].iter().any(|&c| vowel(c));
    let magic = |i: usize| silent_e && i + 2 == n - 1 && !vowel(at(i + 1));
    let last_vowel = (0..n).rev().find(|&i| vowel(b[i]) && !(silent_e && i == n - 1));
    let mut out = String::new();
    let mut i = 0;
    while i < n {
        let c = b[i];
        let (n1, n2) = (at(i + 1), at(i + 2));
        let mut step = 1;
        let p: &str = match c {
            // Several letters at once, longest first.
            _ if rest(i, "eigh") => { step = 4; "eɪ" }
            _ if rest(i, "igh") => { step = 3; "aɪ" }
            _ if rest(i, "ough") || rest(i, "augh") => { step = 4; "ɔː" }
            _ if rest(i, "tion") => { step = 4; "ʃən" }
            _ if rest(i, "sion") => { step = 4; "ʒən" }
            _ if rest(i, "tious") || rest(i, "cious") => { step = 5; "ʃəs" }
            _ if rest(i, "ture") => { step = 4; "tʃɚ" }
            _ if rest(i, "ous") && i + 3 == n => { step = 3; "əs" }
            _ if rest(i, "tch") => { step = 3; "tʃ" }
            _ if rest(i, "ther") && i > 0 => { step = 4; "ðɚ" }
            _ if rest(i, "air") => { step = 3; "ɛɹ" }
            _ if rest(i, "are") && i + 3 == n => { step = 3; "ɛɹ" }
            _ if rest(i, "ear") || rest(i, "eer") => { step = 3; "ɪɹ" }
            _ if rest(i, "ere") && i + 3 == n => { step = 3; "ɪɹ" }
            _ if rest(i, "ire") && i + 3 == n => { step = 3; "aɪɚ" }
            _ if rest(i, "ore") && i + 3 == n => { step = 3; "ɔːɹ" }
            _ if rest(i, "our") => { step = 3; "aʊɚ" }
            _ if rest(i, "ure") && i + 3 == n => { step = 3; "ʊɹ" }
            _ if rest(i, "le") && i + 2 == n && i > 0 && !vowel(b[i - 1]) => { step = 2; "əl" }
            b'a' if n1 == b'r' && !vowel(n2) => { step = 2; "ɑːɹ" }
            b'o' if n1 == b'r' && !vowel(n2) => { step = 2; "ɔːɹ" }
            b'e' | b'i' | b'u' if n1 == b'r' && !vowel(n2) => {
                step = 2;
                // Unstressed at the end ("never", "savior"); stressed "her", "bird", "turn".
                if i + 2 == n && i > 0 && !one_syllable(b) { "ɚ" } else { "ɜː" }
            }
            b'a' if matches!(n1, b'i' | b'y') => { step = 2; "eɪ" }
            b'e' if matches!(n1, b'i' | b'y') => { step = 2; "eɪ" }
            b'e' if matches!(n1, b'e' | b'a') => { step = 2; "iː" }
            b'i' if n1 == b'e' => { step = 2; if i + 2 == n { "aɪ" } else { "iː" } }
            b'o' if matches!(n1, b'a') || (n1 == b'e' && i + 2 == n) => { step = 2; "oʊ" }
            b'o' if n1 == b'o' => { step = 2; "uː" }
            b'o' if n1 == b'u' => { step = 2; "aʊ" }
            b'o' if n1 == b'w' => {
                step = 2;
                if i + 2 == n && i > 0 && matches!(b[i - 1], b'l' | b'r') { "oʊ" } else { "aʊ" }
            }
            b'o' if matches!(n1, b'i' | b'y') => { step = 2; "ɔɪ" }
            b'a' if matches!(n1, b'u' | b'w') => { step = 2; "ɔː" }
            b'e' if n1 == b'w' => { step = 2; "uː" }
            b'u' if matches!(n1, b'e' | b'i') && !magic(i) => { step = 2; "uː" }
            // One vowel.
            b'a' if magic(i) => "eɪ",
            b'e' if magic(i) => "iː",
            b'i' if magic(i) => "aɪ",
            b'o' if magic(i) => "oʊ",
            b'u' if magic(i) => "uː",
            b'y' if magic(i) => "aɪ",
            b'e' if silent_e && i == n - 1 => "",
            b'a' if i == 0 && n > 3 && !vowel(n1) && vowel(n2) => "ɐ",
            b'a' if n1 == b'l' && matches!(n2, b'l' | b'k') => "ɔː",
            b'a' if i + 1 == n => "ə",
            b'a' if Some(i) != last_vowel && !vowel(n1) && vowel(n2) && i > 0 => "eɪ",
            b'a' => "æ",
            b'e' if i + 1 == n && n <= 2 => "iː",
            b'e' => "ɛ",
            b'i' if (rest(i, "ind") || rest(i, "ild")) && i + 3 == n => "aɪ",
            b'i' => "ɪ",
            b'o' if i + 1 == n => "oʊ",
            b'o' if rest(i, "old") || rest(i, "ost") && i + 3 == n && i > 0 && b[i - 1] == b'm' => "oʊ",
            b'o' if Some(i) != last_vowel && !vowel(n1) && n1 != 0 && vowel(n2) => "oʊ",
            b'o' => "ɑː",
            b'u' if matches!(n1, b'l' | b's') && matches!(n2, b'l' | b'h') => "ʊ",
            b'u' if i > 0 && b[i - 1] == b'q' => "",
            b'u' => "ʌ",
            b'y' if i == 0 && vowel(n1) => "j",
            b'y' if i + 1 == n => if b[..i].iter().any(|&c| vowel(c)) { "i" } else { "aɪ" },
            b'y' => "ɪ",
            // Consonants.
            b'c' if n1 == b'k' => { step = 2; "k" }
            b'c' if n1 == b'h' && n2 == b'r' => { step = 2; "k" }
            b'c' if n1 == b'h' => { step = 2; "tʃ" }
            b'c' if matches!(n1, b'e' | b'i' | b'y') => "s",
            b'c' => "k",
            b's' if n1 == b'h' => { step = 2; "ʃ" }
            b's' if n1 == b'c' && matches!(n2, b'e' | b'i' | b'y') => { step = 2; "s" }
            b's' if n1 == b's' => { step = 2; "s" }
            b's' if i > 0 && vowel(b[i - 1]) && (vowel(n1) || i + 1 == n) => "z",
            b't' if n1 == b'h' => { step = 2; "θ" }
            b'p' if n1 == b'h' => { step = 2; "f" }
            b'w' if n1 == b'h' => { step = 2; "w" }
            b'w' if n1 == b'r' && i == 0 => { step = 2; "ɹ" }
            b'k' if n1 == b'n' && i == 0 => { step = 2; "n" }
            b'g' if n1 == b'n' && i == 0 => { step = 2; "n" }
            b'g' if n1 == b'h' => { step = 2; if i == 0 { "ɡ" } else { "" } }
            b'g' if n1 == b'g' => { step = 2; "ɡ" }
            b'g' if matches!(n1, b'e' | b'i' | b'y') => "dʒ",
            b'g' => "ɡ",
            b'n' if n1 == b'g' => { step = 2; "ŋ" }
            b'n' if n1 == b'k' => "ŋ",
            b'q' => "kw",
            b'x' if i == 0 => "z",
            b'x' => "ks",
            b'j' => "dʒ",
            b'r' => "ɹ",
            b'd' if n1 == b'g' && matches!(n2, b'e' | b'i' | b'y') => { step = 2; "dʒ" }
            b'm' if n1 == b'b' && i + 2 == n => { step = 2; "m" }
            b'h' if vowel(n1) && n1 != b'y' => "h",
            b'h' => "",
            b'w' if vowel(n1) => "w",
            b'w' => "",
            c if !vowel(c) && n1 == c => { step = 2; std::str::from_utf8(std::slice::from_ref(&b[i])).unwrap_or("") }
            _ => std::str::from_utf8(std::slice::from_ref(&b[i])).unwrap_or(""),
        };
        out.push_str(p);
        i += step;
    }
    out
}

/// The sounds as the ear compares them: stress and length marks dropped, and sounds that singers (and the
/// model) mix up folded together ("ɐ"/"ʌ"/"ə", "ɑ"/"ɔ", "ɪ"/"i", "ɾ"/"t").
pub fn coarse(ipa: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(ipa.len());
    for c in ipa.chars() {
        match c {
            'ː' | 'ˈ' | 'ˌ' | '.' | '\u{329}' | '\u{32a}' | 'ʰ' | '^' | 'ʔ' | '-' | ' ' => {}
            c if c.is_ascii_digit() => {}
            'ɐ' | 'ʌ' | 'ᵻ' | 'ɜ' | 'ə' => out.push('ə'),
            'ɚ' => { out.push('ə'); out.push('ɹ') }
            'ɾ' => out.push('t'),
            'ɒ' | 'ɔ' | 'a' | 'ɑ' => out.push('ɑ'),
            'ɪ' | 'i' => out.push('i'),
            'ʊ' | 'u' => out.push('u'),
            'ɛ' | 'æ' => out.push('e'),
            'r' | 'ɻ' => out.push('ɹ'),
            'g' => out.push('ɡ'),
            'ɫ' => out.push('l'),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
#[path = "g2p_tests.rs"]
mod tests;
