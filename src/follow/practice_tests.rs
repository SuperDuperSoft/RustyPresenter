use super::*;
use crate::follow::audio::NoEar;
use crate::follow::testing::{FakeEar, VERSE_1, VERSE_2};
use crate::follow::log::LogAction;
use crate::follow::testing::{SONG, text};
use crate::follow::voice::tests::{silence, singing};

/// A 16 kHz mono 16-bit WAV file.
fn wav(pcm: &[i16]) -> Vec<u8> {
    let data = (pcm.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&16_000u32.to_le_bytes());
    b.extend_from_slice(&32_000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for s in pcm { b.extend_from_slice(&s.to_le_bytes()) }
    b
}

#[cfg(unix)]
#[test]
fn practice_runs_on_a_file() {
    let dir = std::env::temp_dir().join(format!("rp-follow-practice-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Sunday.wav");
    // Singing, an instrumental break (here: quiet), singing again.
    let pcm: Vec<i16> = [singing(2.0), silence(5.0), singing(2.0)].concat();
    std::fs::write(&file, wav(&pcm)).unwrap();
    // A stand-in FFmpeg: the WAV's samples, without the header.
    let sh = Path::new("/bin/sh");
    let args: Vec<OsString> = vec!["-c".into(), "tail -c +45 \"$0\"".into(), file.clone().into()];
    let slides: Vec<Slide> = SONG.iter().map(|t| text(t)).collect();
    let log = practice_with(sh, args, slides.clone(), Settings::default(), Box::new(NoEar)).unwrap();
    let acts: Vec<LogAction> = log.iter().map(|l| l.action).collect();
    assert_eq!(acts, [LogAction::Blank, LogAction::Unblank(0)], "{log:?}");
    // Words off about 3 s into the break; back as singing starts again (7 s in).
    assert!((4.9..5.8).contains(&log[0].at), "{}", log[0].at);
    assert!((7.0..7.8).contains(&log[1].at), "{}", log[1].at);
    assert_eq!(log[0].reason, "no singing for 3 s");

    // FFmpeg can't read the file: its words come back as the error.
    let bad: Vec<OsString> = vec!["-c".into(), "echo 'Sunday.wav: Invalid data found' >&2; exit 1".into()];
    assert_eq!(practice_with(sh, bad, slides, Settings::default(), Box::new(NoEar)), Err("Sunday.wav: Invalid data found".into()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn fake_ear_drives_follower_end_to_end() {
    let dir = std::env::temp_dir().join(format!("rp-follow-ear-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Sunday.wav");
    std::fs::write(&file, wav(&singing(11.0))).unwrap();
    let sh = Path::new("/bin/sh");
    let args: Vec<OsString> = vec!["-c".into(), "tail -c +45 \"$0\"".into(), file.clone().into()];
    let slides: Vec<Slide> = SONG.iter().map(|t| text(t)).collect();
    // Verse 1 from 0.5 s, verse 2 from 4.5 s, heard by the ear as the sound goes by.
    let mut ear = FakeEar::sings(0.5, VERSE_1);
    ear.words.extend(FakeEar::sings(4.5, VERSE_2).words);
    let log = practice_with(sh, args, slides, Settings::default(), Box::new(ear)).unwrap();
    let acts: Vec<LogAction> = log.iter().map(|l| l.action).collect();
    assert_eq!(acts, [LogAction::Show(1), LogAction::Show(2)], "{log:?}");
    // Verse 2's words are up before it is sung.
    assert!(log[0].at < 4.5, "{}", log[0].at);
    assert!(log[0].reason.starts_with("heard \""), "{}", log[0].reason);
    let _ = std::fs::remove_dir_all(&dir);
}
