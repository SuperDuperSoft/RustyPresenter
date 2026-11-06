//! Translations that never appear in RustyPresenter: the New World Translation, the Joseph Smith Translation and
//! The Passion Translation (Bible library spec §4). Every way a Bible or a passage gets in comes through here.
//!
//! Language codes and id prefixes are never matched, because eBible's `tptNT` is the Tlachichilco Tepehua New
//! Testament, and ISO 639-3 `tpt` is no relation of The Passion Translation.

use std::sync::LazyLock;

use icu_normalizer::DecomposingNormalizerBorrowed;

use crate::model::{Data, ItemKind, VerseText};

/// Blocked whenever the abbreviation is exactly one of these, `nwt` followed by digits too.
const ABBREVIATIONS: &[&str] = &["nwt", "nwtr", "nwtsty", "rbi8", "bi12", "jst", "hsiv", "tpt"];
/// Also real abbreviations of other things, so these block only in an online catalog, where nobody chose them.
const CATALOG_ABBREVIATIONS: &[&str] = &["iv", "tnm", "tmn", "nwu"];

/// One line per language, written as the Bible writes it. A title or a notice that contains one of these is blocked.
const TITLES: &[&str] = &[
    // New World Translation
    "New World Translation", "Traducción del Nuevo Mundo", "Tradução do Novo Mundo", "Traduction du monde nouveau",
    "Neue-Welt-Übersetzung", "Traduzione del Nuovo Mondo", "Nieuwe-Wereldvertaling", "Przekład Nowego Świata",
    "Ny Verden-oversættelsen", "Ny verden-oversettelsen", "Nya världen-översättningen", "Uuden maailman käännös",
    "Перевод нового мира", "신세계역", "新世界訳", "新世界译本", "新世界譯本", "Nuwe-Wêreld-vertaling",
    "Переклад нового світу", "Μετάφραση Νέου Κόσμου", "Traducerea Lumii Noi", "Překlad nového světa",
    "Preklad nového sveta", "Új Világ fordítás", "Yeni Dünya Çevirisi", "Terjemahan Dunia Baru",
    "Bagong Sanlibutang Salin", "Tafsiri ya Ulimwengu Mpya", "Bản dịch Thế Giới Mới", "ترجمة العالم الجديد",
    "नयी दुनिया अनुवाद", "न्यू वर्ल्ड ट्रांसलेशन",
    // Joseph Smith Translation
    "Joseph Smith Translation", "Joseph Smith's New Translation", "Inspired Version", "Restoration Edition", "Restoration Scriptures",
    // The Passion Translation, and the books it first came out as
    "Passion Translation", "La Pasión Traducción", "Traducción de La Pasión", "Letters from Heaven", "Poetry on Fire",
];

/// Publishers and owners: the strongest signal, since the NWT exists in over 200 languages and no title list is whole.
const NOTICES: &[&str] = &[
    "Watch Tower", "Watchtower", "Wachtturm", "Torre de Vigía", "Tour de Garde", "jw.org", "Jehovah's Witnesses",
    "Testigos de Jehová", "Témoins de Jéhovah", "Zeugen Jehovas", "Testemunhas de Jeová",
    "Herald Publishing House", "Community of Christ", "Reorganized Church of Jesus Christ of Latter Day Saints",
    "Passion & Fire Ministries", "Passion and Fire Ministries",
];

/// Catalog ids taken from public listings.
const IDS: &[&str] = &["yv:1849"];

/// John 1:1 as only these translations read it.
const NWT_JOHN_1_1: &[&str] = &[
    "the Word was a god", "la Palabra era un dios", "a Palavra era um deus", "la Parole était un dieu",
    "das Wort war ein Gott", "la Parola era un dio", "het Woord was een god",
];
const TPT_JOHN_1_1: &[&str] = &["Living Expression", "gospel preached through the Son"];
/// The name as the NWT prints it. "jehova" also finds "Jehovah", "Jehová" and "Jéhovah".
const JEHOVAH: &[&str] = &["jehova", "jeova", "jehowa"];

const GENESIS: u8 = 0;
const PSALMS: u8 = 18;
const MATTHEW: u8 = 39;
const LUKE: u8 = 41;
const JOHN: u8 = 42;
const REVELATION: u8 = 65;

