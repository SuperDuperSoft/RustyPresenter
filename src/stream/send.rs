//! Sending the stream so that a lost connection comes back by itself. The recorder's FFmpeg does not talk to the
//! streaming services: beside the file it sends its encoded picture and sound to this computer itself, one local
//! feed (UDP on 127.0.0.1) for each destination. A small FFmpeg of its own for each destination (a "sender")
//! reads its feed and passes it on, copying, never encoding again. When a sender ends (the internet went, the
//! service closed the connection, a wrong key) it is started again: after 2 s, then 4, 8, 16 and every 30 s,
//! from the first again once it has held for half a minute. So the recording never waits for the internet, one
//! destination's trouble is no other's, and a stream comes back without anyone touching it.
//!
//! A destination whose internet cannot keep up gets a lighter stream (`Ladder`): its sender encodes the picture
//! again at a lower bitrate, and goes back to copying after a long time without trouble.
use crate::platform::proc::spawn_tree;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

/// One destination's connection as its sender sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Link {
    /// Connected and sending (its sender has held for `UP_AFTER`).
    pub up: bool,
    /// How often the sender fell behind and parts of the stream were skipped (`skipped`): the internet cannot keep
    /// up with this destination.
    pub skips: u32,
    /// Tries in a row that ended without connecting (a wrong key, a service refusing, no internet).
    pub failed: u32,
    /// The lighter stream this destination gets because its internet could not keep up (kbit/s, `Ladder`); 0:
    /// the full stream.
    pub kbps: u32,
    /// Already as light as it goes (`Ladder`), or never made lighter (`Lighter` off): skipping now is an internet
    /// too slow for this stream.
    pub lightest: bool,
}

/// Each destination's connection, in the order they were given.
pub type Links = Arc<Mutex<Vec<Link>>>;

/// A sender's line saying it fell behind its local feed and part of the stream was dropped: what an upload too
/// slow for the stream looks like from here.
pub fn skipped(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("circular buffer overrun") || l.contains("buffer overrun")
}

/// A sender that has run this long is connected and sending.
const UP_AFTER: Duration = Duration::from_secs(4);
/// A sender that held this long starts its waits from the shortest again.
const HELD: Duration = Duration::from_secs(30);

/// Making one destination's stream lighter when its internet cannot keep up, and full again later. Only that
/// destination is touched: the recording and the others keep the full picture. A lighter stream is encoded again
/// by its sender at the lower bitrate (`sender_args`); each change starts the sender again (a moment's gap for that
/// destination), so it steps down soon (a few skips close together) and back up only after a long time without
/// any.
#[derive(Clone, Debug)]
pub struct Ladder {
    /// The full stream's bitrate (kbit/s).
    full: u32,
    /// 0: the full stream; then each of `STEPS`.
    pub step: usize,
    skips: VecDeque<Instant>,
    changed: Instant,
}

/// The lighter streams, as parts of the full bitrate.
pub const STEPS: [f32; 3] = [0.7, 0.5, 0.35];
/// Never lighter than this (kbit/s): below it a picture is not worth sending.
pub const FLOOR_KBPS: u32 = 800;
/// This many skips within `SKIP_WINDOW`: one step lighter.
pub const SKIPS_TO_STEP: usize = 3;
pub const SKIP_WINDOW: Duration = Duration::from_secs(30);
/// Without a skip this long, one step back up.
pub const CLEAN_TO_RISE: Duration = Duration::from_secs(5 * 60);

impl Ladder {
    pub fn new(full_kbps: u32, now: Instant) -> Ladder { Ladder { full: full_kbps, step: 0, skips: VecDeque::new(), changed: now } }

    /// The bitrate of `step` (the full stream is given as 0).
    pub fn kbps_at(&self, step: usize) -> u32 {
        if step == 0 { return 0 }
        ((self.full as f32 * STEPS[(step - 1).min(STEPS.len() - 1)]) as u32).max(FLOOR_KBPS).min(self.full)
    }

    pub fn kbps(&self) -> u32 { self.kbps_at(self.step) }

    /// No lighter step is left (the last one, or one already at the floor).
    pub fn lightest(&self) -> bool {
        self.step >= STEPS.len() || (self.step > 0 && self.kbps_at(self.step + 1) >= self.kbps()) || self.kbps_at(1) >= self.full
    }

    /// A skip at `now`: `true` when the stream should now be one step lighter.
    pub fn skipped(&mut self, now: Instant) -> bool {
        self.skips.push_back(now);
        while self.skips.front().is_some_and(|t| now.duration_since(*t) > SKIP_WINDOW) { self.skips.pop_front(); }
        if self.skips.len() >= SKIPS_TO_STEP && !self.lightest() {
            self.step += 1;
            self.skips.clear();
            self.changed = now;
            return true;
        }
        false
    }

