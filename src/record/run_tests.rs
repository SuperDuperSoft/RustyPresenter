use super::*;
use crate::record::sound::Heard;

/// The FFmpeg on this machine, if any: these tests record for real.
fn ffmpeg() -> Option<PathBuf> {
    let found = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("ffmpeg")).find(|p| p.is_file()));
    if found.is_none() { eprintln!("no ffmpeg here: skipped") }
    found
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rp-record-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// `ffprobe`'s view of a file: (codec type, codec, width, height) per stream, and the length in seconds.
fn probe(file: &Path) -> (Vec<(String, String, u64, u64)>, f64) {
    let out = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_type,codec_name,width,height:format=duration", "-of", "json"])
        .arg(file).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let streams = v["streams"].as_array().unwrap().iter().map(|s| (
        s["codec_type"].as_str().unwrap_or("").to_string(), s["codec_name"].as_str().unwrap_or("").to_string(),
        s["width"].as_u64().unwrap_or(0), s["height"].as_u64().unwrap_or(0),
    )).collect();
    let secs = v["format"]["duration"].as_str().and_then(|d| d.parse().ok()).unwrap_or(0.0);
    (streams, secs)
}

fn plan(ffmpeg: PathBuf, dir: &Path, sound: Sound) -> Plan {
    Plan { ffmpeg, encoder: "libx264", preset: Preset::P720, sound, out: dir.join("Recording test.mp4") }
}

/// Records for `secs`, handing in a new solid picture every 100 ms (red, then green), with `each` called as it goes.
fn record_for(plan: Plan, secs: f64, each: impl Fn(&FrameSink, f64)) -> Ended {
    let (tx, rx) = mpsc::channel();
    let rec = Recording::start(plan, move |e| { let _ = tx.send(e); }).expect("starts");
    let sink = rec.sink().clone();
    let t = Instant::now();
    while t.elapsed().as_secs_f64() < secs {
        let colour: [u8; 4] = if t.elapsed().as_secs_f64() < secs / 2.0 { [255, 0, 0, 255] } else { [0, 255, 0, 255] };
        let mut b = sink.buffer();
        for px in b.chunks_exact_mut(4) { px.copy_from_slice(&colour) }
        sink.put(b);
        each(&sink, t.elapsed().as_secs_f64());
        std::thread::sleep(Duration::from_millis(100));
    }
    rec.stop();
    let ended = rx.recv_timeout(Duration::from_secs(60)).expect("finished");
    assert!(rec.wait(Duration::from_secs(5)));
    ended
}

#[test]
fn a_short_recording_makes_an_h264_mp4_of_the_preset_size_and_cleans_up() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("short");
    let ended = record_for(plan(ff, &dir, Sound::None), 2.0, |_, _| {});
    assert_eq!(ended, Ended { file: Some(dir.join("Recording test.mp4")), problem: None });
    let (streams, secs) = probe(&dir.join("Recording test.mp4"));
    assert_eq!(streams, [("video".into(), "h264".into(), 1280, 720)]);
    assert!((1.6..3.0).contains(&secs), "{secs} s");
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(left.len(), 1, "only the MP4 is left: {left:?}");
    assert!(!ended.toast().1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_apps_sound_is_mixed_in_from_the_file_that_played() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("sound");
    let song = dir.join("song.wav");
    assert!(std::process::Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=5"]).arg(&song).status().unwrap().success());
    // The song plays from 0.5 s into the recording, from its start.
    let ended = record_for(plan(ff, &dir, Sound::App), 2.5, |sink, _| {
        let Some(t) = sink.elapsed() else { return };
        let heard: Vec<Heard> = (t >= 0.5).then(|| Heard { key: 1, path: song.clone(), pos: t - 0.5, volume: 1.0 }).into_iter().collect();
        sink.sound.as_ref().unwrap().lock().unwrap().hear(t, &heard);
    });
    assert_eq!(ended.problem, None);
    let (streams, secs) = probe(ended.file.as_ref().unwrap());
    let kinds: Vec<(&str, &str)> = streams.iter().map(|s| (s.0.as_str(), s.1.as_str())).collect();
    assert_eq!(kinds, [("video", "h264"), ("audio", "aac")]);
    assert!((2.0..3.5).contains(&secs), "the sound is cut at the picture's end: {secs} s");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_input_that_cannot_open_ends_the_recording_with_ffmpegs_reason() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("badinput");
    let ended = record_for(plan(ff, &dir, Sound::Input("no-such-input-here".into())), 1.5, |_, _| {});
    let (text, err) = ended.toast();
    assert!(err, "{text}");
    assert!(ended.problem.is_some(), "{ended:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_missing_ffmpeg_does_not_start() {
    let dir = scratch("noff");
    let err = Recording::start(plan(PathBuf::from("/no/such/ffmpeg"), &dir, Sound::None), |_| {}).err().expect("refused");
    assert!(err.starts_with("The recording could not start"), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_part_left_by_a_crash_is_finished_next_time_and_a_broken_one_is_kept_as_mkv() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("left");
    let out = dir.join("Recording old.mp4");
    let part = partial_of(&out);
    assert_eq!(part, dir.join("Recording old.recording.mkv"));
    let made = std::process::Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30:duration=1", "-c:v", "libx264", "-f", "matroska"]).arg(&part).status().unwrap();
    assert!(made.success());
    let broken = dir.join("Recording broken.recording.mkv");
    std::fs::write(&broken, b"not a video at all").unwrap();
    let current = dir.join("Recording now.recording.mkv");
    std::fs::write(&current, b"being written").unwrap();
    let ended = finish_leftovers(&ff, &dir, &current);
    assert_eq!(ended, [
        Ended { file: Some(dir.join("Recording broken.mkv")), problem: None },
        Ended { file: Some(out.clone()), problem: None },
    ]);
    assert!(ended[0].toast().1, "an MKV is not what was asked for");
    assert_eq!(probe(&out).0[0].1, "h264");
    assert!(!part.exists() && current.exists(), "the one being recorded is left alone");
    assert_eq!(leftovers(&dir), [current]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn toasts_say_where_the_file_is_and_what_went_wrong() {
    let mp4 = Ended { file: Some("/r/a.mp4".into()), problem: None };
    assert_eq!(mp4.toast(), ("Recording saved: /r/a.mp4".into(), false));
    let full = Ended { file: Some("/r/a.mp4".into()), problem: Some(tr::disk_full()) };
    assert_eq!(full.toast(), ("The disk is almost full, so the recording stopped. Recording saved: /r/a.mp4".into(), true));
    let none = Ended { file: None, problem: Some("x".into()) };
    assert_eq!(none.toast(), ("x".into(), true));
}

#[test]
fn the_part_and_notes_sit_beside_the_mp4() {
    let out = Path::new("/r/Recording 2026-11-07 10-30-05.mp4");
    assert_eq!(partial_of(out), Path::new("/r/Recording 2026-11-07 10-30-05.recording.mkv"));
    assert_eq!(notes_of(out), Path::new("/r/Recording 2026-11-07 10-30-05.recording.json"));
}
