use super::*;
use crate::follow::g2p;
use crate::follow::testing::{SONG, text};

/// `wav2vec2-base-960h`'s tokens.
pub(crate) fn letters_vocab() -> Vec<String> {
    ["<pad>", "<s>", "</s>", "<unk>", "|", "E", "T", "A", "O", "N", "I", "H", "S", "R", "D", "L", "U", "M", "W", "C", "F", "G", "Y", "P", "B", "V",
        "K", "'", "X", "J", "Q", "Z"].map(String::from).into()
}

/// A stand-in model: each 20 ms frame's first sample is the token it "hears" (×100).
struct Fake { vocab: Vec<String>, units: Units, calls: Arc<Mutex<usize>> }

impl Acoustic for Fake {
    fn log_probs(&self, pcm: &[f32]) -> Result<Vec<Vec<f32>>, String> {
        *self.calls.lock().unwrap() += 1;
        let n = (pcm.len() - 400) / 320 + 1;
        Ok((0..n).map(|i| {
            let id = (pcm[i * 320] * 32768.0 / 100.0).round() as usize;
            (0..self.vocab.len()).map(|v| if v == id { -0.1 } else { -5.0 }).collect()
        }).collect())
    }
    fn vocab(&self) -> &[String] { &self.vocab }
    fn units(&self) -> Units { self.units }
    fn blank(&self) -> usize { 0 }
}

/// Sound that makes `Fake` hear `tokens`, each for `frames` frames, then blanks to `secs` seconds.
fn say(vocab: &[String], tokens: &[&str], frames: usize, secs: f64) -> Vec<i16> {
    let mut pcm = vec![];
    for t in tokens {
        let id = vocab.iter().position(|v| v == t).unwrap_or_else(|| panic!("{t}"));
        for _ in 0..frames { pcm.extend(std::iter::repeat_n((id * 100) as i16, 320)) }
    }
    pcm.resize((secs * 16000.0) as usize, 0);
    pcm
}

fn spell(word: &str) -> Vec<String> { word.chars().map(|c| c.to_string()).collect() }

/// The phoneme ear's words for a song with these slides.
fn lexicon_of(slides: &[&str]) -> Lexicon {
    let lex = Lexicon::default();
    lex.set_song(&SongText::from_slides(&slides.iter().map(|t| text(t)).collect::<Vec<_>>()));
    lex
}

fn keys(heard: &[Heard]) -> Vec<String> { heard.iter().map(|h| h.key.clone()).collect() }

/// The keys the phoneme ear hears in `sounds` (IPA, no pauses), to the end, for a song with these slides.
fn spotted(slides: &[&str], sounds: &str) -> Vec<String> {
    let mut spot = Spotter { lexicon: lexicon_of(slides), buf: vec![] };
    let mut heard = vec![];
    for (i, c) in coarse(sounds).into_iter().enumerate() { heard.extend(spot.push(c, i as f64 * 0.1, 0.9)) }
    heard.extend(spot.flush());
    keys(&heard)
}

/// A phoneme decoder for a one-slide song, fed one 20 ms frame at a time.
struct Hears { d: Decoder, vocab: Vec<String>, t: f64 }

impl Hears {
    /// Knows the words of `slide`; the model can write `sounds` (and "<pad>", the blank).
    fn new(sounds: &[&str], slide: &str) -> Hears {
        let mut vocab = vec!["<pad>".to_string()];
        vocab.extend(sounds.iter().map(|s| s.to_string()));
        Hears { d: Decoder::new(Units::Phonemes, vocab.clone(), 0, lexicon_of(&[slide])), vocab, t: 0.0 }
    }

    /// One frame for each of `tokens`; the words heard.
    fn rows(&mut self, tokens: &[&str]) -> Vec<Heard> {
        let mut out = vec![];
        for tok in tokens {
            let id = self.vocab.iter().position(|v| v == tok).unwrap_or_else(|| panic!("{tok}"));
            let row: Vec<f32> = (0..self.vocab.len()).map(|i| if i == id { -0.2 } else { -4.0 }).collect();
            out.extend(self.d.frame(&row, self.t));
            self.t += 0.02;
        }
        out
    }

    /// `n` frames of nothing new.
    fn blanks(&mut self, n: usize) -> Vec<Heard> { self.rows(&vec!["<pad>"; n]) }
}

