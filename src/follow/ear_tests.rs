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
