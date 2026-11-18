//! Listen: the stream's sound in headphones at the desk, as the stream gets it (after the filters and the sound
//! delay). The recorder's FFmpeg sends a copy of the mix here (raw 48 kHz stereo float, `record::args::Record::
//! listen`), as it does for NDI out. The app reads it as fast as FFmpeg writes and keeps at most `HOLD`, dropping
//! the oldest, so the stream never waits for the headphones; a player takes it from there:
//! - Windows: the chosen output through WASAPI (the same `windows` crate as `pc_sound`);
//! - Linux: `pacat` (pulseaudio-utils; PipeWire's pulse layer understands it too) to the chosen output;
//! - macOS: FFmpeg's `audiotoolbox` output, to the default output (FFmpeg cannot open a Core Audio output by the id
//!   mpv names it with).
use std::collections::VecDeque;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What FFmpeg sends: 48 kHz stereo float.
pub const RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
/// Settings' name for the computer's usual output.
pub const DEFAULT: &str = "default";
/// The most kept for the player: a quarter of a second.
pub const HOLD: Duration = Duration::from_millis(250);
const LOOK_EVERY: Duration = Duration::from_millis(10);

/// The sound waiting for the player, oldest first, at most `HOLD` of it.
#[derive(Debug, Default)]
pub struct Held { samples: VecDeque<f32> }

impl Held {
    fn most() -> usize { (HOLD.as_secs_f64() * f64::from(RATE)) as usize * CHANNELS }

    /// More sound came; what is over `HOLD` is dropped from the oldest end, whole frames at a time.
    pub fn push(&mut self, samples: &[f32]) {
        self.samples.extend(samples);
        let over = self.samples.len().saturating_sub(Held::most());
        let over = over.div_ceil(CHANNELS) * CHANNELS;
        self.samples.drain(..over.min(self.samples.len()));
    }

    /// Up to `frames` frames, oldest first.
    pub fn take(&mut self, frames: usize) -> Vec<f32> {
        let n = (frames * CHANNELS).min(self.samples.len() / CHANNELS * CHANNELS);
        self.samples.drain(..n).collect()
    }

    pub fn frames(&self) -> usize { self.samples.len() / CHANNELS }
}

/// Listening under way. Dropping it stops it.
pub struct Listen {
    /// The local port the recorder's FFmpeg sends the copy to.
    pub port: u16,
    stop: Arc<AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Listen {
    /// Starts playing what comes on `Listen::port` on `output` (`DEFAULT`, or an output's name as Settings keeps
    /// it). `ffmpeg` plays it on a Mac. `Err` when the output cannot be opened.
    pub fn start(output: &str, ffmpeg: &Path) -> Result<Listen, String> {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let held: Arc<Mutex<Held>> = Arc::default();
        let play = player(output, ffmpeg, held.clone(), stop.clone())?;
        let (h, s) = (held, stop.clone());
        let read = std::thread::Builder::new().name("listen-read".into()).spawn(move || take_sound(listener, &h, &s)).map_err(|e| e.to_string())?;
        Ok(Listen { port, stop, threads: vec![read, play] })
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) { let _ = t.join(); }
    }
}

impl Drop for Listen {
    fn drop(&mut self) { self.stop() }
}

/// Takes the stream's sound from the recorder's FFmpeg into `held`, as fast as it comes, until `stop`.
fn take_sound(listener: std::net::TcpListener, held: &Mutex<Held>, stop: &AtomicBool) {
    let _ = listener.set_nonblocking(true);
    let mut conn = loop {
        if stop.load(Ordering::Relaxed) { return }
        match listener.accept() {
            Ok((c, _)) => break c,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => { eprintln!("listen: no sound from the recorder: {e}"); return }
        }
    };
    let _ = conn.set_nonblocking(false);
    let _ = conn.set_read_timeout(Some(Duration::from_millis(200)));
    let mut buf = vec![0u8; 16_384];
    let mut have = 0usize;
    let mut samples: Vec<f32> = vec![];
    while !stop.load(Ordering::Relaxed) {
        match conn.read(&mut buf[have..]) {
            Ok(0) => break,
            Ok(n) => {
                have += n;
                let whole = have / 4 * 4;
                samples.clear();
                samples.extend(buf[..whole].chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])));
                held.lock().unwrap_or_else(|e| e.into_inner()).push(&samples);
                buf.copy_within(whole..have, 0);
                have -= whole;
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
    }
}

/// Linux: the PulseAudio/PipeWire output (sink) that output `name` stands for (`pipewire/<sink>`, `pulse/<sink>`);
/// `None`: the default one.
pub fn sink_of(name: &str) -> Option<&str> {
    name.strip_prefix("pipewire/").or_else(|| name.strip_prefix("pulse/")).filter(|s| !s.is_empty())
}