    /// `true` when, at `now`, the stream should go one step back up.
    pub fn rise(&mut self, now: Instant) -> bool {
        let last = self.skips.back().copied().unwrap_or(self.changed).max(self.changed);
        if self.step > 0 && now.duration_since(last) >= CLEAN_TO_RISE {
            self.step -= 1;
            self.skips.clear();
            self.changed = now;
            return true;
        }
        false
    }
}

/// How a lighter stream is encoded again: the recording's encoder, the full stream's bitrate, and its pictures a
/// second.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lighter {
    pub encoder: &'static str,
    pub full_kbps: u32,
    pub fps: u32,
}

/// The address of the local feed on `port`, as the recorder writes to it.
pub fn feed(port: u16) -> String { format!("udp://127.0.0.1:{port}?pkt_size=1316") }

/// `n` UDP ports on this computer that nobody uses now, one for each destination.
pub fn free_ports(n: usize) -> Option<Vec<u16>> { crate::platform::ports::udp(n) }

/// A sender's FFmpeg: the local feed in, the destination out, nothing encoded again; or, for a lighter stream
/// (`light` with a bitrate above 0: encoder, kbit/s, pictures a second), the picture encoded again at that
/// bitrate (the sound is still copied).
pub fn sender_args(port: u16, address: &str, light: Option<(&str, u32, u32)>) -> Vec<OsString> {
    // A feed that is read too slowly drops what is oldest and goes on (the recorder must never wait).
    let input = format!("udp://127.0.0.1:{port}?overrun_nonfatal=1&fifo_size=100000");
    // Three seconds of looking at the feed before sending, not FFmpeg's five: a stream that comes back is back
    // sooner. Not less: a sender that joins in the middle must see a keyframe (one every 2 s) to learn the
    // picture's header, without which nothing can be sent (tried with one second: it never connected again).
    let mut a: Vec<OsString> = ["-hide_banner", "-loglevel", "warning", "-fflags", "+genpts+discardcorrupt", "-analyzeduration", "3000000", "-probesize", "5000000",
        "-i", &input].map(OsString::from).into();
    match light {
        Some((encoder, kbps, fps)) if kbps > 0 => {
            let (rate, buf, gop) = (format!("{kbps}k"), format!("{}k", kbps * 2), (fps.max(1) * 2).to_string());
            a.extend(["-c:a", "copy", "-c:v", encoder].map(OsString::from));
            if encoder == "libx264" { a.extend(["-preset", "veryfast"].map(OsString::from)) }
            a.extend(["-b:v", &rate, "-maxrate", &rate, "-bufsize", &buf, "-g", &gop, "-pix_fmt", "yuv420p"].map(OsString::from));
        }
        _ => a.extend(["-c", "copy"].map(OsString::from)),
    }
    if address.to_ascii_lowercase().starts_with("srt://") {
        a.extend(["-f", "mpegts"].map(OsString::from));
    } else {
        // RTMP carries FLV, which wants the sound's header apart from its packets.
        a.extend(["-bsf:a", "aac_adtstoasc", "-f", "flv"].map(OsString::from));
    }
    a.push(address.into());
    a
}

/// How long to wait before the `tries`th try in a row (from 1): 2 s, 4, 8, 16, then 30.
pub fn retry_after(tries: u32) -> Duration { Duration::from_secs(match tries { 0 | 1 => 2, 2 => 4, 3 => 8, 4 => 16, _ => 30 }) }

