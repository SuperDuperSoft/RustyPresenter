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
    let mut spot = Spotter::new(lexicon_of(slides));
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
    let mut spot = Spotter::new(lex.clone());
    // Sung a little off: "ɐmeɪzɪn ɡɹeɪs haʊ swiːt" ("amazin'"), with a stray sound before "how".
    let mut heard = vec![];
    let sung = format!("{}{}k{}{}", "ɐmeɪzɪn", g2p::phonemes("grace").unwrap(), g2p::phonemes("how").unwrap(), g2p::phonemes("sweet").unwrap());
    for (i, c) in coarse(&sung).into_iter().enumerate() { heard.extend(spot.push(c, i as f64 * 0.1, 0.9)) }
    heard.extend(spot.flush());
    let keys: Vec<String> = heard.iter().map(|h| h.key.clone()).collect();
    assert_eq!(keys, ["amazing", "grace", "how", "sweet"].map(sound_key));
    assert!(heard.windows(2).all(|w| w[0].at < w[1].at));
    // Without a song, the groups of sounds between pauses.
    let mut spot = Spotter::new(Lexicon::default());
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
    let mut spot = Spotter::new(lexicon_of(&["saved a wretch"]));
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
    // After a whole song word and a breath (the word does not end in that vowel, so it is no held note).
    assert!(h.rows(&["s", "i", "ŋ"]).is_empty());
    assert_eq!(keys(&h.blanks(PAUSE_FRAMES as usize)), [sound_key("sing")]);
    assert!(h.rows(&["u"]).is_empty());
    assert_eq!(keys(&h.blanks(PAUSE_FRAMES as usize)), [sound_key("ooh")]);
}

#[test]
fn a_held_you_is_not_heard_as_ooh() {
    // A song with a sung "Ooh" somewhere. The line being sung ends "... to you", and "you" is held.
    let mut h = Hears::new(&["t", "u", "j"], "life to you ooh");
    assert!(h.rows(&["t", "u", "j", "u"]).is_empty());
    assert_eq!(keys(&h.blanks(PAUSE_FRAMES as usize)), ["to", "you"].map(sound_key));
    // Later in the same note the model writes the vowel again. It is the note, not "Ooh" (it was heard as "Ooh",
    // which moved the place on to the next "Oh" or "I" and brought the next slide up a line early).
    assert!(h.rows(&["u"]).is_empty());
    assert!(h.blanks(PAUSE_FRAMES as usize).is_empty());
    // And again, while the note may still be held.
    assert!(h.rows(&["u"]).is_empty());
    assert!(h.blanks(PAUSE_FRAMES as usize).is_empty());
    // Three seconds of nothing new: the note is over, and an "Ooh" is a word again.
    assert!(h.blanks((HOLD_FRAMES - PAUSE_FRAMES) as usize).is_empty());
    assert!(h.rows(&["u"]).is_empty());
    assert_eq!(keys(&h.blanks(PAUSE_FRAMES as usize)), [sound_key("ooh")]);

    // The vowel written twice before the first pause: "to", "you", and no "Ooh" (it was "to", "you", "Ooh").
    let mut h = Hears::new(&["t", "u", "j"], "life to you ooh");
    assert!(h.rows(&["t", "u", "j", "u"]).is_empty());
    assert!(h.blanks(5).is_empty());
    assert!(h.rows(&["u"]).is_empty());
    assert_eq!(keys(&h.blanks(PAUSE_FRAMES as usize)), ["to", "you"].map(sound_key));
}

#[test]
fn a_vowel_right_after_a_word_is_not_an_ooh() {
    let mut h = Hears::new(&["u", "s", "i", "ŋ"], "sing ooh");
    // In the same breath as a word, a vowel is as likely the rest of that word or a piece of the next one: it
    // is not trusted (it was heard as "Ooh").
    assert!(h.rows(&["s", "i", "ŋ", "u"]).is_empty());
    assert_eq!(keys(&h.blanks(PAUSE_FRAMES as usize)), [sound_key("sing")]);
    // The same vowel after a breath may be that sound still held: not yet.
    assert!(h.rows(&["u"]).is_empty());
    assert!(h.blanks(PAUSE_FRAMES as usize).is_empty());
    // Once the three seconds are up it is heard: an "Ooh" after such a word is late, not lost.
    assert!(h.blanks(HOLD_FRAMES as usize).is_empty());
    assert!(h.rows(&["u"]).is_empty());
    assert_eq!(keys(&h.blanks(PAUSE_FRAMES as usize)), [sound_key("ooh")]);
    assert!(h.rows(&["u"]).is_empty());
    assert_eq!(keys(&h.blanks(PAUSE_FRAMES as usize)), [sound_key("ooh")]);
}

