//! The frame pump: the room draws only when something changes, but the file needs a steady 30 frames a second, so
//! the writer repeats the newest picture until a new one comes (`Pump`). Pictures arrive in `FrameSink` from the
//! UI thread, which never waits for the writer.
use super::sound::SoundLog;
use std::sync::{Mutex, OnceLock, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};

/// How many frames the writer may owe before it gives up on the late ones (2 s): a stalled disk or a busy
/// computer then loses frames rather than writing a long burst.
const MAX_OWED_SECS: u64 = 2;

/// What the writer should do now.
#[derive(Debug, PartialEq, Eq)]
pub struct Due {
    /// Frames to write now (the newest picture, again if nothing new came).
    pub write: u64,
    /// Frames given up because the writer fell too far behind.
    pub dropped: u64,
}

impl Due {
    /// How far behind the writer was: the frames owed beyond the one due now, at `fps`.
    pub fn lag(&self, fps: u32) -> Duration {
        Duration::from_secs_f64((self.write + self.dropped).saturating_sub(1) as f64 / f64::from(fps.max(1)))
    }
}

/// Counts frames against the clock. Frame `n` is due at `n / fps` seconds.
#[derive(Debug)]
pub struct Pump {
    fps: u32,
    sent: u64,
}

impl Pump {
    pub fn new(fps: u32) -> Pump { Pump { fps: fps.max(1), sent: 0 } }

    /// Frames due `elapsed` after the start, counting them as written.
    pub fn due(&mut self, elapsed: Duration) -> Due {
        let want = (elapsed.as_secs_f64() * f64::from(self.fps)).floor() as u64 + 1;
        let owed = want.saturating_sub(self.sent);
        let limit = MAX_OWED_SECS * u64::from(self.fps);
        let dropped = owed.saturating_sub(limit);
        self.sent = want;
        Due { write: owed - dropped, dropped }
    }

    /// When the next frame is due, from the start.
    pub fn next_at(&self) -> Duration { Duration::from_secs_f64(self.sent as f64 / f64::from(self.fps)) }

    pub fn sent(&self) -> u64 { self.sent }
}

/// Where a `w`×`h` picture goes inside `dw`×`dh`, keeping its shape (black bars around): `(x, y, width, height)`.
pub fn fit_rect(w: u32, h: u32, dw: u32, dh: u32) -> (u32, u32, u32, u32) {
    if w == 0 || h == 0 { return (0, 0, dw, dh) }
    let (fw, fh) = if u64::from(w) * u64::from(dh) >= u64::from(h) * u64::from(dw) {
        (dw, ((u64::from(h) * u64::from(dw) + u64::from(w) / 2) / u64::from(w)) as u32)
    } else {
        (((u64::from(w) * u64::from(dh) + u64::from(h) / 2) / u64::from(h)) as u32, dh)
    };
    let (fw, fh) = (fw.clamp(1, dw), fh.clamp(1, dh));
    ((dw - fw) / 2, (dh - fh) / 2, fw, fh)
}

/// A picture as read from the room: RGBA rows, top row first unless `bottom_up` (OpenGL's order).
pub struct Raw<'a> {
    pub w: u32,
    pub h: u32,
    pub bottom_up: bool,
    pub rgba: &'a [u8],
}

/// Draws `src` into the `dw`×`dh` RGBA picture `dst`, top row first, with black bars where the shapes differ. The
/// nearest pixel is taken: this runs only on the slow paths (software drawing, very old OpenGL).
pub fn fit(src: &Raw, dst: &mut [u8], dw: u32, dh: u32) {
    let (x0, y0, fw, fh) = fit_rect(src.w, src.h, dw, dh);
    dst.chunks_exact_mut(4).for_each(|p| p.copy_from_slice(&[0, 0, 0, 255]));
    if src.rgba.len() < (src.w as usize) * (src.h as usize) * 4 || src.w == 0 || src.h == 0 { return }
    let sx: Vec<usize> = (0..fw).map(|x| ((u64::from(x) * u64::from(src.w)) / u64::from(fw)).min(u64::from(src.w) - 1) as usize * 4).collect();
    for y in 0..fh {
        let mut row = ((u64::from(y) * u64::from(src.h)) / u64::from(fh)).min(u64::from(src.h) - 1) as usize;
        if src.bottom_up { row = src.h as usize - 1 - row }
        let s = &src.rgba[row * src.w as usize * 4..(row + 1) * src.w as usize * 4];
        let d0 = ((y0 + y) as usize * dw as usize + x0 as usize) * 4;
        let d = &mut dst[d0..d0 + fw as usize * 4];
        for (px, &o) in d.chunks_exact_mut(4).zip(&sx) {
            px[..3].copy_from_slice(&s[o..o + 3]);
            px[3] = 255;
        }
    }
}

