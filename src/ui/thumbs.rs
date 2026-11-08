//! Slide thumbnails for the operator's grid, drawn off the UI thread the way the web draws them (`slideToLayers`
//! and `drawStatic` in `render.js`, with `logo: false`, `operator.js:183`). Text slides go through a renderer of
//! their own: `Renderer` is one worker with one FIFO, so sharing the room's would queue a whole service's
//! thumbnails ahead of the room's next slide. Video uses the frame the import saved (`MediaFile::thumb`), or black
//! until it has one; the grid draws the play overlay over it.
use crate::{
    model::{AssetId, Background, MediaFile, PassageSlide, Settings, Slide, SlideContent},
    render::{RenderKey, Renderer, parse_color},
    store::Store,
    ui::block_on,
};
use image::{DynamicImage, Rgba, RgbaImage, imageops::{self, FilterType}};
use serde::Serialize;
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::{cell::RefCell, collections::{HashMap, VecDeque}, path::{Path, PathBuf}, rc::Rc};
use std::sync::{Arc, OnceLock, atomic::{AtomicU64, Ordering}, mpsc};
use tokio::sync::oneshot;

pub type Pixels = SharedPixelBuffer<Rgba8Pixel>;

/// Pixels kept for thumbnails already drawn: about 340 at 256×144, so going back to an item costs nothing.
const CACHE_CAP: usize = 48 * 1024 * 1024;
/// The thumbnails' text renderer keeps less than the room's (its results are cached here as whole thumbnails).
const RENDER_CAP: usize = 16 * 1024 * 1024;
/// Thumbnail widths are whole steps of this, so resizing the window does not re-render on every frame.
const STEP: f32 = 64.0;

/// Everything that decides how a thumbnail looks. `layers` is the JSON of the background, media and picture
/// layers (`Slide` and `Background` are not `Eq`/`Hash`). Thumbnails never draw the logo, so it has no field.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ThumbKey { pub render: Option<RenderKey>, pub layers: String, pub size: (u32, u32) }

/// The media layer of a thumbnail: a still, or a video with its saved frame if it has one.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThumbMedia { Still { asset: AssetId }, Video { thumb: Option<AssetId> } }

/// What a thumbnail draws, bottom to top (`drawStatic`): background, media, a deck picture, text.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ThumbJob {
    pub background: Option<Background>,
    pub media: Option<ThumbMedia>,
    pub picture: Option<AssetId>,
    #[serde(skip)]
    pub text: Option<RenderKey>,
}

/// The size to draw a thumbnail for a cell `cell_w` logical pixels wide: the cell's device pixels rounded up to
/// a whole 64 px step, at 16:9.
pub fn thumb_size(cell_w: f32, scale: f32) -> (u32, u32) {
    let steps = ((cell_w * scale).max(1.0) / STEP).ceil() as u32;
    let w = steps.max(1) * STEP as u32;
    (w, w * 9 / 16)
}

/// The saved frame of the video with this asset.
pub fn video_thumb(media: &[MediaFile], asset: &str) -> Option<AssetId> {
    media.iter().find(|m| m.asset == asset).and_then(|m| m.thumb.clone())
}

/// The settings an item's thumbnails are drawn with: its resolved theme (item > service > church default) as the
/// theme, and its resolved background (its own, else the theme's, else Standard's) as the background.
pub fn item_settings(data: &crate::model::Data, item: &crate::model::ServiceItem) -> Settings {
    let service = crate::theme::service_of(data, item.id).map(|(s, _)| s);
    let r = crate::theme::resolve(data, service, item);
    Settings { theme: r.style, background: r.background, ..data.settings.clone() }
}

