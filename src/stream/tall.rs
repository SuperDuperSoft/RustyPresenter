//! The tall (phone) picture put together by the app (design `docs/superpowers/specs/2026-11-17-scene-extras-design.md`).
//! Without the church's tall scenes FFmpeg makes the tall picture itself: the wide one fitted across
//! (`record::args::record`). When the church has a scene with tall places of its own and a tall destination, the
//! writer puts the tall picture together as well and FFmpeg reads it as raw RGBA from a port on this computer: its own
//! task writes the newest tall picture at the stream's frame rate, so FFmpeg starting up never waits on the writer.
use crate::record::pump::Pump;
use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

/// How long FFmpeg may take to connect for the tall picture.
const CONNECT_LIMIT: Duration = Duration::from_secs(30);

/// Whether the app puts the tall picture together: some destination is tall (`tall`) and one of the church's
/// scenes has tall places of its own.
pub fn wanted(own: &[super::own_scenes::OwnScene], tall: &[bool]) -> bool { tall.iter().any(|t| *t) && own.iter().any(|s| s.tall) }

/// Changes the recording's FFmpeg arguments (`record::args::record`) to read the tall picture of `size` at `fps` from
/// `port` instead of fitting the wide one: the input is added after the others, and the tall graph takes it. False
/// when the arguments have no tall picture.
pub fn read_from(a: &mut Vec<OsString>, port: u16, (w, h): (u32, u32), fps: u32) -> bool {
    let graph = a.iter().position(|x| x.to_str().is_some_and(|s| s.starts_with("[0:v]scale=") && s.ends_with("[tall]")));
    let Some(graph) = graph else { return false };
    let first = a.iter().position(|x| x == "-filter_complex" || x == "-map").unwrap_or(a.len());
    let n = a[..first].iter().filter(|x| *x == "-i").count();
    a[graph] = format!("[{n}:v]setsar=1[tall]").into();
    let input = ["-f".to_string(), "rawvideo".into(), "-pix_fmt".into(), "rgba".into(), "-video_size".into(), format!("{w}x{h}"),
        "-framerate".into(), fps.to_string(), "-thread_queue_size".into(), "8".into(), "-i".into(), format!("tcp://127.0.0.1:{port}")];
    // The graph's place moves along with the input put before it.
    a.splice(first..first, input.map(OsString::from));
    true
}

/// The newest tall picture, as the writer leaves it for the feed.
pub type Slot = Arc<Mutex<Arc<Vec<u8>>>>;

/// A black tall picture of `size`, to start with.
pub fn black((w, h): (u32, u32)) -> Arc<Vec<u8>> { Arc::new([0, 0, 0, 255].repeat(w as usize * h as usize)) }

/// Listens on a free port of this computer for FFmpeg.
pub async fn listen() -> std::io::Result<(tokio::net::TcpListener, u16)> {
    let l = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = l.local_addr()?.port();
    Ok((l, port))
}

/// Waits for FFmpeg to connect, then writes the newest picture in `slot` at `fps` until `stop` (then it closes, and
/// FFmpeg ends the tall picture).
pub async fn feed(listener: tokio::net::TcpListener, slot: Slot, fps: u32, stop: CancellationToken) {
    let accepted = tokio::select! {
        a = listener.accept() => a,
        _ = stop.cancelled() => return,
        _ = tokio::time::sleep(CONNECT_LIMIT) => { eprintln!("recording: FFmpeg never came for the tall picture"); return }
    };
    let Ok((mut socket, _)) = accepted.map_err(|e| eprintln!("recording: the tall picture: {e}")) else { return };
    let _ = socket.set_nodelay(true);
    let t0 = Instant::now();
    let mut pump = Pump::new(fps);
    loop {
        let due = pump.due(t0.elapsed());
        let picture = slot.lock().unwrap_or_else(|e| e.into_inner()).clone();
        for _ in 0..due.write {
            if socket.write_all(&picture).await.is_err() { return }
        }
        let wait = pump.next_at().saturating_sub(t0.elapsed());
        tokio::select! {
            _ = stop.cancelled() => break,
            _ = tokio::time::sleep(wait) => {}
        }
    }
    let _ = socket.shutdown().await;
}

