use super::*;
use crate::record::sound::Heard;

/// The FFmpeg on this machine, if any: these tests record for real.
fn ffmpeg() -> Option<PathBuf> {
    // "ffmpeg.exe" on Windows (only "ffmpeg" was looked for, so on Windows these tests never ran).
    let found = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).flat_map(|d| [d.join("ffmpeg"), d.join("ffmpeg.exe")]).find(|p| p.is_file()));
    if found.is_none() { eprintln!("no ffmpeg here: skipped") }
    if found.is_some() { take_turn() }
    found
}

/// How many of these tests record at once. Each starts several FFmpegs (the recorder, a camera or two, a stand-in
/// streaming server) and then looks at what was recorded at given seconds. All thirty-odd at once, as the test
/// runner starts them, is more than a computer's processors can do in time: a camera's FFmpeg started seconds
/// late, its pictures came too slowly to count as a picture, and one test or another failed about one run in
/// three (never the same one). Four at a time, eight runs in a row had none.
const AT_ONCE: usize = 4;

/// Waits for this test's turn (`AT_ONCE`); the turn is given back when the test's thread ends, passed or failed.
fn take_turn() {
    struct Turn;
    impl Drop for Turn {
        fn drop(&mut self) {
            let (busy, free) = &TURNS;
            *busy.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
            free.notify_one();
        }
    }
    static TURNS: (Mutex<usize>, std::sync::Condvar) = (Mutex::new(0), std::sync::Condvar::new());
    thread_local! { static MINE: std::cell::RefCell<Option<Turn>> = const { std::cell::RefCell::new(None) }; }
    // A test that asks for FFmpeg twice has its turn already.
    if MINE.with(|m| m.borrow().is_some()) { return }
    let (busy, free) = &TURNS;
    let mut n = busy.lock().unwrap_or_else(|e| e.into_inner());
    while *n >= AT_ONCE { n = free.wait(n).unwrap_or_else(|e| e.into_inner()) }
    *n += 1;
    drop(n);
    MINE.with(|m| *m.borrow_mut() = Some(Turn));
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
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    // A file FFprobe cannot read has no streams (the assertion that follows says which file it was).
    let streams = v["streams"].as_array().cloned().unwrap_or_default().iter().map(|s| (
        s["codec_type"].as_str().unwrap_or("").to_string(), s["codec_name"].as_str().unwrap_or("").to_string(),
        s["width"].as_u64().unwrap_or(0), s["height"].as_u64().unwrap_or(0),
    )).collect();
    let secs = v["format"]["duration"].as_str().and_then(|d| d.parse().ok()).unwrap_or(0.0);
    (streams, secs)
}