/// The thumbnail of `slide` of an item of `kind` (`slideToLayers`). The background is the item's, else the
/// settings' (`itemBackground`); a video background is drawn as its saved frame. The logo is never drawn. `passage`
/// is the item's (`model::passage`), so the thumbnail's text has the size the room shows.
pub fn thumb(slide: &Slide, kind: &str, passage: &[PassageSlide], item_background: Option<&Background>, settings: &Settings, media: &[MediaFile], size: (u32, u32)) -> (ThumbKey, ThumbJob) {
    let background = match item_background.unwrap_or(&settings.background) {
        Background::Video { asset, dim } if let Some(thumb) = video_thumb(media, asset) => Background::Image { asset: thumb, dim: *dim },
        other => other.clone(),
    };
    let job = match &slide.content {
        SlideContent::Text { text, footer, reference, size: max, parallel } => ThumbJob {
            background: Some(background), media: None, picture: None,
            text: Some(RenderKey {
                text: text.clone(), footer: footer.clone(), reference: reference.clone(), scripture: kind == "scripture",
                size: *max, passage: passage.to_vec(), theme: settings.theme.clone(), canvas: size, parallel: parallel.clone()
            }),
        },
        SlideContent::Image { asset } => ThumbJob { background: None, media: None, picture: Some(asset.clone()), text: None },
        SlideContent::Video { asset, .. } => ThumbJob { background: Some(background), media: Some(ThumbMedia::Video { thumb: video_thumb(media, asset) }), picture: None, text: None },
        SlideContent::Still { asset } => ThumbJob { background: Some(background), media: Some(ThumbMedia::Still { asset: asset.clone() }), picture: None, text: None },
        SlideContent::Audio { .. } => ThumbJob { background: None, media: None, picture: None, text: None },
    };
    let key = ThumbKey { render: job.text.clone(), layers: serde_json::to_string(&job).unwrap_or_default(), size };
    (key, job)
}

/// The thumbnails' text renderer, made on the first text slide (it loads the system fonts).
pub type TextRenderer = OnceLock<Arc<Renderer>>;

/// Draws a thumbnail at `size` on black. A picture that cannot be read is left out, like a broken `<img>`.
pub fn compose(job: &ThumbJob, size: (u32, u32), renderer: &TextRenderer, store: &Store) -> Pixels {
    let (w, h) = (size.0.max(1), size.1.max(1));
    let mut img = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 255]));
    let picture = |asset: &str| decode(&store.asset_path(asset));
    match &job.background {
        // Gradients and Darken are drawn like the room draws them.
        Some(bg) if matches!(bg, Background::Gradient { .. }) || bg.dim() > 0 => {
            let decoded = bg.asset().filter(|_| matches!(bg, Background::Image { .. })).and_then(|a| picture(a)).map(|p| p.into_rgba8());
            let px = crate::render::background::background_pixels(bg, decoded.as_ref(), (w, h));
            over_premultiplied(&mut img, &px.rgba_premul);
        }
        Some(Background::Color { color }) => {
            let [r, g, b, a] = parse_color(color);
            imageops::overlay(&mut img, &RgbaImage::from_pixel(w, h, Rgba([r, g, b, a])), 0, 0);
        }
        // An image background covers the screen; a video one with no saved frame is black.
        Some(Background::Image { asset, .. }) => if let Some(p) = picture(asset) {
            imageops::overlay(&mut img, &p.resize_to_fill(w, h, FilterType::Triangle).into_rgba8(), 0, 0);
        },
        Some(Background::Video { .. } | Background::Gradient { .. }) | None => {}
    }
    match &job.media {
        Some(ThumbMedia::Still { asset }) => if let Some(p) = picture(asset) { contain(&mut img, &p) },
        // The web's `<video>` has a black background over the whole screen.
        Some(ThumbMedia::Video { thumb }) => {
            img = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 255]));
            if let Some(p) = thumb.as_deref().and_then(picture) { contain(&mut img, &p) }
        }
        None => {}
    }
    if let Some(p) = job.picture.as_deref().and_then(picture) { contain(&mut img, &p) }
    if let Some(key) = &job.text {
        let text = block_on(renderer.get_or_init(|| Renderer::with_cap(RENDER_CAP)).render(key.clone()));
        if (text.width, text.height) == (w, h) { over_premultiplied(&mut img, &text.rgba_premul) }
    }
    Pixels::clone_from_slice(img.as_raw(), w, h)
}

