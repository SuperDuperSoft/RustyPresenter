//! The upload speed test (Settings → Livestream → "Test the upload speed"): a few megabytes of zeros sent to a
//! public speed-test address and timed, then whether the internet can carry the stream to every destination that
//! is on, and at which size. Nothing about the church is sent: only zeros.
use crate::record::Preset;
use crate::tr;
use std::time::{Duration, Instant};

/// Where the zeros go: Cloudflare's speed test takes uploads and throws them away.
pub const URL: &str = "https://speed.cloudflare.com/__up";
/// How much is sent: long enough to measure, short enough on a slow line (about 6 s at 5 Mbit/s).
pub const BYTES: usize = 4_000_000;

/// The upload speed in Mbit/s: `bytes` of zeros posted to `url`, timed.
pub async fn measure(client: &reqwest::Client, url: &str, bytes: usize) -> Result<f64, String> {
    let began = Instant::now();
    let r = client.post(url).header("content-type", "application/octet-stream").body(vec![0u8; bytes]).timeout(LIMIT).send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() { return Err(r.status().to_string()) }
    let secs = began.elapsed().as_secs_f64().max(0.001);
    Ok(bytes as f64 * 8.0 / secs / 1_000_000.0)
}

/// What the stream needs at `preset` for `destinations`, in Mbit/s: the picture's highest rate and the sound for
/// each, with half again to spare (a church's internet is shared and never steady).
pub fn needed(preset: Preset, destinations: usize) -> f64 {
    f64::from(preset.max_kbps() + 192) * destinations.max(1) as f64 * 1.5 / 1000.0
}

/// The size to stream at with `mbps` of upload to `destinations` (`None`: too slow even for 720p), and what to tell
/// the operator. The size chosen now (`chosen`) is kept when it fits; otherwise the biggest named size under it that
/// fits is advised.
pub fn advice(mbps: f64, destinations: usize, chosen: Preset) -> (Option<Preset>, String) {
    let speed = format!("{mbps:.1}");
    let need = |p: Preset| format!("{:.0}", needed(p, destinations).ceil());
    if mbps >= needed(chosen, destinations) {
        return (Some(chosen), tr!("Upload: {speed} Mbit/s. Enough to stream in {size}.", speed = speed, size = chosen.label()))
    }
    let (_, h) = chosen.size();
    let smaller = Preset::ALL.into_iter().rev().filter(|p| p.size().1 < h && p.size().1 >= 720).find(|p| mbps >= needed(*p, destinations));
    match smaller {
        Some(p) => (Some(p), tr!("Upload: {speed} Mbit/s. Stream in {size}: {chosen} needs {need} Mbit/s.", speed = speed, size = p.label(), chosen = chosen.label(), need = need(chosen))),
        None => (None, tr!("Upload: {speed} Mbit/s. That is too slow: 720p to every destination that is on needs {need} Mbit/s. Stream to fewer destinations, or use a faster connection.",
            speed = speed, need = need(Preset::P720))),
    }
}

/// How long the test may take before it is given up.
pub const LIMIT: Duration = Duration::from_secs(60);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_stream_needs_grows_with_the_destinations() {
        assert!((needed(Preset::P720, 1) - 4.788).abs() < 0.01, "{}", needed(Preset::P720, 1));
        assert!((needed(Preset::P1080, 2) - 18.576).abs() < 0.01, "{}", needed(Preset::P1080, 2));
    }

    #[test]
    fn the_advice_picks_the_biggest_size_that_fits() {
        let p1080 = Preset::P1080;
        assert_eq!(advice(25.0, 1, p1080).0, Some(Preset::P1080));
        assert_eq!(advice(6.0, 1, p1080).0, Some(Preset::P720));
        assert_eq!(advice(6.0, 2, p1080).0, None, "two destinations need twice as much");
        let (none, text) = advice(2.0, 1, p1080);
        assert_eq!(none, None);
        assert!(text.contains("2.0 Mbit/s") && text.contains("too slow"), "{text}");
        assert!(advice(25.0, 1, p1080).1.contains("1080p"), "{}", advice(25.0, 1, p1080).1);
        // 4K chosen: kept with a fast line, else the biggest size under it that fits.
        assert_eq!(advice(60.0, 1, Preset::P2160).0, Some(Preset::P2160));
        let (p, text) = advice(16.0, 1, Preset::P2160);
        assert_eq!(p, Some(Preset::P1440));
        assert!(text.contains("4K needs"), "{text}");
        // A small typed size that fits is kept.
        assert_eq!(advice(3.0, 1, Preset::P480).0, Some(Preset::P480));
    }

    #[test]
    fn the_speed_is_the_bytes_over_the_time() {
        // A stand-in server on this computer that reads the whole upload, then answers.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/up", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            use std::io::{BufRead, Read, Write};
            let (s, _) = listener.accept().unwrap();
            let mut r = std::io::BufReader::new(s.try_clone().unwrap());
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") { len = v.trim().parse().unwrap() }
                if line == "\r\n" { break }
            }
            let mut body = vec![0u8; len];
            r.read_exact(&mut body).unwrap();
            let mut s = s;
            s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").unwrap();
        });
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let mbps = rt.block_on(measure(crate::bible::sources::client().unwrap(), &url, 1_000_000)).unwrap();
        assert!(mbps > 1.0, "a megabyte on this computer goes fast: {mbps}");
    }
}