/// Pictures on their way from the room to the writer, and the sound heard meanwhile. Shared by the room (UI
/// thread) and the writer thread.
pub struct FrameSink {
    /// The recording's picture size.
    pub size: (u32, u32),
    slot: Mutex<Slot>,
    /// When the first frame was written: time 0 of the file. Set by the writer.
    start: OnceLock<Instant>,
    /// `Some` when the recording takes the app's sound (`sound::SoundLog`).
    pub sound: Option<Mutex<SoundLog>>,
    closed: AtomicBool,
    /// Pictures put since the start, for tests and the log.
    pub(crate) puts: std::sync::atomic::AtomicU64,
}

#[derive(Default)]
struct Slot {
    fresh: Option<Vec<u8>>,
    spare: Vec<Vec<u8>>,
}

impl FrameSink {
    pub fn new(size: (u32, u32), app_sound: bool) -> FrameSink {
        FrameSink { size, slot: Mutex::default(), start: OnceLock::new(), sound: app_sound.then(Mutex::default), closed: AtomicBool::new(false), puts: Default::default() }
    }

    pub fn bytes(&self) -> usize { self.size.0 as usize * self.size.1 as usize * 4 }

    /// A buffer of the right size to fill (its old content is undefined).
    pub fn buffer(&self) -> Vec<u8> {
        let spare = self.slot.lock().unwrap_or_else(|e| e.into_inner()).spare.pop();
        let mut b = spare.unwrap_or_default();
        b.resize(self.bytes(), 0);
        b
    }

    /// Hands over a filled `buffer`: the newest picture.
    pub fn put(&self, buf: Vec<u8>) {
        if buf.len() != self.bytes() || self.is_closed() { return }
        let mut s = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = s.fresh.replace(buf) { s.spare.push(old) }
        s.spare.truncate(3);
        self.puts.fetch_add(1, Ordering::Relaxed);
    }

    /// Fits a picture of any size into the recording's (black bars around) and hands it over.
    pub fn put_raw(&self, raw: &Raw) {
        let mut b = self.buffer();
        if raw.w == self.size.0 && raw.h == self.size.1 && !raw.bottom_up && raw.rgba.len() == b.len() {
            b.copy_from_slice(raw.rgba);
        } else {
            fit(raw, &mut b, self.size.0, self.size.1);
        }
        self.put(b);
    }

    /// The writer takes the newest picture, if one came since it last looked.
    pub fn take(&self) -> Option<Vec<u8>> { self.slot.lock().unwrap_or_else(|e| e.into_inner()).fresh.take() }

    /// The writer gives back a picture it no longer needs.
    pub fn give_back(&self, buf: Vec<u8>) {
        let mut s = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if s.spare.len() < 3 { s.spare.push(buf) }
    }

    /// Time 0 of the file: the first call wins.
    pub fn started(&self) -> Instant { *self.start.get_or_init(Instant::now) }

    /// Seconds into the file, once it started.
    pub fn elapsed(&self) -> Option<f64> { self.start.get().map(|s| s.elapsed().as_secs_f64()) }

    /// The recording ended: the room stops reading pictures for it.
    pub fn close(&self) { self.closed.store(true, Ordering::Release) }