/// `pacat`'s arguments: raw 48 kHz stereo float from its input, to `sink` (`None`: the default output).
pub fn pacat_args(sink: Option<&str>) -> Vec<String> {
    let mut a = vec!["--playback".to_string(), "--raw".into(), "--format=float32le".into(), format!("--rate={RATE}"), format!("--channels={CHANNELS}"),
        "--latency-msec=60".into(), "--client-name=RustyPresenter".into(), "--stream-name=Listen to the stream".into()];
    if let Some(s) = sink { a.push(format!("--device={s}")) }
    a
}

/// FFmpeg's arguments on a Mac: raw 48 kHz stereo float from its input, to the default output.
pub fn mac_args() -> Vec<String> {
    ["-hide_banner", "-loglevel", "error", "-f", "f32le", "-ar", "48000", "-ac", "2", "-i", "pipe:0", "-f", "audiotoolbox", "-"].map(String::from).into()
}

/// Starts the player on a thread of its own: it takes from `held` and plays until `stop`.
fn player(output: &str, ffmpeg: &Path, held: Arc<Mutex<Held>>, stop: Arc<AtomicBool>) -> Result<std::thread::JoinHandle<()>, String> {
    #[cfg(windows)]
    {
        let _ = ffmpeg;
        windows::start(super::pc_sound::device_id(output).to_string(), held, stop)
    }
    #[cfg(not(windows))]
    {
        use std::io::Write;
        let (program, args) = if crate::platform::tools::OS == crate::platform::tools::Os::MacOs {
            (ffmpeg.to_path_buf(), mac_args())
        } else {
            (std::path::PathBuf::from("pacat"), pacat_args(sink_of(output)))
        };
        let mut child = std::process::Command::new(&program).args(&args).stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn()
            .map_err(|e| format!("{}: {e}", program.display()))?;
        let mut stdin = child.stdin.take().ok_or("no input to the player")?;
        std::thread::Builder::new().name("listen-play".into()).spawn(move || {
            let mut bytes: Vec<u8> = vec![];
            while !stop.load(Ordering::Relaxed) {
                let s = held.lock().unwrap_or_else(|e| e.into_inner()).take(usize::MAX / 4);
                if !s.is_empty() {
                    bytes.clear();
                    bytes.extend(s.iter().flat_map(|v| v.to_le_bytes()));
                    if stdin.write_all(&bytes).is_err() { break }
                }
                std::thread::sleep(LOOK_EVERY);
            }
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
        }).map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use ::windows::Win32::Media::Audio::{AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioClient,
        IAudioRenderClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, eConsole, eRender};
    use ::windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
    use ::windows::core::HSTRING;

    struct Opened { client: IAudioClient, render: IAudioRenderClient, frames: u32 }

    /// Opens output `id` (empty: the default) to play 48 kHz stereo float; Windows converts it to the output's own
    /// format. COM must be set up on this thread.
    unsafe fn open(id: &str) -> Result<Opened, String> {
        let e = |what: &str, err: ::windows::core::Error| format!("{what}: {err}");
        // SAFETY: plain COM calls on this thread's apartment; the format is ours and outlives the call.
        unsafe {
            let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|x| e("the sound devices", x))?;
            let device = if id.is_empty() { devices.GetDefaultAudioEndpoint(eRender, eConsole) } else { devices.GetDevice(&HSTRING::from(id)) }
                .map_err(|x| e("the sound output", x))?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(|x| e("the sound output", x))?;
            let block = (CHANNELS * 4) as u16;
            // 3: IEEE float.
            let format = WAVEFORMATEX { wFormatTag: 3, nChannels: CHANNELS as u16, nSamplesPerSec: RATE, nAvgBytesPerSec: RATE * u32::from(block),
                nBlockAlign: block, wBitsPerSample: 32, cbSize: 0 };
            // A tenth of a second of room: the player tops it up every 10 ms.
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, 1_000_000, 0, &format, None)
                .map_err(|x| e("playing on the sound output", x))?;
            let frames = client.GetBufferSize().map_err(|x| e("playing on the sound output", x))?;
            let render: IAudioRenderClient = client.GetService().map_err(|x| e("playing on the sound output", x))?;
            Ok(Opened { client, render, frames })
        }
    }

    pub fn start(id: String, held: Arc<Mutex<Held>>, stop: Arc<AtomicBool>) -> Result<std::thread::JoinHandle<()>, String> {
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let thread = std::thread::Builder::new().name("listen-play".into()).spawn(move || {
            // SAFETY: COM is set up for this thread only, before any COM call on it.
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            // SAFETY: COM is set up above.
            let o = match unsafe { open(&id) } {
                Ok(o) => o,
                Err(e) => { let _ = tx.send(Err(e)); return }
            };
            let _ = tx.send(Ok(()));
            play(&o, &held, &stop);
        }).map_err(|e| e.to_string())?;
        match rx.recv() {
            Ok(Ok(())) => Ok(thread),
            Ok(Err(e)) => { let _ = thread.join(); Err(e) }
            Err(_) => { let _ = thread.join(); Err("the headphones' output could not be opened".into()) }
        }
    }

    /// Tops the output up with what is held, every 10 ms, until stopped.
    fn play(o: &Opened, held: &Mutex<Held>, stop: &AtomicBool) {
        // SAFETY: the client was initialised for this.
        if let Err(e) = unsafe { o.client.Start() } { eprintln!("listen: could not start: {e}"); return }
        while !stop.load(Ordering::Relaxed) {
            // SAFETY: the buffer Windows lends is written for `n` frames and handed back at once.
            let ok = unsafe {
                match o.client.GetCurrentPadding() {
                    Ok(padding) => {
                        let free = o.frames.saturating_sub(padding) as usize;
                        let s = held.lock().unwrap_or_else(|e| e.into_inner()).take(free);
                        let n = s.len() / CHANNELS;
                        if n > 0 {
                            match o.render.GetBuffer(n as u32) {
                                Ok(data) => {
                                    std::ptr::copy_nonoverlapping(s.as_ptr() as *const u8, data, s.len() * 4);
                                    o.render.ReleaseBuffer(n as u32, 0).is_ok()
                                }
                                Err(_) => false,
                            }
                        } else { true }
                    }
                    Err(_) => false,
                }
            };
            // The output went away (unplugged headphones): listening ends; the stream goes on.
            if !ok { eprintln!("listen: the output stopped"); break }
            std::thread::sleep(LOOK_EVERY);
        }
        // SAFETY: as `Start`.
        let _ = unsafe { o.client.Stop() };
    }
}

