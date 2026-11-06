//! Preparing the room's new layers off the UI thread: slides drawn by the shared renderer,
//! pictures decoded at display size, and players created and loaded (paused) for video and audio.
use super::Msg;
use crate::{app::App, live::{Frame, Live, MediaLayer, SlideLayer}, model::{Background, SlideContent, Theme}, render::{Generation, RenderKey, Renderer, Wanted, parse_color}};
use crate::room::{Kind, Want};
use crate::video::{Player, Players, Role};
use slint::{Color, Image, Rgba8Pixel, SharedPixelBuffer};
use crate::ui::block_on;
use std::{path::{Path, PathBuf}, sync::{Arc, mpsc}};

/// Pixels made off the UI thread (an `Image` is not `Send`).
pub enum Px { Straight(SharedPixelBuffer<Rgba8Pixel>), Premul(SharedPixelBuffer<Rgba8Pixel>) }

impl Px {
    pub fn image(self) -> Image {
        match self { Px::Straight(b) => Image::from_rgba8(b), Px::Premul(b) => Image::from_rgba8_premultiplied(b) }
    }
}

/// A player made on a prepare thread. It has no render context yet (the only part of a
/// `Player` tied to a thread), so it may move to the UI thread once.
pub(super) struct Fresh(pub(super) Player);
unsafe impl Send for Fresh {}

/// One prepared layer: a picture over a fill color, or a loaded, paused player.
pub(super) enum Made { Pic(Option<Px>, Color), Player(Fresh) }

/// What the prepare worker shares with the room. `generation` moves on with each new plan, so the worker drops
/// what an older plan still had to do.
pub(super) struct Work { pub(super) app: Arc<App>, pub(super) renderer: Arc<Renderer>, pub(super) video: Players, pub(super) audio: Players, pub(super) generation: Generation }

/// One plan's layers to prepare, with what they need from the live view. `wanted` goes stale once a newer plan
/// starts.
pub(super) struct Job { pub(super) token: u64, pub(super) wants: Vec<Want>, pub(super) frame: Frame, pub(super) theme: Theme, pub(super) logo: Option<String>, pub(super) canvas: (u32, u32), pub(super) wanted: Wanted }