/// The wide picture `wide` (of `wide_size`) fitted across the tall `out` (of `tall`), black above and below.
pub fn fit(wide: &[u8], (w, h): (u32, u32), out: &mut [u8], (tw, th): (u32, u32)) {
    crate::record::pump::fit(&crate::record::pump::Raw { w, h, bottom_up: false, rgba: wide }, out, tw, th);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::own_scenes::OwnScene;

    fn strs(a: &[OsString]) -> Vec<String> { a.iter().map(|s| s.to_string_lossy().into_owned()).collect() }

    #[test]
    fn ffmpeg_reads_the_tall_picture_from_the_app() {
        let mut a: Vec<OsString> = ["-i", "pipe:0", "-f", "lavfi", "-i", "anullsrc", "-filter_complex", "[0:v]scale=720:-2,pad=720:1280:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1[tall]", "-map", "0:v", "-map", "[tall]"]
            .map(OsString::from).to_vec();
        assert!(read_from(&mut a, 5000, (720, 1280), 30));
        let s = strs(&a).join(" ");
        assert!(s.contains("-i anullsrc -f rawvideo -pix_fmt rgba -video_size 720x1280 -framerate 30 -thread_queue_size 8 -i tcp://127.0.0.1:5000 -filter_complex [2:v]setsar=1[tall]"), "{s}");
        let mut none: Vec<OsString> = ["-i", "pipe:0", "-map", "0:v"].map(OsString::from).to_vec();
        assert!(!read_from(&mut none, 5000, (720, 1280), 30), "no tall picture: left alone");
    }

    #[test]
    fn only_with_a_tall_destination_and_a_tall_scene() {
        let tall = OwnScene { tall: true, ..OwnScene::new("t".into()) };
        let wide = OwnScene::new("w".into());
        assert!(wanted(&[wide.clone(), tall.clone()], &[false, true]));
        assert!(!wanted(&[wide.clone()], &[true]));
        assert!(!wanted(&[tall], &[false, false]));
    }

    #[test]
    fn the_wide_picture_fits_across_the_tall_one() {
        let wide = [200u8, 0, 0, 255].repeat(4 * 2);
        let mut out = vec![9u8; 2 * 4 * 4];
        fit(&wide, (4, 2), &mut out, (2, 4));
        assert_eq!((&out[..4], &out[12..16]), (&[0u8, 0, 0, 255][..], &[200u8, 0, 0, 255][..]), "black above, the picture across the middle");
    }

    #[tokio::test]
    async fn the_feed_writes_the_newest_picture_until_stopped() {
        let (l, port) = listen().await.unwrap();
        let slot: Slot = Arc::new(Mutex::new(Arc::new(vec![1, 2, 3, 4])));
        let stop = CancellationToken::new();
        let task = tokio::spawn(feed(l, slot.clone(), 30, stop.clone()));
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let mut got = [0u8; 4];
        tokio::io::AsyncReadExt::read_exact(&mut s, &mut got).await.unwrap();
        assert_eq!(got, [1, 2, 3, 4]);
        *slot.lock().unwrap() = Arc::new(vec![5, 6, 7, 8]);
        // Pictures go on coming at the frame rate; the new one comes within a few.
        let mut seen_new = false;
        for _ in 0..10 { tokio::io::AsyncReadExt::read_exact(&mut s, &mut got).await.unwrap(); if got == [5, 6, 7, 8] { seen_new = true; break } }
        assert!(seen_new);
        stop.cancel();
        task.await.unwrap();
        let mut rest = vec![];
        tokio::io::AsyncReadExt::read_to_end(&mut s, &mut rest).await.unwrap();
        assert_eq!(rest.len() % 4, 0, "it closes between pictures");
    }
}