pub mod tr {
    use crate::tr;
    /// The first two choices of "Listen to the stream".
    pub fn off() -> String { tr!("Off") }
    pub fn usual() -> String { tr!("This computer's usual output") }
    pub fn not_started(e: &str) -> String { tr!("Listening to the stream could not start: {error}", error = e) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_most_a_quarter_second_is_held_and_the_oldest_goes_first() {
        let mut h = Held::default();
        h.push(&[0.1, 0.2, 0.3, 0.4, 0.5]);
        assert_eq!(h.frames(), 2, "half a frame waits for the rest");
        assert_eq!(h.take(1), [0.1, 0.2]);
        assert_eq!(h.take(10), [0.3, 0.4]);
        h.push(&[0.6]);
        assert_eq!(h.take(1), [0.5, 0.6]);
        // A whole second at once: only the newest quarter second stays, in whole frames.
        let second: Vec<f32> = (0..RATE as usize * CHANNELS).map(|i| i as f32).collect();
        h.push(&second);
        assert_eq!(h.frames(), RATE as usize / 4);
        assert_eq!(h.take(1), [second[second.len() - RATE as usize / 2], second[second.len() - RATE as usize / 2 + 1]]);
    }

    #[test]
    fn linux_and_mac_players_are_told_the_format() {
        assert_eq!(sink_of("pipewire/alsa_output.usb-Headset.analog-stereo"), Some("alsa_output.usb-Headset.analog-stereo"));
        assert_eq!((sink_of("pulse/"), sink_of(DEFAULT), sink_of("coreaudio/X")), (None, None, None));
        let a = pacat_args(Some("x")).join(" ");
        assert_eq!(a, "--playback --raw --format=float32le --rate=48000 --channels=2 --latency-msec=60 --client-name=RustyPresenter --stream-name=Listen to the stream --device=x");
        assert!(!pacat_args(None).iter().any(|x| x.starts_with("--device")));
        assert_eq!(mac_args().join(" "), "-hide_banner -loglevel error -f f32le -ar 48000 -ac 2 -i pipe:0 -f audiotoolbox -");
    }

    /// The real thing on this computer: a real FFmpeg sends a tone to Listen, which plays it on the default output
    /// for a second. Skipped where there is no sound output.
    #[cfg(windows)]
    #[test]
    fn a_real_ffmpeg_sends_its_sound_to_be_played() {
        let Ok(mut l) = Listen::start(DEFAULT, Path::new("ffmpeg")) else { return eprintln!("no sound output here: skipped") };
        let out = std::process::Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-re", "-f", "lavfi", "-i", "sine=frequency=440:duration=1,volume=0.02",
            "-c:a", "pcm_f32le", "-ar", "48000", "-ac", "2", "-f", "f32le", &format!("tcp://127.0.0.1:{}", l.port)]).output();
        l.stop();
        let Ok(out) = out else { return eprintln!("no ffmpeg here: skipped") };
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
}
