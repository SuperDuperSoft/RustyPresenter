//! Slide pictures for phones: the remote's live slide (`/remote/live-slide`) and the stage view's live and next
//! slides (`/stage-view/live.jpg`, `/stage-view/next.jpg`). Each is a slide drawn up to 1920 pixels wide and made
//! into a JPEG, so at most `AT_ONCE` are drawn at a time however many devices ask, a request that cannot start
//! within `WAIT` is told the computer is busy, and a picture already drawn is handed out again.
use crate::{app::App, ui::thumbs::{self, Pixels, ThumbKey}};
use axum::{body::Bytes, http::{StatusCode, header}, response::{IntoResponse, Response}};
use std::{sync::{Arc, Mutex}, time::Duration};
use tokio::sync::Semaphore;

/// How many slide pictures are drawn for phones at once.
pub const AT_ONCE: usize = 2;
/// How long a request waits for its turn before it is answered 503.
const WAIT: Duration = Duration::from_secs(2);
/// How many drawn pictures are kept (a few sizes of the live and the next slide).
const KEPT: usize = 8;

static TURNS: Semaphore = Semaphore::const_new(AT_ONCE);
/// The thumbnail renderer the phones' pictures share (made on the first text slide).
static RENDER: thumbs::TextRenderer = thumbs::TextRenderer::new();

/// Which slide: the one on the audience screen now (in the screen's shape), or the one Next would show (16:9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which { Live, Next }

/// What was drawn: which slide, and everything that decides how it looks (`None` is a black picture of that size).
type Drawn = (Which, (u32, u32), Option<ThumbKey>);
static LAST: Mutex<Vec<(Drawn, Bytes)>> = Mutex::new(Vec::new());

/// The size a phone's picture is drawn at: `width` kept to 160-1920 pixels, in the shape of `canvas` (16:9 when
/// it has no size).
pub fn size(width: u32, canvas: (u32, u32)) -> (u32, u32) {
    let w = width.clamp(160, 1920);
    let (cw, ch) = if canvas.0 > 0 && canvas.1 > 0 { canvas } else { (16, 9) };
    (w, ((w as u64 * ch as u64 + cw as u64 / 2) / cw as u64).clamp(1, 1920) as u32)
}

/// Opaque pixels as a JPEG.
pub fn to_jpeg(px: &Pixels) -> Result<Vec<u8>, String> {
    let rgb: Vec<u8> = px.as_bytes().chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .encode(&rgb, px.width(), px.height(), image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
    Ok(out)
}

/// The picture, drawn now on this thread unless the same picture was drawn a moment ago. The drawing itself is not
/// in it (a phone draws its own lines over it).
pub fn jpeg_now(app: &App, which: Which, width: u32) -> Result<Bytes, String> {
    let size = match which { Which::Live => size(width, app.room_canvas()), Which::Next => size(width, (16, 9)) };
    let job = match which { Which::Live => thumbs::live_slide_job(app, size), Which::Next => thumbs::next_slide_job(app, size) };
    let (key, job) = job.unzip();
    let drawn: Drawn = (which, size, key);
    if let Some((_, jpeg)) = LAST.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(d, _)| *d == drawn) { return Ok(jpeg.clone()) }
    let jpeg = Bytes::from(to_jpeg(&thumbs::draw_job(job.as_ref(), size, &RENDER, &app.store))?);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if last.len() >= KEPT { last.remove(0); }
    last.push((drawn, jpeg.clone()));
    Ok(jpeg)
}

/// The picture for a request: drawn off the async workers, `AT_ONCE` at a time. `SERVICE_UNAVAILABLE` when it
/// could not start within `WAIT`.
pub async fn jpeg(app: Arc<App>, which: Which, width: u32) -> Result<Bytes, StatusCode> {
    let Ok(Ok(turn)) = tokio::time::timeout(WAIT, TURNS.acquire()).await else { return Err(StatusCode::SERVICE_UNAVAILABLE) };
    // The turn goes into the worker: a request that is dropped half way still counts until its drawing is done.
    let drawn = tokio::task::spawn_blocking(move || {
        let _turn = turn;
        #[cfg(test)]
        let _counted = test_hook::Drawing::start();
        jpeg_now(&app, which, width)
    }).await;
    match drawn { Ok(Ok(jpeg)) => Ok(jpeg), _ => Err(StatusCode::INTERNAL_SERVER_ERROR) }
}

/// The answer to a picture request.
pub async fn respond(app: Arc<App>, which: Which, width: u32) -> Response {
    match jpeg(app, which, width).await {
        Ok(jpeg) => ([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "no-store")], jpeg).into_response(),
        Err(StatusCode::SERVICE_UNAVAILABLE) => (StatusCode::SERVICE_UNAVAILABLE, [(header::RETRY_AFTER, "2")], crate::tr!("The computer is busy. Try again in a moment.")).into_response(),
        Err(status) => (status, crate::tr!("Could not draw the slide.")).into_response(),
    }
}

#[cfg(test)]
pub(crate) mod test_hook {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NOW: AtomicUsize = AtomicUsize::new(0);
    /// The most requests that ever held a turn at the same moment.
    pub static PEAK: AtomicUsize = AtomicUsize::new(0);

    /// Counts a request for as long as it holds its turn.
    pub struct Drawing;
    impl Drawing {
        pub fn start() -> Self {
            let now = NOW.fetch_add(1, Ordering::SeqCst) + 1;
            PEAK.fetch_max(now, Ordering::SeqCst);
            // Long enough that requests fired together would overlap if nothing held them back.
            std::thread::sleep(std::time::Duration::from_millis(15));
            Self
        }
    }
    impl Drop for Drawing { fn drop(&mut self) { NOW.fetch_sub(1, Ordering::SeqCst); } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_app;
    use std::sync::atomic::Ordering;

    /// B10: however many devices ask at once, at most two pictures are being drawn.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn at_most_two_pictures_are_drawn_at_once() {
        let app = test_app();
        // Twenty different sizes, so none is handed out from memory.
        let asks: Vec<_> = (0..20u32).map(|i| tokio::spawn(jpeg(app.clone(), Which::Live, 200 + 16 * i))).collect();
        let mut ok = 0;
        for a in asks { if a.await.unwrap().is_ok() { ok += 1 } }
        assert_eq!(ok, 20, "none had to wait longer than two seconds");
        let peak = test_hook::PEAK.load(Ordering::SeqCst);
        assert!((1..=AT_ONCE).contains(&peak), "{peak} were drawn at once");
    }

    #[tokio::test]
    async fn the_same_picture_is_drawn_once() {
        let app = test_app();
        let first = jpeg_now(&app, Which::Next, 1234).unwrap();
        // The very same bytes come back: handed out from memory.
        let again = jpeg_now(&app, Which::Next, 1234).unwrap();
        assert_eq!(first.as_ptr(), again.as_ptr(), "handed out again, not drawn again");
        assert_eq!(&first[..2], [0xff, 0xd8], "a JPEG");
        assert_eq!(size(10, (1920, 1080)), (160, 90));
        assert_eq!(size(400, (1080, 1920)), (400, 711), "a portrait screen");
    }
}