/// Keeps destination `index` sent to until `cancel`: runs its sender, and starts it again whenever it ends.
/// `all` are every destination's address, so nothing that is logged holds a key. With `lighter`, a destination
/// whose internet cannot keep up gets a lighter stream (`Ladder`).
#[allow(clippy::too_many_arguments)]
pub async fn keep_sending(ffmpeg: PathBuf, port: u16, address: String, all: Vec<String>, index: usize, links: Links, lighter: Option<Lighter>, cancel: CancellationToken) {
    let set = |up: bool| if let Some(l) = links.lock().unwrap_or_else(|e| e.into_inner()).get_mut(index) { l.up = up };
    let ladder = lighter.as_ref().map(|l| Arc::new(Mutex::new(Ladder::new(l.full_kbps, Instant::now()))));
    let lightest = ladder.as_ref().is_none_or(|l| l.lock().unwrap_or_else(|e| e.into_inner()).lightest());
    if let Some(link) = links.lock().unwrap_or_else(|e| e.into_inner()).get_mut(index) { link.lightest = lightest }
    let mut tries = 0;
    while !cancel.is_cancelled() {
        let began = Instant::now();
        let mut connected = false;
        // This run of the sender: ended early by `cancel`, or to change how light the stream is.
        let run = cancel.child_token();
        let kbps = ladder.as_ref().map_or(0, |l| l.lock().unwrap_or_else(|e| e.into_inner()).kbps());
        let light = lighter.as_ref().map(|l| (l.encoder, kbps, l.fps));
        let mut cmd = tokio::process::Command::new(&ffmpeg);
        cmd.args(sender_args(port, &address, light)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        match spawn_tree(cmd) {
            Ok(mut tree) => {
                if let Some(err) = tree.stderr() {
                    let (all, links, ladder, run) = (all.clone(), links.clone(), ladder.clone(), run.clone());
                    tokio::spawn(async move {
                        let mut lines = BufReader::new(err).lines();
                        while let Ok(Some(l)) = lines.next_line().await {
                            if skipped(&l) {
                                if let Some(link) = links.lock().unwrap_or_else(|e| e.into_inner()).get_mut(index) { link.skips += 1 }
                                if let Some(ladder) = &ladder && ladder.lock().unwrap_or_else(|e| e.into_inner()).skipped(Instant::now()) { run.cancel() }
                            }
                            eprintln!("ffmpeg (stream {}): {}", index + 1, super::redact(&l, &all))
                        }
                    });
                }
                // A lighter stream goes back up after a long time without skips.
                if let Some(ladder) = ladder.clone() && kbps > 0 {
                    let run = run.clone();
                    tokio::spawn(async move {
                        loop {
                            tokio::select! {
                                _ = run.cancelled() => break,
                                _ = tokio::time::sleep(Duration::from_secs(15)) => {}
                            }
                            if ladder.lock().unwrap_or_else(|e| e.into_inner()).rise(Instant::now()) { run.cancel(); break }
                        }
                    });
                }
                // Still running after `UP_AFTER`: it is connected. Then until it ends, or the stream is stopped
                // (which kills it).
                let forever = Duration::from_secs(u64::MAX / 4);
                if tokio::time::timeout(UP_AFTER, tree.wait(forever, &run)).await.is_err() {
                    connected = true;
                    set(true);
                    let _ = tree.wait(forever, &run).await;
                }
            }
            Err(e) => eprintln!("stream {}: the sender could not start: {e}", index + 1),
        }
        // Ended to change how light the stream is: started again at once, not counted as a lost connection (and
        // still shown as connected: if the new one does not connect, it says so when it ends).
        if run.is_cancelled() && !cancel.is_cancelled() && let Some(ladder) = &ladder {
            let (kbps, lightest) = { let l = ladder.lock().unwrap_or_else(|e| e.into_inner()); (l.kbps(), l.lightest()) };
            if let Some(link) = links.lock().unwrap_or_else(|e| e.into_inner()).get_mut(index) { link.kbps = kbps; link.lightest = lightest }
            if kbps == 0 { eprintln!("stream {}: the full stream again", index + 1) } else { eprintln!("stream {}: a lighter stream, {kbps} kbit/s", index + 1) }
            tries = 0;
            continue;
        }
        set(false);
        if cancel.is_cancelled() && connected { break }
        if let Some(l) = links.lock().unwrap_or_else(|e| e.into_inner()).get_mut(index) { l.failed = if connected { 0 } else { l.failed + 1 } }
        if cancel.is_cancelled() { break }
        tries = if began.elapsed() >= HELD { 1 } else { tries + 1 };
        eprintln!("stream {}: the connection ended; trying again in {} s", index + 1, retry_after(tries).as_secs());
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(retry_after(tries)) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(a: Vec<OsString>) -> Vec<String> { a.into_iter().map(|x| x.to_string_lossy().into_owned()).collect() }

    #[test]
    fn a_sender_copies_its_local_feed_to_the_destination() {
        let a = s(sender_args(40123, "rtmp://a.rtmp.youtube.com/live2/key", None));
        assert!(a.windows(2).any(|w| w == ["-i", "udp://127.0.0.1:40123?overrun_nonfatal=1&fifo_size=100000"]), "{a:?}");
        assert!(a.windows(2).any(|w| w == ["-c", "copy"]) && a.windows(2).any(|w| w == ["-f", "flv"]) && a.windows(2).any(|w| w == ["-bsf:a", "aac_adtstoasc"]));
        assert_eq!(a.last().unwrap(), "rtmp://a.rtmp.youtube.com/live2/key");
        // A ladder at the full stream copies too.
        let a = s(sender_args(40123, "srt://h.example.org:9000?streamid=x", Some(("libx264", 0, 30))));
        assert!(a.windows(2).any(|w| w == ["-f", "mpegts"]) && !a.contains(&"aac_adtstoasc".to_string()));
        assert!(a.windows(2).any(|w| w == ["-c", "copy"]), "{a:?}");
        assert_eq!(feed(40123), "udp://127.0.0.1:40123?pkt_size=1316");
    }

    #[test]
    fn a_lighter_stream_encodes_the_picture_again_and_copies_the_sound() {
        let a = s(sender_args(40123, "rtmp://live.twitch.tv/app/k", Some(("libx264", 2100, 30))));
        assert!(!a.windows(2).any(|w| w == ["-c", "copy"]), "{a:?}");
        for pair in [["-c:a", "copy"], ["-c:v", "libx264"], ["-b:v", "2100k"], ["-maxrate", "2100k"], ["-bufsize", "4200k"], ["-g", "60"], ["-f", "flv"]] {
            assert!(a.windows(2).any(|w| w == pair), "{pair:?} in {a:?}");
        }
        let a = s(sender_args(40123, "rtmp://live.twitch.tv/app/k", Some(("h264_nvenc", 1500, 30))));
        assert!(a.windows(2).any(|w| w == ["-c:v", "h264_nvenc"]) && !a.contains(&"veryfast".to_string()));
    }

    #[test]
    fn the_ladder_steps_down_soon_and_up_slowly() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut l = Ladder::new(6000, t0);
        assert_eq!((l.kbps(), l.lightest()), (0, false));
        // Two skips are a hiccup; a third within half a minute is an internet too slow.
        assert!(!l.skipped(at(1)) && !l.skipped(at(2)));
        assert!(l.skipped(at(3)));
        assert_eq!(l.kbps(), 4200);
        // Skips far apart do not add up.
        assert!(!l.skipped(at(10)) && !l.skipped(at(50)) && !l.skipped(at(90)));
        assert!(!l.rise(at(300)), "not yet five minutes clean");
        assert!(l.rise(at(90 + 300)));
        assert_eq!(l.kbps(), 0);
        assert!(!l.rise(at(2000)), "the full stream has nowhere to rise to");
        // All the way down, then no further.
        for s in 0..9 { l.skipped(at(3000 + s)); }
        assert_eq!((l.step, l.kbps(), l.lightest()), (3, 2100, true));
        assert!(!l.skipped(at(3010)) && !l.skipped(at(3011)) && !l.skipped(at(3012)));
        // A slow stream stops at the floor.
        let mut low = Ladder::new(1500, t0);
        for s in 0..3 { low.skipped(at(s)); }
        assert_eq!(low.kbps(), 1050);
        for s in 3..6 { low.skipped(at(s)); }
        assert_eq!((low.kbps(), low.lightest()), (800, true));
        // One already at the floor has nowhere to go.
        assert!(Ladder::new(800, t0).lightest());
    }

    #[tokio::test]
    async fn a_sender_that_cannot_connect_counts_its_tries() {
        let links: Links = Arc::new(Mutex::new(vec![Link::default()]));
        let cancel = CancellationToken::new();
        let run = tokio::spawn(keep_sending(PathBuf::from("no-such-ffmpeg-here"), 40999, "rtmp://127.0.0.1:1/live/k".into(), vec![], 0, links.clone(), None, cancel.clone()));
        tokio::time::sleep(Duration::from_millis(500)).await;
        cancel.cancel();
        let _ = run.await;
        assert_eq!(links.lock().unwrap()[0], Link { up: false, skips: 0, failed: 1, kbps: 0, lightest: true });
    }

    #[test]
    fn a_sender_falling_behind_is_seen_in_what_it_says() {
        assert!(skipped("[udp @ 0000020c] Circular buffer overrun. Surrounding packets may be lost."));
        assert!(!skipped("[flv @ 0x1] Failed to update header with correct duration."));
        assert!(!skipped("frame=  100 fps= 30"));
    }

    #[test]
    fn waits_grow_and_ports_differ() {
        assert_eq!([1, 2, 3, 4, 5, 50].map(|t| retry_after(t).as_secs()), [2, 4, 8, 16, 30, 30]);
        let ports = free_ports(4).unwrap();
        assert_eq!(ports.len(), 4);
        assert!(ports.iter().all(|p| *p > 0) && (0..4).all(|i| (i + 1..4).all(|j| ports[i] != ports[j])), "{ports:?}");
        assert_eq!(free_ports(0), Some(vec![]));
    }
}
