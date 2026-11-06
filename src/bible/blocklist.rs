//! Translations RustyPresenter never offers or shows (Bible library design §4): the New World Translation (any
//! language), the Joseph Smith Translation / Inspired Version and The Passion Translation. Every catalog, typed id,
//! paste, import and fetched passage goes through here; a hit is refused with `strings::BLOCKED`.
//! Patterns are written as their sources spell them and normalised in code, so no pattern is ever folded by hand.
use icu_normalizer::DecomposingNormalizerBorrowed;
use std::sync::LazyLock;

/// Lowercase, NFKD with combining marks dropped, letters with no decomposition folded (`ł` → `l`, `ß` → `ss`…),
/// everything but letters and digits removed, then `ae`/`oe`/`ue` → `a`/`o`/`u` so "Übersetzung" and
/// "Uebersetzung" are one string.
pub fn normalise(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    for c in DecomposingNormalizerBorrowed::new_nfkd().normalize(&lower).chars() {
        if is_mark(c) { continue; }
        match c {
            'ł' => out.push('l'), 'ø' => out.push('o'), 'æ' => out.push_str("ae"), 'œ' => out.push_str("oe"),
            'đ' | 'ð' => out.push('d'), 'ß' => out.push_str("ss"), 'ı' => out.push('i'), 'þ' => out.push_str("th"),
            'ħ' => out.push('h'),
            c if c.is_alphanumeric() => out.push(c),
            _ => {}
        }
    }
    out.replace("ae", "a").replace("oe", "o").replace("ue", "u")
}

/// Combining marks NFKD leaves behind (Latin, Greek, Cyrillic accents; Hebrew points; Arabic vowel signs).
fn is_mark(c: char) -> bool {
    matches!(c, '\u{0300}'..='\u{036F}' | '\u{0483}'..='\u{0489}' | '\u{0591}'..='\u{05C7}' | '\u{064B}'..='\u{065F}'
        | '\u{0670}' | '\u{1AB0}'..='\u{1AFF}' | '\u{1DC0}'..='\u{1DFF}' | '\u{20D0}'..='\u{20FF}' | '\u{FE20}'..='\u{FE2F}')
}

/// Titles, in the spelling each language uses. A title *containing* one of these is blocked.
pub const TITLES: &[&str] = &[
    // New World Translation
    "New World Translation", "Traducción del Nuevo Mundo", "Tradução do Novo Mundo", "Traduction du monde nouveau",
    "Neue-Welt-Übersetzung", "Traduzione del Nuovo Mondo", "Nieuwe-Wereldvertaling", "Nuwe Wêreld-vertaling",
    "Przekład Nowego Świata", "Ny Verden-oversættelsen", "Ny verden-oversettelsen", "Nya världens översättning",
    "Nya världen-översättningen", "Uuden maailman käännös", "Перевод нового мира", "Переклад нового світу",
    "Pereklad novoho svitu", "Μετάφραση Νέου Κόσμου", "Metafrasi Neou Kosmou", "Traducerea Lumii Noi",
    "Překlad nového světa", "Preklad nového sveta", "Új világ fordítás", "Yeni Dünya Çevirisi", "Terjemahan Dunia Baru",
    "Bagong Sanlibutang Salin", "Tafsiri ya Ulimwengu Mpya", "Bản dịch Thế Giới Mới", "ترجمة العالم الجديد",
    "नई दुनिया अनुवाद", "신세계역", "新世界訳", "新世界译本", "新世界譯本",
    // Joseph Smith Translation / Inspired Version
    "Joseph Smith Translation", "Inspired Version", "Joseph Smith's New Translation", "Restoration Edition",
    "Restoration Scriptures",
    // The Passion Translation, and the titles of its single books
    "Passion Translation", "La Pasión Traducción", "Traducción de La Pasión", "Letters from Heaven", "Poetry on Fire",
];

