//! The words over the camera (a lower third): the livestream's third camera layout. A worker looks at the live
//! slide ten times a second; when it is a text slide (a song, a verse) its words are drawn the way the built-in
//! Lower third theme draws them (at the bottom, on a soft band, in the slide's own font and colour) on a clear
//! picture, and that picture goes to the recorder's FFmpeg over a local connection, to be laid over the camera.
//! Pictures, videos and a cleared slide give a clear picture: the camera alone.
use crate::live::SlideLayer;
use crate::model::Theme;
use crate::render::cache::{RenderKey, draw_key};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How often the words picture is sent: words change at slide changes, so ten a second is plenty.
pub const FPS: u32 = 10;
/// The connection FFmpeg listens on is tried for this long (it opens its inputs one after the other).
const CONNECT_FOR: Duration = Duration::from_secs(30);

/// What is live now, as the words picture to draw (`None`: nothing with words).
pub type Source = Arc<dyn Fn() -> Option<RenderKey> + Send + Sync>;

/// `Source` in a box that can be kept in a `record::run::Plan` (which is `Debug`).
#[derive(Clone)]
pub struct Words(pub Source);

impl std::fmt::Debug for Words {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("Words") }
}

/// The lower-third look of `base` (the slide's own theme): its font and colours, at the bottom on a band.
pub fn lower_third(base: &Theme) -> Theme {
    Theme { size: 64, scripture_size: 52, position: "lower".into(), band: true, band_opacity: 55, ..base.clone() }
}

/// The words picture for live slide `slide` (`theme`: the item's own), at `canvas`. `None` for a slide with no
/// words of its own to put over a camera (a picture, a designed slide).
pub fn key_for(slide: &SlideLayer, theme: &Theme, canvas: (u32, u32)) -> Option<RenderKey> {
    let SlideLayer::Text { text, footer, reference, scripture, size, passage, parallel, theme: own } = slide else { return None };
    if text.trim().is_empty() { return None }
    let base = own.clone().unwrap_or_else(|| theme.clone());
    // The slide's own largest size is for the room; over a camera the lower third's size is the one.
    let _ = size;
    Some(RenderKey { text: text.clone(), footer: footer.clone(), reference: reference.clone(), scripture: *scripture, size: 0, passage: passage.clone(),
        parallel: parallel.clone(), theme: lower_third(&base), canvas, design: None })
}

/// FFmpeg's input arguments for the words picture, `size` wide and high, on local `port`.
pub fn input_args(size: (u32, u32), port: u16) -> Vec<String> {
    vec!["-f".into(), "rawvideo".into(), "-pix_fmt".into(), "rgba".into(), "-video_size".into(), format!("{}x{}", size.0, size.1),
        "-framerate".into(), FPS.to_string(), "-thread_queue_size".into(), "64".into(), "-i".into(), format!("tcp://127.0.0.1:{port}?listen=1")]
}

/// The worker. Dropping it stops it.
pub struct Feed {
    pub port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Feed {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() { let _ = t.join(); }
    }
}

impl Drop for Feed {
    fn drop(&mut self) { self.stop() }
}

/// Starts sending the words picture for what `source` says is live, `size` wide and high.
pub fn start(source: Source, size: (u32, u32)) -> Result<Feed, String> {
    let port = std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map_err(|e| e.to_string())?.port();
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    let thread = std::thread::Builder::new().name("stream-words".into()).spawn(move || run(source, size, port, &s)).map_err(|e| e.to_string())?;
    Ok(Feed { port, stop, thread: Some(thread) })
}

fn run(source: Source, size: (u32, u32), port: u16, stop: &AtomicBool) {
    use std::io::Write;
    let began = Instant::now();
    let mut out = loop {
        if stop.load(Ordering::Relaxed) { return }
        match std::net::TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => break s,
            Err(_) if began.elapsed() < CONNECT_FOR => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => { eprintln!("stream words: FFmpeg did not listen: {e}"); return }
        }
    };
    let mut fonts = crate::render::Fonts::new();
    let clear = vec![0u8; size.0 as usize * size.1 as usize * 4];
    let mut shown: Option<RenderKey> = None;
    let mut picture = clear.clone();
    let t0 = Instant::now();
    let mut sent: u64 = 0;
    while !stop.load(Ordering::Relaxed) {
        let want = source();
        if want != shown {
            picture = match &want {
                Some(key) => {
                    let r = draw_key(&mut fonts, key);
                    let bytes = r.rgba_premul.buffer();
                    if (r.width, r.height) == size { bytes.as_bytes().to_vec() } else { clear.clone() }
                }
                None => clear.clone(),
            };
            shown = want;
        }
        // As many pictures as the clock says are due, so FFmpeg's times stay with the camera's.
        let due = (t0.elapsed().as_secs_f64() * FPS as f64) as u64 + 1;
        while sent < due {
            if out.write_all(&picture).is_err() { return }
            sent += 1;
        }
        std::thread::sleep(Duration::from_millis(1000 / FPS as u64 / 2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str) -> SlideLayer {
        SlideLayer::Text { text: t.into(), footer: None, reference: None, scripture: false, size: 120, passage: vec![], parallel: None, theme: None }
    }

    #[test]
    fn the_words_take_the_lower_third_look_in_the_slides_own_font() {
        let theme = Theme { font: "Georgia".into(), color: "#ffee00".into(), size: 90, ..Theme::default() };
        let key = key_for(&text("Amazing grace"), &theme, (1280, 720)).unwrap();
        assert_eq!((key.theme.position.as_str(), key.theme.band, key.theme.size, key.size), ("lower", true, 64, 0));
        assert_eq!((key.theme.font.as_str(), key.theme.color.as_str()), ("Georgia", "#ffee00"));
        assert_eq!(key.canvas, (1280, 720));
        assert_eq!(key_for(&text("  "), &theme, (1280, 720)), None, "no words, nothing over the camera");
        assert_eq!(input_args((1280, 720), 40600).join(" "), "-f rawvideo -pix_fmt rgba -video_size 1280x720 -framerate 10 -thread_queue_size 64 -i tcp://127.0.0.1:40600?listen=1");
    }
}