/// The room's one prepare thread, for as long as the room is open: it takes the newest job, skipping any that a
/// newer one replaced while it was busy (one thread per plan used to queue up on the renderer with fast Next
/// presses). Once the jobs are done it has the renderer draw the slides either side of the live one.
pub(super) fn worker(work: Arc<Work>, send: impl Fn(Msg) + Send + 'static) -> std::io::Result<mpsc::Sender<Job>> {
    let (tx, rx) = mpsc::channel::<Job>();
    std::thread::Builder::new().name("room-prepare".into()).spawn(move || {
        let mut next = None;
        loop {
            let Some(mut job) = next.take().or_else(|| rx.recv().ok()) else { return };
            while let Ok(newer) = rx.try_recv() { job = newer }
            prepare(&work, &job, &send);
            // Nothing new waiting: draw ahead. Something waiting: do that first.
            match rx.try_recv() {
                Ok(newer) => next = Some(newer),
                Err(mpsc::TryRecvError::Empty) => prefetch_neighbours(&work, &job),
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
    })?;
    Ok(tx)
}

enum Source {
    Fill(Color),
    Picture { path: PathBuf, cover: bool, scale: f32 },
    Text(RenderKey),
    Player { role: Role, inst: u64, path: PathBuf },
}

pub(in crate::ui) const BLACK: Color = Color::from_argb_u8(255, 0, 0, 0);

pub(in crate::ui) fn color(s: &str) -> Color { let [r, g, b, a] = parse_color(s); Color::from_argb_u8(a, r, g, b) }

fn source(kind: Kind, job: &Job, app: &App) -> Option<Source> {
    let (f, asset) = (&job.frame, |a: &str| app.store.asset_path(a));
    let picture = |a: &str, cover, scale| Source::Picture { path: asset(a), cover, scale };
    Some(match kind {
        Kind::Background => {
            let b = f.background.as_ref()?;
            match &b.v {
                Background::Color { color: c } => Source::Fill(color(c)),
                Background::Image { asset } => picture(asset, true, 1.0),
                Background::Video { asset: a } => Source::Player { role: Role::Background, inst: b.inst, path: asset(a) },
            }
        }
        Kind::Media => {
            let m = f.media.as_ref()?;
            match &m.v {
                MediaLayer::Still { asset } => picture(asset, false, 1.0),
                MediaLayer::Video { asset: a, end, .. } => Source::Player { role: Role::Foreground(*end), inst: m.inst, path: asset(a) },
            }
        }
        Kind::Slide => match f.slide.as_ref()? {
            SlideLayer::Text { text, footer, reference, scripture, size: max, passage, parallel } => Source::Text(RenderKey {
                text: text.clone(), footer: footer.clone(), reference: reference.clone(), scripture: *scripture,
                size: *max, passage: passage.clone(), theme: job.theme.clone(), canvas: job.canvas, parallel: parallel.clone()
            }),
            SlideLayer::Image { asset } => picture(asset, false, 1.0),
        },
        // The logo sits on black at up to 70 % of the screen (`logoEl` in `render.js`).
        Kind::Logo => match &job.logo { Some(a) => picture(a, false, 0.7), None => Source::Fill(BLACK) },
        Kind::Audio => {
            let a = f.audio.as_ref()?;
            Source::Player { role: Role::Audio { looped: a.v.looped }, inst: a.inst, path: asset(&a.v.asset) }
        }
    })
}

/// Prepares every layer of `job`, players first (they take longest to be ready), and sends each
/// as it is done. A player that cannot be made is reported by `Players` for its instance instead.
/// Once a newer plan has started, what is left is dropped unsent (the room ignores an old plan anyway).
pub(super) fn prepare(work: &Work, job: &Job, send: impl Fn(Msg)) {
    let mut sources: Vec<_> = job.wants.iter().map(|w| (w.kind, source(w.kind, job, &work.app).unwrap_or(Source::Fill(Color::default())))).collect();
    sources.sort_by_key(|(_, s)| !matches!(s, Source::Player { .. }));
    let token = job.token;
    for (kind, s) in sources {
        if !job.wanted.still() { return }
        let made = match s {
            Source::Fill(c) => Ok(Made::Pic(None, c)),
            Source::Picture { path, cover, scale } => {
                let canvas = ((job.canvas.0 as f32 * scale) as u32, (job.canvas.1 as f32 * scale) as u32);
                // Pictures sit on black, as in the web room; image backgrounds cover the screen.
                picture(&path, canvas, cover).map(|px| Made::Pic(Some(px), BLACK))
            }
            Source::Text(key) => {
                // Shared with the renderer's cache, not copied.
                let Some(r) = block_on(work.renderer.render_if(key, job.wanted.clone())) else { return };
                Ok(Made::Pic(Some(Px::Premul(r.rgba_premul.buffer())), Color::default()))
            }
            Source::Player { role, inst, path } => {
                let players = if matches!(role, Role::Audio { .. }) { &work.audio } else { &work.video };
                let device = work.app.data.lock().unwrap().settings.audio_device.clone();
                let Some(p) = players.create(role, inst, &device) else { continue };
                p.set_paused(true); // starts at the swap, like `startMedia` in `output.js`
                if role == Role::Background {
                    // Fill the screen like CSS `object-fit: cover`.
                    let _ = unsafe { p.mpv().set_property(p.handle(), "panscan", "1.0") };
                }
                p.load(&path, work.app.resume_position(inst).unwrap_or(0.0)); // after a crash, where it was
                Ok(Made::Player(Fresh(p)))
            }
        };
        send(Msg::Made { token, kind, made });
    }
}

/// The render keys of the live item's text slides either side of the live one, as the room would draw them.
pub(super) fn neighbours(live: &Live, theme: &Theme, canvas: (u32, u32)) -> Vec<RenderKey> {
    let (Some(item), Some(cursor)) = (live.item.as_ref(), live.cursor.as_ref()) else { return vec![] };
    let scripture = item.kind == "scripture";
    let passage = crate::model::passage(&item.kind, &item.slides);
    [cursor.slide + 1, cursor.slide.wrapping_sub(1)].into_iter().filter_map(|i| match &item.slides.get(i)?.content {
        SlideContent::Text { text, footer, reference, size, parallel } => Some(RenderKey {
            text: text.clone(), footer: footer.clone(), reference: reference.clone(), scripture,
            size: *size, passage: passage.clone(), theme: theme.clone(), canvas, parallel: parallel.clone()
        }),
        _ => None,
    }).collect()
}

/// While the room shows a text slide, has the renderer draw the next and previous ones when it is idle.
fn prefetch_neighbours(work: &Work, job: &Job) {
    if !matches!(job.frame.slide, Some(SlideLayer::Text { .. })) { return }
    let keys = { let live = work.app.live.lock().unwrap(); neighbours(&live, &job.theme, job.canvas) };
    for k in keys { work.renderer.prefetch(k) }
}

fn unreadable<E>(_: E) -> String { "picture could not be loaded".into() }

/// Decodes a picture (by its content, not its name) and shrinks it to fit (`cover`: to fill) `w`×`h`.
pub(in crate::ui) fn picture(path: &Path, (w, h): (u32, u32), cover: bool) -> Result<Px, String> {
    let img = image::ImageReader::open(path).map_err(unreadable)?.with_guessed_format().map_err(unreadable)?.decode().map_err(unreadable)?.into_rgba8();
    let (iw, ih) = img.dimensions();
    let (sx, sy) = (w.max(1) as f32 / iw as f32, h.max(1) as f32 / ih as f32);
    let s = if cover { sx.max(sy) } else { sx.min(sy) };
    let img = if s < 1.0 {
        let size = |n: u32| ((n as f32 * s).round() as u32).max(1);
        image::imageops::resize(&img, size(iw), size(ih), image::imageops::FilterType::Triangle)
    } else { img };
    Ok(Px::Straight(SharedPixelBuffer::clone_from_slice(img.as_raw(), img.width(), img.height())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{live::{Cursor, LiveItem}, model::Slide};

    fn text(t: &str) -> Slide { Slide { label: String::new(), content: SlideContent::Text { text: t.into(), footer: None, reference: None, size: 90, parallel: None }, notes: String::new() } }

    #[test]
    fn neighbours_are_the_text_slides_either_side_of_the_live_one() {
        let mut live = Live::new();
        assert!(neighbours(&live, &Theme::default(), (64, 36)).is_empty(), "nothing live");
        let picture = Slide { label: String::new(), content: SlideContent::Image { asset: "p.png".into() }, notes: String::new() };
        live.item = Some(LiveItem {
            item_id: uuid::Uuid::nil(), title: "John 3".into(), kind: "scripture".into(), slides: vec![text("one"), text("two"), picture, text("four")],
            background: Background::Color { color: "#000000".into() },
        });
        let at = |live: &mut Live, slide| live.cursor = Some(Cursor { item_id: uuid::Uuid::nil(), item_index: 0, slide });
        let texts = |live: &Live| neighbours(live, &Theme::default(), (64, 36)).into_iter().map(|k| { assert!(k.scripture && k.canvas == (64, 36)); k.text }).collect::<Vec<_>>();
        at(&mut live, 0);
        assert_eq!(texts(&live), ["two"], "nothing before the first slide");
        // The same size and passage as the room's own key, or the drawing done ahead is never used.
        let key = neighbours(&live, &Theme::default(), (64, 36)).remove(0);
        assert_eq!(key.size, 90);
        assert_eq!(key.passage.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["one", "two", "four"]);
        at(&mut live, 1);
        assert_eq!(texts(&live), ["one"], "a picture slide is not drawn ahead");
        at(&mut live, 3);
        assert_eq!(texts(&live), Vec::<String>::new(), "after the last: nothing; before it: a picture");
    }
}
