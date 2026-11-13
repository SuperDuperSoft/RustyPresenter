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
fn every_exception_has_two_sounds_or_is_a() {
    // A word of one sound is in every other word, so the phoneme ear leaves it out (`Lexicon::set_song`): only
    // "a" may be one. And a word listed twice would have its second way ignored.
    for (i, (word, ipa)) in EXCEPTIONS.iter().enumerate() {
        assert!(coarse(ipa).len() >= 2 || *word == "a", "{word}");
        assert!(EXCEPTIONS[..i].iter().all(|(w, _)| w != word), "{word} is listed twice");
    }
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