/// The slide on the room screen now, drawn like a thumbnail at `size` (the phone's drawing view and the stage's
/// drawing panel). Black when nothing is on screen. Never the phone drawing itself.
pub fn live_slide(app: &crate::app::App, size: (u32, u32), renderer: &TextRenderer) -> Pixels {
    let job = {
        let data = app.data.lock().unwrap();
        let live = app.live.lock().unwrap();
        let on_air = live.frame.slide.is_some() || live.frame.media.is_some();
        live.cursor.as_ref().zip(live.item.as_ref()).filter(|_| on_air).and_then(|(c, item)| {
            let slide = item.slides.get(c.slide)?;
            let settings = Settings { theme: item.theme.clone(), background: item.background.clone(), ..data.settings.clone() };
            let passage = crate::model::passage(&item.kind, &item.slides);
            Some(thumb(slide, &item.kind, &passage, Some(&item.background), &settings, &data.media, size).1)
        })
    };
    match job {
        Some(job) => compose(&job, size, renderer, &app.store),
        None => {
            let black = RgbaImage::from_pixel(size.0.max(1), size.1.max(1), Rgba([0, 0, 0, 255]));
            Pixels::clone_from_slice(black.as_raw(), black.width(), black.height())
        }
    }
}

/// N10: the slide Next would show, drawn like a thumbnail at `size` (the stage's next-slide picture). Black at the end
/// of the service.
pub fn next_slide(app: &crate::app::App, size: (u32, u32), renderer: &TextRenderer) -> Pixels {
    let job = {
        let data = app.data.lock().unwrap();
        let live = app.live.lock().unwrap();
        live.next_slot(&crate::app::DataPlan(&data)).map(|(item, n)| {
            let settings = Settings { theme: item.theme.clone(), background: item.background.clone(), ..data.settings.clone() };
            let passage = crate::model::passage(&item.kind, &item.slides);
            thumb(&item.slides[n], &item.kind, &passage, Some(&item.background), &settings, &data.media, size).1
        })
    };
    match job {
        Some(job) => compose(&job, size, renderer, &app.store),
        None => {
            let black = RgbaImage::from_pixel(size.0.max(1), size.1.max(1), Rgba([0, 0, 0, 255]));
            Pixels::clone_from_slice(black.as_raw(), black.width(), black.height())
        }
    }
}

/// `over` (premultiplied, same size) drawn onto opaque `img`.
pub fn put_over(img: &mut Pixels, over: &[u8]) {
    let (w, h) = (img.width(), img.height());
    let mut rgba = RgbaImage::from_raw(w, h, img.as_bytes().to_vec()).unwrap_or_default();
    over_premultiplied(&mut rgba, over);
    *img = Pixels::clone_from_slice(rgba.as_raw(), w, h);
}

/// Draws `p` as large as fits, centred (`object-fit: contain`).
fn contain(img: &mut RgbaImage, p: &DynamicImage) {
    let (w, h) = img.dimensions();
    let p = p.resize(w, h, FilterType::Triangle).into_rgba8();
    imageops::overlay(img, &p, i64::from((w - p.width()) / 2), i64::from((h - p.height()) / 2));
}

/// Composites premultiplied RGBA of the same size over an opaque image.
fn over_premultiplied(img: &mut RgbaImage, src: &[u8]) {
    for (d, s) in img.as_mut().chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let keep = 255 - u16::from(s[3]);
        for c in 0..3 { d[c] = (u16::from(s[c]) + (u16::from(d[c]) * keep + 127) / 255).min(255) as u8; }
    }
}

