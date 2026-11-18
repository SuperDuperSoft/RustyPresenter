use super::*;
use std::f64::consts::PI;

/// `secs` of a tone whose pitch follows `hz(t)`, with a second harmonic, as 16-bit samples.
fn tone(secs: f64, hz: impl Fn(f64) -> f64, env: impl Fn(f64) -> f64) -> Vec<i16> {
    let mut phase = 0.0;
    (0..(secs * RATE as f64) as usize).map(|i| {
        let t = i as f64 / RATE as f64;
        phase += 2.0 * PI * hz(t) / RATE as f64;
        let v = env(t) * (0.3 * phase.sin() + 0.1 * (2.0 * phase).sin());
        (v * 32767.0) as i16
    }).collect()
}

/// A sung melody: a note every half second, with a 5.5 Hz vibrato.
pub(crate) fn singing(secs: f64) -> Vec<i16> {
    let notes = [220.0, 247.0, 262.0, 247.0];
    tone(secs, |t| notes[(t / 0.5) as usize % 4] * 2f64.powf(0.3 / 12.0 * (2.0 * PI * 5.5 * t).sin()), |_| 1.0)
}

/// Talk: syllables whose pitch glides up or down, with short gaps.
fn speaking(secs: f64) -> Vec<i16> {
    tone(secs, |t| {
        let p = (t % 0.25) / 0.18;
        if (t / 0.25) as usize % 2 == 0 { 210.0 * (140.0f64 / 210.0).powf(p.min(1.0)) } else { 140.0 * (200.0f64 / 140.0).powf(p.min(1.0)) }
    }, |t| if t % 0.25 < 0.18 { 1.0 } else { 0.0 })
}

/// Noise bursts (drums, a band without a voice).
fn noise(secs: f64) -> Vec<i16> {
    let mut seed: u32 = 1;
    (0..(secs * RATE as f64) as usize).map(|i| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let on = (i as f64 / RATE as f64) % 0.25 < 0.2;
        if on { ((seed >> 16) as i16 as f32 * 0.3) as i16 } else { 0 }
    }).collect()
}

pub(crate) fn silence(secs: f64) -> Vec<i16> { vec![0; (secs * RATE as f64) as usize] }

/// Runs `pcm` through a tracker; the labels from the second half second on, and the last onset rate.
fn track(t: &mut VoiceTracker, pcm: &[i16], from: f64) -> (Vec<Voice>, f32) {
    let mut labels = vec![];
    let mut rate = 0.0;
    for (i, f) in pcm.chunks(FRAME).enumerate() {
        let (v, r) = t.frame(f, from + i as f64 * 0.02);
        if i >= 25 { labels.push(v) }
        rate = r;
    }
    (labels, rate)
}

#[test]
fn voice_tracker_labels_clips() {
    for (name, pcm, want) in [
        ("singing", singing(2.0), Voice::Singing),
        ("talk", speaking(2.0), Voice::Speaking),
        ("noise", noise(2.0), Voice::Music),
        ("silence", silence(2.0), Voice::Silence),
    ] {
        let (labels, _) = track(&mut VoiceTracker::new(), &pcm, 0.0);
        let right = labels.iter().filter(|&&v| v == want).count();
        assert!(right * 10 >= labels.len() * 9, "{name}: {labels:?}");
    }
    // One tracker through singing, then quiet: the label follows within half a second.
    let mut t = VoiceTracker::new();
    let (a, _) = track(&mut t, &singing(2.0), 0.0);
    let (b, _) = track(&mut t, &silence(2.0), 2.0);
    assert_eq!((a.last(), b.last()), (Some(&Voice::Singing), Some(&Voice::Silence)));
    // A dead-steady note is an instrument, not a voice.
    let (organ, _) = track(&mut VoiceTracker::new(), &tone(2.0, |_| 220.0, |_| 1.0), 0.0);
    assert_eq!(organ.last(), Some(&Voice::Music));
}

#[test]
fn wide_vibrato_over_a_band_is_still_singing() {
    let vocal = tone(2.0, |t| 180.0 * 2f64.powf((2.0 * PI * 6.0 * t).sin() / 12.0), |_| 1.0);
    let pcm: Vec<_> = vocal.into_iter().enumerate().map(|(i, sample)| {
        let t = i as f64 / RATE as f64;
        let chord: f64 = [110.0, 164.81, 220.0].iter().map(|hz| (2.0 * PI * hz * t).sin()).sum();
        (f64::from(sample) + 980.0 * chord) as i16
    }).collect();
    let (labels, _) = track(&mut VoiceTracker::new(), &pcm, 0.0);
    let sung = labels.iter().filter(|&&v| v == Voice::Singing).count();
    assert!(sung * 10 >= labels.len() * 8, "{sung}/{}: {labels:?}", labels.len());
}

#[test]
fn a_straight_tail_stays_sung_and_a_new_instrument_stays_music() {
    let pcm = tone(2.0, |t| {
        let vibrato = if t < 1.0 { 0.35 * (2.0 * PI * 6.0 * t).sin() } else { 0.0 };
        220.0 * 2f64.powf(vibrato / 12.0)
    }, |_| 1.0);
    let mut tracker = VoiceTracker::new();
    let (labels, _) = track(&mut tracker, &pcm, 0.0);
    assert_eq!(labels.last(), Some(&Voice::Singing));
    assert!(tracker.held_since(1.98).is_some());
    track(&mut tracker, &silence(0.5), 2.0);
    let (labels, _) = track(&mut tracker, &tone(1.5, |_| 220.0, |_| 1.0), 2.5);
    assert_eq!(labels.last(), Some(&Voice::Music));
    assert!(tracker.held_since(3.98).is_none());
}

#[test]
fn bass_pitch_and_mixer_dc_offset() {
    let pcm = tone(0.04, |_| 70.0, |_| 1.0);
    let window: Vec<_> = pcm.iter().map(|&s| f32::from(s) / 32768.0).collect();
    assert!((pitch_hz(&window).unwrap() - 70.0).abs() < 1.0);
    let (labels, _) = track(&mut VoiceTracker::new(), &vec![1000; RATE], 0.0);
    assert!(labels.iter().all(|&label| label == Voice::Silence));
}

#[test]
fn onset_rate_from_clicks() {
    // A short click every half second (2 a second) for 10 s.
    let pcm: Vec<i16> = (0..10 * RATE).map(|i| if (i + RATE / 4) % (RATE / 2) < 80 { 12_000 } else { 0 }).collect();
    let mut t = VoiceTracker::new();
    let (_, rate) = track(&mut t, &pcm, 0.0);
    assert!((rate - 2.0).abs() < 0.3, "{rate}");
    assert!(t.level() < 0.01);
    // Quiet: the rate falls away.
    let (_, rate) = track(&mut t, &silence(5.0), 10.0);
    assert_eq!(rate, 0.0);
}
