//! Sending the stream so that a lost connection comes back by itself. The recorder's FFmpeg does not talk to the
//! streaming services: beside the file it sends its encoded picture and sound to this computer itself, one local
//! feed (UDP on 127.0.0.1) for each destination. A small FFmpeg of its own for each destination (a "sender")
//! reads its feed and passes it on, copying, never encoding again. When a sender ends (the internet went, the
//! service closed the connection, a wrong key) it is started again: after 2 s, then 4, 8, 16 and every 30 s,
//! from the first again once it has held for half a minute. So the recording never waits for the internet, one
//! destination's trouble is no other's, and a stream comes back without anyone touching it.
use crate::platform::proc::spawn_tree;
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

/// The address of the local feed on `port`, as the recorder writes to it.
pub fn feed(port: u16) -> String { format!("udp://127.0.0.1:{port}?pkt_size=1316") }

/// `n` UDP ports on this computer that nobody uses now, one for each destination.
pub fn free_ports(n: usize) -> Option<Vec<u16>> {
    // All held at once, so no two are the same; freed together for FFmpeg to take.
    let held: Vec<std::net::UdpSocket> = (0..n).map(|_| std::net::UdpSocket::bind("127.0.0.1:0")).collect::<Result<_, _>>().ok()?;
    held.iter().map(|s| s.local_addr().ok().map(|a| a.port())).collect()
}

/// A sender's FFmpeg: the local feed in, the destination out, nothing encoded again.
pub fn sender_args(port: u16, address: &str) -> Vec<OsString> {
    // A feed that is read too slowly drops what is oldest and goes on (the recorder must never wait).
    let input = format!("udp://127.0.0.1:{port}?overrun_nonfatal=1&fifo_size=100000");
    // Three seconds of looking at the feed before sending, not FFmpeg's five: a stream that comes back is back
    // sooner. Not less: a sender that joins in the middle must see a keyframe (one every 2 s) to learn the
    // picture's header, without which nothing can be sent (tried with one second: it never connected again).
    let mut a: Vec<OsString> = ["-hide_banner", "-loglevel", "warning", "-fflags", "+genpts+discardcorrupt", "-analyzeduration", "3000000", "-probesize", "5000000",
        "-i", &input, "-c", "copy"].map(OsString::from).into();
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
/// `all` are every destination's address, so nothing that is logged holds a key.
pub async fn keep_sending(ffmpeg: PathBuf, port: u16, address: String, all: Vec<String>, index: usize, links: Links, cancel: CancellationToken) {
    let set = |up: bool| if let Some(l) = links.lock().unwrap_or_else(|e| e.into_inner()).get_mut(index) { l.up = up };
    let mut tries = 0;
    while !cancel.is_cancelled() {
        let began = Instant::now();
        let mut cmd = tokio::process::Command::new(&ffmpeg);
        cmd.args(sender_args(port, &address)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        match spawn_tree(cmd) {
            Ok(mut tree) => {
                if let Some(err) = tree.stderr() {
                    let (all, links) = (all.clone(), links.clone());
                    tokio::spawn(async move {
                        let mut lines = BufReader::new(err).lines();
                        while let Ok(Some(l)) = lines.next_line().await {
                            if skipped(&l) && let Some(link) = links.lock().unwrap_or_else(|e| e.into_inner()).get_mut(index) { link.skips += 1 }
                            eprintln!("ffmpeg (stream {}): {}", index + 1, super::redact(&l, &all))
                        }
                    });
                }
                // Still running after `UP_AFTER`: it is connected. Then until it ends, or the stream is stopped
                // (which kills it).
                let forever = Duration::from_secs(u64::MAX / 4);
                if tokio::time::timeout(UP_AFTER, tree.wait(forever, &cancel)).await.is_err() {
                    set(true);
                    let _ = tree.wait(forever, &cancel).await;
                }
            }
            Err(e) => eprintln!("stream {}: the sender could not start: {e}", index + 1),
        }
        set(false);
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

    #[test]
    fn a_sender_copies_its_local_feed_to_the_destination() {
        let s = |a: Vec<OsString>| a.into_iter().map(|x| x.to_string_lossy().into_owned()).collect::<Vec<_>>();
        let a = s(sender_args(40123, "rtmp://a.rtmp.youtube.com/live2/key"));
        assert!(a.windows(2).any(|w| w == ["-i", "udp://127.0.0.1:40123?overrun_nonfatal=1&fifo_size=100000"]), "{a:?}");
        assert!(a.windows(2).any(|w| w == ["-c", "copy"]) && a.windows(2).any(|w| w == ["-f", "flv"]) && a.windows(2).any(|w| w == ["-bsf:a", "aac_adtstoasc"]));
        assert_eq!(a.last().unwrap(), "rtmp://a.rtmp.youtube.com/live2/key");
        let a = s(sender_args(40123, "srt://h.example.org:9000?streamid=x"));
        assert!(a.windows(2).any(|w| w == ["-f", "mpegts"]) && !a.contains(&"aac_adtstoasc".to_string()));
        assert_eq!(feed(40123), "udp://127.0.0.1:40123?pkt_size=1316");
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