fn plan(ffmpeg: PathBuf, dir: &Path, sound: Sound) -> Plan {
    Plan { ffmpeg, encoder: "libx264", preset: Preset::P720, sound, out: dir.join("Recording test.mp4"), stream: vec![], pc_sound: None, cameras: vec![], look: None, controls: Controls::default(), sound_fx: args::SoundFx::default(), chapters: None, tall: vec![], kbps: 0, delay_secs: 0, lighter: false, ndi: Default::default(), words_file: false, more_inputs: vec![], listen: None, more: Default::default(), ways: vec![], camera_lost: Default::default() }
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

/// Records, handing in a red picture every 100 ms, for as long as `step` says to go on (it is given the recording
/// and the seconds into the file so far), at most `limit` seconds. For tests that wait for something to happen
/// (a camera's first picture, while the whole suite runs) rather than a fixed second: a real failure still fails,
/// after `limit`.
fn record_while(plan: Plan, limit: f64, mut step: impl FnMut(&Recording, f64) -> bool) -> Ended {
    let (tx, rx) = mpsc::channel();
    let rec = Recording::start(plan, move |e| { let _ = tx.send(e); }).expect("starts");
    let sink = rec.sink().clone();
    let t = Instant::now();
    while t.elapsed().as_secs_f64() < limit {
        let mut b = sink.buffer();
        for px in b.chunks_exact_mut(4) { px.copy_from_slice(&[255, 0, 0, 255]) }
        sink.put(b);
        if !step(&rec, sink.elapsed().unwrap_or(0.0)) { break }
        std::thread::sleep(Duration::from_millis(100));
    }
    rec.stop();
    let ended = rx.recv_timeout(Duration::from_secs(60)).expect("finished");
    assert!(rec.wait(Duration::from_secs(5)));
    ended
}

/// When things happen in `record_while`: each the seconds into the file at which it did.
#[derive(Default, Debug)]
struct Marks { seen: Option<f64>, changed: Option<f64>, seen_again: Option<f64>, end: Option<f64> }

#[test]
fn a_short_recording_makes_an_h264_mp4_of_the_preset_size_and_cleans_up() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("short");
    let ended = record_for(plan(ff, &dir, Sound::None), 2.0, |_, _| {});
    assert_eq!(ended, Ended { file: Some(dir.join("Recording test.mp4")), problem: None, ..Ended::default() });
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

/// The sound input passes through the app (`stream::board_feed`): one that cannot be opened (unplugged, renamed)
/// gives silence and the recording goes on, where FFmpeg opening it itself ended the recording. The operator hears of
/// it from the no-sound warning.
#[test]
fn an_input_that_cannot_open_gives_silence_and_the_recording_goes_on() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("badinput");
    let ended = record_for(plan(ff, &dir, Sound::Input("no-such-input-here".into())), 2.5, |_, _| {});
    assert_eq!(ended.problem, None, "{ended:?}");
    let (streams, secs) = probe(ended.file.as_ref().unwrap());
    let kinds: Vec<&str> = streams.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(kinds, ["video", "audio"]);
    assert!((2.0..3.5).contains(&secs), "{secs} s");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Live with a sound delay: FFmpeg takes the held-back sound, and the file has picture and sound to the end.
#[test]
fn a_sound_delay_keeps_the_stream_going() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("delay");
    let dead = format!("rtmp://127.0.0.1:{}/live/TEST-KEY", std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port());
    let ended = record_for(Plan { stream: vec![dead], sound_fx: args::SoundFx { delay_ms: 300, even: true, ..args::SoundFx::default() }, ..plan(ff, &dir, Sound::Input("no-such-input-here".into())) }, 3.0, |_, _| {});
    assert_eq!(ended.problem, None, "{ended:?}");
    let (streams, secs) = probe(ended.file.as_ref().unwrap());
    let kinds: Vec<&str> = streams.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(kinds, ["video", "audio"]);
    assert!((2.4..4.0).contains(&secs), "{secs} s");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Live with a second input (each on its own track) and Listen on: both inputs come through the app (silence by
/// the clock: neither is plugged in here), the file has the mix and a track for each, and the headphones' copy
/// never holds FFmpeg up (where this computer has no sound output, Listen does not start and nothing changes).
#[test]
fn a_second_input_is_mixed_and_listen_gets_a_copy() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("second-input");
    let dead = format!("rtmp://127.0.0.1:{}/live/TEST-KEY", std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port());
    let p = Plan {
        stream: vec![dead], sound_fx: args::SoundFx { tracks: true, board_filters: args::Filters::SAFE, ..args::SoundFx::default() },
        more_inputs: vec![("no-such-second-input".into(), 4, args::Filters { gate: args::Strength::Light, ..args::Filters::SAFE })],
        listen: Some(crate::stream::listen::DEFAULT.into()),
        ..plan(ff, &dir, Sound::Input("no-such-input-here".into()))
    };
    let ended = record_for(p, 3.0, |_, _| {});
    assert_eq!(ended.problem, None, "{ended:?}");
    let (streams, secs) = probe(ended.file.as_ref().unwrap());
    let kinds: Vec<&str> = streams.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(kinds, ["video", "audio", "audio", "audio"]);
    assert!((2.0..4.0).contains(&secs), "{secs} s");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Hold: the recording (and so the stream) keeps the picture it had; set off, the live picture again.
#[test]
fn hold_keeps_the_picture_until_it_is_let_go() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("hold");
    let p = plan(ff.clone(), &dir, Sound::None);
    let hold = p.controls.hold.clone();
    // The room: red, then green from 2 s. Held from 1 s to 3 s.
    let ended = record_for(p, 4.0, |_, t| hold.store((1.0..3.0).contains(&t), std::sync::atomic::Ordering::Relaxed));
    assert_eq!(ended.problem, None);
    let rec = ended.file.unwrap();
    let at = |t: &str| {
        let png = dir.join(format!("f{t}.png"));
        assert!(std::process::Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-y", "-ss", t, "-i"]).arg(&rec).args(["-frames:v", "1"]).arg(&png)
            .status().unwrap().success());
        image::open(&png).unwrap().to_rgb8().get_pixel(640, 360).0
    };
    let (held, after) = (at("2.6"), at("3.6"));
    assert!(held[0] > 180 && held[1] < 60, "still red while held, though the room is green: {held:?}");
    assert!(after[1] > 180 && after[0] < 60, "green once let go: {after:?}");
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
        Ended { file: Some(dir.join("Recording broken.mkv")), problem: None, kept_mkv: true, ..Ended::default() },
        Ended { file: Some(out.clone()), problem: None, ..Ended::default() },
    ]);
    assert!(ended[0].toast().1, "an MKV is not what was asked for");
    assert_eq!(probe(&out).0[0].1, "h264");
    assert!(!part.exists() && current.exists(), "the one being recorded is left alone");
    assert_eq!(leftovers(&dir), [current]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn toasts_say_where_the_file_is_and_what_went_wrong() {
    let mp4 = Ended { file: Some("/r/a.mp4".into()), problem: None, ..Ended::default() };
    assert_eq!(mp4.toast(), ("Recording saved: /r/a.mp4".into(), false));
    let full = Ended { file: Some("/r/a.mp4".into()), problem: Some(tr::disk_full()), ..Ended::default() };
    // An MKV asked for in More settings is no trouble; cut into pieces, the first is named and the rest counted.
    assert_eq!(Ended { file: Some("/r/a.mkv".into()), ..Ended::default() }.toast().1, false);
    let pieces = Ended { file: Some("/r/a part 1.mp4".into()), pieces: vec!["/r/a part 1.mp4".into(), "/r/a part 2.mp4".into()], ..Ended::default() };
    assert_eq!(pieces.toast(), (tr::saved_pieces("/r/a part 1.mp4", 2), false));
    assert_eq!(full.toast(), ("The disk is almost full, so the recording stopped. Recording saved: /r/a.mp4".into(), true));
    let none = Ended { file: None, problem: Some("x".into()), ..Ended::default() };
    assert_eq!(none.toast(), ("x".into(), true));
}

#[test]
fn the_part_and_notes_sit_beside_the_mp4() {
    let out = Path::new("/r/Recording 2026-11-07 10-30-05.mp4");
    assert_eq!(partial_of(out), Path::new("/r/Recording 2026-11-07 10-30-05.recording.mkv"));
    assert_eq!(notes_of(out), Path::new("/r/Recording 2026-11-07 10-30-05.recording.json"));
}

/// Go Live for real, on this computer alone: a second FFmpeg listens as a streaming service would (RTMP on
/// 127.0.0.1). The recorder is given its address and one that nobody listens on. Eight seconds in, the server is
/// killed (the internet goes) and a new one listens on the same address: the stream must come back by itself.
#[test]
fn go_live_reaches_a_server_survives_a_dead_one_and_comes_back_after_a_drop() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("live");
    let port = |()| std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let (good, dead) = (format!("rtmp://127.0.0.1:{}/live/TEST-KEY-1234", port(())), format!("rtmp://127.0.0.1:{}/live/DEAD-KEY-5678", port(())));
    let listen = |to: &Path| std::process::Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-y", "-listen", "1", "-analyzeduration", "2000000", "-f", "flv", "-i", &good, "-c", "copy"])
        .arg(to).stdin(Stdio::null()).stderr(std::fs::File::create(to.with_extension("log")).unwrap()).spawn().expect("the listening FFmpeg starts");
    let (first, second) = (dir.join("received-1.flv"), dir.join("received-2.flv"));
    let servers = std::cell::RefCell::new(vec![listen(&first)]);
    std::thread::sleep(Duration::from_millis(1500));
    let ended = record_for(Plan { stream: vec![dead, good.clone()], ..plan(ff.clone(), &dir, Sound::None) }, 28.0, |_, t| {
        let mut servers = servers.borrow_mut();
        if t >= 8.0 && servers.len() == 1 {
            let _ = servers[0].kill();
            let _ = servers[0].wait();
            servers.push(listen(&second));
        }
    });
    // The second server stops when the stream ends.
    let mut servers = servers.into_inner();
    let t = Instant::now();
    while servers[1].try_wait().unwrap().is_none() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(100)) }
    let _ = servers[1].kill();
    let _ = servers[1].wait();
    // The recording never waited for the internet: it is whole, from its first second (with the silent sound
    // a stream needs).
    assert_eq!((ended.file.clone(), ended.problem.clone()), (Some(dir.join("Recording test.mp4")), None), "{ended:?}");
    let (streams, secs) = probe(&dir.join("Recording test.mp4"));
    assert!(streams.contains(&("video".into(), "h264".into(), 1280, 720)) && (25.0..31.0).contains(&secs), "{streams:?} {secs} s");
    // (What the first server got is not looked at: it was killed in the middle of writing its file. That a
    // server gets picture and sound, with the other destination dead, is what the second one shows.)
    // And the stream came back to the new server by itself (some seven seconds after the drop: the wait, the
    // next keyframe, three seconds of looking at the feed, the connection).
    let (again, again_secs) = probe(&second);
    assert!(again.contains(&("video".into(), "h264".into(), 1280, 720)), "the stream did not come back: {again:?}; the new server got {} bytes and said: {}",
        std::fs::metadata(&second).map(|m| m.len()).unwrap_or(0), std::fs::read_to_string(second.with_extension("log")).unwrap_or_default());
    assert!(again.iter().any(|s| s.0 == "audio" && s.1 == "aac"), "{again:?}");
    assert!(again_secs > 3.0, "{again_secs} s arrived after the drop");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The PC's sound in a stream, for real: while the recorder is live to a local server, a quiet tone plays on this
