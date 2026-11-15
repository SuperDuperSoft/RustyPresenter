use super::*;
use crate::record::sound::Heard;

/// The FFmpeg on this machine, if any: these tests record for real.
fn ffmpeg() -> Option<PathBuf> {
    // "ffmpeg.exe" on Windows (only "ffmpeg" was looked for, so on Windows these tests never ran).
    let found = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).flat_map(|d| [d.join("ffmpeg"), d.join("ffmpeg.exe")]).find(|p| p.is_file()));
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
    Plan { ffmpeg, encoder: "libx264", preset: Preset::P720, sound, out: dir.join("Recording test.mp4"), stream: vec![], pc_sound: None, camera: None, words: None }
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

/// A camera in the stream, for real, with a stand-in camera (a blue test picture, made at the clock's pace like a
/// camera's): the stream's picture is the camera, with the room screen (red) small in the bottom right corner.
#[test]
fn go_live_puts_the_camera_and_the_screen_together() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("camera");
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let url = format!("rtmp://127.0.0.1:{port}/live/TEST-KEY");
    let received = dir.join("received.flv");
    let mut server = std::process::Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-y", "-listen", "1", "-analyzeduration", "2000000", "-f", "flv", "-i", &url, "-c", "copy"])
        .arg(&received).stdin(Stdio::null()).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let camera = args::Camera { input: ["-re", "-f", "lavfi", "-i", "color=c=blue:size=640x480:rate=30"].map(String::from).into(), layout: args::Layout::CameraScreen,
        words_port: None, feed_port: None };
    let ended = record_for(Plan { stream: vec![url], camera: Some(camera), ..plan(ff.clone(), &dir, Sound::None) }, 8.0, |_, _| {});
    let t = Instant::now();
    while server.try_wait().unwrap().is_none() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(100)) }
    let _ = server.kill();
    let _ = server.wait();
    assert_eq!(ended.problem, None, "{ended:?}");
    // A picture from the stream two seconds in (the room screen was red then).
    let png = dir.join("frame.png");
    assert!(std::process::Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-y", "-ss", "2", "-i"]).arg(&received).args(["-frames:v", "1"]).arg(&png)
        .status().unwrap().success());
    let img = image::open(&png).unwrap().to_rgb8();
    let (w, h) = img.dimensions();
    assert_eq!((w, h), (1280, 720));
    let at = |x: u32, y: u32| img.get_pixel(x, y).0;
    // A 4:3 camera in a 16:9 stream: black bars at the sides, the camera in the middle.
    let (bar, blue) = (at(60, 300), at(640, 300));
    assert!(blue[2] > 180 && blue[0] < 60, "the camera fills the middle: {blue:?}");
    assert!(bar.iter().all(|c| *c < 30), "black bars at the sides: {bar:?}");
    let corner = at(w - 32 - 160, h - 18 - 90);
    assert!(corner[0] > 180 && corner[2] < 60, "the room screen in the bottom right corner: {corner:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The words over the camera, for real: a stand-in blue camera, and a song slide as the live words. The stream's
/// top is the camera; its bottom has the words' band and white letters.
#[test]
fn go_live_puts_the_words_over_the_camera() {
    let Some(ff) = ffmpeg() else { return };
    let dir = scratch("words");
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let url = format!("rtmp://127.0.0.1:{port}/live/TEST-KEY");
    let received = dir.join("received.flv");
    let mut server = std::process::Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-y", "-listen", "1", "-analyzeduration", "2000000", "-f", "flv", "-i", &url, "-c", "copy"])
        .arg(&received).stdin(Stdio::null()).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let camera = args::Camera { input: ["-re", "-f", "lavfi", "-i", "color=c=blue:size=1280x720:rate=30"].map(String::from).into(), layout: args::Layout::CameraWords,
        words_port: None, feed_port: None };
    let slide = crate::live::SlideLayer::Text { text: "Amazing grace how sweet the sound".into(), footer: None, reference: None, scripture: false, size: 0,
        passage: vec![], parallel: None, theme: None };
    let words = crate::stream::lyrics::Words(std::sync::Arc::new(move || crate::stream::lyrics::key_for(&slide, &crate::model::Theme::default(), (1280, 720))));
    let ended = record_for(Plan { stream: vec![url], camera: Some(camera), words: Some(words), ..plan(ff.clone(), &dir, Sound::None) }, 8.0, |_, _| {});
    let t = Instant::now();
    while server.try_wait().unwrap().is_none() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(100)) }
    let _ = server.kill();
    let _ = server.wait();
    assert_eq!(ended.problem, None, "{ended:?}");
    let png = dir.join("frame.png");
    assert!(std::process::Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-y", "-ss", "4", "-i"]).arg(&received).args(["-frames:v", "1"]).arg(&png)
        .status().unwrap().success());
    let img = image::open(&png).unwrap().to_rgb8();
    let top = img.get_pixel(640, 100).0;
    assert!(top[2] > 180 && top[0] < 60, "the camera at the top: {top:?}");
    // The bottom third: the band darkens the camera, and letters are near-white.
    let bottom: Vec<[u8; 3]> = (0..1280).step_by(2).flat_map(|x| (520..700).step_by(2).map(move |y| (x, y))).map(|(x, y)| img.get_pixel(x, y).0).collect();
    let white = bottom.iter().filter(|p| p.iter().all(|c| *c > 200)).count();
    eprintln!("REAL words: {white} white of {}", bottom.len());
    assert!(white > 200, "letters in the lower third: {white}");
    // The band wraps the line of words (seen: x 265-1010, y 612-690 for this line), darkening the camera; beside it
    // the camera is as it was.
    let (band, beside) = (img.get_pixel(272, 618).0, img.get_pixel(100, 650).0);
    assert!(band[2] < 150, "the band behind the words: {band:?}");
    assert!(beside[2] > 180 && beside[0] < 60, "the camera beside the band: {beside:?}");
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
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let url = format!("rtmp://127.0.0.1:{port}/live/TEST-KEY");
    let received = dir.join("received.flv");
    let mut server = std::process::Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-y", "-listen", "1", "-analyzeduration", "2000000", "-f", "flv", "-i", &url, "-c", "copy"])
        .arg(&received).stdin(Stdio::null()).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let camera = args::Camera { input: args::camera_input(crate::platform::tools::OS, &name), layout: args::Layout::CameraWords, words_port: None, feed_port: None };
    let slide = crate::live::SlideLayer::Text { text: "Amazing grace how sweet the sound".into(), footer: None, reference: None, scripture: false, size: 0,
        passage: vec![], parallel: None, theme: None };
    let words = crate::stream::lyrics::Words(std::sync::Arc::new(move || crate::stream::lyrics::key_for(&slide, &crate::model::Theme::default(), (1280, 720))));
    let began = Instant::now();
    let ended = record_for(Plan { stream: vec![url], camera: Some(camera), words: Some(words), ..plan(ff.clone(), &dir, Sound::None) }, 10.0, |_, _| {});
    let t = Instant::now();
    while server.try_wait().unwrap().is_none() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(100)) }
    let _ = server.kill();
    let _ = server.wait();
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