#[test]
fn letters_collapse_into_words() {
    let v = letters_vocab();
    let mut d = Decoder::new(Units::Letters, v.clone(), 0, Lexicon::default());
    let row = |t: &str| -> Vec<f32> { let id = v.iter().position(|x| x == t).unwrap(); (0..v.len()).map(|i| if i == id { -0.2 } else { -4.0 }).collect() };
    let mut heard = vec![];
    // A repeated frame is one letter; "|" and a pause end a word.
    for (i, t) in ["G", "R", "R", "A", "<pad>", "C", "E", "|", "N", "<pad>", "I", "G", "H", "T"].iter().enumerate() {
        heard.extend(d.frame(&row(t), i as f64 * 0.02));
    }
    for i in 0..PAUSE_FRAMES { heard.extend(d.frame(&row("<pad>"), 1.0 + i as f64 * 0.02)) }
    let keys: Vec<&str> = heard.iter().map(|h| h.key.as_str()).collect();
    assert_eq!(keys, [sound_key("grace"), sound_key("night")]);
    assert!((heard[0].at - 0.12).abs() < 1e-9 && heard[0].conf > 0.7);
}

#[test]
fn phonemes_find_the_songs_words() {
    let song = SongText::from_slides(&SONG.iter().map(|t| text(t)).collect::<Vec<_>>());
    let lex = Lexicon::default();
    lex.set_song(&song);
    let mut spot = Spotter { lexicon: lex.clone(), buf: vec![] };
    // Sung a little off: "ɐmeɪzɪn ɡɹeɪs haʊ swiːt" ("amazin'"), with a stray sound before "how".
    let mut heard = vec![];
    let sung = format!("{}{}k{}{}", "ɐmeɪzɪn", g2p::phonemes("grace").unwrap(), g2p::phonemes("how").unwrap(), g2p::phonemes("sweet").unwrap());
    for (i, c) in coarse(&sung).into_iter().enumerate() { heard.extend(spot.push(c, i as f64 * 0.1, 0.9)) }
    heard.extend(spot.flush());
    let keys: Vec<String> = heard.iter().map(|h| h.key.clone()).collect();
    assert_eq!(keys, ["amazing", "grace", "how", "sweet"].map(sound_key));
    assert!(heard.windows(2).all(|w| w[0].at < w[1].at));
    // Without a song, the groups of sounds between pauses.
    let mut spot = Spotter { lexicon: Lexicon::default(), buf: vec![] };
    for c in coarse("ɡɹeɪs") { assert!(spot.push(c, 0.0, 1.0).is_empty()) }
    assert_eq!(spot.flush().into_iter().map(|h| h.key).collect::<Vec<_>>(), [coarse("ɡɹeɪs").into_iter().collect::<String>()]);
}

#[test]
fn lexicon_skips_one_sound_words() {
    // "a" is one sound: it is in every other word, so it is not listened for.
    let words = lexicon_of(&["a grace"]).words();
    assert_eq!(words.len(), 1, "{words:?}");
    assert_eq!(words[0].key, sound_key("grace"));
}

#[test]
fn a_is_not_heard_in_every_schwa() {
    let mut spot = Spotter { lexicon: lexicon_of(&["saved a wretch"]), buf: vec![] };
    let mut heard = vec![];
    // Stray sounds with a schwa at the front, again and again: none of them is "a".
    for (i, c) in coarse("əkəpə").into_iter().enumerate() { heard.extend(spot.push(c, i as f64 * 0.1, 0.9)) }
    heard.extend(spot.flush());
    assert!(heard.is_empty(), "{heard:?}");
}

#[test]
fn lexicon_keeps_a_sung_ooh_and_ah() {
    // One full vowel is a sung syllable and is listened for; the neutral "a" and "uh" are not.
    let words = lexicon_of(&["ooh ah a uh grace"]).words();
    let phones: Vec<Vec<char>> = words.iter().map(|w| w.phones.clone()).collect();
    assert_eq!(phones, [coarse("uː"), coarse("ɑː"), coarse(&g2p::phonemes("grace").unwrap())], "{words:?}");
    assert_eq!(words[0].key, sound_key("ooh"));
}