/// computer's default output (FFplay); the server must hear it. Plays a tone: run by hand with `--ignored`.
#[cfg(windows)]
#[test]
#[ignore]
fn go_live_sends_what_the_pc_plays() {
    let Some(ff) = ffmpeg() else { return };
    let ffplay = ff.with_file_name("ffplay.exe");
    if !ffplay.is_file() { return eprintln!("no ffplay here: skipped") }
    let dir = scratch("pcsound");
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let url = format!("rtmp://127.0.0.1:{port}/live/TEST-KEY");
    let received = dir.join("received.flv");
    let mut server = std::process::Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-y", "-listen", "1", "-analyzeduration", "2000000", "-f", "flv", "-i", &url, "-c", "copy"])
        .arg(&received).stdin(Stdio::null()).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let tone = std::cell::RefCell::new(None::<std::process::Child>);
    let ended = record_for(Plan { stream: vec![url.clone()], pc_sound: Some(String::new()), ..plan(ff.clone(), &dir, Sound::None) }, 12.0, |_, t| {
        // A 1 kHz tone, quiet, from 4 s to 7 s.
        if t >= 4.0 && tone.borrow().is_none() {
            *tone.borrow_mut() = Some(std::process::Command::new(&ffplay).args(["-hide_banner", "-loglevel", "error", "-nodisp", "-autoexit", "-f", "lavfi",
                "-i", "sine=frequency=1000:duration=3,volume=0.1"]).stdin(Stdio::null()).spawn().unwrap());
        }
    });
    if let Some(mut c) = tone.into_inner() { let _ = c.wait(); }
    let t = Instant::now();
    while server.try_wait().unwrap().is_none() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(100)) }
    let _ = server.kill();
    let _ = server.wait();
    assert_eq!(ended.problem, None, "{ended:?}");
    // The loudest moment of what the server got, and of its first three seconds (before the tone).
    let loudest_of = |file: &Path, extra: &[&str]| -> f64 {
        let out = std::process::Command::new(&ff).args(["-hide_banner", "-nostats"]).args(extra).arg("-i").arg(file)
            .args(["-vn", "-af", "volumedetect", "-f", "null", "-"]).output().unwrap();
        let text = String::from_utf8_lossy(&out.stderr);
        text.lines().find_map(|l| l.split("max_volume:").nth(1)).and_then(|v| v.trim().trim_end_matches("dB").trim().parse().ok()).unwrap_or(-200.0)
    };
    let (all, before) = (loudest_of(&received, &[]), loudest_of(&received, &["-t", "2"]));
    eprintln!("REAL stream loudest {all} dB; before the tone {before} dB; the recording {} dB", loudest_of(&dir.join("Recording test.mp4"), &[]));
    assert!(all > -40.0, "the tone did not reach the stream: {all} dB");
    std::fs::remove_dir_all(&dir).unwrap();
}

use crate::stream::scene::{AutoPrefs, Choice, LiveInfo, Scene, SlideKind};

/// The scenes as the operator would have them: `live` is what is live (changed by the test as it goes), the live
/// slide's words are `words`.
fn look(live: Arc<Mutex<(LiveInfo, Choice)>>, words: &str, auto: AutoPrefs) -> crate::stream::pictures::Look {
    let slide = crate::live::SlideLayer::Text { text: words.into(), footer: None, reference: None, scripture: false, size: 0, passage: vec![], parallel: None, theme: None };
    crate::stream::pictures::Look {
        info: Arc::new(move || { let (i, c) = live.lock().unwrap().clone(); (i, c, None) }),
        words: Arc::new(move || crate::stream::lyrics::key_for(&slide, &crate::model::Theme::default(), (1280, 720))),
        auto, cards: Default::default(), theme: Default::default(), layout: Default::default(), own: vec![], more: Default::default(),
    }
}

/// One scene, chosen by hand, for the whole stream.
fn only(scene: Scene, words: &str) -> crate::stream::pictures::Look {
    look(Arc::new(Mutex::new((LiveInfo::default(), Choice::pick(Some(scene), None)))), words, AutoPrefs::default())
}

/// A scene change fades over half a second, from the picture going out to the new scene's.
#[test]
fn the_mixer_fades_between_scenes() {
    use crate::stream::pictures::Shown;
    let (red, blue) = ([200u8, 0, 0, 255].repeat(4), Arc::new([0u8, 0, 200, 255].repeat(4)));
    let mut m = Mixer::new((2, 2));
    let t0 = Instant::now();
    m.update(&Shown { scene: Scene::Screen, ..Shown::default() }, &red, Some(blue.clone()), true, t0);
    assert_eq!(&m.out()[..4], &[200, 0, 0, 255], "the room at once: nothing to fade from yet");
    let camera = Shown { scene: Scene::Camera, ..Shown::default() };
    m.update(&camera, &red, Some(blue.clone()), false, t0);
    assert_eq!(&m.out()[..4], &[200, 0, 0, 255], "the fade starts from the room");
    m.update(&camera, &red, Some(blue.clone()), false, t0 + Duration::from_millis(250));
    assert_eq!(&m.out()[..4], &[100, 0, 100, 255], "half way");
    m.update(&camera, &red, Some(blue.clone()), false, t0 + Duration::from_millis(600));
    assert_eq!(&m.out()[..4], &[0, 0, 200, 255], "the camera once the fade is over");
}

/// A camera switched with Fade blends from the camera before over the same half second as a scene change; one
/// switched without is there at once.
#[test]
fn a_camera_switch_can_fade_from_the_camera_before() {
    use crate::stream::pictures::Shown;
    let (blue, green) = (Arc::new([0u8, 0, 200, 255].repeat(4)), Arc::new([0u8, 200, 0, 255].repeat(4)));
    let room = [0u8; 16];
    let camera = Shown { scene: Scene::Camera, ..Shown::default() };
    let mut m = Mixer::new((2, 2));
    let t0 = Instant::now();
    m.update(&camera, &room, Some(blue.clone()), true, t0);
    assert_eq!(&m.out()[..4], &[0, 0, 200, 255]);
    m.fade_from_now(t0);
    m.update(&camera, &room, Some(green.clone()), false, t0 + Duration::from_millis(250));
    assert_eq!(&m.out()[..4], &[0, 100, 100, 255], "half way from the first camera to the second");
    m.update(&camera, &room, Some(green.clone()), false, t0 + Duration::from_millis(600));
    assert_eq!(&m.out()[..4], &[0, 200, 0, 255], "the second camera once the fade is over");
    m.update(&camera, &room, Some(blue.clone()), false, t0 + Duration::from_millis(700));
    assert_eq!(&m.out()[..4], &[0, 0, 200, 255], "a cut: at once");
}

/// A card scene does not depend on the camera or the room: their new pictures do not make it put together again.
#[test]
fn a_card_is_not_put_together_again_for_each_camera_picture() {
    use crate::stream::pictures::Shown;
    let room = [9u8, 9, 9, 255].repeat(4);
    let card = Shown { scene: Scene::Hold, card: Some(Arc::new([0u8, 0, 0, 0].repeat(4))), ..Shown::default() };
    let mut m = Mixer::new((2, 2));
    let t0 = Instant::now();
    m.update(&card, &room, Some(Arc::new([1u8; 16].to_vec())), true, t0);
    let after_first = m.composes;
    m.update(&card, &room, Some(Arc::new([2u8; 16].to_vec())), true, t0 + Duration::from_millis(33));
    m.update(&card, &room, Some(Arc::new([3u8; 16].to_vec())), true, t0 + Duration::from_millis(66));
    assert_eq!(m.composes, after_first, "new camera and room pictures change nothing on a card");
}

