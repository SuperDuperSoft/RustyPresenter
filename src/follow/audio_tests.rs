use super::*;
use crate::follow::rules::{Mode, Voice};
use crate::follow::testing::Fake;
use crate::follow::{Follower, Light};
use std::time::Instant;

fn strs(a: &[OsString]) -> Vec<String> { a.iter().map(|s| s.to_string_lossy().into_owned()).collect() }

#[test]
fn ffmpeg_gives_16k_mono_samples() {
    let win = strs(&device_args(Os::Windows, "Microphone (USB Audio)"));
    assert!(win.windows(4).any(|w| w == ["-f", "dshow", "-i", "audio=Microphone (USB Audio)"]), "{win:?}");
    assert!(win.windows(2).any(|w| w == ["-audio_buffer_size", "50"]));
    assert!(win.ends_with(&["-vn", "-ac", "1", "-ar", "16000", "-f", "s16le", "-"].map(String::from)), "{win:?}");
    let linux = strs(&device_args(Os::Linux, "alsa_input.usb"));
    assert!(linux.windows(4).any(|w| w == ["-f", "pulse", "-i", "alsa_input.usb"]));
    let file = strs(&file_args(Path::new("Sunday.mp4")));
    assert!(file.windows(2).any(|w| w == ["-i", "Sunday.mp4"]) && file.ends_with(&["-".to_string()]));
}

/// A stand-in FFmpeg: a shell line.
#[cfg(unix)]
fn fake(script: &str) -> (PathBuf, Vec<OsString>) { ("/bin/sh".into(), vec!["-c".into(), script.into()]) }

#[cfg(unix)]
#[test]
fn input_lost_falls_back_to_manual() {
    // One second of quiet, then the device goes away.
    let (sh, args) = fake("head -c 32000 /dev/zero; echo 'device unplugged' >&2; exit 1");
    let (tx, rx) = channel();
    let cap = Capture::run(sh.clone(), args.clone(), Box::new(NoEar), tx).unwrap();
    let mut got = vec![];
    let until = Instant::now() + Duration::from_secs(10);
    while !matches!(got.last(), Some(FollowEvent::InputLost(_))) && Instant::now() < until {
        if let Ok(ev) = rx.recv_timeout(Duration::from_millis(100)) { got.push(ev) }
    }
    assert_eq!(got.first(), Some(&FollowEvent::Listening { words: false }));
    assert_eq!(got.iter().filter(|e| matches!(e, FollowEvent::Voice(Voice::Silence, _))).count(), 10);
    assert_eq!(got.last(), Some(&FollowEvent::InputLost("device unplugged".into())));
    let until = Instant::now() + Duration::from_secs(5);
    while cap.running() && Instant::now() < until { std::thread::sleep(Duration::from_millis(10)) }
    assert!(!cap.running());
    drop(cap);

    // Through the follower: Automatic goes back to Manual and the operator is told.
    let host = Fake::song(Mode::Auto);
    let f = Follower::start_with(host.clone());
    let cap = Capture::run(sh, args, Box::new(NoEar), f.sender()).unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while host.notices.lock().unwrap().is_empty() && Instant::now() < until { std::thread::sleep(Duration::from_millis(20)) }
    let notices = host.notices.lock().unwrap().clone();
    assert_eq!(notices.len(), 1);
    assert!(notices[0].contains("device unplugged"), "{notices:?}");
    let until = Instant::now() + Duration::from_secs(5);
    while f.state().mode != Mode::Manual && Instant::now() < until { std::thread::sleep(Duration::from_millis(20)) }
    assert_eq!((f.state().mode, f.state().light), (Mode::Manual, Light::Off));
    drop(cap);

    // No FFmpeg at all: a plain error, no hang.
    let (tx, _rx) = channel();
    assert!(Capture::run("/no/such/ffmpeg".into(), vec![], Box::new(NoEar), tx).is_err());
}

#[cfg(unix)]
#[test]
fn capture_stops_when_dropped() {
    // An input that never ends: dropping the capture ends FFmpeg without a lost-input notice.
    let (sh, args) = fake("while true; do head -c 3200 /dev/zero; sleep 0.1; done");
    let (tx, rx) = channel();
    let cap = Capture::run(sh, args, Box::new(NoEar), tx).unwrap();
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), FollowEvent::Listening { words: false });
    assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok());
    assert!(cap.running());
    assert_eq!(cap.level(), 0.0);
    let t = Instant::now();
    drop(cap);
    assert!(t.elapsed() < Duration::from_secs(5));
    assert!(rx.try_iter().all(|e| !matches!(e, FollowEvent::InputLost(_))));
}
