//! The sound input (a sound board, a microphone) of a recording or a livestream, passed through the app. A small
//! FFmpeg of its own reads the input, and this passes its sound on to the recorder's FFmpeg over a local connection
//! FFmpeg listens on. On the way:
//! - **Mute** (the stream's panic button) puts silence in its place at once;
//! - its level is watched: quiet for `QUIET_FOR` and the operator is told (a muted send, a pulled cable);
//! - when the input stops (a USB sound interface unplugged) silence goes on by the clock and the input is opened
//!   again every `RETRY`. FFmpeg reading the input itself ended the whole recording then.
use super::pc_sound::{Feed, Format};
use crate::platform::proc::spawn_tree;
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// What is sent: 16-bit stereo at 48 kHz, whatever the input is.
pub const FORMAT: Format = Format { samples: "s16le", rate: 48_000, channels: 2 };
/// Quieter than this (about −60 dB of full scale) is no sound. Room noise on a live microphone is louder; a send
/// that is off is far quieter.
pub const QUIET_PEAK: i32 = 33;
/// No sound for this long: the operator is told.
pub const QUIET_FOR: Duration = Duration::from_secs(10);
/// No sound from the input's FFmpeg for this long: it has stopped or not started yet, and silence is put in.
const GONE_AFTER: Duration = Duration::from_millis(200);
/// How long after the input stopped it is opened again.
pub const RETRY: Duration = Duration::from_secs(2);
const LOOK_EVERY: Duration = Duration::from_millis(10);
const CONNECT_FOR: Duration = Duration::from_secs(30);

/// The input's own FFmpeg: sound input `name` of FFmpeg input `format` ("dshow", …) as raw `FORMAT` on its output.
pub fn input_args(format: &str, name: &str) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin"].map(String::from).into();
    // dshow holds half a second of sound by default; 50 ms keeps it close to the picture.
    if format == "dshow" { a.extend(["-audio_buffer_size".into(), "50".into()]) }
    a.extend(["-f".into(), format.into(), "-i".into(), crate::record::args::device_input(format, name), "-vn".into(),
        "-f".into(), FORMAT.samples.into(), "-ar".into(), FORMAT.rate.to_string(), "-ac".into(), FORMAT.channels.to_string(), "-".into()]);
    a
}

/// The sound may run this far behind the clock, or this far ahead of it, before it is brought back (`drift_step`).
/// An input's sound always comes a little late and in pieces, so "behind" swings by a piece's length around a
/// small lateness of its own; these sit outside that swing.
const BEHIND_MOST: Duration = Duration::from_millis(150);
const AHEAD_MOST: Duration = Duration::from_millis(50);

/// Keeps a sound input in step with the clock, and so with the picture, which is timed by the clock. A sound card
/// counts its samples by its own crystal, never quite at the computer's pace: a little slow, and its sound is
/// short of the picture by more and more (a tenth of a second or more over a service, seconds with a poor USB
/// interface); a little fast, and it runs ahead. Nothing put that right while the input kept sending.
///
/// `written` frames have gone out `elapsed` after the start, at `rate` frames a second. +1: one frame is said
/// twice now; -1: one is left out; 0: in step. One frame at a time (a 48,000th of a second, a few times a second
/// at most) cannot be heard, where a gap of silence could.
pub fn drift_step(elapsed: Duration, written: u64, rate: u32) -> i64 {
    let due = (elapsed.as_secs_f64() * f64::from(rate)) as i64;
    let behind = due - written as i64;
    let frames = |d: Duration| (d.as_secs_f64() * f64::from(rate)) as i64;
    if behind > frames(BEHIND_MOST) { 1 } else if behind < -frames(AHEAD_MOST) { -1 } else { 0 }
}

/// `drift_step` done to `buf` (whole frames of `frame` bytes, at least one): its last frame said twice, or left out.
/// The frames added (+1, -1 or 0).
fn keep_step(buf: &mut Vec<u8>, frame: usize, step: i64) -> i64 {
    if frame == 0 || buf.len() < frame { return 0 }
    match step {
        1 => { buf.extend_from_within(buf.len() - frame..); 1 }
        -1 => { buf.truncate(buf.len() - frame); -1 }
        _ => 0,
    }
}

