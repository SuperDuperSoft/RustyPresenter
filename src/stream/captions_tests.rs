use super::*;
use crate::follow::ear::Units;

fn vocab() -> Vec<String> {
    ["<pad>", "<s>", "</s>", "<unk>", "|", "E", "T", "A", "O", "N", "I", "H", "S", "R", "D", "L", "U", "M", "W", "C", "F", "G", "Y", "P", "B", "V",
        "K", "'", "X", "J", "Q", "Z"].map(String::from).into()
}

/// A stand-in model: each 20 ms frame's first sample is the token it "hears" (×100).
struct Fake(Vec<String>);

impl Acoustic for Fake {
    fn log_probs(&self, pcm: &[f32]) -> Result<Vec<Vec<f32>>, String> {
        let n = (pcm.len() - 400) / 320 + 1;
        Ok((0..n).map(|i| {
            let id = (pcm[i * 320] * 32768.0 / 100.0).round() as usize;
            (0..self.0.len()).map(|v| if v == id { -0.1 } else { -5.0 }).collect()
        }).collect())
    }
    fn vocab(&self) -> &[String] { &self.0 }
    fn units(&self) -> Units { Units::Letters }
    fn blank(&self) -> usize { 0 }
}

/// Sound that makes `Fake` write `text` ("GOOD MORNING": a space is a word break), each letter for 3 frames with a
/// blank between letters, then silence to `secs` seconds.
fn say(text: &str, secs: f64) -> Vec<i16> {
    let v = vocab();
    let mut pcm = vec![];
    for c in text.chars() {
        let t = if c == ' ' { "|".to_string() } else { c.to_string() };
        let id = v.iter().position(|x| *x == t).unwrap_or_else(|| panic!("{t}"));
        for _ in 0..3 { pcm.extend(std::iter::repeat_n((id * 100) as i16, 320)) }
        pcm.extend(std::iter::repeat_n(0i16, 320));
    }
    pcm.resize((secs * 16000.0) as usize, 0);
    pcm
}

fn row(v: &[String], tok: &str) -> Vec<f32> { v.iter().map(|t| if t == tok { -0.1 } else { -5.0 }).collect() }

#[test]
fn letters_are_read_as_written_into_words() {
    let v = vocab();
    let mut l = Letters::new(v.clone(), 0);
    let mut out = vec![];
    let mut t = 0.0;
    for tok in ["H", "H", "<pad>", "I", "|", "T", "H", "E", "E", "<pad>", "E", "R", "E"] {
        out.extend(l.frame(&row(&v, tok), t));
        t += 0.02;
    }
    // A pause ends the last word.
    for _ in 0..PAUSE_FRAMES { out.extend(l.frame(&row(&v, "<pad>"), t)); t += 0.02 }
    let words: Vec<&str> = out.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(words, ["hi", "theere"], "repeats collapse, a blank between two of the same keeps both");
    assert!((out[0].start - 0.0).abs() < 1e-9 && out[1].start > 0.09);
}

#[test]
fn words_get_capitals_where_a_church_expects_them() {
    assert_eq!(tidy("GOOD", true), "Good");
    assert_eq!(tidy("morning", false), "morning");
    assert_eq!(tidy("i", false), "I");
    assert_eq!(tidy("i'm", false), "I'm");
    assert_eq!(tidy("jesus", false), "Jesus");
    assert_eq!(tidy("god's", false), "God's");
    assert_eq!(tidy("goodness", false), "goodness");
}

fn spoken(text: &str, start: f64) -> Spoken { Spoken { text: text.into(), start, end: start + 0.3 } }

#[test]
fn two_lines_roll_up_and_clear_after_a_quiet_spell() {
    let mut b = Board::default();
    let mut gone = vec![];
    let mut t = 0.0;
    for w in "good morning church today we read from the book of john chapter three".split(' ') {
        gone.extend(b.word(&spoken(w, t)));
        t += 0.4;
    }
    for l in b.text().lines() { assert!(l.chars().count() <= LINE, "{l}") }
    assert_eq!(b.text().lines().count(), LINES);
    assert_eq!(gone.len(), 1, "the first line rolled off the top");
    assert_eq!(gone[0].text, "Good morning church today we");
    assert!(gone[0].end > gone[0].start);
    assert!(b.text().ends_with("chapter three"));
    // Still talking: nothing taken off. Quiet for `CLEAR_AFTER`: all of it, as cues.
    assert!(b.tick(t + 1.0).is_empty());
    let cleared = b.tick(t + CLEAR_AFTER + 0.5);
    assert_eq!(cleared.len(), 2);
    assert!(b.text().is_empty());
    // After a pause the next word starts a sentence.
    b.word(&spoken("amen", t + 10.0));
    assert_eq!(b.text(), "Amen");
    b.word(&spoken("and", t + 10.4));
    assert_eq!(b.text(), "Amen and");
    b.word(&spoken("so", t + 13.0));
    assert_eq!(b.text(), "Amen and So");
}

