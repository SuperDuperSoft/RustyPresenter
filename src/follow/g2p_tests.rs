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