/// The loudest sample of 16-bit sound `bytes`.
pub fn peak(bytes: &[u8]) -> i32 {
    bytes.chunks_exact(2).map(|b| (i16::from_le_bytes([b[0], b[1]]) as i32).abs()).max().unwrap_or(0)
}

/// How long the input has been quiet.
#[derive(Default)]
pub struct Quiet { quiet: Duration }

impl Quiet {
    /// Takes in sound lasting `took` whose loudest sample is `peak`. True when it has been quiet for `QUIET_FOR`.
    pub fn hear(&mut self, peak: i32, took: Duration) -> bool {
        self.quiet = if peak > QUIET_PEAK { Duration::ZERO } else { self.quiet + took };
        self.quiet >= QUIET_FOR
    }
}

/// The feed under way. Dropping it stops it and the input's FFmpeg.
pub struct Board {
    pub feed: Feed,
    /// Whether the input has been quiet for `QUIET_FOR`.
    pub silent: Arc<AtomicBool>,
    /// How loud what goes out is, for the meter (0 while muted).
    pub level: Arc<super::level::Level>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Board {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() { let _ = t.join(); }
    }
}

impl Drop for Board {
    fn drop(&mut self) { self.stop() }
}

/// What reads the input.
#[derive(Clone)]
enum Reader {
    /// A program (FFmpeg, `parec`) with its arguments, sending `FORMAT` on its output.
    Program(PathBuf, Vec<String>),
    /// The phone's page (`super::phone_mic`).
    Phone,
}

/// Starts passing sound input `name` (FFmpeg input `format`) on, read by `ffmpeg` (on Linux, by `parec` when this
/// FFmpeg has no PulseAudio: `record::devices::sound_reader`; the phone, from its page); silence while `mute` is set.
pub fn start(ffmpeg: PathBuf, format: &str, name: &str, mute: Arc<AtomicBool>) -> Result<Board, String> {
    let port = crate::platform::ports::tcp().map_err(|e| e.to_string())?;
    let (stop, silent) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let (s, q) = (stop.clone(), silent.clone());
    let level: Arc<super::level::Level> = Arc::default();
    let l = level.clone();
    let reader = if name == super::phone_mic::INPUT { Reader::Phone } else {
        let (program, args) = crate::record::devices::sound_reader(&ffmpeg, format, name);
        Reader::Program(program, args)
    };
    let thread = std::thread::Builder::new().name("stream-board".into()).spawn(move || run(reader, port, &s, &q, &mute, &l)).map_err(|e| e.to_string())?;
    Ok(Board { feed: Feed { format: FORMAT, port }, silent, level, stop, thread: Some(thread) })
}

/// An input being read: its sound comes on `rx`.
struct Input {
    stop: Arc<AtomicBool>,
    /// The program's reading thread; `None` for the phone, which never ends by itself.
    thread: Option<std::thread::JoinHandle<()>>,
    rx: mpsc::Receiver<Vec<u8>>,
}

impl Input {
    fn finished(&self) -> bool { self.thread.as_ref().is_some_and(|t| t.is_finished()) }
    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread { let _ = t.join(); }
    }
}

fn open(reader: &Reader) -> Option<Input> {
    match reader {
        Reader::Program(program, args) => spawn_input(program.clone(), args.clone()).map(|(stop, thread, rx)| Input { stop, thread: Some(thread), rx }),
        Reader::Phone => Some(Input { stop: Arc::default(), thread: None, rx: super::phone_mic::listen() }),
    }
}

