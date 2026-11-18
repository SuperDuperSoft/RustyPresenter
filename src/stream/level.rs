//! How loud the stream's sound is, for the meter beside Mute while live: the sound feeds (`board_feed`,
//! `pc_sound`) hand in the loudest sample of each piece they pass on (0 while muted), and the operator's window
//! takes the loudest since it last looked, four times a second, as bars.
use std::sync::atomic::{AtomicU32, Ordering};

/// The loudest sample (16-bit scale, 0 to 32767) since the window last looked.
#[derive(Debug, Default)]
pub struct Level(AtomicU32);

impl Level {
    /// A piece of sound whose loudest sample is `peak` went out.
    pub fn hear(&self, peak: i32) { self.0.fetch_max(peak.clamp(0, 32_767) as u32, Ordering::Relaxed); }

    /// The loudest since the last call, and starts again from silence.
    pub fn take(&self) -> i32 { self.0.swap(0, Ordering::Relaxed) as i32 }
}

/// The meter's lit bars (0 to 5) for `peak`: about −50, −35, −20, −10 and −3 dB of full scale.
pub fn bars(peak: i32) -> i32 {
    if peak <= 0 { return 0 }
    let db = 20.0 * (f64::from(peak) / 32_767.0).log10();
    [-50.0, -35.0, -20.0, -10.0, -3.0].iter().filter(|t| db >= **t).count() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loudest_since_the_last_look_is_kept_once() {
        let l = Level::default();
        l.hear(1000);
        l.hear(20_000);
        l.hear(300);
        assert_eq!(l.take(), 20_000);
        assert_eq!(l.take(), 0, "taken: silence until more comes");
        l.hear(-5);
        assert_eq!(l.take(), 0, "nothing below silence");
    }

    #[test]
    fn bars_follow_the_loudness_in_decibels() {
        assert_eq!(bars(0), 0);
        assert_eq!(bars(50), 0, "below -50 dB: nothing");
        assert_eq!(bars(150), 1, "about -47 dB");
        assert_eq!(bars(1_500), 2, "about -27 dB... -35 to -20 is two");
        assert_eq!(bars(5_000), 3, "about -16 dB");
        assert_eq!(bars(16_000), 4, "about -6 dB");
        assert_eq!(bars(32_767), 5, "full scale");
    }
}
