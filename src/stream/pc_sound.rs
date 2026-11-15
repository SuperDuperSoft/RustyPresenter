//! The PC's own sound in a livestream (Jake, November 14, 2026: "we get to decide devices"). Settings →
//! Livestream picks one of the computer's sound outputs; what plays on it (videos, songs, audio items, and anything
//! else sent to that output) is captured as it plays (Windows' "loopback" of the output) and sent to the recorder's
//! FFmpeg over a local connection FFmpeg listens on. The sound input chosen for the recording (a sound board) is
//! mixed in by FFmpeg (`record::args`).
//!
//! Windows sends nothing from an output while it is silent, and FFmpeg would then wait for sound and hold the
//! picture back. So the gaps are filled with silence, by the clock: the stream always has a second of sound for
//! every second of picture.
//!
//! macOS and Linux: not built (`start` says so, and the stream goes out without the PC's sound).
use std::time::Duration;

/// How the captured sound is written: as FFmpeg reads raw sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    /// FFmpeg's name for the samples: "f32le" or "s16le".
    pub samples: &'static str,
    pub rate: u32,
    pub channels: u16,
}

impl Format {
    pub fn frame_bytes(&self) -> usize { self.channels as usize * if self.samples == "f32le" { 4 } else { 2 } }
}

/// What FFmpeg is told to read: the format, and the local port it listens on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feed { pub format: Format, pub port: u16 }

impl Feed {
    /// FFmpeg's input arguments for the feed.
    pub fn input_args(&self) -> Vec<String> {
        vec!["-f".into(), self.format.samples.into(), "-ar".into(), self.format.rate.to_string(), "-ac".into(), self.format.channels.to_string(),
            "-thread_queue_size".into(), "1024".into(), "-i".into(), format!("tcp://127.0.0.1:{}?listen=1", self.port)]
    }
}

/// Wait this long between looks for new sound; the gap filler keeps up to this much behind the clock.
const LOOK_EVERY: Duration = Duration::from_millis(10);
/// Silence is put in once the sound is this far behind the clock (Windows hands sound over every 10 ms or so).
const GAP: Duration = Duration::from_millis(60);
/// The connection FFmpeg listens on is tried for this long (it opens its inputs one after the other).
const CONNECT_FOR: Duration = Duration::from_secs(30);

/// How many frames of silence to put in now, so the sound written keeps up with the clock: none while it is less
/// than `GAP` behind; else up to `LOOK_EVERY` behind.
pub fn silence_needed(elapsed: Duration, written_frames: u64, rate: u32) -> u64 {
    let due = (elapsed.as_secs_f64() * rate as f64) as u64;
    let behind = due.saturating_sub(written_frames);
    if behind < (GAP.as_secs_f64() * rate as f64) as u64 { return 0 }
    behind - (LOOK_EVERY.as_secs_f64() * rate as f64) as u64
}

/// The device id Windows knows an output by, from the name the app keeps (`wasapi/{…}`); empty: the computer's
/// default output.
pub fn device_id(name: &str) -> &str { name.strip_prefix("wasapi/").unwrap_or("") }

/// A capture under way. Dropping it stops it.
pub struct Capture {
    pub feed: Feed,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Capture {
    pub fn stop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.thread.take() { let _ = t.join(); }
    }
}

impl Drop for Capture {
    fn drop(&mut self) { self.stop() }
}