/// Copyright, publisher and description lines: the strongest signal, since the NWT exists in 200+ languages.
pub const NOTICES: &[&str] = &[
    "Watch Tower", "Watchtower", "Wachtturm", "Torre de Vigía", "Tour de Garde", "jw.org", "Jehovah's Witnesses",
    "Testigos de Jehová", "Témoins de Jéhovah", "Zeugen Jehovas", "Testemunhas de Jeová",
    "Herald Publishing House", "Community of Christ", "Reorganized Church of Jesus Christ of Latter Day Saints",
    "Passion & Fire Ministries", "Passion and Fire Ministries",
];

/// Notices that block only together ("BroadStreet" alone publishes other books).
const NOTICE_PAIRS: &[(&str, &str)] = &[("BroadStreet", "Brian Simmons")];

/// Whole abbreviations (normalised). `nwt` also blocks with digits after it (NWT13).
const ABBREVIATIONS: &[&str] = &["nwt", "nwtr", "nwtsty", "rbi8", "bi12", "tnm", "tmn", "nwu", "nwü", "jst", "iv", "hsiv", "tpt"];

/// Translation ids known to be blocked. `yv:1849` is The Passion Translation on YouVersion (public version listing;
/// to check against the live list with a key).
pub const IDS: &[&str] = &["yv:1849"];

struct Patterns { titles: Vec<String>, notices: Vec<String>, pairs: Vec<(String, String)>, abbrs: Vec<String> }

static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| Patterns {
    titles: TITLES.iter().map(|t| normalise(t)).collect(),
    notices: NOTICES.iter().map(|t| normalise(t)).collect(),
    pairs: NOTICE_PAIRS.iter().map(|(a, b)| (normalise(a), normalise(b))).collect(),
    abbrs: ABBREVIATIONS.iter().map(|t| normalise(t)).collect(),
});

fn abbr_hit(abbr: &str) -> bool {
    let a = normalise(abbr);
    if a.is_empty() { return false; }
    PATTERNS.abbrs.contains(&a) || a.strip_prefix("nwt").is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
}

fn text_hit(titles: &[&str], notices: &[&str], ids: &[&str]) -> bool {
    let p = &*PATTERNS;
    if ids.iter().any(|id| IDS.iter().any(|b| b.eq_ignore_ascii_case(id.trim()))) { return true; }
    if titles.iter().chain(notices).map(|t| normalise(t)).any(|t| p.titles.iter().any(|x| t.contains(x.as_str()))) { return true; }
    let notices: Vec<String> = notices.iter().chain(titles).map(|t| normalise(t)).collect();
    notices.iter().any(|n| p.notices.iter().any(|x| n.contains(x.as_str())))
        || p.pairs.iter().any(|(a, b)| notices.iter().any(|n| n.contains(a.as_str())) && notices.iter().any(|n| n.contains(b.as_str())))
}

/// True for a blocked translation: by abbreviation (whole value), title, copyright/publisher notice or known id.
pub fn blocked(abbr: &str, titles: &[&str], notices: &[&str], ids: &[&str]) -> bool {
    abbr_hit(abbr) || text_hit(titles, notices, ids)
}

/// `blocked` for a catalog row in a language: an abbreviation that is only the language's own code (eBible `tptNT`,
/// a Tlachichilco Tepehua "TPT", ISO 639-3 `tpt`) does not block on its own.
pub fn blocked_row(lang: &str, abbr: &str, titles: &[&str], notices: &[&str], ids: &[&str]) -> bool {
    let same_as_lang = !lang.trim().is_empty() && normalise(lang.split(['-', '_']).next().unwrap_or("")) == normalise(abbr);
    (abbr_hit(abbr) && !same_as_lang) || text_hit(titles, notices, ids)
}

/// A typed translation id or abbreviation (Settings, paste, an item from the action API).
pub fn blocked_id(id: &str) -> bool {
    let id = id.trim();
    if IDS.iter().any(|b| b.eq_ignore_ascii_case(id)) { return true; }
    // Prefixed ids are opaque numbers or hashes: only the known-ids table applies.
    !id.contains(':') && abbr_hit(id)
}

