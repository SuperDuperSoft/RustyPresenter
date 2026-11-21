use super::*;
use crate::follow::lyrics::sound_key;
use crate::follow::testing::{FakeEar, SONG, VERSE_1, VERSE_2};
use serde_json::json;

/// A tape's lines for `SONG` sung straight through from 2 s on, three words a second, each word delivered half a
/// second after it was sung. `clicks`: the slide changes made by hand, as (when, slide).
fn tape_lines(clicks: &[(f64, usize)]) -> String {
    let mut lines: Vec<(f64, serde_json::Value)> = vec![
        (0.0, json!({ "tape": {} })),
        (1.0, json!({ "song": { "item": "x", "title": "Amazing Grace", "slides": SONG } })),
        (1.0, json!({ "live": { "slide": 0, "words": true, "mode": "manual", "by": "hand" } })),
    ];
    let words: Vec<&str> = VERSE_1.split_whitespace().chain(VERSE_2.split_whitespace()).collect();
    for (i, w) in words.iter().enumerate() {
        let sung = 2.0 + i as f64 / 3.0;
        lines.push((sung + 0.5, json!({ "heard": sound_key(w), "conf": 0.95, "sung": sung })));
    }
    let end = 2.0 + words.len() as f64 / 3.0 + 2.0;
    let mut t = 1.0;
    while t < end {
        lines.push((t, json!({ "voice": if t >= 2.0 { "singing" } else { "silence" }, "onsets": 3.0, "held": null })));
        t += 0.1;
    }
    for &(at, slide) in clicks { lines.push((at, json!({ "live": { "slide": slide, "words": true, "mode": "manual", "by": "hand" } }))) }
    lines.push((end, json!({ "end": true })));
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    lines.into_iter().map(|(a, mut v)| { v["a"] = json!(a); v.to_string() }).collect::<Vec<_>>().join("\n")
}

/// The second verse starts at 2 s + 13 words.
const VERSE_2_AT: f64 = 2.0 + 13.0 / 3.0;

#[test]
fn a_tape_is_played_through_the_follower_and_held_against_the_clicks() {
    let takes = takes(&tape_lines(&[(VERSE_2_AT - 0.5, 1)]));
    assert_eq!(takes.len(), 1);
    let take = &takes[0];
    assert_eq!((take.heard.len(), take.screen.len(), take.slides.len()), (26, 2, 3));
    let (auto, trace) = replay(take, &Settings::default());
    assert_eq!(auto[0], Shown { at: 1.0, slide: 0, words: true, by_hand: true }, "it starts where the song went live");
    let shows: Vec<usize> = auto.iter().skip(1).map(|s| s.slide).collect();
    assert_eq!(shows, [1, 2], "the second verse, then the third as the second ends: {auto:?}");
    assert!(trace.iter().any(|l| l.contains("SHOW slide 2")), "the listening log comes with it");
    let s = score(take, &auto);
    assert_eq!(s.changes.len(), 1);
    let c = s.changes[0];
    assert!(c.by_hand && c.slide == 1 && c.auto_at.is_some_and(|a| (a - c.at).abs() <= ON_TIME_S), "{c:?}");
    assert_eq!((s.on_time(), s.early(), s.late(), s.missed()), (1, 0, 0, 0));
    assert_eq!(s.extra.len(), 1, "the tape never went on to the third slide");
    assert!(s.same > 0.5 && s.same < 1.0, "{}", s.same);
    let lines = s.lines(take);
    assert!(lines[0].starts_with("Amazing Grace — at 0:01.0 on the tape") && lines[0].contains("1 slide changes (1 by hand), 26 words heard"), "{}", lines[0]);
    assert!(lines[1].contains("1 on time, 0 early, 0 late, 0 missed, 1 of its own"), "{}", lines[1]);
    assert!(lines[2].contains("slide 2") && lines[2].contains("hand") && lines[3].contains("only auto"), "{lines:?}");
}

