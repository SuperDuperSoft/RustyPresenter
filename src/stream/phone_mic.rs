//! A phone as a microphone for the stream. The phone opens its page (`network::phone_mic`, `web/phone-mic.html`),
//! which sends what its microphone hears, 100 ms at a time, as 16-bit mono numbers. They come in here, are made
//! 48 kHz stereo (`board_feed::FORMAT`), and go to the sound input that reads the phone (`board_feed` with input
//! `INPUT`), which fills the gaps with silence by the clock when the phone stops, so the stream never waits for it.
use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The name the phone has among the sound inputs (saved as `input:<INPUT>`).
pub const INPUT: &str = "RustyPresenter phone microphone";
/// The rates a phone may send at (anything else is refused).
pub const RATES: std::ops::RangeInclusive<u32> = 8_000..=192_000;
/// The phone counts as sending while it sent within this long.
const LATELY: Duration = Duration::from_secs(2);

/// Where the phone's sound goes: the input reading it now, if any.
#[derive(Default)]
struct Hub {
    to: Option<mpsc::Sender<Vec<u8>>>,
    resampler: Resampler,
    heard: Option<Instant>,
}

static HUB: Mutex<Option<Hub>> = Mutex::new(None);

/// Tests that use the hub take turns.
#[cfg(test)]
pub(crate) static TEST_TURN: Mutex<()> = Mutex::new(());

fn hub() -> MutexGuard<'static, Option<Hub>> { HUB.lock().unwrap_or_else(|e| e.into_inner()) }

/// A sound input starts reading the phone: its sound comes on the receiver from now on (an earlier reader gets
/// nothing more).
pub fn listen() -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    let mut h = hub();
    let h = h.get_or_insert_with(Hub::default);
    h.to = Some(tx);
    h.resampler = Resampler::default();
    rx
}

/// The phone sent `pcm` (16-bit mono, little-endian, at `rate`). True when the stream takes it now.
pub fn hand_in(rate: u32, pcm: &[u8]) -> bool {
    if !RATES.contains(&rate) { return false }
    let samples: Vec<i16> = pcm.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    let mut h = hub();
    let h = h.get_or_insert_with(Hub::default);
    h.heard = Some(Instant::now());
    let Some(to) = &h.to else { return false };
    let out = h.resampler.to_feed(rate, &samples);
    if to.send(out).is_err() { h.to = None; return false }
    true
}

/// Whether a phone sent sound in the last two seconds.
pub fn heard_lately() -> bool { hub().as_ref().and_then(|h| h.heard).is_some_and(|t| t.elapsed() < LATELY) }

/// Whether the stream takes the phone's sound now.
pub fn taken() -> bool { hub().as_ref().is_some_and(|h| h.to.is_some()) }

/// Turns the phone's mono sound at its rate into `board_feed::FORMAT` (48 kHz stereo 16-bit), piece by piece:
/// straight lines between its samples, carried over from one piece to the next so the joins are smooth.
#[derive(Debug, Default)]
pub struct Resampler {
    /// Where the next sample falls, in the next piece's samples (-1 is the last one of the piece before).
    at: f64,
    /// The last sample of the piece before.
    last: i16,
    rate: u32,
}

impl Resampler {
    pub fn to_feed(&mut self, rate: u32, mono: &[i16]) -> Vec<u8> {
        let to = super::board_feed::FORMAT.rate;
        let mut out = Vec::with_capacity(mono.len() * 4 * to as usize / rate.max(1) as usize + 8);
        let mut put = |s: i16| { let b = s.to_le_bytes(); out.extend_from_slice(&[b[0], b[1], b[0], b[1]]) };
        if rate != self.rate { *self = Resampler { rate, ..Resampler::default() } }
        if rate == to {
            mono.iter().for_each(|s| put(*s));
            return out;
        }
        let n = mono.len();
        if n == 0 { return out }
        let step = f64::from(rate) / f64::from(to);
        let get = |i: isize| if i < 0 { self.last } else { mono[i as usize] };
        let mut at = self.at;
        while at < (n - 1) as f64 {
            let i = at.floor() as isize;
            let (a, b) = (f64::from(get(i)), f64::from(get(i + 1)));
            put((a + (b - a) * (at - i as f64)).round() as i16);
            at += step;
        }
        self.at = at - n as f64;
        self.last = mono[n - 1];
        out
    }
}