/// Written once for every language, so that no pattern is ever folded by hand.
fn normal_list(list: &[&str]) -> Vec<String> { list.iter().map(|s| normalise(s)).collect() }
static NORMAL_TITLES: LazyLock<Vec<String>> = LazyLock::new(|| normal_list(TITLES));
static NORMAL_NOTICES: LazyLock<Vec<String>> = LazyLock::new(|| normal_list(NOTICES));
static NORMAL_NWT: LazyLock<Vec<String>> = LazyLock::new(|| normal_list(NWT_JOHN_1_1));
static NORMAL_TPT: LazyLock<Vec<String>> = LazyLock::new(|| normal_list(TPT_JOHN_1_1));
static NORMAL_JEHOVAH: LazyLock<Vec<String>> = LazyLock::new(|| normal_list(JEHOVAH));

/// Lowercase, no accents, no spaces or punctuation, so "NewWorld Translation", "Neue-Welt-Uebersetzung" and
/// "Przekład" all compare as the Bible's own publisher would mean them.
pub fn normalise(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut folded = String::with_capacity(lower.len());
    for c in DecomposingNormalizerBorrowed::new_nfkd().normalize(&lower).chars() {
        match c {
            'ł' => folded.push('l'),
            'ø' => folded.push('o'),
            'æ' => folded.push_str("ae"),
            'œ' => folded.push_str("oe"),
            'đ' | 'ð' => folded.push('d'),
            'ß' => folded.push_str("ss"),
            'ı' => folded.push('i'),
            'þ' => folded.push_str("th"),
            'ħ' => folded.push('h'),
            c => folded.push(c),
        }
    }
    // `is_alphanumeric` also takes out the marks NFKD just split off.
    folded.replace("ae", "a").replace("oe", "o").replace("ue", "u").chars().filter(|c| c.is_alphanumeric()).collect()
}

fn any_in(text: &str, patterns: &[String]) -> bool { patterns.iter().any(|p| text.contains(p.as_str())) }

fn abbreviation_hit(abbr: &str) -> bool {
    let a = normalise(abbr);
    let numbered = a.strip_prefix("nwt").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    numbered || ABBREVIATIONS.contains(&a.as_str())
}

fn title_hit(text: &str) -> bool {
    // "The Compassion Translation" is someone else's; the bare word "passion" never blocks.
    any_in(&normalise(text).replace("compassion", ""), &NORMAL_TITLES)
}

fn notice_hit(text: &str) -> bool {
    let n = normalise(text);
    any_in(&n, &NORMAL_NOTICES) || (n.contains("broadstreet") && n.contains("briansimmons"))
}

fn named(titles: &[&str], notices: &[&str], ids: &[&str]) -> bool {
    titles.iter().any(|t| title_hit(t))
        || notices.iter().any(|n| title_hit(n) || notice_hit(n))
        || ids.iter().any(|id| IDS.contains(id))
}

/// A translation that may not be used. `notices` is anything its source says about it: copyright, publisher,
/// description.
pub fn blocked(abbr: &str, titles: &[&str], notices: &[&str], ids: &[&str]) -> bool {
    abbreviation_hit(abbr) || named(titles, notices, ids)
}

/// `blocked`, for a row of a catalog that gives each Bible's language. An abbreviation that is only the row's own
/// language code says nothing about the Bible.
pub fn blocked_row(lang: &str, abbr: &str, titles: &[&str], notices: &[&str], ids: &[&str]) -> bool {
    (abbreviation_hit(abbr) && normalise(abbr) != normalise(lang)) || named(titles, notices, ids)
}

/// `blocked`, for an online catalog: nobody typed these abbreviations, so the ones with other meanings count too.
pub fn blocked_online(abbr: &str, titles: &[&str], notices: &[&str], ids: &[&str]) -> bool {
    blocked(abbr, titles, notices, ids) || CATALOG_ABBREVIATIONS.contains(&normalise(abbr).as_str())
}

/// One verse that only a blocked translation reads this way. For text that arrives a passage at a time.
pub fn blocked_verse(book: u8, chapter: u16, verse: u16, text: &str) -> bool {
    let n = || normalise(text);
    match (book, chapter, verse) {
        (JOHN, 1, 1) => { let t = n(); any_in(&t, &NORMAL_NWT) || any_in(&t, &NORMAL_TPT) }
        (MATTHEW, 4, 10) | (LUKE, 4, 8) => any_in(&n(), &NORMAL_JEHOVAH),
        (PSALMS, 23, 1) => n().contains("mybestfriend"),
        (MATTHEW, 7, 1) => n().contains("judgenotunrighteously"),
        _ => false,
    }
}