#[test]
fn a_lone_ooh_is_heard_at_a_pause() {
    let mut h = Hears::new(&["u", "s", "i", "ŋ"], "sing ooh ooh ooh");
    // One held "Ooh" and a breath: with nothing else waiting it is the song's word.
    assert!(h.rows(&["u"]).is_empty());
    let heard = h.blanks(PAUSE_FRAMES as usize);
    assert_eq!(keys(&heard), [sound_key("ooh")]);
    assert!(heard[0].at.abs() < 1e-9 && heard[0].conf > 0.7, "{heard:?}");
    // Two with a short gap between them: both.
    assert!(h.rows(&["u"]).is_empty());
    assert!(h.blanks(5).is_empty());
    assert!(h.rows(&["u"]).is_empty());
    assert_eq!(keys(&h.blanks(PAUSE_FRAMES as usize)), [sound_key("ooh"), sound_key("ooh")]);
    // Straight after a whole song word.
    assert!(h.rows(&["s", "i", "ŋ", "u"]).is_empty());
    assert_eq!(keys(&h.blanks(PAUSE_FRAMES as usize)), [sound_key("sing"), sound_key("ooh")]);
}

#[test]
fn an_ooh_is_not_heard_inside_other_sounds() {
    // (A long word in the song, so that nothing is decided before the pause.)
    let mut h = Hears::new(&["u", "k", "p", "s", "i", "ŋ"], "sing ooh hallelujah");
    // A vowel among stray sounds, in a long run and before a pause: none of them is "Ooh".
    assert!(h.rows(&["k", "u", "p", "u", "k", "u", "p", "u", "k"]).is_empty());
    assert!(h.blanks(PAUSE_FRAMES as usize).is_empty());
    assert!(h.rows(&["k", "u"]).is_empty());
    assert!(h.blanks(PAUSE_FRAMES as usize).is_empty());
    // A stray sound was thrown away before the word at this pause: the vowel after it is not trusted either.
    assert!(h.rows(&["k", "s", "i", "ŋ", "u"]).is_empty());
    assert_eq!(keys(&h.blanks(PAUSE_FRAMES as usize)), [sound_key("sing")]);
    // Run together with no pause, it is not looked for at all.
    assert!(spotted(&["sing ooh"], "ukupuku").is_empty());
}

#[test]
fn an_exact_short_word_beats_a_longer_one_with_a_sound_missing() {
    // "your" comes first in the song; "you are" is still "you", "are" (it was "your", "are").
    assert_eq!(spotted(&["your you are"], "juːɑːɹ"), ["you", "are"].map(sound_key));
    assert_eq!(spotted(&["your you"], "juː"), [sound_key("you")]);
    // The longer word, sung whole, is still itself.
    assert_eq!(spotted(&["your you"], "jʊɹ"), [sound_key("your")]);
    // "pray" is not "prayer" sung "pray-er" with its ending lost.
    assert_eq!(spotted(&["prayer pray"], "pɹeɪ"), [sound_key("pray")]);
    assert_eq!(spotted(&["prayer pray"], "pɹeɪɚ"), [sound_key("prayer")]);
}

#[test]
fn i_is_not_heard_as_im() {
    // "I'm" is earlier in the song; a sung "I am" is "I", "am" (it was "I'm", "am").
    assert_eq!(spotted(&["I'm no longer I am"], "aɪæm"), ["I", "am"].map(sound_key));
    assert_eq!(spotted(&["I'm no longer I am"], "aɪm"), [sound_key("I'm")]);
    // And "I'd" is itself, not an earlier "died" with its first sound missing.
    assert_eq!(spotted(&["he died I'd"], "aɪd"), [sound_key("I'd")]);
    assert_eq!(spotted(&["he died I'd"], "daɪd"), [sound_key("died")]);
}

#[test]
fn youre_is_heard() {
    // A song with "You're" and no "your".
    assert_eq!(spotted(&["you're a good good father"], "jʊɹ"), [sound_key("you're")]);
}

#[test]
fn either_pronunciation_is_heard() {
    let lex = lexicon_of(&["blessed be your name amen"]);
    // As sung ("bless-ed", "ay-men"), then as said.
    for (blessed, amen) in [("blɛsɪd", "eɪmɛn"), ("blɛst", "ɑːmɛn")] {
        let mut spot = Spotter { lexicon: lex.clone(), buf: vec![] };
        let mut heard = vec![];
        for (i, c) in coarse(blessed).into_iter().chain(coarse(amen)).enumerate() { heard.extend(spot.push(c, i as f64 * 0.1, 0.9)) }
        heard.extend(spot.flush());
        assert_eq!(keys(&heard), ["blessed", "amen"].map(sound_key), "{blessed} {amen}");
    }
}

