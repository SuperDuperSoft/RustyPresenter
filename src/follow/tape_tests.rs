use super::*;
use crate::follow::review;
use crate::follow::testing::SONG;

fn tmp() -> PathBuf { std::env::temp_dir().join(format!("rp-follow-tape-{}", Uuid::new_v4())) }

fn when() -> chrono::NaiveDateTime { chrono::NaiveDate::from_ymd_opt(2026, 11, 20).unwrap().and_hms_opt(19, 30, 5).unwrap() }

fn screen(item: Uuid, slide: usize, hand: u64) -> Screen {
    Screen { item, title: "Amazing Grace".into(), song: true, slides: SONG.iter().map(|s| s.to_string()).collect(), slide, words_up: true, mode: Mode::Manual,
        hand, video: false, hold: false }
}

/// A second of sound in 20 ms frames, from `at` on the sound input's own clock; each sample is its number.
fn second(tape: &Tape, at: f64, events: impl Fn(usize) -> Vec<FollowEvent>) {
    for n in 0..50 {
        let pcm: Vec<i16> = (0..320).map(|i| (n * 320 + i) as i16).collect();
        tape.frame(&pcm, at + n as f64 * 0.02, &events(n));
    }
}

#[test]
fn a_tape_keeps_the_sound_and_what_happened_in_step() {
    let root = tmp();
    let tape = Tape::default();
    assert!(!tape.on() && tape.running().is_none());
    // Nothing is kept while no tape runs.
    tape.frame(&[1; 320], 0.0, &[]);
    tape.screen(&screen(Uuid::new_v4(), 0, 1));
    let dir = tape.start(&root, when(), json!({ "app": "test" })).unwrap();
    assert_eq!(dir, root.join("2026-11-20_19-30-05"));
    assert!(tape.on());
    assert_eq!(tape.start(&root, when(), json!({})).unwrap(), dir, "one tape at a time");
    let item = Uuid::new_v4();
    tape.screen(&screen(item, 0, 1));
    // The sound input's clock is its own (here it has run for 100 s): the tape's lines are in tape time.
    second(&tape, 100.0, |n| match n {
        4 => vec![FollowEvent::Audio { voice: Voice::Singing, onsets: 2.5, at: 100.08, held_since: Some(100.02) }],
        30 => vec![FollowEvent::Heard(Heard { key: "KRS".into(), at: 100.2, conf: 0.9 })],
        _ => vec![],
    });
    tape.screen(&screen(item, 0, 1));
    tape.screen(&screen(item, 1, 2));
    tape.screen(&screen(item, 2, 2));
    tape.trace(&["     1.00  SHOW slide 3".to_string()]);
    assert_eq!(tapes(&root), [dir.clone()]);
    let (running, secs) = tape.running().unwrap();
    assert_eq!(running, dir);
    assert!((1.0..1.6).contains(&secs), "{secs}");
    assert_eq!(tape.stop(), Some(dir.clone()));
    assert!(!tape.on() && tape.stop().is_none());

    // The sound: a header that says how much there is, then the samples as they came.
    let wav = std::fs::read(dir.join(SOUND_FILE)).unwrap();
    assert_eq!(&wav[..4], b"RIFF");
    let data = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
    assert_eq!(data, wav.len() - 44);
    assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
    let samples: Vec<i16> = wav[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    // A little silence first (the time before the first frame came), then the second of sound.
    let lead = samples.len() - 16_000;
    assert!(lead < 8_000, "{lead}");
    assert!(samples[..lead].iter().all(|s| *s == 0));
    assert_eq!(samples[lead..lead + 4], [0, 1, 2, 3]);
    let lead_s = lead as f64 / 16_000.0;

    let lines: Vec<Value> = std::fs::read_to_string(dir.join(LINES_FILE)).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines[0]["tape"]["app"], "test");
    assert_eq!(lines[0]["when"], "2026-11-20 19:30:05");
    let find = |key: &str| lines.iter().filter(|l| l.get(key).is_some()).collect::<Vec<_>>();
    let song = find("song");
    assert_eq!((song.len(), song[0]["song"]["title"].as_str(), song[0]["song"]["slides"].as_array().unwrap().len()), (1, Some("Amazing Grace"), 3), "the song once");
    let near = |v: &Value, want: f64| (v.as_f64().unwrap() - want).abs() < 0.002;
    let voice = find("voice");
    assert_eq!(voice[0]["voice"], "singing");
    assert!(near(&voice[0]["a"], lead_s + 0.08) && near(&voice[0]["held"], lead_s + 0.02), "{}", voice[0]);
    let heard = find("heard");
    assert_eq!(heard[0]["heard"], "KRS");
    assert!(near(&heard[0]["sung"], lead_s + 0.2) && near(&heard[0]["a"], lead_s + 0.62), "{}", heard[0]);
    // The screen: where it stood, then each change and who made it. The same state again is not a line.
    let live: Vec<(u64, &str)> = find("live").iter().map(|l| (l["live"]["slide"].as_u64().unwrap(), l["live"]["by"].as_str().unwrap())).collect();
    assert_eq!(live, [(0, "hand"), (1, "hand"), (2, "auto")]);
    assert_eq!(find("trace")[0]["trace"], "     1.00  SHOW slide 3");
    assert_eq!(lines.last().unwrap()["end"], true);
    let times: Vec<f64> = lines.iter().map(|l| l["a"].as_f64().unwrap()).collect();
    assert!(times.iter().all(|t| (0.0..2.5).contains(t)), "{times:?}");

    // And read back as a song to review.
    let takes = review::read(&dir).unwrap();
    assert_eq!(takes.len(), 1);
    let t = &takes[0];
    assert_eq!((t.title.as_str(), t.slides.len(), t.labels.len(), t.heard.len(), t.screen.len()), ("Amazing Grace", 3, 1, 1, 3));
    assert_eq!((t.screen[1].slide, t.screen[1].by_hand, t.screen[2].by_hand), (1, true, false));
    assert!(t.until >= t.from && t.heard[0].1.key == "KRS");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_break_in_the_sound_is_kept_as_silence() {
    let root = tmp();
    let tape = Tape::default();
    let dir = tape.start(&root, when(), json!({})).unwrap();
    second(&tape, 0.0, |_| vec![]);
    // The input was changed: its clock starts again, 0.7 s later.
    std::thread::sleep(std::time::Duration::from_millis(700));
    second(&tape, 0.0, |n| if n == 0 { vec![FollowEvent::Heard(Heard { key: "A".into(), at: 0.0, conf: 1.0 })] } else { vec![] });
    tape.stop();
    let wav = std::fs::read(dir.join(SOUND_FILE)).unwrap();
    let secs = (wav.len() - 44) as f64 / 32_000.0;
    assert!((2.6..3.2).contains(&secs), "two seconds of sound and the gap between them: {secs}");
    let takes_text = std::fs::read_to_string(dir.join(LINES_FILE)).unwrap();
    let heard: Value = serde_json::from_str(takes_text.lines().find(|l| l.contains("\"heard\"")).unwrap()).unwrap();
    assert!((1.6..2.2).contains(&heard["sung"].as_f64().unwrap()), "after the gap, in tape time: {heard}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn something_else_live_ends_the_song() {
    let root = tmp();
    let tape = Tape::default();
    let dir = tape.start(&root, when(), json!({})).unwrap();
    let (song, sermon) = (Uuid::new_v4(), Uuid::new_v4());
    tape.screen(&screen(song, 0, 1));
    second(&tape, 0.0, |_| vec![]);
    tape.screen(&Screen { item: sermon, title: "Sermon".into(), song: false, slides: vec![], slide: 0, words_up: true, mode: Mode::Manual, hand: 2, video: false, hold: false });
    tape.screen(&Screen { item: sermon, title: "Sermon".into(), song: false, slides: vec![], slide: 1, words_up: true, mode: Mode::Manual, hand: 3, video: false, hold: false });
    second(&tape, 1.0, |_| vec![]);
    tape.screen(&screen(song, 2, 4));
    tape.stop();
    let text = std::fs::read_to_string(dir.join(LINES_FILE)).unwrap();
    assert_eq!(text.lines().filter(|l| l.contains("\"other\"")).count(), 1, "the sermon once");
    let takes = review::read(&dir).unwrap();
    assert_eq!(takes.len(), 2, "the song, and the song again");
    assert!(takes[0].until <= takes[1].from && takes[0].until - takes[0].from >= 0.9);
    assert_eq!(takes[1].screen[0].slide, 2);
    let _ = std::fs::remove_dir_all(root);
}