/// The church's own scene shows the cameras it names, whichever is on air, and is put together again only when one
/// of them (or the room it shows) has a new picture.
#[test]
fn the_mixer_puts_a_church_scene_together_from_the_cameras_it_names() {
    use crate::stream::own_scenes::{Layer, OwnScene, Place, Shows, prepare};
    use crate::stream::pictures::Shown;
    let ours = OwnScene { id: uuid::Uuid::from_u128(3), name: "Two cameras".into(), layers: vec![
        Layer { shows: Shows::Camera { number: 1 }, place: Place::LeftHalf, ..Layer::default() },
        Layer { shows: Shows::Camera { number: 2 }, place: Place::RightHalf, ..Layer::default() },
    ], ..OwnScene::default() };
    let size = (4, 2);
    let p = Arc::new(prepare(&ours, size, &Default::default(), &mut crate::render::Fonts::bundled_only()));
    let shown = Shown { scene: Scene::Own(ours.id), own: Some(p), ..Shown::default() };
    let solid = |c: [u8; 4]| Arc::new(c.repeat(8));
    let (red, blue, room) = (solid([200, 0, 0, 255]), solid([0, 0, 200, 255]), [9u8, 9, 9, 255].repeat(8));
    let mut m = Mixer::new(size);
    m.cameras = vec![Some(red.clone()), Some(blue.clone()), Some(solid([1, 1, 1, 255]))];
    let t0 = Instant::now();
    m.update(&shown, &room, Some(solid([7, 7, 7, 255])), true, t0);
    assert_eq!((&m.out()[..4], &m.out()[12..16]), (&[200, 0, 0, 255][..], &[0, 0, 200, 255][..]), "camera 1 on the left, camera 2 on the right");
    let first = m.composes;
    m.cameras[2] = Some(solid([2, 2, 2, 255]));
    m.update(&shown, &room, Some(solid([8, 8, 8, 255])), true, t0 + Duration::from_millis(33));
    assert_eq!(m.composes, first, "a camera it does not show, the camera on air and the room change nothing");
    m.cameras[1] = Some(solid([0, 200, 0, 255]));
    m.update(&shown, &room, None, false, t0 + Duration::from_millis(66));
    assert_eq!(&m.out()[12..16], &[0, 200, 0, 255], "a new picture from camera 2");
}

/// A scene can come in with a slide or a cut instead of the fade.
#[test]
fn the_mixer_changes_scenes_the_way_the_scene_asks() {
    use crate::stream::{pictures::Shown, transition::Transition};
    let (red, blue) = ([200u8, 0, 0, 255].repeat(4), Arc::new([0u8, 0, 200, 255].repeat(4)));
    let t0 = Instant::now();
    let mut m = Mixer::new((2, 2));
    m.update(&Shown { scene: Scene::Screen, ..Shown::default() }, &red, Some(blue.clone()), true, t0);
    let camera = Shown { scene: Scene::Camera, transition: Transition::Slide, ..Shown::default() };
    m.update(&camera, &red, Some(blue.clone()), false, t0);
    m.update(&camera, &red, Some(blue.clone()), false, t0 + Duration::from_millis(250));
    assert_eq!((&m.out()[..4], &m.out()[4..8]), (&[200u8, 0, 0, 255][..], &[0u8, 0, 200, 255][..]), "half way: the room going out left, the camera coming in right");
    m.update(&camera, &red, Some(blue.clone()), false, t0 + Duration::from_millis(600));
    assert_eq!(&m.out()[..4], &[0, 0, 200, 255]);
    let cut = Shown { scene: Scene::Screen, transition: Transition::Cut, ..Shown::default() };
    m.update(&cut, &red, Some(blue.clone()), false, t0 + Duration::from_millis(700));
    assert_eq!(&m.out()[..4], &[200, 0, 0, 255], "a cut: at once");
}

/// With the church's tall scenes the Mixer puts the tall picture together: the scene's tall places, or the wide
/// picture fitted across for a scene without them.
#[test]
fn the_mixer_puts_the_tall_picture_together() {
    use crate::stream::own_scenes::{Layer, Now, OwnScene, Place, Shows, prepare_at};
    use crate::stream::pictures::Shown;
    let solid = |c: [u8; 4], n: usize| Arc::new(c.repeat(n));
    let (wide, tall) = ((4, 2), (2, 4));
    let room = [0u8, 0, 200, 255].repeat(8);
    let mut m = Mixer { tall: Some(TallMix::new(tall)), ..Mixer::new(wide) };
    let t0 = Instant::now();
    m.update(&Shown { scene: Scene::Screen, ..Shown::default() }, &room, None, true, t0);
    let t = m.tall_out().expect("a new tall picture").to_vec();
    assert_eq!((&t[..4], &t[12..16]), (&[0u8, 0, 0, 255][..], &[0u8, 0, 200, 255][..]), "black above, the wide picture across the middle");
    assert!(m.tall_out().is_none(), "taken once until it changes");
    let ours = OwnScene { tall: true, layers: vec![
        Layer { shows: Shows::Room, ..Layer::default() },
        Layer { shows: Shows::Camera { number: 0 }, place: Place::RightHalf, tall: Some(Place::Box { x: 0, y: 50, w: 100, h: 50 }), ..Layer::default() },
    ], ..OwnScene::new("Phones".into()) };
    let mut fonts = crate::render::Fonts::bundled_only();
    let now = Now::local(None);
    let shown = Shown { scene: Scene::Own(ours.id), own: Some(Arc::new(prepare_at(&ours, wide, false, &Default::default(), &mut fonts, &now))),
        own_tall: Some(Arc::new(prepare_at(&ours, tall, true, &Default::default(), &mut fonts, &now))), transition: crate::stream::transition::Transition::Cut, ..Shown::default() };
    m.update(&shown, &room, Some(solid([200, 0, 0, 255], 8)), true, t0 + Duration::from_millis(33));
    let t = m.tall_out().expect("the tall scene").to_vec();
    assert_eq!((&t[..4], &t[28..32]), (&[0u8, 0, 200, 255][..], &[200u8, 0, 0, 255][..]), "the room on top, the camera across the bottom");
    assert_eq!((&m.out()[..4], &m.out()[12..16]), (&[0u8, 0, 200, 255][..], &[200u8, 0, 0, 255][..]), "the wide picture keeps its own places");
}