#[test]
fn sung_hallelujah_is_heard() {
    let mut spot = Spotter { lexicon: lexicon_of(&["hallelujah"]), buf: vec![] };
    let mut heard = vec![];
    // "Hah-leh-loo-yah": three sounds away from the spoken "hælɪluːjə", too far for one way of saying it.
    for (i, c) in coarse("hɑːlɛluːjɑː").into_iter().enumerate() { heard.extend(spot.push(c, i as f64 * 0.1, 0.9)) }
    heard.extend(spot.flush());
    assert_eq!(keys(&heard), [sound_key("hallelujah")]);
}

#[test]
fn a_held_note_inside_a_word_waits() {
    let mut h = Hears::new(&["s", "ɑ", "u", "n", "d"], "sound");
    // "Sou——nd": the note is held for 0.8 s (the model writes nothing new), then the word ends.
    assert!(h.rows(&["s", "ɑ", "u"]).is_empty());
    assert!(h.blanks(40).is_empty());
    assert!(h.rows(&["n", "d"]).is_empty());
    let heard = h.blanks(PAUSE_FRAMES as usize);
    assert_eq!(keys(&heard), [sound_key("sound")]);
    // Heard at its last sound: the 45th frame.
    assert!((heard[0].at - 0.88).abs() < 1e-6, "{heard:?}");
}

#[test]
fn a_held_sound_is_not_heard_as_how() {
    let mut spot = Spotter { lexicon: lexicon_of(&SONG), buf: vec![] };
    for c in coarse("ðəsɑu") { assert!(spot.push(c, 0.0, 0.9).is_empty()) }
    // The pause of a held note: "the" is whole and is heard; "sou" is one sound from "how" but begins "sound",
    // so it waits.
    assert_eq!(keys(&spot.pause()), [sound_key("the")]);
    for c in coarse("nd") { assert!(spot.push(c, 0.0, 0.9).is_empty()) }
    assert_eq!(keys(&spot.flush()), [sound_key("sound")]);
}

#[test]
fn a_dropped_ending_is_not_held_back() {
    let mut h = Hears::new(&["ɡ", "ɹ", "eɪ"], "grace");
    // "Gra-" and a breath: only the ending is missing, so it is heard at the pause, as before.
    assert!(h.rows(&["ɡ", "ɹ", "eɪ"]).is_empty());
    assert!(h.blanks(PAUSE_FRAMES as usize - 1).is_empty());
    assert_eq!(keys(&h.blanks(1)), [sound_key("grace")]);
}

#[test]
fn a_whole_short_word_is_not_held_back() {
    // "see" is also how "seated" begins: a whole word is heard at the pause all the same.
    let mut h = Hears::new(&["s", "i"], "see seated");
    assert!(h.rows(&["s", "i"]).is_empty());
    assert_eq!(keys(&h.blanks(PAUSE_FRAMES as usize)), [sound_key("see")]);
}

#[test]
fn a_begun_word_is_given_up_after_three_seconds() {
    let mut h = Hears::new(&["s", "ɑ", "u", "n", "d"], "sound");
    assert!(h.rows(&["s", "ɑ", "u"]).is_empty());
    // Three seconds of nothing new: the word was never finished. Its ending, when it comes, is no word.
    assert!(h.blanks(HOLD_FRAMES as usize).is_empty());
    assert!(h.rows(&["n", "d"]).is_empty());
    assert!(h.blanks(PAUSE_FRAMES as usize).is_empty());
}

#[test]
fn a_begun_word_given_up_is_not_taken_for_a_shorter_one() {
    let mut h = Hears::new(&["ð", "ə", "s", "ɑ", "u", "n", "d"], "how sweet the sound");
    assert!(h.rows(&["ð", "ə", "s", "ɑ", "u"]).is_empty());
    // The pause: "the" is whole; "sou" begins "sound" and waits.
    assert_eq!(keys(&h.blanks(PAUSE_FRAMES as usize)), [sound_key("the")]);
    // Three seconds of nothing new. "Sou" is one sound from "how", but it began "sound": it is dropped, not
    // heard as "how" three seconds late.
    assert!(h.blanks((HOLD_FRAMES - PAUSE_FRAMES) as usize).is_empty());
    assert!(h.rows(&["n", "d"]).is_empty());
    assert!(h.blanks(PAUSE_FRAMES as usize).is_empty());
}