/// A file of many verses that names nothing, or names itself something else. For the NWT both signs must be there,
/// so that one verse does not condemn a Bible; for the others either of their signs is enough.
pub fn blocked_text<'a>(verses: impl IntoIterator<Item = ((u8, u16, u16), &'a str)>) -> bool {
    const JEHOVAH_VERSES: usize = 20;
    let (mut john, mut jehovah, mut genesis_50) = (false, 0, 0);
    for ((book, chapter, verse), text) in verses {
        match (book, chapter, verse) {
            (JOHN, 1, 1) => {
                let t = normalise(text);
                if any_in(&t, &NORMAL_TPT) { return true }
                john = any_in(&t, &NORMAL_NWT);
            }
            (PSALMS, 23, 1) | (MATTHEW, 7, 1) => if blocked_verse(book, chapter, verse, text) { return true },
            (GENESIS, 50, v) => genesis_50 = genesis_50.max(v),
            _ => {}
        }
        if (MATTHEW..=REVELATION).contains(&book) && any_in(&normalise(text), &NORMAL_JEHOVAH) { jehovah += 1 }
    }
    genesis_50 > 30 || (john && jehovah >= JEHOVAH_VERSES)
}

/// `(book, chapter, verse)` of a verse as stored on a slide. A verse with no readable reference has none of the
/// signs that depend on where it is.
fn place(v: &VerseText) -> Option<(u8, u16, u16)> {
    let r = super::parse_reference(&v.reference).ok()?;
    let start = r.parts.first()?.start;
    Some((r.book, start.0, start.1?))
}

/// A passage that came from outside `lookup`: pasted, sent to the action API, or read from a saved service.
pub fn blocked_passage(translation: &str, verses: &[VerseText]) -> bool {
    blocked(translation, &[], &[], &[translation])
        || verses.iter().any(|v| place(v).is_some_and(|(b, c, n)| blocked_verse(b, c, n, &v.text)))
}

/// Takes out every passage that may not be used, from every saved service, and such Bibles from the picker's
/// favourites and recent list. How many passages went.
pub fn strip(data: &mut Data) -> usize {
    let id = |t: &String| !blocked(t, &[], &[], &[t]);
    data.settings.favourites.retain(id);
    data.settings.recent.retain(id);
    data.services.iter_mut().map(|s| strip_items(&mut s.items)).sum()
}