/// The tall stream for real: the destination gets 720×1280 (the wide picture across the middle), the file stays
/// 1280×720.
#[test]
fn go_live_sends_a_tall_stream_to_a_tall_destination() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("tall");
    let (srv, url, received) = server(&ff, &dir);
    let ended = record_for(Plan { stream: vec![url], tall: vec![true], ..plan(ff.clone(), &dir, Sound::None) }, 6.0, |_, _| {});
    finish_server(srv);
    assert_eq!(ended.problem, None, "{ended:?}");
    let (streams, _) = probe(&received);
    assert!(streams.iter().any(|s| s.0 == "video" && (s.2, s.3) == (720, 1280)), "{streams:?}");
    let (streams, _) = probe(&dir.join("Recording test.mp4"));
    assert!(streams.iter().any(|s| s.0 == "video" && (s.2, s.3) == (1280, 720)), "{streams:?}");
    // The wide picture (red, then green) across the middle, black above and below.
    let img = frame_at(&ff, &dir, &received, "1");
    let (top, mid) = (img.get_pixel(360, 100).0, img.get_pixel(360, 640).0);
    assert!(top.iter().all(|c| *c < 30) && is_red(mid), "top {top:?}, middle {mid:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The church's tall scene for real: the app puts the tall picture together (blue across the top, the room under
/// it) and FFmpeg reads it from the app; the wide picture keeps its own places (all blue).
#[test]
fn go_live_sends_the_churchs_tall_scene_to_a_tall_destination() {
    use crate::stream::own_scenes::{Layer, OwnScene, Place, Shows};
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("tall-scene");
    let (srv, url, received) = server(&ff, &dir);
    let ours = OwnScene { tall: true, layers: vec![
        Layer { shows: Shows::Room, place: Place::LeftHalf, tall: Some(Place::Full), ..Layer::default() },
        Layer { shows: Shows::Colour { colour: "#0000ff".into() }, place: Place::Full, tall: Some(Place::Box { x: 0, y: 0, w: 100, h: 50 }), ..Layer::default() },
    ], ..OwnScene::new("Phones".into()) };
    let mut look = only(Scene::Own(ours.id), "x");
    look.own = vec![ours];
    let ended = record_for(Plan { stream: vec![url], tall: vec![true], look: Some(look), ..plan(ff.clone(), &dir, Sound::None) }, 6.0, |_, _| {});
    finish_server(srv);
    assert_eq!(ended.problem, None, "{ended:?}");
    let (streams, _) = probe(&received);
    assert!(streams.iter().any(|s| s.0 == "video" && (s.2, s.3) == (720, 1280)), "{streams:?}");
    let img = frame_at(&ff, &dir, &received, "1");
    let (top, bottom) = (img.get_pixel(360, 300).0, img.get_pixel(360, 1000).0);
    assert!(top[2] > 180 && top[0] < 60 && is_red(bottom), "top {top:?}, bottom {bottom:?}");
    let wide = frame_at(&ff, &dir, &dir.join("Recording test.mp4"), "1");
    let w = wide.get_pixel(320, 360).0;
    assert!(w[2] > 180 && w[0] < 60, "the wide picture all blue: {w:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Chapters for real: "Welcome" live at first, "Amazing Grace" from half way; the MP4 has both, named.
#[test]
fn a_recording_has_a_chapter_at_each_item() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("chapters");
    let began = Instant::now();
    let source = super::super::chapters::Source(Arc::new(move || Some(if began.elapsed().as_secs_f64() < 2.0 { "Welcome" } else { "Amazing Grace" }.to_string())));
    let ended = record_for(Plan { chapters: Some(source), ..plan(ff, &dir, Sound::None) }, 4.5, |_, _| {});
    assert_eq!(ended.problem, None, "{ended:?}");
    let out = std::process::Command::new("ffprobe").args(["-v", "error", "-show_chapters", "-of", "json"]).arg(dir.join("Recording test.mp4")).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    let chapters: Vec<(String, f64)> = v["chapters"].as_array().cloned().unwrap_or_default().iter()
        .map(|c| (c["tags"]["title"].as_str().unwrap_or("").to_string(), c["start_time"].as_str().and_then(|s| s.parse().ok()).unwrap_or(-1.0))).collect();
    assert_eq!(chapters.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), ["Welcome", "Amazing Grace"], "{chapters:?}");
    assert!(chapters[0].1 < 1.5 && (1.5..3.5).contains(&chapters[1].1), "{chapters:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A short recording on this computer keeps up, and says how far behind it was (the writer's most since asked).
#[test]
fn a_recording_says_how_far_behind_it_is() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("lag");
    let (tx, rx) = mpsc::channel();
    let rec = Recording::start(plan(ff, &dir, Sound::None), move |e| { let _ = tx.send(e); }).expect("starts");
    let mut lags = vec![];
    for _ in 0..3 {
        let mut b = rec.sink().buffer();
        b.fill(200);
        rec.sink().put(b);
        std::thread::sleep(Duration::from_secs(1));
        lags.push(rec.lag());
    }
    rec.stop();
    let ended = rx.recv_timeout(Duration::from_secs(60)).expect("finished");
    assert!(rec.wait(Duration::from_secs(5)));
    assert_eq!(ended.problem, None, "{ended:?}");
    assert!(!lags.is_empty());
    assert!(lags.iter().all(|l| *l < Duration::from_secs(2)), "{lags:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Trying an encoder for real: x264 works in every full FFmpeg; a made-up one does not.
#[test]
fn an_encoder_is_tried_before_it_is_used() {
    let Some(ff) = ffmpeg() else { return };
    assert!(encoder_works(&ff, "libx264"));
    assert!(!encoder_works(&ff, "h264_no_such_encoder"));
}

/// Every sound filter at once, through the real FFmpeg: it accepts them all.
#[test]
fn ffmpeg_takes_the_sound_filters() {
    let Some(ff) = ffmpeg() else { return };
    let chain = format!("afftdn=nf=-25,volume=6dB,adelay=300:all=1,{}", args::EVEN_OUT);
    let out = std::process::Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-af", &chain, "-f", "null", "-"])
        .output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

fn lavfi_camera(spec: &str) -> Vec<Vec<String>> { vec![["-re", "-f", "lavfi", "-i", spec].map(String::from).into()] }

/// A local server that takes one stream into `received`; stopped by `finish_server`.
fn server(ff: &Path, dir: &Path) -> (std::process::Child, String, PathBuf) {
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let url = format!("rtmp://127.0.0.1:{port}/live/TEST-KEY");
    let received = dir.join("received.flv");
    let server = std::process::Command::new(ff).args(["-hide_banner", "-loglevel", "error", "-y", "-listen", "1", "-analyzeduration", "2000000", "-f", "flv", "-i", &url, "-c", "copy"])
        .arg(&received).stdin(Stdio::null()).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    (server, url, received)
}

fn finish_server(mut server: std::process::Child) {
    let t = Instant::now();
    while server.try_wait().unwrap().is_none() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(100)) }
    let _ = server.kill();
    let _ = server.wait();
}

/// The picture at `t` seconds into `file`.
fn frame_at(ff: &Path, dir: &Path, file: &Path, t: &str) -> image::RgbImage {
    let png = dir.join(format!("f{t}.png"));
    assert!(std::process::Command::new(ff).args(["-hide_banner", "-loglevel", "error", "-y", "-ss", t, "-i"]).arg(file).args(["-frames:v", "1"]).arg(&png)
        .status().unwrap().success());
    image::open(&png).unwrap().to_rgb8()
}

/// Every quarter second of `file`, as pictures.
fn quarter_seconds(ff: &Path, dir: &Path, file: &Path) -> Vec<image::RgbImage> {
    let q = dir.join("quarters");
    std::fs::create_dir_all(&q).unwrap();
    assert!(std::process::Command::new(ff).args(["-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(file).args(["-vf", "fps=4"]).arg(q.join("q%03d.png"))
        .status().unwrap().success());
    let mut names: Vec<PathBuf> = std::fs::read_dir(&q).unwrap().map(|e| e.unwrap().path()).collect();
    names.sort();
    names.iter().map(|n| image::open(n).unwrap().to_rgb8()).collect()
}

/// The first picture of `file`, from 2 s on, for which `seen` holds (a camera's own FFmpeg can take a few seconds
/// to start while the whole suite runs).
fn first_frame_where(ff: &Path, dir: &Path, file: &Path, seen: impl Fn(&image::RgbImage) -> bool) -> image::RgbImage {
    let frames: Vec<image::RgbImage> = (2..=7).map(|t| frame_at(ff, dir, file, &t.to_string())).collect();
    frames.iter().find(|f| seen(f)).cloned().unwrap_or_else(|| frames[1].clone())
}

fn is_blue(p: [u8; 3]) -> bool { p[2] > 180 && p[0] < 60 }
fn is_red(p: [u8; 3]) -> bool { p[0] > 180 && p[2] < 60 }
/// The audience screen after half way in `record_for`.
fn is_green(p: [u8; 3]) -> bool { p[1] > 180 && p[0] < 60 && p[2] < 60 }

/// The Camera scene, for real, with a stand-in 4:3 camera (blue, made at the clock's pace like a camera's): the
/// camera in the middle of the 16:9 stream with black bars at the sides.
#[test]
fn go_live_shows_the_camera_with_black_bars() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("camera");
    let (srv, url, received) = server(&ff, &dir);
    let ended = record_for(Plan { stream: vec![url], cameras: lavfi_camera("color=c=blue:size=640x480:rate=30"), look: Some(only(Scene::Camera, "x")),
        ..plan(ff.clone(), &dir, Sound::None) }, 8.0, |_, _| {});
    finish_server(srv);
    assert_eq!(ended.problem, None, "{ended:?}");
    let img = first_frame_where(&ff, &dir, &received, |f| is_blue(f.get_pixel(640, 300).0));
    assert_eq!(img.dimensions(), (1280, 720));
    let (bar, mid) = (img.get_pixel(60, 300).0, img.get_pixel(640, 300).0);
    assert!(is_blue(mid), "the camera fills the middle: {mid:?}");
    assert!(bar.iter().all(|c| *c < 30), "black bars at the sides: {bar:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The words over the camera, for real: a stand-in blue camera, and a song slide as the live words. The stream's
/// top is the camera; its bottom has the words' band and white letters.
#[test]
fn go_live_puts_the_words_over_the_camera() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("words");
    let (srv, url, received) = server(&ff, &dir);
    let ended = record_for(Plan { stream: vec![url], cameras: lavfi_camera("color=c=blue:size=1280x720:rate=30"),
        look: Some(only(Scene::CameraWords, "Amazing grace how sweet the sound")), ..plan(ff.clone(), &dir, Sound::None) }, 8.0, |_, _| {});
    finish_server(srv);
    assert_eq!(ended.problem, None, "{ended:?}");
    let img = first_frame_where(&ff, &dir, &received, |f| is_blue(f.get_pixel(640, 100).0));
    let top = img.get_pixel(640, 100).0;
    assert!(is_blue(top), "the camera at the top: {top:?}");
    // The bottom third: the band darkens the camera, and letters are near-white.
    let bottom: Vec<[u8; 3]> = (0..1280).step_by(2).flat_map(|x| (520..700).step_by(2).map(move |y| (x, y))).map(|(x, y)| img.get_pixel(x, y).0).collect();
    let white = bottom.iter().filter(|p| p.iter().all(|c| *c > 200)).count();
    eprintln!("REAL words: {white} white of {}", bottom.len());
    assert!(white > 200, "letters in the lower third: {white}");
    let beside = img.get_pixel(100, 650).0;
    assert!(is_blue(beside), "the camera beside the band: {beside:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The scene follows what is live while streaming: a song first (the camera with words), then a presentation
/// slide with "Presentations: with the camera in the corner" (the audience screen, the camera in the corner).
#[test]
fn go_live_switches_scene_while_live() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("switch");
    let dead = format!("rtmp://127.0.0.1:{}/live/TEST-KEY", std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port());
    let item = Some(uuid::Uuid::from_u128(7));
    let live = Arc::new(Mutex::new((LiveInfo { cursor_item: item, slide: SlideKind::Words, ..LiveInfo::default() }, Choice::default())));
    let l = live.clone();
    let corner = AutoPrefs { on: true, presentation_camera: true };
    // Waited for, not timed: the camera's first picture (late while the whole suite runs), a second and a half of
    // the song with it, then the picture slide; then two seconds more. The audience screen is red.
    let mut m = Marks::default();
    let ended = record_while(Plan { stream: vec![dead], cameras: lavfi_camera("color=c=blue:size=1280x720:rate=30"), look: Some(look(live, "Grace", corner)),
        ..plan(ff.clone(), &dir, Sound::None) }, 40.0, |rec, t| {
        if m.seen.is_none() && rec.camera_seen() == Some(true) { m.seen = Some(t) }
        if m.changed.is_none() && m.seen.is_some_and(|s| t > s + 1.5) { l.lock().unwrap().0.slide = SlideKind::Picture; m.changed = Some(t) }
        m.changed.is_none_or(|c| t < c + 2.0)
    });
    assert_eq!(ended.problem, None, "{ended:?}");
    assert!(m.changed.is_some(), "the camera never sent a picture: {m:?}");
    // In order, every quarter second (a busy computer's file can run behind the clock, so no fixed second is
    // looked at): the camera filling the picture (C) while the song is live, then the audience screen filling it with
    // the camera in the corner (P), and not the camera alone again.
    let rec = dir.join("Recording test.mp4");
    let seen: String = quarter_seconds(&ff, &dir, &rec).iter().map(|f| {
        if is_blue(f.get_pixel(640, 100).0) { 'C' } else if is_red(f.get_pixel(300, 200).0) && is_blue(f.get_pixel(1088, 598).0) { 'P' } else { '.' }
    }).collect();
    let first_p = seen.find('P').unwrap_or_else(|| panic!("the audience screen with the camera in the corner never came: {seen} ({m:?})"));
    assert!(seen[..first_p].contains('C'), "the camera while the song is live, first: {seen}");
    assert!(!seen[first_p..].contains('C'), "the picture slide holds: {seen}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Two cameras: the operator switches from the first (blue) to the second (green) while live.
#[test]
fn go_live_switches_between_two_cameras() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("twocams");
    let dead = format!("rtmp://127.0.0.1:{}/live/TEST-KEY", std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port());
    let cameras = [lavfi_camera("color=c=blue:size=1280x720:rate=30"), lavfi_camera("color=c=green:size=1280x720:rate=30")].concat();
    let p = Plan { stream: vec![dead], cameras, look: Some(only(Scene::Camera, "x")), ..plan(ff.clone(), &dir, Sound::None) };
    let active = p.controls.camera.clone();
    // Waited for: the first camera's picture, a second and a half of it, the switch; the second camera's picture,
    // a second and a half of it.
    let mut m = Marks::default();
    let ended = record_while(p, 40.0, |rec, t| {
        if m.seen.is_none() && rec.camera_seen() == Some(true) { m.seen = Some(t) }
        if m.changed.is_none() && m.seen.is_some_and(|s| t > s + 1.5) { active.store(1, std::sync::atomic::Ordering::Relaxed); m.changed = Some(t) }
        // The writer looks at the switch within a picture; the second camera's state is seen after that.
        if m.changed.is_some_and(|c| t > c + 0.3) && m.seen_again.is_none() && rec.camera_seen() == Some(true) { m.seen_again = Some(t) }
        if m.end.is_none() && let Some(a) = m.seen_again && t > a + 1.5 { m.end = Some(t) }
        m.end.is_none()
    });
    assert_eq!(ended.problem, None, "{ended:?}");
    assert!(m.end.is_some(), "a camera never sent a picture: {m:?}");
    // In order, every quarter second (no fixed second: a busy computer's file can run behind the clock): the first
    // camera (B), then the second (G), and not the first again.
    let rec = dir.join("Recording test.mp4");
    let seen: String = quarter_seconds(&ff, &dir, &rec).iter().map(|f| f.get_pixel(640, 360).0)
        .map(|p| if is_blue(p) { 'B' } else if p[1] > 100 && p[2] < 60 && p[0] < 60 { 'G' } else { '.' }).collect();
    let first_g = seen.find('G').unwrap_or_else(|| panic!("the second camera never came: {seen} ({m:?})"));
    assert!(seen[..first_g].contains('B'), "the first camera, first: {seen}");
    assert!(!seen[first_g..].contains('B'), "the second camera holds: {seen}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Two cameras live, for real: the small Live picture is the stream's (the blue camera) and Next is the second
/// camera (green). Fade puts the second out and the first becomes next; the stream shows the second after.
#[test]
fn go_live_shows_small_live_and_next_pictures_and_fades_to_the_next_camera() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("peek");
    let dead = format!("rtmp://127.0.0.1:{}/live/TEST-KEY", std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port());
    let cameras = [lavfi_camera("color=c=blue:size=1280x720:rate=30"), lavfi_camera("color=c=green:size=1280x720:rate=30")].concat();
    let p = Plan { stream: vec![dead], cameras, look: Some(only(Scene::Camera, "x")), ..plan(ff.clone(), &dir, Sound::None) };
    p.controls.next.store(1, std::sync::atomic::Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    let rec = Recording::start(p, move |e| { let _ = tx.send(e); }).expect("starts");
    let middle = |s: &Option<Arc<super::super::peek::Small>>| s.as_ref().map(|s| {
        assert_eq!((s.width, s.height), (320, 180));
        let p = ((90 * s.width + 160) * 4) as usize;
        [s.rgba[p], s.rgba[p + 1], s.rgba[p + 2]]
    });
    let wait_for = |what: &str, ok: &dyn Fn(Option<[u8; 3]>, Option<[u8; 3]>) -> bool| {
        let t = Instant::now();
        loop {
            let peek = rec.peek();
            let (live, next) = (middle(&peek.live), middle(&peek.next));
            if ok(live, next) { break t.elapsed() }
            assert!(t.elapsed() < Duration::from_secs(15), "{what}: live {live:?}, next {next:?}");
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    wait_for("the first camera live, the second next", &|l, n| l.is_some_and(is_blue) && n.is_some_and(|p| p[1] > 100 && p[0] < 60 && p[2] < 60));
    rec.switch_camera(1, true);
    rec.set_next(0);
    let took = wait_for("the second camera live, the first next", &|l, n| l.is_some_and(|p| p[1] > 100 && p[0] < 60 && p[2] < 60) && n.is_some_and(is_blue));
    assert!(took < Duration::from_secs(3), "the switch shows within a moment: {took:?}");
    std::thread::sleep(Duration::from_secs(1));
    rec.stop();
    let ended = rx.recv_timeout(Duration::from_secs(60)).expect("finished");
    assert_eq!(ended.problem, None, "{ended:?}");
    let rec_file = dir.join("Recording test.mp4");
    let (_, secs) = probe(&rec_file);
    let last = frame_at(&ff, &dir, &rec_file, &format!("{:.1}", (secs - 0.5).max(0.0))).get_pixel(640, 360).0;
    assert!(last[1] > 100 && last[0] < 60 && last[2] < 60, "the second camera at the end: {last:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The real camera (`RP_REAL_CAMERA`, a name as FFmpeg lists it) with the words over it, live to a local server for
/// ten seconds. What is checked is the picture's numbers (it may show people): it is not one flat colour, so the
/// camera's picture came through. Everything it made is deleted. `cargo test real_camera -- --ignored --nocapture`.
#[cfg(windows)]
#[test]
#[ignore]
fn real_camera_goes_live_with_the_words() {
    let Some(ff) = ffmpeg() else { return };
    let name = std::env::var("RP_REAL_CAMERA").expect("RP_REAL_CAMERA");
    let dir = scratch("realcam");
    let (srv, url, received) = server(&ff, &dir);
    let began = Instant::now();
    let ended = record_for(Plan { stream: vec![url], cameras: vec![args::camera_input(crate::platform::tools::OS, &name)],
        look: Some(only(Scene::CameraWords, "Amazing grace how sweet the sound")), ..plan(ff.clone(), &dir, Sound::None) }, 10.0, |_, _| {});
    finish_server(srv);
    let (streams, secs) = probe(&received);
    let png = dir.join("frame.png");
    let got_frame = std::process::Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-y", "-ss", "6", "-i"]).arg(&received).args(["-frames:v", "1"]).arg(&png)
        .status().map(|s| s.success()).unwrap_or(false);
    let (spread, size) = if got_frame {
        let img = image::open(&png).unwrap().to_rgb8();
        // How far the brightest and darkest parts of the top half are apart: a camera's picture varies; a flat
        // colour (no camera) does not.
        let lum: Vec<u32> = (0..img.width()).step_by(8).flat_map(|x| (0..img.height() / 2).step_by(8).map(move |y| (x, y)))
            .map(|(x, y)| { let p = img.get_pixel(x, y).0; (p[0] as u32 * 3 + p[1] as u32 * 6 + p[2] as u32) / 10 }).collect();
        (lum.iter().max().unwrap() - lum.iter().min().unwrap(), img.dimensions())
    } else { (0, (0, 0)) };
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("REAL camera {name:?}: {:.1} s in all, stream {secs:.1} s {streams:?}, frame {size:?}, brightness spread {spread}; problem {:?}; files deleted",
        began.elapsed().as_secs_f64(), ended.problem);
    assert_eq!(ended.problem, None);
    assert!(streams.iter().any(|s| s.0 == "video" && s.2 == 1280 && s.3 == 720), "{streams:?}");
    assert!(spread > 20, "the camera's picture did not come through (flat): {spread}");
}

/// A camera that stops in the middle (unplugged) and comes back, for each camera scene. The stand-in camera runs
/// for 2.5 s each time it is started; `camera_feed` starts it again 2 s after it stops. So in the recording the
/// camera (blue) is seen first, then the audience screen (red) in the gap, then the camera again; and the stream and
/// the recording run to the end. (The destination is one nobody listens on: the recording is what is looked at.)
#[test]
fn a_camera_that_stops_drops_out_and_comes_back() {
    let Some(ff) = ffmpeg() else { return };
    for scene in [Scene::Camera, Scene::ScreenCamera, Scene::CameraWords] {
        let dir = scratch("camback");
        let dead = format!("rtmp://127.0.0.1:{}/live/TEST-KEY", std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port());
        // The audience screen: red for the first 4.6 s, then green (`record_for` changes at half way).
        let ended = record_for(Plan { stream: vec![dead], cameras: lavfi_camera("color=c=blue:size=1280x720:rate=30:duration=2.5"), look: Some(only(scene, "Grace")),
            ..plan(ff.clone(), &dir, Sound::None) }, 9.2, |_, _| {});
        assert_eq!(ended.problem, None, "{scene:?}: {ended:?}");
        let rec = dir.join("Recording test.mp4");
        let (_, secs) = probe(&rec);
        assert!(secs > 8.0, "{scene:?}: the recording ended with the camera ({secs} s)");
        // Where the camera shows in this scene: the middle, or (screen filling it) the corner.
        let (x, y) = if scene == Scene::ScreenCamera { (1280 - 32 - 160, 720 - 32 - 90) } else { (640, 200) };
        // In order, whenever the stand-in camera got going (later while the whole suite runs): the camera, the
        // audience screen in the gap (red, or green after half way), the camera again.
        let seen: Vec<char> = quarter_seconds(&ff, &dir, &rec).iter().map(|f| f.get_pixel(x, y).0)
            .map(|p| if is_blue(p) { 'C' } else if is_red(p) || is_green(p) { 'R' } else { '.' }).collect();
        let seen: String = seen.into_iter().collect();
        eprintln!("REAL {scene:?}: every quarter second {seen}");
        let first = seen.find('C').unwrap_or_else(|| panic!("{scene:?}: the camera never showed: {seen}"));
        let gap = seen[first..].find('R').map(|i| first + i).unwrap_or_else(|| panic!("{scene:?}: the audience screen while the camera is gone: {seen}"));
        assert!(seen[gap..].contains('C'), "{scene:?}: the camera back: {seen}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// More settings, for real: 60 pictures a second, an MKV, cut into pieces; and "Save the last minutes" while it
/// records. The pieces play one after the other, the last minutes are a file of their own.
#[test]
fn more_settings_make_an_mkv_in_pieces_and_save_the_last_minutes() {
    use super::super::advanced::{Format, Keep, More};
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("more");
    let more = More { fps: 60, keep: Keep { format: Format::Mkv, split_minutes: 30, ..Keep::default() }, ..More::default() };
    // Pieces of half an hour are too long for a test: one of 2 s is asked of `split` itself below.
    let p = Plan { out: dir.join("Recording test.mkv"), more, ..plan(ff.clone(), &dir, Sound::None) };
    let mut saved: Option<Result<PathBuf, String>> = None;
    let ended = record_while(p, 20.0, |rec, t| {
        // Once the recording has some seconds on disk (its FFmpeg can be slow to start while the suite runs).
        if t > 6.0 && saved.is_none() { saved = Some(rec.save_last(1)()) }
        t < 7.0
    });
    assert_eq!(ended.problem, None, "{ended:?}");
    let file = dir.join("Recording test.mkv");
    assert_eq!((ended.file.as_deref(), ended.pieces.len(), ended.kept_mkv), (Some(file.as_path()), 0, false), "shorter than a piece: whole");
    assert!(!ended.toast().1, "an MKV asked for is no trouble");
    let (streams, secs) = probe(&file);
    assert_eq!(streams[0].1, "h264");
    let rate = std::process::Command::new("ffprobe").args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=r_frame_rate", "-of", "csv=p=0"]).arg(&file).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&rate.stdout).trim(), "60/1");
    // The last minute, saved while it recorded (all of the 6 s so far).
    let last = saved.expect("asked").expect("saved");
    assert_eq!(last, dir.join("Recording test last 1 minutes.mkv"));
    let (_, last_secs) = probe(&last);
    assert!(last_secs > 2.0 && last_secs < secs, "{last_secs} of {secs}");
    // Cut into pieces of 2 s: three or so, the whole gone, each playable.
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let pieces = rt.block_on(split(&ff, &file, 2, Format::Mkv));
    assert!(pieces.len() >= 2, "{pieces:?}");
    assert!(!file.exists() && pieces[0] == dir.join("Recording test part 1.mkv"));
    let total: f64 = pieces.iter().map(|p| probe(p).1).sum();
    assert!((total - secs).abs() < 0.5, "{total} of {secs}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Live with "record without the words" on (More settings): a second recording beside the first, the camera alone
/// where the stream has the words over it, with the recording's sound.
#[test]
fn go_live_records_the_stream_without_the_words_too() {
    use super::super::advanced::{Keep, More};
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("clean");
    let dead = format!("rtmp://127.0.0.1:{}/live/TEST-KEY", std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port());
    let more = More { keep: Keep { clean: true, ..Keep::default() }, ..More::default() };
    let mut m = Marks::default();
    let ended = record_while(Plan { stream: vec![dead], cameras: lavfi_camera("color=c=blue:size=1280x720:rate=30"), more,
        look: Some(only(Scene::CameraWords, "Amazing grace how sweet the sound")), ..plan(ff.clone(), &dir, Sound::None) }, 40.0, |rec, t| {
        if m.seen.is_none() && rec.camera_seen() == Some(true) { m.seen = Some(t) }
        m.seen.is_none_or(|s| t < s + 3.0)
    });
    assert_eq!(ended.problem, None, "{ended:?}");
    let seen = m.seen.unwrap_or_else(|| panic!("the camera never sent a picture"));
    let (rec, clean) = (dir.join("Recording test.mp4"), dir.join("Recording test clean.mp4"));
    assert!(clean.is_file() && !dir.join("Recording test clean.recording.mkv").exists(), "the clean recording, finished");
    let (streams, secs) = probe(&clean);
    assert!(streams.iter().any(|s| s.0 == "video" && s.2 == 1280) && streams.iter().any(|s| s.0 == "audio"), "picture and sound: {streams:?}");
    assert!(secs > 2.0, "{secs}");
    let at = format!("{:.2}", seen + 1.5);
    let (with, without) = (frame_at(&ff, &dir, &rec, &at), frame_at(&ff, &dir, &clean, &at));
    let white = |img: &image::RgbImage| (0..1280).step_by(4).flat_map(|x| (520..700).step_by(4).map(move |y| (x, y))).filter(|(x, y)| img.get_pixel(*x, *y).0.iter().all(|c| *c > 200)).count();
    assert!(white(&with) > 50, "the words in the stream: {}", white(&with));
    assert_eq!(white(&without), 0, "none in the clean recording");
    assert!(is_blue(without.get_pixel(640, 600).0), "the camera where the words were: {:?}", without.get_pixel(640, 600).0);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Live with the words file on: the words alone are recorded beside the recording, see-through around them, with
/// the words at the bottom (the lower third).
#[test]
fn go_live_records_the_words_alone_beside_the_recording() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("wordsfile");
    let dead = format!("rtmp://127.0.0.1:{}/live/TEST-KEY", std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port());
    let ended = record_for(Plan { stream: vec![dead], cameras: lavfi_camera("color=c=blue:size=1280x720:rate=30"),
        look: Some(only(Scene::CameraWords, "Amazing grace how sweet the sound")), words_file: true, ..plan(ff.clone(), &dir, Sound::None) }, 6.0, |_, _| {});
    assert_eq!(ended.problem, None, "{ended:?}");
    let words = dir.join("Recording test words.mov");
    assert!(words.is_file() && dir.join("Recording test.mp4").is_file(), "both files");
    let (streams, secs) = probe(&words);
    assert_eq!(streams, vec![("video".to_string(), "qtrle".to_string(), 1280, 720)]);
    assert!(secs > 3.0, "{secs} s");
    let out = std::process::Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-ss", "4", "-i"]).arg(&words)
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgba", "-"]).output().unwrap();
    let rgba = out.stdout;
    assert_eq!(rgba.len(), 1280 * 720 * 4);
    let alpha = |x: usize, y: usize| rgba[(y * 1280 + x) * 4 + 3];
    assert_eq!(alpha(640, 100), 0, "nothing at the top: see-through");
    let shown = (520..700).step_by(4).flat_map(|y| (0..1280).step_by(4).map(move |x| (x, y))).filter(|(x, y)| alpha(*x, *y) > 0).count();
    assert!(shown > 200, "the words' band in the lower third: {shown}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Live captions go on top of the picture as soon as they change, and come off again.
#[test]
fn live_captions_are_laid_over_the_picture() {
    use crate::stream::pictures::Shown;
    let blue = Arc::new([0u8, 0, 200, 255].repeat(4));
    let room = [0u8; 16];
    let camera = Shown { scene: Scene::Camera, ..Shown::default() };
    let mut m = Mixer::new((2, 2));
    let t0 = Instant::now();
    m.update(&camera, &room, Some(blue.clone()), true, t0);
    let mut caption = vec![0u8; 16];
    caption[12..16].copy_from_slice(&[255, 255, 255, 255]);
    m.captions = Some(Arc::new(caption));
    m.update(&camera, &room, Some(blue.clone()), false, t0 + Duration::from_millis(40));
    assert_eq!(&m.out()[12..16], &[255, 255, 255, 255], "the caption's pixel");
    assert_eq!(&m.out()[..4], &[0, 0, 200, 255], "the camera around it");
    m.captions = None;
    m.update(&camera, &room, Some(blue.clone()), false, t0 + Duration::from_millis(80));
    assert_eq!(&m.out()[12..16], &[0, 0, 200, 255], "taken off");
}