pub mod tr {
    use crate::tr;
    /// The phone among the sound inputs in Settings.
    pub fn input_label() -> String { tr!("Phone microphone (open its page on the phone)") }
    // The phone's page.
    pub fn title() -> String { tr!("Phone microphone") }
    pub fn start() -> String { tr!("Start sending sound") }
    pub fn stop() -> String { tr!("Stop") }
    pub fn sending() -> String { tr!("Sending to the stream. Keep this page open and the phone awake.") }
    pub fn not_taken() -> String { tr!("Sending, but the stream is not using this phone yet. Choose \"Phone microphone\" in Settings → Media → Livestream, then go live.") }
    pub fn needs_secure() -> String { tr!("The phone only lets a secure page use its microphone. Open the secure page below; the first time, the phone warns that it is not private: tap Advanced (or Show Details) and go on to it.") }
    pub fn no_microphone() -> String { tr!("The phone did not let the page use its microphone. Allow it in the browser's settings for this page, then try again.") }
    pub fn needs_stage_view() -> String { tr!("Turn on the stage screen for phones in RustyPresenter (Settings → Network…), and sign in to it on this phone first.") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(bytes: &[u8]) -> Vec<(i16, i16)> {
        bytes.chunks_exact(4).map(|b| (i16::from_le_bytes([b[0], b[1]]), i16::from_le_bytes([b[2], b[3]]))).collect()
    }

    #[test]
    fn the_phones_sound_becomes_48_khz_stereo_with_smooth_joins() {
        let mut r = Resampler::default();
        // At 48 kHz: each sample twice, left and right.
        assert_eq!(samples(&r.to_feed(48_000, &[1, -2, 300])), [(1, 1), (-2, -2), (300, 300)]);
        // 24 kHz: a sample between each two, on the straight line.
        let mut r = Resampler::default();
        let a = samples(&r.to_feed(24_000, &[0, 100, 200]));
        assert_eq!(a.iter().map(|s| s.0).collect::<Vec<_>>(), [0, 50, 100, 150]);
        // The next piece goes on from the last sample: no jump at the join.
        let b = samples(&r.to_feed(24_000, &[300, 400]));
        assert_eq!(b.iter().map(|s| s.0).collect::<Vec<_>>(), [200, 250, 300, 350]);
        // Over many pieces the length is right: one second of 44.1 kHz is one second of 48 kHz.
        let mut r = Resampler::default();
        let total: usize = (0..10).map(|_| r.to_feed(44_100, &vec![1000i16; 4410]).len() / 4).sum();
        assert!((47_990..=48_000).contains(&total), "{total}");
        assert!(r.to_feed(44_100, &[]).is_empty());
    }

    /// The hub is shared by the whole program: one test drives it from start to end.
    #[test]
    fn sound_from_the_phone_reaches_the_input_reading_it() {
        let _turn = TEST_TURN.lock().unwrap_or_else(|e| e.into_inner());
        let pcm: Vec<u8> = [500i16, -500].iter().flat_map(|s| s.to_le_bytes()).collect();
        assert!(!hand_in(1000, &pcm), "a rate no phone has");
        let rx = listen();
        assert!(taken());
        assert!(hand_in(48_000, &pcm));
        assert!(heard_lately());
        assert_eq!(samples(&rx.recv().unwrap()), [(500, 500), (-500, -500)]);
        // A new reader takes over; the old one gets nothing more.
        let rx2 = listen();
        assert!(hand_in(48_000, &pcm));
        assert!(rx.try_recv().is_err() && rx2.try_recv().is_ok());
        // The reader gone: not taken.
        drop(rx2);
        assert!(!hand_in(48_000, &pcm));
        assert!(!taken());
    }
}
