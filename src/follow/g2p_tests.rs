use super::*;

#[test]
fn english_words_to_espeak_phonemes() {
    for (word, ipa) in [
        ("grace", "ɡɹeɪs"), ("Grace", "ɡɹeɪs"), ("sweet", "swiːt"), ("sound", "saʊnd"), ("night", "naɪt"), ("shine", "ʃaɪn"),
        ("king", "kɪŋ"), ("praise", "pɹeɪz"), ("saved", "seɪvd"), ("singing", "sɪŋɪŋ"), ("wretch", "ɹɛtʃ"), ("toils", "tɔɪlz"),
        ("snares", "snɛɹz"), ("blind", "blaɪnd"), ("found", "faʊnd"), ("see", "siː"), ("the", "ðə"), ("amazing", "ɐmeɪzɪŋ"),
        ("graces", "ɡɹeɪsᵻz"),
    ] {
        assert_eq!(phonemes(word).as_deref(), Some(ipa), "{word}");
    }
}

#[test]
fn worship_words() {
    // Words the rules alone get wrong; plurals and apostrophe words come through the same table.
    for (word, ipa) in [
        ("emmanuel", "ɪmænjuːɛl"), ("Yahweh", "jɑːweɪ"), ("messiah", "məsaɪə"), ("Zion", "zaɪən"), ("Israel", "ɪzɹeɪəl"),
        ("saviour", "seɪvjɚ"), ("blood", "blʌd"), ("died", "daɪd"), ("forever", "fɚɹɛvɚ"), ("angels", "eɪndʒəlz"),
        ("eyes", "aɪz"), ("I'm", "aɪm"), ("don't", "doʊnt"), ("he's", "hiːz"),
    ] {
        assert_eq!(phonemes(word).as_deref(), Some(ipa), "{word}");
    }
}

#[test]
fn common_words_the_rules_built_wrong() {
    // Each of these was two sounds or more from how it is said (or one, in a word too short to spare one), so
    // the phoneme ear never heard it: "weɪtɚ", "twoʊ", "ʌz", "ɡoʊn", "hɪɹd", "boʊdi", "pɹoʊmaɪz", "toʊdʒɛðɚ", "ɔːeɪ".
    for (word, ipa) in [
        ("water", "wɔːɾɚ"), ("two", "tuː"), ("us", "ʌs"), ("gone", "ɡɔːn"), ("heard", "hɜːd"), ("body", "bɑːdi"),
        ("promise", "pɹɑːmɪs"), ("together", "təɡɛðɚ"), ("away", "ɐweɪ"), ("because", "bɪkʌz"), ("every", "ɛvɹi"),
        ("young", "jʌŋ"), ("truth", "tɹuːθ"), ("given", "ɡɪvən"),
        // Through the same table: plurals, "-ed", "-ing", "-ful".
        ("waters", "wɔːɾɚz"), ("promises", "pɹɑːmɪsᵻz"), ("walked", "wɔːkt"), ("moved", "muːvd"), ("covered", "kʌvɚd"),
        ("used", "juːzd"), ("searching", "sɜːtʃɪŋ"), ("wonderful", "wʌndɚfəl"), ("friends", "fɹɛndz"),
        // And by the rules: "-ore", a soft "c" before "-ing" and "-ed", "-fy".
        ("more", "moːɹ"), ("adore", "ɐdoːɹ"), ("dancing", "dænsɪŋ"), ("danced", "dænst"), ("rejoicing", "ɹɛdʒɔɪsɪŋ"),
        ("glorify", "ɡloʊɹɪfaɪ"), ("deeply", "diːpli"),
    ] {
        assert_eq!(phonemes(word).as_deref(), Some(ipa), "{word}");
    }
}