#[test]
fn a_held_note_is_timed_from_where_it_began() {
    // The pauses come 0.4 s apart here, so the long silence (`HOLD_FRAMES`) never ends the note: only its age does.
    let mut h = Hears::new(&["t", "u", "j"], "life to you ooh");
    assert!(h.rows(&["j", "u"]).is_empty());
    assert_eq!(keys(&h.blanks(20)), [sound_key("you")]);
    // The vowel written again every 0.4 s (the last time 2.8 s after the "you"): for three seconds it is that note.
    for _ in 0..7 {
        assert!(h.rows(&["u"]).is_empty());
        assert!(h.blanks(19).is_empty());
    }
    // 3.2 s after it: sung "Ooh"s. They are late after a word that ends in their vowel, never lost for good.
    for _ in 0..2 {
        assert!(h.rows(&["u"]).is_empty());
        assert_eq!(keys(&h.blanks(19)), [sound_key("ooh")]);
    }
}

#[test]
fn many_oohs_in_one_breath_wait_for_the_pause() {
    // More "Ooh"s in one breath than the longest word has sounds: none is thrown away to make room (the rest
    // would then look like vowels after a stray sound, and not be heard).
    let mut h = Hears::new(&["u", "s", "i", "ŋ"], "sing ooh");
    for _ in 0..6 {
        assert!(h.rows(&["u"]).is_empty());
        assert!(h.blanks(3).is_empty());
    }
    assert_eq!(keys(&h.blanks(PAUSE_FRAMES as usize)), vec![sound_key("ooh"); 6]);
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
        let mut spot = Spotter::new(lex.clone());
        let mut heard = vec![];
        for (i, c) in coarse(blessed).into_iter().chain(coarse(amen)).enumerate() { heard.extend(spot.push(c, i as f64 * 0.1, 0.9)) }
        heard.extend(spot.flush());
        assert_eq!(keys(&heard), ["blessed", "amen"].map(sound_key), "{blessed} {amen}");
    }
}