#[test]
fn the_captions_file_is_webvtt() {
    assert_eq!(vtt_time(3723.4567), "01:02:03.457");
    assert_eq!(vtt_time(-1.0), "00:00:00.000");
    assert_eq!(vtt_cue(&Cue { start: 1.0, end: 2.5, text: "Good morning".into() }), "00:00:01.000 --> 00:00:02.500\nGood morning\n\n");
    assert_eq!(file_beside(Path::new("C:/r/Recording 2026-11-17 10-30-00.mp4")), Path::new("C:/r/Recording 2026-11-17 10-30-00.vtt"));
}

#[test]
fn youtube_gets_a_numbered_post_with_the_time_said() {
    assert_eq!(youtube_url("http://upload.youtube.com/closedcaption?cid=abc", 3), "http://upload.youtube.com/closedcaption?cid=abc&seq=3");
    assert_eq!(youtube_url(" https://x.example/c ", 1), "https://x.example/c?seq=1");
    let at = SystemTime::UNIX_EPOCH + Duration::from_millis(1_794_916_800_123);
    assert_eq!(youtube_body(at, "Good morning\nchurch"), "2026-11-17T12:00:00.123\nGood morning<br>church\n");
}

#[test]
fn captions_listen_to_their_own_input_or_the_streams() {
    let p = Prefs { on: true, ..Prefs::default() };
    assert_eq!(p.listen_to(Some("Line In (USB)")).as_deref(), Some("Line In (USB)"));
    assert_eq!(p.listen_to(None), None);
    let own = Prefs { input: "Pulpit mic".into(), ..p.clone() };
    assert_eq!(own.listen_to(Some("Line In (USB)")).as_deref(), Some("Pulpit mic"));
    assert!(p.on_picture() && p.file(), "on the picture and in a file unless turned off");
    assert_eq!(serde_json::to_string(&Prefs::default()).unwrap(), "{}");
}

#[test]
fn the_captions_go_over_talk_but_not_over_song_words_or_cards() {
    use crate::stream::{pictures::Shown, scene::Scene};
    assert!(shows_over(&Shown { scene: Scene::Camera, ..Shown::default() }));
    assert!(!shows_over(&Shown { scene: Scene::CameraWords, words: Some(Arc::new(vec![])), ..Shown::default() }));
    assert!(!shows_over(&Shown { scene: Scene::StartingSoon, ..Shown::default() }));
}

#[test]
fn the_model_hears_the_words_through_the_rolling_window() {
    let mut l = Listen::new(Box::new(Fake(vocab())));
    let pcm = say("GOOD MORNING CHURCH", 4.0);
    let mut words = vec![];
    for (i, chunk) in pcm.chunks(320).enumerate() {
        words.extend(l.push(chunk, i as f64 * 0.02));
        words.extend(l.run());
    }
    words.extend(l.flush());
    let text: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(text, ["good", "morning", "church"]);
}

/// Writes 16 kHz mono 16-bit `pcm` as a WAV file.
fn wav(path: &Path, pcm: &[i16]) {
    let data: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
    let mut f = Vec::new();
    f.extend(b"RIFF");
    f.extend((36 + data.len() as u32).to_le_bytes());
    f.extend(b"WAVEfmt ");
    f.extend(16u32.to_le_bytes());
    f.extend(1u16.to_le_bytes());
    f.extend(1u16.to_le_bytes());
    f.extend(16000u32.to_le_bytes());
    f.extend(32000u32.to_le_bytes());
    f.extend(2u16.to_le_bytes());
    f.extend(16u16.to_le_bytes());
    f.extend(b"data");
    f.extend((data.len() as u32).to_le_bytes());
    f.extend(data);
    std::fs::write(path, f).unwrap();
}

fn ffmpeg() -> Option<PathBuf> {
    let found = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).flat_map(|d| [d.join("ffmpeg"), d.join("ffmpeg.exe")]).find(|p| p.is_file()));
    if found.is_none() { eprintln!("no ffmpeg here: skipped") }
    found
}