#[test]
fn early_late_and_missed_changes_are_told_apart() {
    let shown = |at: f64, slide: usize, by_hand: bool| Shown { at, slide, words: true, by_hand };
    let take = Take { item: None, title: String::new(), from: 0.0, until: 100.0, slides: vec![String::new(); 6], labels: vec![], heard: vec![],
        screen: vec![shown(0.0, 0, true), shown(10.0, 1, true), shown(20.0, 2, true), shown(30.0, 3, false), shown(40.0, 4, true), shown(50.0, 1, true)] };
    // On time, 3 s early, 5 s late, never; a change back to slide 2 long after is not the tape's; one of its own.
    let auto = [shown(0.0, 0, true), shown(10.5, 1, false), shown(17.0, 2, false), shown(35.0, 3, false), shown(70.0, 1, false), shown(80.0, 5, false)];
    let s = score(&take, &auto);
    assert_eq!(s.changes.iter().map(|c| c.auto_at).collect::<Vec<_>>(), [Some(10.5), Some(17.0), Some(35.0), None, None]);
    assert_eq!((s.on_time(), s.early(), s.late(), s.missed()), (1, 1, 1, 2));
    assert_eq!(s.extra.iter().map(|e| e.slide).collect::<Vec<_>>(), [1, 5]);
    // The same slide: 0–10, 10.5–17, 20–30, 35–40 and 70–80 of 100 s.
    assert!((s.same - 0.415).abs() < 0.01, "{}", s.same);
    // Words off and on again on the same slide is not a change of slide.
    let blank = [shown(0.0, 0, true), Shown { at: 5.0, slide: 0, words: false, by_hand: false }, shown(8.0, 0, false), shown(10.0, 1, false)];
    assert_eq!(changes(&blank).len(), 1);
}

#[test]
fn the_report_names_every_song_and_counts_what_was_on_time() {
    let dir = std::env::temp_dir().join(format!("rp-follow-review-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(LINES_FILE), tape_lines(&[(VERSE_2_AT - 0.5, 1)])).unwrap();
    let report = report(&dir, &Settings::default(), None).unwrap();
    assert!(report[0].starts_with("Practice tape rp-follow-review-"), "{}", report[0]);
    assert_eq!(report[1], "1 of 1 slide changes made on time by auto-follow as it is now.");
    assert!(report[2].starts_with("Amazing Grace"));
    std::fs::write(dir.join(LINES_FILE), "{\"a\":0,\"tape\":{}}\n{\"a\":3,\"end\":true}").unwrap();
    assert_eq!(super::report(&dir, &Settings::default(), None).unwrap()[1], "No song was live while this tape ran.");
    assert!(super::report(&dir.join("nope"), &Settings::default(), None).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_sound_can_be_heard_again_by_another_ear() {
    let dir = std::env::temp_dir().join(format!("rp-follow-rehear-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    // Twelve seconds of (silent) sound, and a tape that heard nothing at the time.
    let mut wav = vec![0u8; 44];
    wav.extend(std::iter::repeat_n(0u8, 12 * 32_000));
    std::fs::write(dir.join(SOUND_FILE), wav).unwrap();
    let take = Take { item: None, title: "Amazing Grace".into(), from: 1.0, until: 12.0, slides: SONG.iter().map(|s| s.to_string()).collect(), labels: vec![], heard: vec![],
        screen: vec![Shown { at: 1.0, slide: 0, words: true, by_hand: true }] };
    assert_eq!(replay(&take, &Settings::default()).0.len(), 1, "nothing heard, nothing moved");
    let words = format!("{VERSE_1} {VERSE_2}");
    let (auto, trace) = rehear(&dir, &take, &Settings::default(), |_| Box::new(FakeEar::sings(2.0, &words))).unwrap();
    assert_eq!(auto.iter().skip(1).map(|s| s.slide).collect::<Vec<_>>(), [1, 2], "{trace:?}");
    assert!(auto[1].at >= 4.0 && auto[1].at < VERSE_2_AT, "as the first slide's last line is sung: {}", auto[1].at);
    let _ = std::fs::remove_dir_all(dir);
}