fn run(reader: Reader, port: u16, stop: &AtomicBool, silent: &AtomicBool, mute: &AtomicBool, level: &super::level::Level) {
    let began = Instant::now();
    let mut out = loop {
        if stop.load(Ordering::Relaxed) { return }
        match std::net::TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => break s,
            Err(_) if began.elapsed() < CONNECT_FOR => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => { eprintln!("sound input: FFmpeg did not listen: {e}"); return }
        }
    };
    let _ = out.set_nodelay(true);
    let frame = FORMAT.frame_bytes();
    let mut input: Option<Input> = None;
    let mut next_try = Instant::now();
    let mut quiet = Quiet::default();
    let t0 = Instant::now();
    let mut last_heard = t0;
    let mut written: u64 = 0;
    let mut buf: Vec<u8> = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        if input.as_ref().is_some_and(Input::finished) { input = None; next_try = Instant::now() + RETRY }
        if input.is_none() && Instant::now() >= next_try {
            input = open(&reader);
            if input.is_none() { next_try = Instant::now() + RETRY }
        }
        buf.clear();
        if let Some(i) = &input { while let Ok(b) = i.rx.try_recv() { buf.extend_from_slice(&b) } }
        let heard = buf.len() / frame;
        buf.truncate(heard * frame);
        if heard > 0 { last_heard = Instant::now() }
        let p = peak(&buf);
        written += heard as u64;
        // While the input sends: held to the clock a frame at a time (`drift_step`).
        if heard > 0 { written = written.saturating_add_signed(keep_step(&mut buf, frame, drift_step(t0.elapsed(), written, FORMAT.rate))) }
        // Nothing from the input for a while: silence, by the clock, so the picture is not held back.
        if last_heard.elapsed() >= GONE_AFTER {
            let gap = super::pc_sound::silence_needed(t0.elapsed(), written, FORMAT.rate);
            buf.resize(buf.len() + gap as usize * frame, 0);
            written += gap;
        }
        let took = Duration::from_secs_f64((buf.len() / frame) as f64 / FORMAT.rate as f64);
        silent.store(quiet.hear(p, took), Ordering::Relaxed);
        let muted = mute.load(Ordering::Relaxed);
        if muted { buf.fill(0) }
        level.hear(if muted { 0 } else { p });
        if !buf.is_empty() && out.write_all(&buf).is_err() { break }
        std::thread::sleep(LOOK_EVERY);
    }
    if let Some(i) = input { i.stop() }
}

