//! The bench for the real ear: not run with the other tests (`--ignored`). It hears a recording with the speech
//! model on this computer and prints what the follower makes of it, to see how the ear and the follower behave on
//! real sound.
//!
//! - `RP_LAB_MODEL`: a model's folder (`<data>/deps/follow-models/wav2vec2-base-960h`).
//! - `RP_LAB_DIR`: a folder with `sound.wav` (16 kHz mono, 16 bit) and `lyrics.txt` (a song as the library has it),
//!   or a practice tape's folder (then every song on the tape is heard again).
//! - `RP_LAB_LINES`: lines to a slide (2).
use super::audio::Ear;
use super::ear::{Acoustic, Lexicon, Stream};
use super::review::{self, Shown, Take};
use super::rules::Settings;
use super::tape::{LINES_FILE, SOUND_FILE};
use std::path::PathBuf;
use std::time::Instant;

fn model() -> Option<super::model::Ctc> {
    let dir = PathBuf::from(std::env::var("RP_LAB_MODEL").ok()?);
    Some(super::model::load(&dir).expect("the model loads"))
}

fn clock(t: f64) -> String { format!("{}:{:04.1}", (t / 60.0) as u64, t % 60.0) }

#[test]
#[ignore]
fn lab_hear_a_recording_with_the_real_ear() {
    let (Ok(dir), Some(_)) = (std::env::var("RP_LAB_DIR"), model()) else { return };
    let dir = PathBuf::from(dir);
    let settings = Settings::default();
    let takes: Vec<Take> = if dir.join(LINES_FILE).is_file() {
        review::read(&dir).unwrap()
    } else {
        let lyrics = std::fs::read_to_string(dir.join("lyrics.txt")).unwrap();
        let lines = std::env::var("RP_LAB_LINES").ok().and_then(|l| l.parse().ok()).unwrap_or(2);
        let song = crate::model::Song { id: uuid::Uuid::new_v4(), title: "Lab".into(), lyrics, ..Default::default() };
        let slides = crate::slides::song_slides(&song, &crate::model::Settings { lines_per_slide: lines, credits: "none".into(), ..Default::default() });
        let words = |s: &crate::model::Slide| match &s.content { crate::model::SlideContent::Text { text, .. } => text.clone(), _ => String::new() };
        let secs = (std::fs::metadata(dir.join(SOUND_FILE)).unwrap().len() - 44) as f64 / 32_000.0;
        vec![Take { item: None, title: "Lab".into(), from: 0.0, until: secs, slides: slides.iter().map(words).collect(), labels: vec![], heard: vec![],
            screen: vec![Shown { at: 0.0, slide: 0, words: true, by_hand: true }] }]
    };
    for take in &takes {
        println!("== {} ({} to {})", take.title, clock(take.from), clock(take.until));
        for (i, s) in take.slides.iter().enumerate() { println!("slide {:>2}: {}", i + 1, s.replace('\n', " / ")) }
        let began = Instant::now();
        let (auto, trace) = review::rehear(&dir, take, &settings, |l| Box::new(Stream::new(Box::new(model().unwrap()), l)) as Box<dyn Ear>).unwrap();
        let took = began.elapsed().as_secs_f64();
        for l in &trace { println!("{l}") }
        println!("shown: {}", auto.iter().map(|s| format!("{} slide {}{}", clock(s.at), s.slide + 1, if s.words { "" } else { " (words off)" })).collect::<Vec<_>>().join(", "));
        println!("heard {:.0} s of sound in {took:.1} s ({:.2} of real time)", take.until - take.from, took / (take.until - take.from).max(0.1));
        if take.screen.len() > 1 { for l in review::score(take, &auto).lines(take) { println!("{l}") } }
    }
}

/// How long the model takes for the windows the ear could use.
#[test]
#[ignore]
fn lab_time_the_model() {
    let Some(m) = model() else { return };
    let pcm: Vec<f32> = (0..48_000).map(|i| ((i as f32 * 0.05).sin() + (i as f32 * 0.013).sin()) * 0.2).collect();
    m.log_probs(&pcm[..16_000]).unwrap();
    for secs in [0.5, 1.0, 1.5, 2.0, 3.0] {
        let n = (secs * 16_000.0) as usize;
        let began = Instant::now();
        for _ in 0..5 { m.log_probs(&pcm[..n]).unwrap(); }
        println!("{secs:.1} s of sound: {:.0} ms", began.elapsed().as_secs_f64() * 200.0);
    }
    let _ = Lexicon::default();
}