/// `strip` for one list of service items.
pub fn strip_items(items: &mut Vec<crate::model::ServiceItem>) -> usize {
    let before = items.len();
    items.retain(|i| !matches!(&i.kind, ItemKind::Scripture { translation, verses, .. } if blocked_passage(translation, verses)));
    before - items.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bible::BOOKS;

    fn title(t: &str) -> bool { blocked("", &[t], &[], &[]) }
    fn notice(t: &str) -> bool { blocked("", &[], &[t], &[]) }
    fn verse(reference: &str, text: &str) -> VerseText { VerseText::new(reference, text) }

    #[test]
    fn book_numbers_are_the_ones_in_the_book_list() {
        for (index, name) in [(GENESIS, "Genesis"), (PSALMS, "Psalms"), (MATTHEW, "Matthew"), (LUKE, "Luke"), (JOHN, "John"), (REVELATION, "Revelation")] {
            assert_eq!(BOOKS[index as usize].1, name);
        }
    }

    #[test]
    fn folding_makes_the_written_forms_one_string() {
        assert_eq!(normalise("NewWorld Translation"), normalise("New World Translation"));
        assert_eq!(normalise("Neue-Welt-Übersetzung"), normalise("Neue-Welt-Uebersetzung"));
        assert_eq!(normalise("Przekład"), "przeklad");
        assert_eq!(normalise("Straße"), "strasse");
        assert_eq!(normalise("ÓØ þ"), "ooth");
        assert_eq!(normalise("Ｊｏｈｎ"), "john", "NFKD folds full-width letters");
    }

    #[test]
    fn abbreviations_block_only_as_the_whole_value() {
        for a in ["NWT", "nwt", "N.W.T.", "NWT2013", "nwtsty", "RBI8", "JST", "TPT", "HSIV"] { assert!(blocked(a, &[], &[], &[]), "{a}"); }
        for a in ["KJV", "WEB", "NWTX", "TPTNT", "JSTU", ""] { assert!(!blocked(a, &[], &[], &[]), "{a}"); }
    }

    #[test]
    fn catalog_abbreviations_count_only_in_online_catalogs() {
        for a in ["IV", "TNM", "TMN", "NWU", "NWÜ", "nwue"] {
            assert!(!blocked(a, &[], &[], &[]), "{a}");
            assert!(blocked_online(a, &[], &[], &[]), "{a}");
        }
    }

    #[test]
    fn every_listed_title_is_blocked_as_written() {
        for t in TITLES { assert!(title(t), "{t}"); }
        for t in NOTICES { assert!(notice(t), "{t}"); }
    }

    #[test]
    fn real_titles_in_their_own_scripts() {
        for t in [
            "New World Translation of the Holy Scriptures", "NEW WORLD TRANSLATION", "NewWorld Translation",
            "La Biblia. Traducción del Nuevo Mundo", "Przekład Nowego Świata", "Ny Verden-oversættelsen", "신세계역 성경",
            "新世界譯本", "Новый мир: Перевод нового мира", "Neue-Welt-Uebersetzung", "Tradução do Novo Mundo da Bíblia Sagrada",
            "The Passion Translation", "TPT: The Passion Translation New Testament", "The Holy Scriptures, Inspired Version",
        ] { assert!(title(t), "{t}"); }
    }

    #[test]
    fn the_word_passion_alone_never_blocks() {
        for t in ["The Passion of the Christ", "Passion Week Readings", "The Compassion Translation", "King James Version", "Reina-Valera 1909",
            "World English Bible", "New International Version", "Nuevo Testamento", "Berean Standard Bible", "New Living Translation"] {
            assert!(!title(t), "{t}");
        }
    }

    #[test]
    fn notices_block_on_the_publisher() {
        assert!(notice("© 2013 Watch Tower Bible and Tract Society of Pennsylvania"));
        assert!(notice("Copyright Wachtturm Bibel- und Traktat-Gesellschaft"));
        assert!(notice("Published by Herald Publishing House"));
        assert!(notice("Copyright © 2020 Passion & Fire Ministries, Inc."));
        assert!(notice("BroadStreet Publishing, translated by Brian Simmons"));
        assert!(!notice("BroadStreet Publishing"), "they publish other books");
        assert!(!notice("Brian Simmons"));
        assert!(!notice("© 2017 Holman Bible Publishers"));
    }

    #[test]
    fn known_ids_block_and_nothing_else_about_an_id_does() {
        assert!(blocked("", &[], &[], &["yv:1849"]));
        assert!(!blocked("", &[], &[], &["yv:111", "ebible:tptNT", "tptNT"]));
    }

    #[test]
    fn tepehua_is_a_language_and_stays() {
        assert!(!blocked_row("tpt", "TPT", &["Tlachichilco Tepehua New Testament"], &["© SIL"], &["ebible:tptNT"]));
        assert!(!blocked_row("tpt", "NT", &["Tepehua"], &[], &["tptNT"]));
        assert!(blocked_row("eng", "TPT", &["Some Bible"], &[], &[]), "the real abbreviation in another language");
        assert!(blocked_row("tpt", "TPT", &["The Passion Translation"], &[], &[]), "a title still blocks");
    }

    #[test]
    fn single_verses() {
        assert!(blocked_verse(JOHN, 1, 1, "In the beginning was the Word, and the Word was with God, and the Word was a god."));
        assert!(blocked_verse(JOHN, 1, 1, "En el principio era la Palabra, y la Palabra estaba con Dios, y la Palabra era un dios."));
        assert!(blocked_verse(JOHN, 1, 1, "In the very beginning the Living Expression was already there."));
        assert!(blocked_verse(MATTHEW, 4, 10, "It is Jehovah your God you must worship."));
        assert!(blocked_verse(LUKE, 4, 8, "Es a Jehová tu Dios a quien tienes que adorar."));
        assert!(blocked_verse(PSALMS, 23, 1, "The Lord is my best friend and my shepherd."));
        assert!(blocked_verse(MATTHEW, 7, 1, "Judge not unrighteously, that ye be not judged."));
    }

    #[test]
    fn a_sign_counts_only_in_its_own_verse() {
        assert!(!blocked_verse(JOHN, 1, 1, "In the beginning was the Word, and the Word was with God, and the Word was God."));
        assert!(!blocked_verse(MATTHEW, 4, 10, "Thou shalt worship the Lord thy God."));
        assert!(!blocked_verse(GENESIS, 22, 14, "And Abraham called the name of that place Jehovahjireh."));
        assert!(!blocked_verse(PSALMS, 22, 1, "my best friend"));
    }

    fn fixture(lines: &[((u8, u16, u16), &'static str)]) -> Vec<((u8, u16, u16), &'static str)> { lines.to_vec() }

    #[test]
    fn a_whole_file_needs_both_nwt_signs() {
        const WORD: &str = "the Word was a god";
        let jehovah: Vec<_> = (1..=20).map(|v| ((40u8, 1u16, v as u16), "Jehovah said")).collect();
        let mut both = jehovah.clone();
        both.push(((JOHN, 1, 1), WORD));
        assert!(blocked_text(both.iter().copied()));
        assert!(!blocked_text(jehovah[1..].iter().copied().chain([((JOHN, 1, 1), WORD)])), "19 verses are not 20");
        assert!(!blocked_text(jehovah.iter().copied()));
        assert!(!blocked_text([((JOHN, 1, 1), WORD)]));
    }

    #[test]
    fn jehovah_in_the_old_testament_does_not_count() {
        let ot: Vec<_> = (1..=30).map(|v| ((1u8, 3u16, v as u16), "Jehovah said")).collect();
        assert!(!blocked_text(ot.iter().copied().chain([((JOHN, 1, 1), "the Word was a god")])), "the ASV prints Jehovah");
    }

    #[test]
    fn a_whole_file_with_one_sign_of_the_others() {
        assert!(blocked_text(fixture(&[((PSALMS, 23, 1), "The Lord is my best friend")])));
        assert!(blocked_text(fixture(&[((JOHN, 1, 1), "the Living Expression was there")])));
        assert!(blocked_text(fixture(&[((MATTHEW, 7, 1), "Judge not unrighteously")])));
        assert!(blocked_text(fixture(&[((GENESIS, 50, 31), "And Joseph said")])), "Joseph Smith's added prophecy");
        assert!(!blocked_text(fixture(&[((GENESIS, 50, 26), "So Joseph died")])));
    }

    #[test]
    fn the_bundled_bibles_are_not_fingerprinted() {
        for name in ["KJV", "WEB", "BSB"] {
            let b = crate::bible::built_in(name).unwrap();
            assert!(!blocked_text(b.verses.iter().map(|(k, t)| (*k, t.as_str()))), "{name}");
            assert!(!blocked(name, &[], &[], &[name]), "{name}");
        }
    }

    #[test]
    fn a_passage_from_outside_lookup() {
        assert!(blocked_passage("TPT", &[]));
        assert!(blocked_passage("yv:1849", &[]));
        assert!(blocked_passage("ABC", &[verse("John 1:1", "and the Word was a god.")]), "under another name");
        assert!(!blocked_passage("KJV", &[verse("John 1:1", "and the Word was God.")]));
        assert!(!blocked_passage("ABC", &[verse("nonsense", "and the Word was a god.")]), "a verse with no place has no sign");
    }

    #[test]
    fn strip_takes_only_blocked_passages() {
        use crate::model::{Service, ServiceItem};
        let item = |translation: &str, verses: Vec<VerseText>| {
            let kind = ItemKind::Scripture { reference: "John 1:1".into(), translation: translation.into(), verses, per_slide: 0, text_size: 0, also: vec![], layout: Default::default() };
            ServiceItem { id: uuid::Uuid::new_v4(), kind, background: None, notes: String::new() }
        };
        let mut s = Service::new("Sunday");
        s.items = vec![item("KJV", vec![verse("John 1:1", "and the Word was God.")]), item("TPT", vec![]), item("NWT", vec![])];
        let mut d = Data { services: vec![s], ..Data::default() };
        assert_eq!(strip(&mut d), 2);
        assert_eq!(d.services[0].items.len(), 1);
        assert_eq!(strip(&mut d), 0);
    }
}