/// Starts the input's FFmpeg on a thread of its own that hands its sound over in pieces, until the input stops or
/// the flag is set (the FFmpeg is then stopped). `None` when the thread could not start.
fn spawn_input(ffmpeg: PathBuf, args: Vec<String>) -> Option<(Arc<AtomicBool>, std::thread::JoinHandle<()>, mpsc::Receiver<Vec<u8>>)> {
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    let (tx, rx) = mpsc::channel();
    let thread = std::thread::Builder::new().name("stream-board-read".into()).spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
        rt.block_on(async move {
            use tokio::io::AsyncReadExt;
            let mut cmd = tokio::process::Command::new(&ffmpeg);
            cmd.args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
            let mut tree = match spawn_tree(cmd) { Ok(t) => t, Err(e) => { eprintln!("sound input: could not start: {e}"); return } };
            let Some(mut stdout) = tree.stdout() else { return };
            let mut b = vec![0u8; 16_384];
            while !s.load(Ordering::Relaxed) {
                match tokio::time::timeout(Duration::from_millis(500), stdout.read(&mut b)).await {
                    Ok(Ok(0)) | Ok(Err(_)) => break,
                    Ok(Ok(n)) => { if tx.send(b[..n].to_vec()).is_err() { break } }
                    // Nothing for half a second: a slow input still opening, or stuck; looked at again.
                    Err(_) => continue,
                }
            }
            tree.kill();
        });
    }).ok()?;
    Some((stop, thread, rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An hour of an input whose clock is `ppm` parts in a million off, handed over every 10 ms: how far the sound
    /// written ends from the clock, in milliseconds (+: behind), and the furthest it ever was.
    fn an_hour(ppm: f64) -> (f64, f64) {
        let rate = f64::from(FORMAT.rate);
        // `taken`: the frames taken from the card; `written`: the frames sent on (one more or fewer now and then).
        let (mut written, mut taken, mut worst) = (0u64, 0u64, 0.0f64);
        for tick in 1..=360_000u64 {
            let t = Duration::from_millis(tick * 10);
            // What the card has made by now, by its own clock, in whole frames.
            let made = (t.as_secs_f64() * rate * (1.0 + ppm / 1e6)) as u64;
            let heard = made - taken;
            taken = made;
            let mut buf = vec![0u8; heard as usize * 4];
            written += heard;
            if heard > 0 { written = written.saturating_add_signed(keep_step(&mut buf, 4, drift_step(t, written, FORMAT.rate))) }
            assert_eq!(buf.len() % 4, 0);
            worst = worst.max(((t.as_secs_f64() * rate - written as f64) / rate * 1000.0).abs());
        }
        ((3600.0 * rate - written as f64) / rate * 1000.0, worst)
    }

    #[test]
    fn a_sound_card_a_little_slow_or_fast_is_held_to_the_clock() {
        // 500 parts in a million is 1.8 s over an hour left alone.
        let (slow, worst) = an_hour(-500.0);
        assert!(slow > 100.0 && slow < 170.0 && worst < 170.0, "a slow card ends {slow:.0} ms behind (worst {worst:.0})");
        let (fast, worst) = an_hour(500.0);
        assert!(fast < -30.0 && fast > -70.0 && worst < 170.0, "a fast card ends {fast:.0} ms off (worst {worst:.0})");
        // A good card (20 parts in a million, 72 ms an hour) is left alone.
        let (good, _) = an_hour(-20.0);
        assert!((good - 72.0).abs() < 2.0, "{good:.1}");
    }

    #[test]
    fn a_frame_is_said_twice_or_left_out() {
        let rate = FORMAT.rate;
        let s = Duration::from_secs;
        assert_eq!(drift_step(s(10), 480_000, rate), 0, "in step");
        assert_eq!(drift_step(s(10), 480_000 - 4_800, rate), 0, "a tenth of a second late is an input's own lateness");
        assert_eq!(drift_step(s(10), 480_000 - 9_600, rate), 1, "a fifth of a second behind");
        assert_eq!(drift_step(s(10), 480_000 + 4_800, rate), -1, "a tenth of a second ahead");
        let mut buf = vec![1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!((keep_step(&mut buf, 4, 1), buf.clone()), (1, vec![1, 2, 3, 4, 5, 6, 7, 8, 5, 6, 7, 8]));
        assert_eq!((keep_step(&mut buf, 4, -1), buf.len()), (-1, 8));
        assert_eq!(keep_step(&mut Vec::new(), 4, 1), 0, "nothing came: nothing to say twice");
    }

    #[test]
    fn the_inputs_ffmpeg_sends_16_bit_stereo() {
        let a = input_args("dshow", "Line In (USB)").join(" ");
        assert_eq!(a, "-hide_banner -loglevel error -nostdin -audio_buffer_size 50 -f dshow -i audio=Line In (USB) -vn -f s16le -ar 48000 -ac 2 -");
        assert!(!input_args("pulse", "default").contains(&"-audio_buffer_size".to_string()));
    }

    #[test]
    fn quiet_for_ten_seconds_is_silent_and_any_sound_starts_again() {
        let s = Duration::from_secs;
        assert_eq!(peak(&[0x10, 0x00, 0xF0, 0xFF, 0x00, 0x80]), 32768, "the loudest, either way");
        let mut q = Quiet::default();
        assert!(!q.hear(10, s(9)));
        assert!(q.hear(QUIET_PEAK, s(1)), "ten seconds of hiss under the line");
        assert!(!q.hear(QUIET_PEAK + 1, Duration::from_millis(10)), "sound");
        assert!(!q.hear(0, s(9)) && q.hear(0, s(2)));
    }

    fn ffmpeg() -> Option<PathBuf> {
        let found = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).flat_map(|d| [d.join("ffmpeg"), d.join("ffmpeg.exe")]).find(|p| p.is_file()));
        if found.is_none() { eprintln!("no ffmpeg here: skipped") }
        found
    }

    /// Plays the recorder: listens on the feed's port and reads `secs` seconds, calling `each` with the time and
    /// each piece read. Returns the seconds of sound that came.
    fn listen(board: &Board, secs: u64, mut each: impl FnMut(Duration, &[u8])) -> f64 {
        let listener = std::net::TcpListener::bind(("127.0.0.1", board.feed.port)).unwrap();
        let (mut conn, _) = listener.accept().unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let t = Instant::now();
        let mut bytes = 0usize;
        let mut b = vec![0u8; 65536];
        while t.elapsed() < Duration::from_secs(secs) {
            use std::io::Read;
            let n = match conn.read(&mut b) { Ok(0) | Err(_) => break, Ok(n) => n };
            bytes += n;
            each(t.elapsed(), &b[..n]);
        }
        bytes as f64 / (FORMAT.rate as f64 * FORMAT.frame_bytes() as f64)
    }

    /// A real FFmpeg as the input: a tone for 1 s, then nothing (a send turned off). The sound comes at the clock's
    /// pace, and the quiet is noticed once it has lasted ten seconds. About 13 s.
    #[test]
    fn a_real_input_is_passed_on_and_heard_to_go_quiet() {
        let Some(ffmpeg) = ffmpeg() else { return };
        let mut board = start(ffmpeg, "lavfi", "aevalsrc=if(lt(t\\,1)\\,0.3*sin(2*PI*440*t)\\,0):s=48000:c=stereo,arealtime", Arc::default()).unwrap();
        let silent = board.silent.clone();
        let (mut loud, mut noticed) = (0, None);
        let secs = listen(&board, 13, |t, b| {
            if t < Duration::from_millis(1500) { loud = loud.max(peak(b)) }
            if noticed.is_none() && silent.load(Ordering::Relaxed) { noticed = Some(t) }
        });
        board.stop();
        assert!(loud > 5000, "the tone came through: {loud}");
        assert!((11.5..14.0).contains(&secs), "{secs:.2} s of sound in 13 s");
        let at = noticed.expect("the quiet was noticed").as_secs_f64();
        assert!((10.5..12.5).contains(&at), "noticed {at:.1} s in (quiet from about 1 s)");
    }

    /// The phone as the input: what it sends goes out, and silence by the clock once it stops. About 3 s.
    #[test]
    fn the_phones_sound_is_passed_on_like_any_input() {
        let _turn = super::super::phone_mic::TEST_TURN.lock().unwrap_or_else(|e| e.into_inner());
        let mut board = start(PathBuf::from("no-ffmpeg-needed"), "dshow", super::super::phone_mic::INPUT, Arc::default()).unwrap();
        let tone: Vec<u8> = (0..4800).flat_map(|i| (if i % 2 == 0 { 3000i16 } else { -3000 }).to_le_bytes()).collect();
        let sending = Arc::new(AtomicBool::new(true));
        let s = sending.clone();
        // The phone: 100 ms of sound ten times a second for 1.5 s, then nothing.
        let phone = std::thread::spawn(move || {
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(1500) && s.load(Ordering::Relaxed) {
                super::super::phone_mic::hand_in(48_000, &tone);
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let mut loud = 0;
        let secs = listen(&board, 3, |t, b| if t < Duration::from_millis(900) { loud = loud.max(peak(b)) });
        sending.store(false, Ordering::Relaxed);
        let _ = phone.join();
        board.stop();
        assert_eq!(loud, 3000, "the phone's sound came through");
        assert!((2.5..3.5).contains(&secs), "{secs:.2} s of sound in 3 s");
    }

    /// Mute while a tone plays: silence goes out from then on; unmuted, the tone again.
    #[test]
    fn a_real_input_is_muted_at_once() {
        let Some(ffmpeg) = ffmpeg() else { return };
        let mute = Arc::new(AtomicBool::new(false));
        let mut board = start(ffmpeg, "lavfi", "sine=frequency=440:sample_rate=48000,volume=0.3,arealtime", mute.clone()).unwrap();
        let (mut before, mut muted, mut after) = (0, 0, 0);
        let m = mute.clone();
        listen(&board, 5, |t, b| {
            let ms = t.as_millis();
            if ms >= 2000 && ms < 3500 { m.store(true, Ordering::Relaxed) } else if ms >= 3500 { m.store(false, Ordering::Relaxed) }
            // A moment's grace after each switch for what was already on its way.
            match ms { 1000..2000 => before = before.max(peak(b)), 2300..3500 => muted = muted.max(peak(b)), 3800.. => after = after.max(peak(b)), _ => {} }
        });
        board.stop();
        // The tone peaks near 870 (FFmpeg's sine is 1/8 of full scale, then 0.3, then mono to stereo).
        assert!(before > 500 && after > 500, "the tone: {before} before, {after} after");
        assert_eq!(muted, 0, "muted: silence");
    }
}
