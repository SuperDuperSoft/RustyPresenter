//! Stream delay: what goes out to the destinations is held back a while (10 s, 30 s, a minute), so something that
//! should not go out (a wrong slide, a word said near a microphone, a private moment) can be stopped with Hold or
//! Mute before anyone watching sees it. The recording is not held back.
//!
//! The recorder writes each destination's local feed (`send::feed`) as it always does; with a delay, a relay here
//! reads that feed, keeps each packet for the delay and passes it on to a second local port, which the sender
//! reads (`send::keep_sending`). The packets go out as far apart as they came, so the sender sees the same stream,
//! only later. What is held is limited (`MAX_HELD`): past that the oldest is dropped, never the app's memory.
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// The delays Settings offers, in seconds, after None.
pub const DELAYS: [u32; 3] = [10, 30, 60];

/// The most a relay holds (a minute of the largest bitrate Settings offers, with room to spare).
pub const MAX_HELD: usize = 160 * 1024 * 1024;

/// A delay from Settings: one that Settings offers, else none.
pub fn checked(secs: u32) -> u32 { if DELAYS.contains(&secs) { secs } else { 0 } }

/// Packets held until their time to go.
pub struct Queue {
    delay: Duration,
    cap: usize,
    held: VecDeque<(Instant, Vec<u8>)>,
    bytes: usize,
    /// Packets dropped because too much was held.
    pub dropped: u64,
}

impl Queue {
    pub fn new(delay: Duration, cap: usize) -> Queue { Queue { delay, cap, held: VecDeque::new(), bytes: 0, dropped: 0 } }

    /// A packet that came `at`: it goes at `at + delay`.
    pub fn push(&mut self, at: Instant, packet: Vec<u8>) {
        self.bytes += packet.len();
        self.held.push_back((at + self.delay, packet));
        while self.bytes > self.cap && let Some((_, old)) = self.held.pop_front() {
            self.bytes -= old.len();
            self.dropped += 1;
        }
    }

    /// The packets whose time has come by `now`, oldest first.
    pub fn due(&mut self, now: Instant) -> Vec<Vec<u8>> {
        let mut out = vec![];
        while self.held.front().is_some_and(|(t, _)| *t <= now) {
            let (_, p) = self.held.pop_front().expect("there is a front");
            self.bytes -= p.len();
            out.push(p);
        }
        out
    }

    /// When the next packet goes; `None`: nothing is held.
    pub fn next_at(&self) -> Option<Instant> { self.held.front().map(|(t, _)| *t) }

    pub fn len(&self) -> usize { self.held.len() }

    pub fn is_empty(&self) -> bool { self.held.is_empty() }

    pub fn bytes(&self) -> usize { self.bytes }
}

/// Takes the local feed on port `from` now (before the recorder starts writing to it), so nothing is lost.
pub async fn listen(from: u16) -> std::io::Result<tokio::net::UdpSocket> { tokio::net::UdpSocket::bind(("127.0.0.1", from)).await }

/// Passes what comes in on `socket` to port `to` on this computer, `delay` later, until `cancel`.
pub async fn relay(socket: tokio::net::UdpSocket, to: u16, delay: Duration, cancel: CancellationToken) {
    let out = match tokio::net::UdpSocket::bind(("127.0.0.1", 0)).await {
        Ok(s) => s,
        Err(e) => return eprintln!("stream delay: cannot send on: {e}"),
    };
    let target = std::net::SocketAddr::from(([127, 0, 0, 1], to));
    let mut queue = Queue::new(delay, MAX_HELD);
    let mut buf = vec![0u8; 65536];
    let mut told = 0;
    loop {
        let wait = queue.next_at().map(|t| t.saturating_duration_since(Instant::now())).unwrap_or(Duration::from_secs(3600));
        tokio::select! {
            _ = cancel.cancelled() => break,
            r = socket.recv(&mut buf) => match r {
                Ok(n) => queue.push(Instant::now(), buf[..n].to_vec()),
                // Windows says a packet sent earlier found nobody listening; nothing to do with this feed.
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {}
                Err(e) => { eprintln!("stream delay: the feed ended: {e}"); break }
            },
            _ = tokio::time::sleep(wait) => {}
        }
        for p in queue.due(Instant::now()) {
            // A sender that is starting again is not listening yet: what it misses is gone, as without a delay.
            let _ = out.send_to(&p, target).await;
        }
        if queue.dropped > told {
            told = queue.dropped;
            eprintln!("stream delay: too much held; {told} packets dropped so far");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_packet_goes_after_the_delay_and_in_order() {
        let t0 = Instant::now();
        let mut q = Queue::new(Duration::from_secs(10), 1000);
        q.push(t0, vec![1]);
        q.push(t0 + Duration::from_millis(5), vec![2, 2]);
        assert!(q.due(t0 + Duration::from_secs(9)).is_empty());
        assert_eq!(q.next_at(), Some(t0 + Duration::from_secs(10)));
        assert_eq!(q.due(t0 + Duration::from_secs(10)), vec![vec![1]]);
        assert_eq!((q.len(), q.bytes()), (1, 2));
        assert_eq!(q.due(t0 + Duration::from_secs(11)), vec![vec![2, 2]]);
        assert!(q.is_empty() && q.next_at().is_none() && q.bytes() == 0);
    }

    #[test]
    fn too_much_held_drops_the_oldest() {
        let t0 = Instant::now();
        let mut q = Queue::new(Duration::from_secs(60), 10);
        for i in 0..5u8 { q.push(t0, vec![i; 4]) }
        assert_eq!((q.len(), q.bytes(), q.dropped), (2, 8, 3));
        assert_eq!(q.due(t0 + Duration::from_secs(60)), vec![vec![3; 4], vec![4; 4]]);
    }

    #[test]
    fn only_the_delays_settings_offers() {
        assert_eq!([0, 10, 30, 60, 5, 600].map(checked), [0, 10, 30, 60, 0, 0]);
    }

    #[tokio::test]
    async fn the_relay_passes_the_feed_on_later() {
        let ports = crate::stream::send::free_ports(2).unwrap();
        let socket = listen(ports[0]).await.unwrap();
        let sink = tokio::net::UdpSocket::bind(("127.0.0.1", ports[1])).await.unwrap();
        let cancel = CancellationToken::new();
        let run = tokio::spawn(relay(socket, ports[1], Duration::from_millis(300), cancel.clone()));
        let writer = tokio::net::UdpSocket::bind(("127.0.0.1", 0)).await.unwrap();
        let sent = Instant::now();
        for i in 0..3u8 { writer.send_to(&[i; 1316], ("127.0.0.1", ports[0])).await.unwrap(); }
        let mut buf = [0u8; 2000];
        let mut got = vec![];
        for _ in 0..3 {
            let n = tokio::time::timeout(Duration::from_secs(5), sink.recv(&mut buf)).await.expect("it came").unwrap();
            got.push((n, buf[0]));
        }
        assert!(sent.elapsed() >= Duration::from_millis(290), "held back: {:?}", sent.elapsed());
        assert_eq!(got, [(1316, 0), (1316, 1), (1316, 2)]);
        cancel.cancel();
        run.await.unwrap();
    }
}