/// FFmpeg reading at the speed of speech, as from a microphone.
fn live_args(file: &Path) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec!["-re".into()];
    a.extend(crate::follow::audio::file_args(file));
    a
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rp-captions-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The whole way with a real FFmpeg at the pace of speech (the stand-in model): the words come on the picture,
/// go off after the quiet, and are in the captions file when it stops. About 8 s.
#[test]
fn captions_go_on_the_picture_and_into_the_file() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = temp_dir("run");
    let file = dir.join("talk.wav");
    let mut pcm = say("GOOD MORNING CHURCH", 1.5);
    pcm.extend(vec![0i16; 16000 * 6]);
    wav(&file, &pcm);
    let slot: Slot = Arc::default();
    let vtt = dir.join("Recording.vtt");
    let setup = Setup { ffmpeg, args: live_args(&file), picture: Some((320, 180)), slot: slot.clone(), file: Some(vtt.clone()), youtube: String::new(),
        delay: Duration::ZERO, mute: Arc::default(), retry: Duration::from_millis(100) };
    let mut c = start(setup, Box::new(|| Ok(Box::new(Fake(vocab())) as Box<dyn Acoustic>))).unwrap();
    let (mut seen, mut text_seen, mut cleared) = (false, String::new(), false);
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(9) {
        let pic = slot.lock().unwrap().clone();
        if let Some(p) = &pic { seen |= p.len() == 320 * 180 * 4 && p.iter().any(|b| *b != 0); text_seen = c.text.lock().unwrap().clone() }
        if seen && pic.is_none() { cleared = true; break }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(c.state(), State::Listening);
    c.stop();
    assert!(seen, "the captions were drawn on the picture");
    assert_eq!(text_seen, "Good morning church");
    assert!(cleared, "taken off after the quiet");
    let written = std::fs::read_to_string(&vtt).unwrap();
    assert!(written.starts_with("WEBVTT\n\n"), "{written}");
    assert!(written.contains(" --> ") && written.contains("Good morning church"), "{written}");
    let _ = std::fs::remove_dir_all(dir);
}

/// Mute: nothing is captioned while it is on.
#[test]
fn nothing_is_captioned_while_the_stream_is_muted() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = temp_dir("mute");
    let file = dir.join("talk.wav");
    wav(&file, &say("GOOD MORNING", 3.0));
    let slot: Slot = Arc::default();
    let vtt = dir.join("Recording.vtt");
    let setup = Setup { ffmpeg, args: live_args(&file), picture: None, slot: slot.clone(), file: Some(vtt.clone()), youtube: String::new(),
        delay: Duration::ZERO, mute: Arc::new(AtomicBool::new(true)), retry: Duration::from_millis(100) };
    let c = start(setup, Box::new(|| Ok(Box::new(Fake(vocab())) as Box<dyn Acoustic>))).unwrap();
    std::thread::sleep(Duration::from_secs(4));
    drop(c);
    assert_eq!(std::fs::read_to_string(&vtt).unwrap(), "WEBVTT\n\n");
    assert!(slot.lock().unwrap().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

/// Real speech (Windows' own speaking voice) through the real English model, when both are on this computer. The
/// model, not the test, decides the spelling, so only clear, common words are looked for.
#[test]
fn real_speech_is_written_out_by_the_english_model() {
    let Ok(model) = model() else { eprintln!("no English speech model here: skipped"); return };
    if !cfg!(windows) { return }
    let dir = temp_dir("speech");
    let file = dir.join("speech.wav");
    let script = format!("Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
        $f = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono); \
        $s.Rate = -2; $s.SetOutputToWaveFile('{}', $f); $s.Speak('Good morning everyone. Please open your bibles to the book of John.'); $s.Dispose()", file.display());
    let ok = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script]).status().is_ok_and(|s| s.success());
    let Ok(bytes) = std::fs::read(&file).map_err(|_| ()).and_then(|b| if ok { Ok(b) } else { Err(()) }) else { eprintln!("no speaking voice here: skipped"); return };
    let at = bytes.windows(4).position(|w| w == b"data").expect("a data chunk") + 8;
    let pcm: Vec<i16> = bytes[at..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    let mut l = Listen::new(model);
    let mut words = vec![];
    for (i, chunk) in pcm.chunks(320).enumerate() {
        words.extend(l.push(chunk, i as f64 * 0.02));
        words.extend(l.run());
    }
    words.extend(l.run());
    words.extend(l.flush());
    let mut board = Board::default();
    for w in &words { board.word(w); }
    let heard: Vec<String> = words.iter().map(|w| w.text.clone()).collect();
    eprintln!("heard: {}", heard.join(" "));
    // The model's spelling and word breaks are its own ("every one"): a word counts when it is heard, or is in the
    // words run together.
    let joined = heard.concat();
    let found = ["good", "morning", "everyone", "open", "your", "bibles", "book", "the"].iter().filter(|w| heard.iter().any(|h| h == *w) || joined.contains(*w)).count();
    assert!(found >= 6, "most of the clear words were heard: {heard:?}");
    assert!(board.text().lines().all(|l| l.chars().count() <= LINE));
    let _ = std::fs::remove_dir_all(dir);
}