/// Content fingerprints, for text that arrives with no title: one distinctive verse is enough.
pub fn blocked_verse(book: u8, chapter: u16, verse: u16, text: &str) -> bool {
    static PRINTS: LazyLock<Vec<((u8, u16, u16), String)>> = LazyLock::new(|| [
        ((42, 1, 1), "the Word was a god"),         // NWT
        ((42, 1, 1), "Living Expression"),          // TPT
        ((42, 1, 1), "gospel preached through the Son"), // JST
        ((39, 4, 10), "Jehovah"),                   // NWT
        ((41, 4, 8), "Jehovah"),                    // NWT
        ((18, 23, 1), "my best friend"),            // TPT
        ((39, 7, 1), "judge not unrighteously"),    // JST
    ].into_iter().map(|(k, t)| (k, normalise(t))).collect());
    let mut prints = PRINTS.iter().filter(|(k, _)| *k == (book, chapter, verse)).peekable();
    if prints.peek().is_none() { return false; }
    let t = normalise(text);
    prints.any(|(_, p)| t.contains(p.as_str()))
}

/// The fingerprint verses of a whole Bible (an import, a file on disk), read through `get`.
pub fn blocked_text<'a>(get: impl Fn(u8, u16, u16) -> Option<&'a str>) -> bool {
    [(42, 1, 1), (39, 4, 10), (41, 4, 8), (18, 23, 1), (39, 7, 1)].into_iter()
        .any(|(b, c, v)| get(b, c, v).is_some_and(|t| blocked_verse(b, c, v, t)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_matches_its_own_spelling() {
        for t in TITLES { assert!(blocked("", &[t], &[], &[]), "title {t}"); }
        for n in NOTICES { assert!(blocked("", &[], &[n], &[]), "notice {n}"); }
        for a in ABBREVIATIONS { assert!(blocked(a, &[], &[], &[]), "abbreviation {a}"); }
        for id in IDS { assert!(blocked("", &[], &[], &[id]) && blocked_id(id), "id {id}"); }
    }

    #[test]
    fn native_titles_in_longer_names_and_other_spellings() {
        for t in [
            "Les Saintes Écritures. Traduction du monde nouveau", "Traducción del Nuevo Mundo de las Santas Escrituras",
            "Tradução do Novo Mundo das Escrituras Sagradas", "NEUE-WELT-UEBERSETZUNG der Heiligen Schrift",
            "Neue Welt Übersetzung", "Biblia — Przekład Nowego Świata", "Ny Verden-oversættelsen af De Hellige Skrifter",
            "Nuwewêreldvertaling", "신세계역 성경", "聖經新世界譯本", "Святе Письмо — Переклад нового світу",
            "Uuden maailman käännös", "New World Translation of the Holy Scriptures (Study Edition)", "NewWorld Translation",
            "The Holy Scriptures — Inspired Version", "Joseph Smith’s New Translation of the Bible",
            "The Passion Translation New Testament", "Psalms: Poetry on Fire", "Letters from Heaven by the Apostle Paul",
            "Bản Dịch Thế Giới Mới",
        ] { assert!(blocked("", &[t], &[], &[]), "{t}"); }
    }

    #[test]
    fn notices_block_whatever_the_title() {
        assert!(blocked("SB", &["Святое Писание"], &["© 2007 Watch Tower Bible and Tract Society of Pennsylvania"], &[]));
        assert!(blocked("", &["Biblia"], &["Testigos de Jehová"], &[]));
        assert!(blocked("", &["John: Eternal Love"], &["© 2017 Passion & Fire Ministries, Inc."], &[]));
        assert!(blocked("", &["Psalms"], &["BroadStreet Publishing Group", "Translated by Brian Simmons"], &[]));
        assert!(!blocked("", &["Psalms"], &["BroadStreet Publishing Group"], &[]), "BroadStreet alone publishes other books");
        assert!(!blocked("", &["Passion week readings"], &[], &[]), "the bare word passion never blocks");
    }

    #[test]
    fn abbreviation_variants() {
        for a in ["NWT", "nwt13", "NWT-R", "nwtsty", "TNM", "TMN", "NWÜ", "nwue", "JST", "IV", "HSIV", "TPT", " tpt "] {
            assert!(blocked(a, &[], &[], &[]), "{a}");
        }
        for a in ["NIV", "KJV", "RVR1960", "NWTX", "IVB", "JSTx", "tptNT", "TPTE", "NT"] { assert!(!blocked(a, &[], &[], &[]), "{a}"); }
        assert!(blocked_id("tpt") && blocked_id("NWT") && !blocked_id("yv:1") && !blocked_id("ab:tpt") && !blocked_id("KJV"));
    }

    #[test]
    fn tepehua_tpt_stays_allowed() {
        assert!(!blocked("tptNT", &["Tlachichilco Tepehua New Testament"], &[], &["ebible:tptNT"]));
        assert!(!blocked_row("tpt", "TPT", &["Nuevo Testamento en tepehua de Tlachichilco"], &[], &[]));
        assert!(blocked_row("en", "TPT", &["Some Bible"], &[], &[]), "TPT in English is still blocked");
        assert!(blocked_row("tpt", "TPT", &["The Passion Translation"], &[], &[]), "a title hit still blocks");
    }

    #[test]
    fn ordinary_bibles_are_allowed() {
        for (a, t, n) in [
            ("KJV", "King James Version", "Public domain"), ("NIV", "New International Version", "Biblica, Inc."),
            ("RVR1960", "Reina-Valera 1960", "Sociedades Bíblicas en América Latina"), ("LUT", "Lutherbibel 2017", "Deutsche Bibelgesellschaft"),
            ("NWB", "Nowa Biblia Gdańska", ""), ("NBV21", "Nieuwe Bijbelvertaling", "Nederlands Bijbelgenootschap"),
            ("NTV", "Nueva Traducción Viviente", "Tyndale House Foundation"), ("CUV", "新標點和合本", ""),
            ("RSV", "Revised Standard Version", "National Council of Churches"), ("NET", "New English Translation", "Biblical Studies Press"),
        ] { assert!(!blocked(a, &[t], &[n], &[]), "{a}"); }
    }

    #[test]
    fn folds_letters_without_decompositions() {
        assert_eq!(normalise("Przekład"), "przeklad");
        assert_eq!(normalise("Straße"), "strasse");
        assert_eq!(normalise("Ø Đ ı ħ þ"), "odihth");
        assert_eq!(normalise("Æsir"), normalise("Asir"));
        assert_eq!(normalise("Świata"), "swiata");
        assert_eq!(normalise("Über Uebersetzung"), normalise("UberUbersetzung"));
    }

    #[test]
    fn fingerprints() {
        assert!(blocked_verse(42, 1, 1, "In the beginning was the Word, and the Word was with God, and the Word was a god."));
        assert!(blocked_verse(39, 4, 10, "It is written: ‘It is Jehovah your God you must worship.’"));
        assert!(blocked_verse(18, 23, 1, "The Lord is my best friend and my shepherd."));
        assert!(!blocked_verse(42, 1, 1, "In the beginning was the Word, and the Word was with God, and the Word was God."));
        assert!(!blocked_verse(1, 22, 14, "And Abraham called the name of that place Jehovahjireh"), "KJV place names are fine");
        let kjv = crate::bible::kjv();
        if kjv.len() > 1000 {
            for r in ["John 1:1", "Matthew 4:10", "Luke 4:8", "Psalm 23:1", "Matthew 7:1"] {
                for b in ["KJV", "WEB", "BSB"] {
                    let (_, v) = crate::bible::built_in(b).unwrap().lookup(r).unwrap();
                    let p = crate::bible::parse_reference(r).unwrap();
                    let (c, n) = (p.parts[0].start.0, p.parts[0].start.1.unwrap());
                    assert!(!blocked_verse(p.book, c, n, &v[0].text), "{b} {r}");
                }
            }
        }
    }
}