/// Starts capturing output `device` (the app's name for it; empty: the default output). Returns once the format is
/// known; the sound goes out when FFmpeg starts listening on `Capture::feed`.
pub fn start(device: &str) -> Result<Capture, String> {
    #[cfg(windows)]
    return windows::start(device_id(device).to_string());
    #[cfg(not(windows))]
    { let _ = device; Err("capturing the computer's sound is only built for Windows".into()) }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;
    use ::windows::Win32::Media::Audio::{AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, IAudioCaptureClient, IAudioClient,
        IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, eConsole, eRender};
    use ::windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree};
    use ::windows::core::HSTRING;

    struct Opened { client: IAudioClient, capture: IAudioCaptureClient, format: Format }

    /// Opens the output for loopback capture. COM must be set up on this thread.
    unsafe fn open(id: &str) -> Result<Opened, String> {
        let e = |what: &str, err: ::windows::core::Error| format!("{what}: {err}");
        // SAFETY: plain COM calls on this thread's apartment; the format Windows allocates is copied and freed.
        unsafe {
            let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|x| e("the sound devices", x))?;
            let device = if id.is_empty() { devices.GetDefaultAudioEndpoint(eRender, eConsole) } else { devices.GetDevice(&HSTRING::from(id)) }
                .map_err(|x| e("the sound output", x))?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(|x| e("the sound output", x))?;
            let mix = client.GetMixFormat().map_err(|x| e("the sound format", x))?;
            let w: WAVEFORMATEX = *mix;
            let tag = w.wFormatTag;
            let bits = w.wBitsPerSample;
            let samples = match (tag, bits) {
                (3, 32) | (0xFFFE, 32) => Some("f32le"),
                (1, 16) | (0xFFFE, 16) => Some("s16le"),
                _ => None,
            };
            let format = samples.map(|samples| Format { samples, rate: w.nSamplesPerSec, channels: w.nChannels });
            // A one-second buffer: the reader looks every 10 ms.
            let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 10_000_000, 0, mix, None);
            CoTaskMemFree(Some(mix as *const core::ffi::c_void));
            let format = format.ok_or_else(|| format!("the sound output's format is not one that can be sent ({bits}-bit, tag {tag})"))?;
            init.map_err(|x| e("capturing the sound output", x))?;
            let capture: IAudioCaptureClient = client.GetService().map_err(|x| e("capturing the sound output", x))?;
            Ok(Opened { client, capture, format })
        }
    }

    pub fn start(id: String) -> Result<Capture, String> {
        let port = std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map_err(|e| e.to_string())?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel::<Result<Format, String>>();
        let s = stop.clone();
        let thread = std::thread::Builder::new().name("pc-sound".into()).spawn(move || {
            // SAFETY: COM is set up for this thread only, before any COM call on it.
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            // SAFETY: COM is set up above.
            let opened = match unsafe { open(&id) } {
                Ok(o) => o,
                Err(e) => { let _ = tx.send(Err(e)); return }
            };
            let _ = tx.send(Ok(opened.format));
            run(opened, port, &s);
        }).map_err(|e| e.to_string())?;
        match rx.recv() {
            Ok(Ok(format)) => Ok(Capture { feed: Feed { format, port }, stop, thread: Some(thread) }),
            Ok(Err(e)) => { let _ = thread.join(); Err(e) }
            Err(_) => { let _ = thread.join(); Err("the sound capture ended".into()) }
        }
    }

    /// Connects to FFmpeg, then sends what plays (and silence for the gaps) until stopped or FFmpeg goes.
    fn run(o: Opened, port: u16, stop: &AtomicBool) {
        let began = Instant::now();
        let mut out = loop {
            if stop.load(Ordering::Relaxed) { return }
            match std::net::TcpStream::connect(("127.0.0.1", port)) {
                Ok(s) => break s,
                Err(_) if began.elapsed() < CONNECT_FOR => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => { eprintln!("pc sound: FFmpeg did not listen: {e}"); return }
            }
        };
        let _ = out.set_nodelay(true);
        let frame = o.format.frame_bytes();
        // SAFETY: the client was initialised for this capture.
        if let Err(e) = unsafe { o.client.Start() } { eprintln!("pc sound: could not start: {e}"); return }
        let t0 = Instant::now();
        let mut written: u64 = 0;
        let mut buf: Vec<u8> = Vec::with_capacity(48_000 * frame / 10);
        'run: while !stop.load(Ordering::Relaxed) {
            buf.clear();
            loop {
                // SAFETY: the buffer Windows lends is read for `frames` frames and handed back at once.
                let got = unsafe {
                    match o.capture.GetNextPacketSize() {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(e) => { eprintln!("pc sound: {e}"); break 'run }
                    }
                    let (mut data, mut frames, mut flags) = (std::ptr::null_mut::<u8>(), 0u32, 0u32);
                    if let Err(e) = o.capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None) { eprintln!("pc sound: {e}"); break 'run }
                    let n = frames as usize * frame;
                    if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                        buf.resize(buf.len() + n, 0);
                    } else {
                        buf.extend_from_slice(std::slice::from_raw_parts(data, n));
                    }
                    let _ = o.capture.ReleaseBuffer(frames);
                    frames as u64
                };
                written += got;
            }
            let gap = silence_needed(t0.elapsed(), written, o.format.rate);
            buf.resize(buf.len() + gap as usize * frame, 0);
            written += gap;
            if !buf.is_empty() && out.write_all(&buf).is_err() { break }
            std::thread::sleep(LOOK_EVERY);
        }
        // SAFETY: as `Start`.
        let _ = unsafe { o.client.Stop() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_fills_the_gaps_by_the_clock() {
        let ms = Duration::from_millis;
        // Sound has come as it should: nothing to put in.
        assert_eq!(silence_needed(ms(1000), 48_000, 48_000), 0);
        assert_eq!(silence_needed(ms(1050), 48_000, 48_000), 0, "less than 60 ms behind");
        // Nothing has played for a second: all but the last 10 ms is filled.
        assert_eq!(silence_needed(ms(1000), 0, 48_000), 48_000 - 480);
        assert_eq!(silence_needed(ms(500), 1_000_000, 48_000), 0, "ahead of the clock");
    }

    #[test]
    fn the_feed_and_the_device() {
        let f = Feed { format: Format { samples: "f32le", rate: 48_000, channels: 2 }, port: 40500 };
        assert_eq!(f.input_args().join(" "), "-f f32le -ar 48000 -ac 2 -thread_queue_size 1024 -i tcp://127.0.0.1:40500?listen=1");
        assert_eq!(f.format.frame_bytes(), 8);
        assert_eq!(Format { samples: "s16le", rate: 44_100, channels: 6 }.frame_bytes(), 12);
        assert_eq!(device_id("wasapi/{0.0.0.00000000}.{abc}"), "{0.0.0.00000000}.{abc}");
        assert_eq!((device_id(""), device_id("auto")), ("", ""));
    }

    /// Diagnosis by hand: the loudest sample captured from output `RP_PC_SOUND` (empty: the default) while FFplay
    /// plays a tone. `cargo test captured_peak -- --ignored --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn captured_peak_while_a_tone_plays() {
        let device = std::env::var("RP_PC_SOUND").unwrap_or_default();
        let mut c = start(&device).expect("capture starts");
        let f = c.feed.format;
        let listener = std::net::TcpListener::bind(("127.0.0.1", c.feed.port)).unwrap();
        let (mut conn, _) = listener.accept().unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let ffplay = std::env::var("RP_FFPLAY").unwrap_or_else(|_| "ffplay".into());
        let mut tone = std::process::Command::new(ffplay).args(["-hide_banner", "-loglevel", "error", "-nodisp", "-autoexit", "-f", "lavfi",
            "-i", "sine=frequency=1000:duration=2,volume=0.1"]).spawn().expect("ffplay");
        let t = std::time::Instant::now();
        let mut bytes = vec![];
        let mut b = vec![0u8; 65536];
        while t.elapsed() < Duration::from_secs(4) {
            use std::io::Read;
            match conn.read(&mut b) { Ok(0) | Err(_) => break, Ok(n) => bytes.extend_from_slice(&b[..n]) }
        }
        let _ = tone.wait();
        c.stop();
        let peak = if f.samples == "f32le" {
            bytes.chunks_exact(4).map(|x| f32::from_le_bytes([x[0], x[1], x[2], x[3]]).abs()).fold(0.0f32, f32::max)
        } else {
            bytes.chunks_exact(2).map(|x| (i16::from_le_bytes([x[0], x[1]]) as f32 / 32768.0).abs()).fold(0.0f32, f32::max)
        };
        println!("REAL device {device:?} {f:?}: {} s captured, peak {peak:.4}", bytes.len() / f.frame_bytes() / f.rate as usize);
    }

    /// The real thing on this computer: the default output is captured for two seconds while FFmpeg reads it.
    /// Nothing needs to play: silence must still come at the clock's pace.
    #[cfg(windows)]
    #[test]
    fn the_default_output_is_captured_at_the_clocks_pace() {
        let Ok(mut c) = start("") else { return eprintln!("no sound output here: skipped") };
        let f = c.feed.format;
        assert!(f.rate >= 8000 && f.channels >= 1, "{f:?}");
        let listener = std::net::TcpListener::bind(("127.0.0.1", c.feed.port)).expect("the port is free for the test to listen on");
        let (mut conn, _) = listener.accept().unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let t = std::time::Instant::now();
        let mut got = 0usize;
        let mut b = vec![0u8; 65536];
        while t.elapsed() < Duration::from_secs(2) {
            use std::io::Read;
            match conn.read(&mut b) { Ok(0) | Err(_) => break, Ok(n) => got += n }
        }
        c.stop();
        let secs = got as f64 / (f.rate as f64 * f.frame_bytes() as f64);
        assert!((1.7..2.3).contains(&secs), "{secs:.2} s of sound in 2 s ({f:?})");
    }
}