/// A thumbnail to draw on the worker thread, asked for in request generation `generation`.
struct Job { key: ThumbKey, job: ThumbJob, generation: u64, reply: oneshot::Sender<Pixels> }

/// The newest thumbnails drawn, up to `CACHE_CAP` bytes (the newest is always kept).
#[derive(Default)]
struct Lru { map: HashMap<ThumbKey, Pixels>, order: VecDeque<ThumbKey>, bytes: usize }

impl Lru {
    fn get(&mut self, k: &ThumbKey) -> Option<Pixels> {
        let px = self.map.get(k)?.clone();
        if let Some(i) = self.order.iter().position(|x| x == k) && let Some(k) = self.order.remove(i) { self.order.push_back(k); }
        Some(px)
    }

    fn put(&mut self, k: ThumbKey, px: Pixels) {
        if self.map.contains_key(&k) { return; }
        self.bytes += px.as_bytes().len();
        self.map.insert(k.clone(), px);
        self.order.push_back(k);
        while self.bytes > CACHE_CAP && self.order.len() > 1 {
            let Some(old) = self.order.pop_front() else { break };
            if let Some(px) = self.map.remove(&old) { self.bytes -= px.as_bytes().len(); }
        }
    }
}

type Done = Box<dyn FnOnce(Pixels)>;

/// The worker's side: its queue, and the generation whose requests are still wanted.
struct Worker { jobs: mpsc::Sender<Job>, generation: Arc<AtomicU64> }

/// Draws thumbnails and keeps the recent ones. Lives on the UI thread: `done` runs there, so it needs no `Send`.
pub struct Thumbs {
    /// Inline mode's renderer (the worker has its own).
    renderer: TextRenderer,
    store_root: PathBuf,
    cache: Rc<RefCell<Lru>>,
    /// The callers waiting for each thumbnail on the worker, so a thumbnail asked for twice is drawn once.
    waiting: Rc<RefCell<HashMap<ThumbKey, Vec<Done>>>>,
    /// `None` in inline mode (tests), which draws at once on the calling thread.
    worker: Option<Worker>,
}

impl Thumbs {
    /// Thumbnails drawn on a worker thread of their own.
    pub fn new(store_root: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let generation = Arc::new(AtomicU64::new(0));
        let (worker_generation, store) = (generation.clone(), Store { root: store_root.clone() });
        let started = std::thread::Builder::new().name("slide-thumbs".into()).spawn(move || {
            let worker_renderer = TextRenderer::new();
            for j in rx {
                // A request for a grid that has since been replaced is dropped; its caller gets nothing.
                if j.generation != worker_generation.load(Ordering::Acquire) { continue }
                let _ = j.reply.send(compose(&j.job, j.key.size, &worker_renderer, &store));
            }
        });
        let worker = started.ok().map(|_| Worker { jobs: tx, generation });
        Self { renderer: TextRenderer::new(), store_root, cache: Rc::default(), waiting: Rc::default(), worker }
    }

    /// Thumbnails drawn at once on the calling thread, for tests on the testing backend (no event loop).
    pub fn inline(store_root: PathBuf) -> Self {
        Self { renderer: TextRenderer::new(), store_root, cache: Rc::default(), waiting: Rc::default(), worker: None }
    }

    /// Calls `done` on the UI thread with the thumbnail for `key`: at once when it is cached (or in inline mode),
    /// else once the worker has drawn it. `done` is dropped uncalled when `cancel_pending` runs first.
    pub fn request(&self, key: ThumbKey, job: ThumbJob, done: impl FnOnce(Pixels) + 'static) {
        let cached = self.cache.borrow_mut().get(&key);
        if let Some(px) = cached { return done(px) }
        let Some(worker) = &self.worker else { return self.draw_here(key, &job, done) };
        if let Some(w) = self.waiting.borrow_mut().get_mut(&key) { w.push(Box::new(done)); return; }
        let (reply, rx) = oneshot::channel::<Pixels>();
        let (cache, waiting, k) = (self.cache.clone(), self.waiting.clone(), key.clone());
        let delivered = slint::spawn_local(async move {
            let Ok(px) = rx.await else { return };
            cache.borrow_mut().put(k.clone(), px.clone());
            let callers = waiting.borrow_mut().remove(&k);
            for done in callers.into_iter().flatten() { done(px.clone()) }
        });
        if delivered.is_err() { return self.draw_here(key, &job, done) }
        self.waiting.borrow_mut().insert(key.clone(), vec![Box::new(done)]);
        let _ = worker.jobs.send(Job { key, job, generation: worker.generation.load(Ordering::Acquire), reply });
    }