#[test]
fn sung_hallelujah_is_heard() {
    let mut spot = Spotter::new(lexicon_of(&["hallelujah"]));
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
    let mut spot = Spotter::new(lexicon_of(&SONG));
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
fn practice_finishes_a_word_without_a_final_pause() {
    let vocab = letters_vocab();
    let model = Fake { vocab: vocab.clone(), units: Units::Letters, calls: Arc::new(Mutex::new(0)) };
    let mut stream = Stream::new(Box::new(model), Lexicon::default());
    let mut pcm = vec![0; RATE * 2 / 5];
    pcm.extend(say(&vocab, &["G", "R", "A", "C", "E"], 1, 0.12));
    let mut heard = vec![];
    for (i, frame) in pcm.chunks_exact(FRAME).enumerate() {
        heard.extend(stream.feed(frame, i as f64 * STEP_S));
    }
    assert!(heard.is_empty());
    assert_eq!(keys(&stream.finish()), [sound_key("grace")]);
    assert!(stream.finish().is_empty());
}

#[test]
fn discontinuous_audio_does_not_collapse_the_first_repeated_token() {
    let vocab = letters_vocab();
    let model = Fake { vocab: vocab.clone(), units: Units::Letters, calls: Arc::new(Mutex::new(0)) };
    let mut stream = Stream::new(Box::new(model), Lexicon::default());
    assert!(stream.feed(&say(&vocab, &["G"], 25, 0.5), 0.0).is_empty());
    let next = say(&vocab, &["G", "R", "A", "C", "E", "|"], 2, 0.6);
    assert_eq!(keys(&stream.feed(&next, 4.0)), [sound_key("grace")]);
}

#[test]
fn an_overloaded_ear_retains_only_the_latest_audio_window() {
    // Leave the consumer idle to simulate inference that falls behind the live input.
    let (tx, wake) = sync_channel(1);
    let (_heard, rx) = channel();
    let pending = Arc::new(Mutex::new(VecDeque::new()));
    let mut ear = CtcEar { tx: Some(tx), pending: pending.clone(), rx, thread: None };
    for i in 0..3 * MAX_QUEUED_FRAMES {
        ear.frames(&[0; FRAME], i as f64 * STEP_S);
    }
    assert_eq!(wake.try_iter().count(), 1);
    let pending = pending.lock().unwrap();
    assert_eq!(pending.len(), MAX_QUEUED_FRAMES);
    assert_eq!(pending.front().unwrap().1, 2.0 * WINDOW_S);
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

impl Hears {
    /// One frame with these guesses and their probabilities (the first is the model's first guess).
    fn frame(&mut self, guesses: &[(&str, f32)]) -> Vec<Heard> {
        let mut row = vec![-6.0f32; self.vocab.len()];
        for (tok, p) in guesses {
            let id = self.vocab.iter().position(|v| v == tok).unwrap_or_else(|| panic!("{tok}"));
            row[id] = p.ln();
        }
        let out = self.d.frame(&row, self.t);
        self.t += 0.02;
        out
    }
}

#[test]
fn the_models_second_guess_is_taken_for_a_sound() {
    let sounds = ["ɡ", "k", "ɹ", "eɪ", "s", "z"];
    // "grace", sung so that the model writes "k" for its first sound and "z" for its last, each time with the
    // word's own sound a close second. Two wrong sounds were one too many for a word of five: it was not heard.
    let mut h = Hears::new(&sounds, "grace");
    let mut heard = h.frame(&[("k", 0.5), ("ɡ", 0.35)]);
    heard.extend(h.rows(&["ɹ", "eɪ"]));
    heard.extend(h.frame(&[("z", 0.5), ("s", 0.4)]));
    heard.extend(h.blanks(PAUSE_FRAMES as usize));
    assert_eq!(keys(&heard), [sound_key("grace")]);
    assert!(heard[0].conf > 0.3 && heard[0].conf < 0.75, "less sure than a word heard exactly: {heard:?}");
    // The same sounds with nothing behind them are not the word.
    let mut h = Hears::new(&sounds, "grace");
    let mut heard = h.rows(&["k", "ɹ", "eɪ", "z"]);
    heard.extend(h.blanks(PAUSE_FRAMES as usize));
    assert!(heard.is_empty(), "{heard:?}");
    // Nor with a second guess the model hardly considered.
    let mut h = Hears::new(&sounds, "grace");
    let mut heard = h.frame(&[("k", 0.8), ("ɡ", 0.1)]);
    heard.extend(h.rows(&["ɹ", "eɪ"]));
    heard.extend(h.frame(&[("z", 0.8), ("s", 0.1)]));
    heard.extend(h.blanks(PAUSE_FRAMES as usize));
    assert!(heard.is_empty(), "{heard:?}");
}

#[test]
fn a_short_word_is_heard_by_a_close_second_guess_only() {
    // "the" has two sounds and no whole edit to spare. "d" with "ð" a close second is still "the".
    let mut h = Hears::new(&["ð", "d", "ə"], "the");
    let mut heard = h.frame(&[("d", 0.5), ("ð", 0.4)]);
    heard.extend(h.rows(&["ə"]));
    heard.extend(h.blanks(PAUSE_FRAMES as usize));
    assert_eq!(keys(&heard), [sound_key("the")]);
    // A second guess further off is not enough for so short a word.
    let mut h = Hears::new(&["ð", "d", "ə"], "the");
    let mut heard = h.frame(&[("d", 0.6), ("ð", 0.2)]);
    heard.extend(h.rows(&["ə"]));
    heard.extend(h.blanks(PAUSE_FRAMES as usize));
    assert!(heard.is_empty(), "{heard:?}");
}

#[test]
fn second_guesses_that_fold_to_one_sound_add_up() {
    // "ɪ" and "iː" are the same sound to the ear (`coarse`): together they are a close second to "ɛ".
    let vocab: Vec<String> = ["<pad>", "ɛ", "ɪ", "iː", "s"].map(String::from).into();
    let d = Decoder::new(Units::Phonemes, vocab, 0, Lexicon::default());
    let row = [-9.0, 0.5f32.ln(), 0.2f32.ln(), 0.2f32.ln(), 0.1f32.ln()];
    let alts = d.second_guesses(&row, 1, 0.5);
    assert_eq!(alts.len(), 1);
    let i = alts[0].iter().find(|a| a.0 == 'i').expect("i");
    assert!((i.1 - 0.8).abs() < 1e-4, "{alts:?}");
    assert!(alts[0].iter().all(|a| a.0 != 's'), "0.1 is under a quarter of 0.5: {alts:?}");
}

/// A letter decoder for a song with these slides.
struct Writes { d: Decoder, vocab: Vec<String>, t: f64 }

impl Writes {
    fn new(slides: &[&str]) -> Writes {
        let vocab = letters_vocab();
        Writes { d: Decoder::new(Units::Letters, vocab.clone(), 0, lexicon_of(slides)), vocab, t: 0.0 }
    }

    /// The model writes `text`, a frame for each letter: "|" is a word break, "_" a frame of nothing (between
    /// two of the same letter). The keys heard.
    fn write(&mut self, text: &str) -> Vec<String> {
        let mut out = vec![];
        for c in text.chars() {
            let tok = if c == '_' { "<pad>".to_string() } else { c.to_string() };
            out.extend(self.frame(&[(tok.as_str(), 0.82)]));
        }
        out
    }

    /// One frame with these guesses and their probabilities.
    fn frame(&mut self, guesses: &[(&str, f32)]) -> Vec<String> {
        let mut row = vec![-6.0f32; self.vocab.len()];
        for (tok, p) in guesses {
            let id = self.vocab.iter().position(|v| v == tok).unwrap_or_else(|| panic!("{tok}"));
            row[id] = p.ln();
        }
        let out = self.d.frame(&row, self.t);
        self.t += 0.02;
        keys(&out)
    }

    fn pause(&mut self) -> Vec<String> { keys(&self.d.pause()) }
}

const GRACE_LINE: &str = "amazing grace how sweet the sound";

#[test]
fn a_written_word_a_letter_off_is_the_songs_word() {
    let mut w = Writes::new(&[GRACE_LINE]);
    // "amazin'", "swee": the song's words (they were heard as words of their own, which matched nothing).
    assert_eq!(w.write("AMAZIN|GRACE|HOW|SWE_E|"), ["amazing", "grace", "how", "sweet"].map(sound_key));
    // A word that sounds the same is the song's word as before, however it is written.
    assert_eq!(w.write("GRAYS|"), [sound_key("grace")]);
    // A short word a whole letter off is another word: it waits (it may be half a word), and is then heard as
    // written.
    assert!(w.write("SHE|").is_empty());
    assert_eq!(w.write("SOUND|"), ["she", "sound"].map(sound_key));
    // But a short word whose letter was the model's close second guess is the song's.
    let mut heard = w.write("TH");
    heard.extend(w.frame(&[("A", 0.45), ("E", 0.4)]));
    heard.extend(w.write("|"));
    assert_eq!(heard, [sound_key("the")]);
}

#[test]
fn words_written_as_one_are_heard_one_by_one() {
    let mut w = Writes::new(&[GRACE_LINE]);
    // Singers leave no pauses, so the model writes no breaks.
    assert_eq!(w.write("AMAZINGRACE|"), ["amazing", "grace"].map(sound_key));
    assert_eq!(w.write("HOWSWE_ETHESOUND|"), ["how", "sweet", "the", "sound"].map(sound_key));
    // Each at its own time: "amazing" at its last letter (the one "G" may be either word's), "grace" at its own.
    let v = letters_vocab();
    let mut d = Decoder::new(Units::Letters, v.clone(), 0, lexicon_of(&[GRACE_LINE]));
    let mut heard = vec![];
    for (i, c) in "AMAZINGRACE|".chars().enumerate() {
        let id = v.iter().position(|x| *x == c.to_string()).unwrap();
        let row: Vec<f32> = (0..v.len()).map(|j| if j == id { -0.2 } else { -4.0 }).collect();
        heard.extend(d.frame(&row, i as f64 * 0.02));
    }
    assert!((0.099..0.121).contains(&heard[0].at) && (heard[1].at - 0.20).abs() < 1e-9, "{heard:?}");
    // One long word that only holds a song word is not a run of song words.
    let mut w = Writes::new(&[GRACE_LINE]);
    assert!(w.write("SOUNDTRACKS|").is_empty());
    assert_eq!(w.pause(), [sound_key("soundtracks")]);
}

#[test]
fn a_word_written_in_two_halves_is_heard_whole() {
    let mut w = Writes::new(&["hallelujah amazing grace"]);
    // A held note in the middle of a word looks like a word break to the model.
    assert!(w.write("HAL_LE|").is_empty(), "no song word yet: it waits for its other half");
    assert_eq!(w.write("LUJAH|"), [sound_key("hallelujah")]);
    // Two halves that make two song words.
    assert!(w.write("AMAZ|").is_empty());
    assert_eq!(w.write("INGRACE|"), ["amazing", "grace"].map(sound_key));
}

#[test]
fn a_long_run_with_no_break_is_decided_as_it_goes() {
    let mut w = Writes::new(&["amazing grace"]);
    // A minute of singing in one breath, as the model writes it: the words do not wait for the pause.
    let line = "AMAZINGRACE".repeat(8);
    let heard = w.write(&line);
    assert!(heard.len() >= 10, "{heard:?}");
    assert!(heard.iter().all(|k| *k == sound_key("amazing") || *k == sound_key("grace")), "{heard:?}");
    let all: Vec<String> = heard.into_iter().chain(w.write("|")).chain(w.pause()).collect();
    assert_eq!(all.len(), 16, "every word once, across the cuts: {all:?}");
}

#[test]
fn talk_is_heard_as_it_is_written() {
    let mut w = Writes::new(&[GRACE_LINE]);
    // No word of the song: each is heard as written, one word late (it might have been half a word), and nothing
    // waits across a pause.
    assert!(w.write("WELCOME|").is_empty());
    assert_eq!(w.write("FRIENDS|"), [sound_key("welcome")]);
    assert_eq!(w.pause(), [sound_key("friends")]);
    // A song word after talk: the talk first, in the order it was said.
    assert!(w.write("TODAY|").is_empty());
    assert_eq!(w.write("GRACE|"), ["today", "grace"].map(sound_key));
    assert!(w.pause().is_empty());
}