    pub fn is_closed(&self) -> bool { self.closed.load(Ordering::Acquire) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration { Duration::from_millis(n) }

    #[test]
    fn the_first_frame_is_due_at_once_then_one_every_thirtieth() {
        let mut p = Pump::new(30);
        assert_eq!(p.due(ms(0)), Due { write: 1, dropped: 0 });
        assert_eq!(p.due(ms(10)), Due { write: 0, dropped: 0 });
        assert_eq!(p.next_at(), Duration::from_secs_f64(1.0 / 30.0));
        assert_eq!(p.due(ms(34)), Due { write: 1, dropped: 0 });
        assert_eq!(p.due(ms(100)), Due { write: 2, dropped: 0 }, "frames 2 and 3");
        assert_eq!(p.sent(), 4);
    }

    #[test]
    fn a_late_writer_catches_up_with_repeats_so_the_time_stays_right() {
        let mut p = Pump::new(30);
        p.due(ms(0));
        assert_eq!(p.due(ms(1000)), Due { write: 30, dropped: 0 });
        assert_eq!(p.sent(), 31, "one second is 30 frames after frame 0");
    }

    #[test]
    fn how_far_behind_the_writer_is() {
        assert_eq!(Due { write: 1, dropped: 0 }.lag(30), Duration::ZERO, "one frame due is on time");
        assert_eq!(Due { write: 0, dropped: 0 }.lag(30), Duration::ZERO);
        assert_eq!(Due { write: 31, dropped: 0 }.lag(30), Duration::from_secs(1));
        assert_eq!(Due { write: 60, dropped: 90 }.lag(30), Duration::from_secs_f64(149.0 / 30.0));
    }

    #[test]
    fn a_writer_stuck_for_long_drops_what_is_more_than_two_seconds_late() {
        let mut p = Pump::new(30);
        p.due(ms(0));
        assert_eq!(p.due(ms(5000)), Due { write: 60, dropped: 90 });
        assert_eq!(p.due(ms(5034)), Due { write: 1, dropped: 0 });
    }

    #[test]
    fn ten_seconds_at_any_tick_rate_is_three_hundred_frames_after_the_first() {
        for step in [7, 33, 50, 120] {
            let mut p = Pump::new(30);
            let mut n = 0;
            let mut t = 0;
            while t < 10_000 { n += p.due(ms(t)).write; t += step }
            n += p.due(ms(10_000)).write;
            assert_eq!(n, 301, "frames 0 to 300, step {step}");
        }
    }

    #[test]
    fn fit_rect_keeps_the_shape_with_bars() {
        assert_eq!(fit_rect(1920, 1080, 1280, 720), (0, 0, 1280, 720));
        assert_eq!(fit_rect(1024, 768, 1280, 720), (160, 0, 960, 720), "4:3 gets bars left and right");
        assert_eq!(fit_rect(2560, 1080, 1920, 1080), (0, 135, 1920, 810), "ultra-wide gets bars top and bottom");
        assert_eq!(fit_rect(0, 0, 1280, 720), (0, 0, 1280, 720));
    }

    fn px(img: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        img[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn fit_scales_flips_and_bars() {
        // 2×2: top row red, green; bottom row blue, white. Given bottom row first, as OpenGL reads it.
        let (r, g, b, w) = ([255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255; 4]);
        let gl: Vec<u8> = [b, w, r, g].concat();
        let mut out = vec![0u8; 8 * 4 * 4];
        fit(&Raw { w: 2, h: 2, bottom_up: true, rgba: &gl }, &mut out, 8, 4);
        assert_eq!(px(&out, 8, 0, 0), [0, 0, 0, 255], "bar");
        assert_eq!(px(&out, 8, 2, 0), r);
        assert_eq!(px(&out, 8, 5, 0), g);
        assert_eq!(px(&out, 8, 2, 3), b);
        assert_eq!(px(&out, 8, 5, 3), w);
        assert_eq!(px(&out, 8, 7, 3), [0, 0, 0, 255], "bar");
    }

    #[test]
    fn fit_makes_every_pixel_opaque() {
        let src = vec![10u8, 20, 30, 0];
        let mut out = vec![0u8; 4 * 4];
        fit(&Raw { w: 1, h: 1, bottom_up: false, rgba: &src }, &mut out, 2, 2);
        assert!(out.chunks(4).all(|p| p == [10, 20, 30, 255]), "{out:?}");
    }

    #[test]
    fn the_sink_keeps_only_the_newest_picture_and_reuses_buffers() {
        let s = FrameSink::new((2, 1), false);
        assert_eq!(s.take(), None);
        let mut a = s.buffer();
        a.fill(1);
        s.put(a);
        let mut b = s.buffer();
        b.fill(2);
        s.put(b);
        assert_eq!(s.take(), Some(vec![2; 8]));
        assert_eq!(s.take(), None);
        s.put(vec![0; 3]);
        assert_eq!(s.take(), None, "a picture of the wrong size is refused");
        s.put_raw(&Raw { w: 2, h: 1, bottom_up: true, rgba: &[9, 9, 9, 9, 8, 8, 8, 8] });
        assert_eq!(s.take(), Some(vec![9, 9, 9, 255, 8, 8, 8, 255]));
        s.close();
        s.put(vec![0; 8]);
        assert_eq!(s.take(), None, "nothing after the end");
    }
}