    /// Forgets every request still waiting (the grid they were for was replaced); the worker skips their jobs.
    pub fn cancel_pending(&self) {
        if let Some(w) = &self.worker { w.generation.fetch_add(1, Ordering::AcqRel); }
        self.waiting.borrow_mut().clear();
    }

    fn draw_here(&self, key: ThumbKey, job: &ThumbJob, done: impl FnOnce(Pixels)) {
        let px = compose(job, key.size, &self.renderer, &Store { root: self.store_root.clone() });
        self.cache.borrow_mut().put(key, px.clone());
        done(px)
    }
}

/// Decodes a picture by its content, not its name; `None` when it cannot be read.
fn decode(path: &Path) -> Option<DynamicImage> {
    image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_settings_use_the_resolved_theme_and_background() {
        let mut d = crate::model::Data::default();
        let t = crate::theme::SavedTheme { id: uuid::Uuid::new_v4(), name: "X".into(), style: crate::model::Theme { size: 99, ..Default::default() },
            background: Some(Background::Gradient { from: "#0b1a33".into(), to: "#000000".into(), angle: 180, dim: 0 }) };
        d.themes.push(t.clone());
        d.services[0].theme = Some(t.id);
        let item = crate::model::ServiceItem { id: uuid::Uuid::new_v4(), kind: crate::model::ItemKind::Text { title: "a".into(), body: "b".into() }, background: None, notes: String::new(), theme: None };
        d.services[0].items.push(item.clone());
        let s = item_settings(&d, &item);
        assert_eq!((s.theme.size, s.background), (99, t.background.unwrap()));
    }

    #[test]
    fn thumbnails_draw_gradient_backgrounds() {
        let job = ThumbJob { background: Some(Background::Gradient { from: "#0b1a33".into(), to: "#000000".into(), angle: 180, dim: 0 }), media: None, picture: None, text: None };
        let px = compose(&job, (32, 18), &TextRenderer::new(), &store());
        let top = &px.as_bytes()[..4];
        assert!(top[0].abs_diff(0x0b) <= 3 && top[1].abs_diff(0x1a) <= 3 && top[2].abs_diff(0x33) <= 3, "{top:?}");
    }
    use crate::model::Theme;

    fn text(t: &str) -> Slide { Slide::new(t, SlideContent::Text { text: t.into(), footer: None, reference: None, size: 0, parallel: None }) }

    fn store() -> Store { Store::open(std::env::temp_dir().join(format!("rp-thumbs-{}", uuid::Uuid::new_v4()))).unwrap() }

    fn settings() -> Settings { Settings { theme: Theme { font: "Noto Sans".into(), ..Theme::default() }, ..Settings::default() } }

    #[test]
    fn thumb_size_rounds_to_64px_steps() {
        assert_eq!(thumb_size(230.0, 1.0), (256, 144));
        assert_eq!(thumb_size(256.0, 1.0), (256, 144), "an exact step stays");
        assert_eq!(thumb_size(257.0, 1.0), (320, 180));
        assert_eq!(thumb_size(300.0, 1.5), (512, 288), "device pixels: 450 → 512");
        assert_eq!(thumb_size(0.0, 1.0), (64, 36), "never empty");
        // Resizing within a step keeps the size, so nothing is drawn again.
        assert_eq!(thumb_size(201.0, 1.0), thumb_size(250.0, 1.0));
    }

    #[test]
    fn thumb_key_ignores_logo() {
        let mut with_logo = settings();
        with_logo.logo = Some("logo.png".into());
        let slide = text("Amazing grace");
        let (a, _) = thumb(&slide, "song", &[], None, &settings(), &[], (256, 144));
        let (b, _) = thumb(&slide, "song", &[], None, &with_logo, &[], (256, 144));
        assert_eq!(a, b);
        // The item's own background is part of the key.
        let red = Background::Color { color: "#ff0000".into() };
        let (c, job) = thumb(&slide, "song", &[], Some(&red), &with_logo, &[], (256, 144));
        assert_ne!(a, c);
        assert_eq!(job.background, Some(red));
    }

    #[test]
    fn thumb_layers_follow_slide_to_layers() {
        let s = settings();
        let slide = |content| Slide::new("x", content);
        let (_, deck) = thumb(&slide(SlideContent::Image { asset: "a.png".into() }), "deck", &[], None, &s, &[], (64, 36));
        assert_eq!((deck.background, deck.picture.as_deref()), (None, Some("a.png")), "a deck picture has no background");
        let (_, video) = thumb(&slide(SlideContent::Video { asset: "v.mp4".into(), autoplay: true, end: Default::default() }), "video", &[], None, &s, &[], (64, 36));
        assert_eq!(video.media, Some(ThumbMedia::Video { thumb: None }));
        let (_, audio) = thumb(&slide(SlideContent::Audio { asset: "a.mp3".into(), looped: false }), "audio", &[], None, &s, &[], (64, 36));
        assert_eq!(audio, ThumbJob { background: None, media: None, picture: None, text: None });
        let (k, scripture) = thumb(&text("In the beginning"), "scripture", &[], None, &s, &[], (64, 36));
        assert!(scripture.text.unwrap().scripture);
        assert_eq!(k.render.unwrap().canvas, (64, 36));
    }

    #[test]
    fn thumb_text_slide_renders_non_blank() {
        let s = settings();
        let (key, job) = thumb(&text("Amazing grace"), "song", &[], None, &s, &[], (320, 180));
        let store = store();
        // On a worker thread, as the grid does.
        let px = std::thread::spawn(move || compose(&job, key.size, &TextRenderer::new(), &store)).join().unwrap();
        assert_eq!((px.width(), px.height()), (320, 180));
        let lit = px.as_slice().iter().filter(|p| (p.r, p.g, p.b) != (0, 0, 0)).count();
        assert!(lit > 0, "the text drew something over the black background");
        assert!(px.as_slice().iter().all(|p| p.a == 255), "a thumbnail is opaque");
    }

    fn slide(content: SlideContent) -> Slide { Slide::new("x", content) }

    fn video_with_a_saved_frame(store: &Store) -> MediaFile {
        RgbaImage::from_pixel(16, 9, Rgba([0, 255, 0, 255])).save(store.asset_path("frame.png")).unwrap();
        MediaFile { id: uuid::Uuid::new_v4(), name: "Loop".into(), kind: crate::model::MediaKind::Video, asset: "v.mp4".into(), status: crate::model::MediaStatus::Ready, original_asset: None, duration: None, thumb: Some("frame.png".into()), size: None, folder: String::new() }
    }

    #[test]
    fn video_slide_thumb_uses_media_thumb() {
        let store = store();
        let media = [video_with_a_saved_frame(&store)];
        let video = slide(SlideContent::Video { asset: "v.mp4".into(), autoplay: true, end: Default::default() });
        let (key, job) = thumb(&video, "video", &[], None, &settings(), &media, (64, 36));
        assert_eq!(job.media, Some(ThumbMedia::Video { thumb: Some("frame.png".into()) }));
        let px = compose(&job, key.size, &TextRenderer::new(), &store);
        assert_eq!(px.as_slice()[18 * 64 + 32].g, 255, "the saved frame, not black");
        let (without, _) = thumb(&video, "video", &[], None, &settings(), &[], (64, 36));
        assert_ne!(key, without, "a frame arriving draws the thumbnail again");
        let other = slide(SlideContent::Video { asset: "other.mp4".into(), autoplay: true, end: Default::default() });
        assert_eq!(thumb(&other, "video", &[], None, &settings(), &media, (64, 36)).1.media, Some(ThumbMedia::Video { thumb: None }), "only its own frame");
    }

    #[test]
    fn video_background_thumb_uses_media_thumb() {
        let store = store();
        let media = [video_with_a_saved_frame(&store)];
        let back = Background::Video { asset: "v.mp4".into(), dim: 0 };
        let (key, job) = thumb(&text("Amazing grace"), "song", &[], Some(&back), &settings(), &media, (64, 36));
        assert_eq!(job.background, Some(Background::Image { asset: "frame.png".into(), dim: 0 }));
        assert_eq!(compose(&job, key.size, &TextRenderer::new(), &store).as_slice()[0].g, 255);
        let mut s = settings();
        s.background = back.clone();
        assert_eq!(thumb(&text("a"), "song", &[], None, &s, &media, (64, 36)).1.background, Some(Background::Image { asset: "frame.png".into(), dim: 0 }), "the settings' background too");
        assert_eq!(thumb(&text("a"), "song", &[], Some(&back), &settings(), &[], (64, 36)).1.background, Some(back), "no frame yet: black, as before");
    }

    #[test]
    fn colour_background_and_contained_picture() {
        let store = store();
        // A 4×2 white picture on a blue background: contained, so blue bands above and below.
        let pic = RgbaImage::from_pixel(4, 2, Rgba([255, 255, 255, 255]));
        pic.save(store.asset_path("p.png")).unwrap();
        let job = ThumbJob { background: Some(Background::Color { color: "#0000ff".into() }), media: Some(ThumbMedia::Still { asset: "p.png".into() }), picture: None, text: None };
        let px = compose(&job, (64, 36), &TextRenderer::new(), &store);
        let at = |x: usize, y: usize| { let p = px.as_slice()[y * 64 + x]; (p.r, p.g, p.b) };
        assert_eq!(at(32, 1), (0, 0, 255), "background above the picture");
        assert_eq!(at(32, 18), (255, 255, 255), "the picture in the middle");
        // A video covers everything with black; a missing picture draws nothing.
        let video = ThumbJob { media: Some(ThumbMedia::Video { thumb: None }), ..job.clone() };
        let px = compose(&video, (64, 36), &TextRenderer::new(), &store);
        assert!(px.as_slice().iter().all(|p| (p.r, p.g, p.b) == (0, 0, 0)));
        let missing = ThumbJob { media: Some(ThumbMedia::Still { asset: "gone.png".into() }), ..job };
        let px = compose(&missing, (64, 36), &TextRenderer::new(), &store);
        assert_eq!(px.as_slice()[18 * 64 + 32].b, 255, "only the background");
    }

    #[test]
    fn inline_request_draws_once_then_serves_from_cache() {
        crate::ui::ui_test_init();
        let store = store();
        let thumbs = Thumbs::inline(store.root.clone());
        let (key, job) = thumb(&text("Amazing grace"), "song", &[], None, &settings(), &[], (64, 36));
        let got: Rc<RefCell<Vec<Pixels>>> = Rc::default();
        for _ in 0..2 { let g = got.clone(); thumbs.request(key.clone(), job.clone(), move |px| g.borrow_mut().push(px)); }
        let got = got.borrow();
        assert_eq!(got.len(), 2, "done ran before request returned");
        assert_eq!(got[0].as_bytes().as_ptr(), got[1].as_bytes().as_ptr(), "the second came from the cache");
    }
}