/// A camera that stops in the middle (unplugged) and comes back, for each layout. The stand-in camera runs for
/// 2.5 s each time it is started; `camera_feed` starts it again 2 s after it stops. So in the recording the
/// camera (blue) is seen first, then the room screen (red) in the gap, then the camera again; and the stream and
/// the recording run to the end. (The destination is one nobody listens on: the recording is what is looked at.)
#[test]
fn a_camera_that_stops_drops_out_and_comes_back() {
    let Some(ff) = ffmpeg() else { return };
    for layout in [args::Layout::CameraScreen, args::Layout::ScreenCamera, args::Layout::CameraWords] {
        let dir = scratch("camback");
        let dead = format!("rtmp://127.0.0.1:{}/live/TEST-KEY", std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port());
        let camera = args::Camera { input: ["-re", "-f", "lavfi", "-i", "color=c=blue:size=1280x720:rate=30:duration=2.5"].map(String::from).into(), layout,
            words_port: None, feed_port: None };
        let slide = crate::live::SlideLayer::Text { text: "Grace".into(), footer: None, reference: None, scripture: false, size: 0, passage: vec![], parallel: None, theme: None };
        let words = crate::stream::lyrics::Words(std::sync::Arc::new(move || crate::stream::lyrics::key_for(&slide, &crate::model::Theme::default(), (1280, 720))));
        // The room screen: red for the first 4.6 s, then green (`record_for` changes at half way).
        let ended = record_for(Plan { stream: vec![dead], camera: Some(camera), words: Some(words), ..plan(ff.clone(), &dir, Sound::None) }, 9.2, |_, _| {});
        assert_eq!(ended.problem, None, "{layout:?}: {ended:?}");
        let rec = dir.join("Recording test.mp4");
        let (_, secs) = probe(&rec);
        assert!(secs > 8.0, "{layout:?}: the recording ended with the camera ({secs} s)");
        // Where the camera shows in this layout: the middle, or (screen filling it) the corner.
        let (x, y) = if layout == args::Layout::ScreenCamera { (1280 - 32 - 160, 720 - 32 - 90) } else { (640, 200) };
        let at = |t: &str| {
            let png = dir.join(format!("f{t}.png"));
            assert!(std::process::Command::new(&ff).args(["-hide_banner", "-loglevel", "error", "-y", "-ss", t, "-i"]).arg(&rec).args(["-frames:v", "1"]).arg(&png)
                .status().unwrap().success());
            image::open(&png).unwrap().to_rgb8().get_pixel(x, y).0
        };
        let (first, gap, back) = (at("1.8"), at("3.9"), at("6.4"));
        eprintln!("REAL {layout:?}: camera {first:?}, gap {gap:?}, back {back:?}");
        assert!(first[2] > 180 && first[0] < 60, "{layout:?}: the camera at first: {first:?}");
        assert!(gap[0] > 180 && gap[2] < 60, "{layout:?}: the room screen while the camera is gone: {gap:?}");
        assert!(back[2] > 180 && back[0] < 60, "{layout:?}: the camera back: {back:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[test]
fn the_log_says_when_the_sound_input_goes_silent_and_keeps_only_warnings() {
    assert_eq!(log_line("[Parsed_silencedetect_0 @ 000001] [info] silence_start: 10.02"), Said::Silence(true));
    assert_eq!(log_line("[silencedetect @ 0x55] [info] silence_end: 31.5 | silence_duration: 21.48"), Said::Silence(false));
    assert_eq!(log_line("[info] Stream mapping:"), Said::Nothing);
    assert_eq!(log_line("[tee @ 0x1] [warning] Slave muxer #1 failed"), Said::Line("[tee @ 0x1] Slave muxer #1 failed".into()));
    assert_eq!(log_line("[error] Conversion failed!"), Said::Line("Conversion failed!".into()));
    // Without a sound input the lines have no level, as before.
    assert_eq!(log_line("[tee @ 0x1] Slave muxer #1 failed"), Said::Line("[tee @ 0x1] Slave muxer #1 failed".into()));
}

/// A real FFmpeg, its log read as the recorder reads it: sound for 2 s, then nothing.
#[test]
fn a_real_ffmpeg_notes_that_the_sound_went_silent() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let quiet_after = 2;
    let source = format!("aevalsrc=if(lt(t\\,{quiet_after})\\,0.3*sin(2*PI*440*t)\\,0):s=48000");
    let run = |total: u32| {
        let out = std::process::Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "level+info", "-nostats", "-f", "lavfi", "-i", &source, "-af", &args::listen(),
            "-t", &total.to_string(), "-f", "null", "-"]).output().expect("FFmpeg runs");
        let text = String::from_utf8_lossy(&out.stderr).to_string();
        assert!(out.status.success(), "{text}");
        let heard: Vec<Said> = text.lines().map(log_line).filter(|s| *s != Said::Nothing).collect();
        (heard, text)
    };
    let (heard, text) = run(quiet_after + args::SILENCE.1 + 2);
    assert_eq!(heard.first(), Some(&Said::Silence(true)), "{text}");
    assert!(heard.iter().all(|s| matches!(s, Said::Silence(_))), "nothing else to log: {heard:?}");
    // A second short of the whole silence: nothing said yet.
    let (heard, text) = run(quiet_after + args::SILENCE.1 - 1);
    assert!(heard.is_empty(), "{heard:?} {text}");
}
