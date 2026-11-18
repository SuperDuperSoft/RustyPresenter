//! Ports on this computer for the app's own local feeds (the stream's feeds, the sound input, the PC's sound).
//!
//! A port the system hands out by itself (binding port 0) is one it hands out again to the next program that asks:
//! freed for FFmpeg to take, it can be gone before FFmpeg takes it (FFmpeg's own sending sockets get such ports).
//! So these come from below every system's range for that (Windows and macOS from 49152, Linux from 32768), and
//! one after the other within the app, so that two feeds started together never get the same one.

const FIRST: u16 = 20000;
const LAST: u16 = 32000;

use std::sync::atomic::{AtomicU32, Ordering};

/// Where the next look starts: from a place picked by this program's process id, so two copies (or two test
/// programs) running at once start apart, then onwards.
static NEXT: AtomicU32 = AtomicU32::new(u32::MAX);

/// The next port to try, going round `FIRST..LAST`.
fn next() -> u16 {
    let span = u32::from(LAST - FIRST);
    let _ = NEXT.compare_exchange(u32::MAX, std::process::id().wrapping_mul(7919) % span, Ordering::Relaxed, Ordering::Relaxed);
    FIRST + (NEXT.fetch_add(1, Ordering::Relaxed) % span) as u16
}

/// The first port on from `next` that `free` says is free, trying at most the whole range once.
fn find<T>(free: impl Fn(u16) -> Option<T>) -> Option<(u16, T)> {
    (FIRST..LAST).find_map(|_| { let p = next(); free(p).map(|t| (p, t)) })
}

/// `n` UDP ports nobody uses now, all different. `None` when there are not that many.
pub fn udp(n: usize) -> Option<Vec<u16>> {
    // All held until every one is found; then freed together for FFmpeg to take.
    let held: Vec<(u16, std::net::UdpSocket)> = (0..n).map(|_| find(|p| std::net::UdpSocket::bind(("127.0.0.1", p)).ok())).collect::<Option<_>>()?;
    Some(held.into_iter().map(|(p, _)| p).collect())
}

/// A TCP port nobody listens on now.
pub fn tcp() -> std::io::Result<u16> {
    find(|p| std::net::TcpListener::bind(("127.0.0.1", p)).ok()).map(|(p, _)| p)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::AddrInUse, "no free port for a local feed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_ports_are_ones_the_system_does_not_hand_out_and_never_repeat() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..50 {
            for p in udp(3).unwrap() {
                assert!((FIRST..LAST).contains(&p), "{p}");
                assert!(seen.insert(p), "{p} twice");
            }
            let p = tcp().unwrap();
            assert!((FIRST..LAST).contains(&p), "{p}");
            assert!(seen.insert(p), "{p} twice");
        }
        assert_eq!(udp(0), Some(vec![]));
    }

    #[test]
    fn a_port_in_use_is_passed_over() {
        // A port a few ahead of the next one handed out, held here for both kinds (never moving the place back:
        // other tests take ports at the same time, and none may get one twice).
        let _ = next();
        let span = u32::from(LAST - FIRST);
        let (held, _udp, _tcp) = (3..60).find_map(|ahead| {
            let p = FIRST + ((NEXT.load(Ordering::Relaxed) + ahead) % span) as u16;
            Some((p, std::net::UdpSocket::bind(("127.0.0.1", p)).ok()?, std::net::TcpListener::bind(("127.0.0.1", p)).ok()?))
        }).unwrap();
        // The looks go on past it.
        for _ in 0..70 {
            assert_ne!(udp(1).unwrap()[0], held);
            assert_ne!(tcp().unwrap(), held);
        }
    }
}