#[test]
fn junk_tail_is_not_kept() {
    let mut h = Hears::new(&["k", "p", "ɡ", "ɹ", "eɪ", "s"], "grace");
    // Sounds that begin no song word are dropped at the pause, as before: they are not in the way of the next word.
    assert!(h.rows(&["k", "p"]).is_empty());
    assert!(h.blanks(PAUSE_FRAMES as usize).is_empty());
    assert!(h.rows(&["ɡ", "ɹ", "eɪ", "s"]).is_empty());
    assert_eq!(keys(&h.blanks(PAUSE_FRAMES as usize)), [sound_key("grace")]);
}

#[test]
fn stream_hears_each_word_once() {
    let v = letters_vocab();
    let calls = Arc::new(Mutex::new(0));
    let fake = || Box::new(Fake { vocab: v.clone(), units: Units::Letters, calls: calls.clone() });
    let mut tokens: Vec<String> = spell("GRACE");
    tokens.push("|".into());
    tokens.extend(spell("HOW"));
    tokens.push("|".into());
    tokens.extend(spell("SWEET"));
    let t: Vec<&str> = tokens.iter().map(String::as_str).collect();
    // A word every half second or so, across many overlapping windows.
    let pcm = say(&v, &t, 5, 4.0);
    let mut s = Stream::new(fake(), Lexicon::default());
    let mut heard = vec![];
    for (i, f) in pcm.chunks(320).enumerate() { heard.extend(s.feed(f, 10.0 + i as f64 * 0.02)) }
    let keys: Vec<String> = heard.iter().map(|h| h.key.clone()).collect();
    assert_eq!(keys, ["grace", "how", "sweet"].map(sound_key));
    // Heard at its last letter: "T" is the 15th token, 0.1 s each.
    assert!((heard[2].at - 11.4).abs() < 1e-6, "{heard:?}");
    // Runs every half second, not every frame.
    assert!((7..=9).contains(&*calls.lock().unwrap()), "{}", calls.lock().unwrap());

    // The same on the ear's own thread, the sound coming in as a room's would (here 4× faster).
    let mut ear = CtcEar::new(fake(), Lexicon::default());
    assert!(ear.words());
    let mut heard = vec![];
    for (i, f) in pcm.chunks(320).enumerate() {
        heard.extend(ear.frames(f, i as f64 * 0.02));
        std::thread::sleep(Duration::from_millis(5));
    }
    heard.extend(ear.settle(Duration::from_millis(500)));
    assert_eq!(heard.into_iter().map(|h| h.key).collect::<Vec<_>>(), ["grace", "how", "sweet"].map(sound_key));

    // All at once (a model slower than the room): it skips to the newest sound, never hearing a word twice.
    let mut ear = CtcEar::new(fake(), Lexicon::default());
    let mut heard = vec![];
    for (i, f) in pcm.chunks(320).enumerate() { heard.extend(ear.frames(f, i as f64 * 0.02)) }
    heard.extend(ear.settle(Duration::from_millis(500)));
    assert!(heard.len() <= 3, "{heard:?}");
}

#[test]
fn speed_check_picks_model() {
    let ms = Duration::from_millis;
    // The big model is fast enough: the small one isn't even tried.
    let mut tried = vec![];
    assert_eq!(speed_check_with(|c| { tried.push(c); Some(ms(1200)) }), Choice::Best);
    assert_eq!(tried, [Choice::Best]);
    // Too slow (2 s for 3 s of sound): the small one.
    assert_eq!(speed_check_with(|c| Some(if c == Choice::Best { ms(2000) } else { ms(700) })), Choice::Fast);
    // Only the small one downloaded.
    assert_eq!(speed_check_with(|c| (c == Choice::Fast).then(|| ms(900))), Choice::Fast);
    // Nothing fast enough, or nothing downloaded: no words.
    assert_eq!(speed_check_with(|_| Some(ms(1600))), Choice::Off);
    assert_eq!(speed_check_with(|_| None), Choice::Off);
}