#[test]
fn every_exception_has_two_sounds_or_is_a() {
    // A word of one sound is in every other word, so the phoneme ear leaves it out (`Lexicon::set_song`): only
    // "a" may be one. (The sung "Ah"s of one sound have their own table, `LONE`.) And a word listed twice would
    // have its second way ignored.
    for (i, (word, ipa)) in EXCEPTIONS.iter().enumerate() {
        assert!(coarse(ipa).len() >= 2 || *word == "a", "{word}");
        assert!(EXCEPTIONS[..i].iter().all(|(w, _)| w != word), "{word} is listed twice");
    }
}

#[test]
fn more_apostrophe_words() {
    // "You're" was built by the rules as "jaʊɚ", two sounds away from any way it is sung: never heard.
    for (word, ipa) in [
        ("You're", "jʊɹ"), ("you’re", "jʊɹ"), ("they're", "ðɛɹ"), ("doesn't", "dʌzənt"), ("wasn't", "wʌzənt"),
    ] {
        assert_eq!(phonemes(word).as_deref(), Some(ipa), "{word}");
    }
    // "You're" and "your" are heard as the same sounds.
    assert_eq!(phonemes("you're"), phonemes("your"));
}

#[test]
fn one_sound_words_the_ear_listens_for() {
    // Two sounds or more: always. One full vowel (a sung "Ooh", "Ah", "awe"): yes. The neutral "a" and "uh", a
    // lone consonant and no sound at all: no.
    for (word, yes) in [
        ("grace", true), ("oh", true), ("ohh", true), ("ooh", true), ("Ah", true), ("ahh", true), ("awe", true),
        ("a", false), ("uh", false), ("mm", false),
    ] {
        assert_eq!(variants(word).iter().any(|p| listened(&coarse(p))), yes, "{word}");
    }
    assert!(!listened(&[]));
    assert!(!listened(&coarse("ə")) && !listened(&coarse("m")) && listened(&coarse("uː")) && listened(&coarse("ðə")));
    // The sung syllables the rules get wrong: one full vowel each, and not in the main table too.
    for (word, ipa) in LONE {
        assert!(coarse(ipa).len() == 1 && listened(&coarse(ipa)), "{word}");
        assert!(EXCEPTIONS.iter().all(|(w, _)| w != word), "{word} is listed twice");
        assert_eq!(phonemes(word).as_deref(), Some(*ipa), "{word}");
    }
    assert_eq!(phonemes("ooh").as_deref(), Some("uː"));
    assert_eq!(phonemes("Ah").as_deref(), Some("ɑː"));
    assert_eq!(phonemes("ohh").as_deref(), Some("oʊ"));
}

#[test]
fn variants_first_is_the_plain_one() {
    assert_eq!(variants("grace"), ["ɡɹeɪs"]);
    assert_eq!(variants("blessed"), ["blɛst", "blɛsɪd"]);
    assert_eq!(variants("Blessed,"), ["blɛst", "blɛsɪd"]);
    assert!(variants("恩典").is_empty());
    // The sung ways are for long words only: a short one would be heard everywhere.
    for (word, ways) in SUNG {
        assert_eq!(variants(word).len(), 1 + ways.len(), "{word}");
        for way in ways.iter() { assert!(coarse(way).len() >= 4, "{word} {way}") }
    }
}

#[test]
fn other_scripts_keep_their_letters() {
    assert_eq!(phonemes("恩典"), None);
    assert_eq!(key("恩典"), "恩典");
    assert_eq!(key("grace"), "ɡɹeɪs");
    assert_eq!(phonemes("¡"), None);
}

#[test]
fn letters_for_the_letter_model() {
    assert_eq!(letters("Grace,"), "GRACE");
    assert_eq!(letters("don't"), "DONT");
    assert_eq!(letters("Jesús"), "JESUS");
    assert_eq!(letters("Слава"), "слава");
}

#[test]
fn coarse_folds_sounds_singers_mix_up() {
    assert_eq!(coarse("ɡɹeɪs"), coarse("ɡɹˈeɪs"));
    assert_eq!(coarse("ðə"), coarse("ðʌ"));
    assert_eq!(coarse("lɔst"), coarse("lɑːst"));
    assert_eq!(coarse("nɛvɚ"), "nevəɹ".chars().collect::<Vec<_>>());
}
