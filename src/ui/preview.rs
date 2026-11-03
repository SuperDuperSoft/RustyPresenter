//! The operator's live monitor (`renderLive()`, `operator.js:212-252`): the live frame drawn small, off the UI
//! thread, with the room's layer rules (`ui/room/prepare.rs`): the background colour or picture (hidden under a
//! foreground video), a still, the slide from a `Renderer`, the logo. Video is never played a second time: while
//! the frame has one, the operator asks for the room player's current frame 4 times a second (`FrameGrabber`).
//! Like `Thumbs`, the work runs on worker threads and `done` runs on the UI thread, so it needs no `Send` bound;
//! an inline mode does it all at once for tests.
use super::{block_on, room::prepare::{self, BLACK, Px}, thumbs::TextRenderer};
use crate::{
    live::{Frame, MediaLayer, SlideLayer},
    model::{Background, Theme},
    render::{RenderKey, Renderer},
    room::{self, Kind},
    store::Store,
    video::FrameGrabber,
};
use slint::{Color, Rgb8Pixel, SharedPixelBuffer};
use std::{cell::{Cell, RefCell}, path::PathBuf, rc::Rc, sync::mpsc};
use tokio::sync::oneshot;

/// The monitor's text renderer keeps little: it draws one slide at a time.
const RENDER_CAP: usize = 8 * 1024 * 1024;

/// The monitor's layers, bottom to top (as `RoomWindow`).
pub const LAYERS: [Kind; 4] = [Kind::Background, Kind::Media, Kind::Slide, Kind::Logo];

pub type VideoPixels = SharedPixelBuffer<Rgb8Pixel>;

/// What one layer shows, from the live frame (the room's `prepare::source`, without players).
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// Not shown: the layer is empty, or it is a video, which is drawn under the layers.
    Hidden,
    Fill(String),
    /// A picture on black; `cover` fills the screen (an image background), `scale` is its share (the logo's 70 %).
    Picture { asset: String, cover: bool, scale: f32 },
    Text(RenderKey),
}

/// One layer as drawn: a picture over a fill, or hidden.
pub enum Drawn { Pic(Option<Px>, Color), Hidden }

/// What each layer of `frame` shows at `size` (`prepare::source` in the room).
pub fn sources(frame: &Frame, theme: &Theme, logo: Option<&str>, size: (u32, u32)) -> [(Kind, Source); 4] {
    let picture = |asset: &str, cover, scale| Source::Picture { asset: asset.into(), cover, scale };
    let bg = match frame.background.as_ref().map(|b| &b.v) {
        Some(Background::Color { color }) => Source::Fill(color.clone()),
        Some(Background::Image { asset }) => picture(asset, true, 1.0),
        Some(Background::Video { .. }) | None => Source::Hidden,
    };
    let media = match frame.media.as_ref().map(|m| &m.v) {
        Some(MediaLayer::Still { asset }) => picture(asset, false, 1.0),
        Some(MediaLayer::Video { .. }) | None => Source::Hidden,
    };
    let slide = match &frame.slide {
        Some(SlideLayer::Text { text, footer, reference, scripture }) => Source::Text(RenderKey {
            text: text.clone(), footer: footer.clone(), reference: reference.clone(), scripture: *scripture, theme: theme.clone(), canvas: size,
        }),
        Some(SlideLayer::Image { asset }) => picture(asset, false, 1.0),
        None => Source::Hidden,
    };
    // The logo sits on black at up to 70 % of the screen; without a logo file it is plain black.
    let logo = match (frame.logo, logo) {
        (false, _) => Source::Hidden,
        (true, Some(a)) => picture(a, false, 0.7),
        (true, None) => Source::Fill("#000000".into()),
    };
    [(Kind::Background, bg), (Kind::Media, media), (Kind::Slide, slide), (Kind::Logo, logo)]
}

/// The playback instance of the video the room draws: the foreground video, else the background video. The flag is
/// true for the background video, which the room crops to fill the screen (`panscan=1.0`) rather than letterboxes.
pub fn drawn_video(frame: &Frame) -> Option<(u64, bool)> {
    let fg = frame.media.as_ref().filter(|m| matches!(m.v, MediaLayer::Video { .. })).map(|m| (m.inst, false));
    fg.or_else(|| frame.background.as_ref().filter(|b| matches!(b.v, Background::Video { .. })).map(|b| (b.inst, true)))
}

/// The monitor's `bg-visible`: false while a foreground video is live, as in the room (spec §5).
pub fn background_visible(frame: &Frame) -> bool { room::draw_background(frame) }

fn draw(source: &Source, size: (u32, u32), renderer: &TextRenderer, store: &Store) -> Drawn {
    match source {
        Source::Hidden => Drawn::Hidden,
        Source::Fill(c) => Drawn::Pic(None, prepare::color(c)),
        // A picture that cannot be read leaves black, like the room's layer that failed (it keeps the old one).
        Source::Picture { asset, cover, scale } => {
            let canvas = (((size.0 as f32) * scale) as u32, ((size.1 as f32) * scale) as u32);
            Drawn::Pic(prepare::picture(&store.asset_path(asset), canvas, *cover).ok(), BLACK)
        }
        Source::Text(key) => {
            let r = block_on(renderer.get_or_init(|| Renderer::with_cap(RENDER_CAP)).render(key.clone()));
            Drawn::Pic(Some(Px::Premul(r.rgba_premul.buffer())), Color::default())
        }
    }
}

type Layers = Vec<(Kind, Drawn)>;

struct Job { layers: Vec<(Kind, Source)>, size: (u32, u32), reply: oneshot::Sender<Layers> }

struct Grab { grabber: FrameGrabber, max_w: u32, reply: oneshot::Sender<Option<VideoPixels>> }

/// Draws the live monitor's layers and fetches video frames. Lives on the UI thread.
pub struct Preview {
    store: Store,
    /// Inline mode's renderer (the worker has its own).
    renderer: TextRenderer,
    /// What each layer was last asked to show, so only a changed layer is drawn again.
    shown: RefCell<[Option<Source>; 4]>,
    size: Cell<(u32, u32)>,
    /// `None` in inline mode (tests), which draws at once on the calling thread.
    jobs: Option<mpsc::Sender<Job>>,
    grabs: Option<mpsc::Sender<Grab>>,
    /// A grab is on its way: the next tick is skipped.
    grabbing: Rc<Cell<bool>>,
}

impl Preview {
    /// Layers drawn on one worker thread, frames grabbed on another (a grab may wait up to 500 ms).
    pub fn new(store_root: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let store = Store { root: store_root.clone() };
        let jobs = std::thread::Builder::new().name("live-preview".into()).spawn(move || {
            let renderer = TextRenderer::new();
            for j in rx {
                let drawn = j.layers.iter().map(|(k, s)| (*k, draw(s, j.size, &renderer, &store))).collect();
                let _ = j.reply.send(drawn);
            }
        }).ok().map(|_| tx);
        let (tx, rx) = mpsc::channel::<Grab>();
        let grabs = std::thread::Builder::new().name("live-preview-grab".into()).spawn(move || {
            for g in rx { let _ = g.reply.send(g.grabber.grab(g.max_w)); }
        }).ok().map(|_| tx);
        Self { store: Store { root: store_root }, renderer: TextRenderer::new(), shown: RefCell::default(), size: Cell::default(), jobs, grabs, grabbing: Rc::default() }
    }

    /// Everything at once on the calling thread, for tests on the testing backend (no event loop).
    pub fn inline(store_root: PathBuf) -> Self {
        Self { store: Store { root: store_root }, renderer: TextRenderer::new(), shown: RefCell::default(), size: Cell::default(), jobs: None, grabs: None, grabbing: Rc::default() }
    }

    /// The size the layers are drawn at. A new size draws every layer again at the next `show`.
    pub fn set_size(&self, size: (u32, u32)) -> bool {
        if self.size.replace(size) == size { return false }
        *self.shown.borrow_mut() = Default::default();
        true
    }

    pub fn size(&self) -> (u32, u32) { self.size.get() }

    /// Draws the layers of `frame` that changed since the last call and hands them to `done` on the UI thread,
    /// in order. Nothing changed: `done` is not called.
    pub fn show(&self, frame: &Frame, theme: &Theme, logo: Option<&str>, done: impl FnOnce(Layers) + 'static) {
        let size = self.size.get();
        let changed: Vec<(Kind, Source)> = {
            let mut shown = self.shown.borrow_mut();
            let mut changed = Vec::new();
            for (i, (k, s)) in sources(frame, theme, logo, size).into_iter().enumerate() {
                if shown[i].as_ref() == Some(&s) { continue }
                shown[i] = Some(s.clone());
                changed.push((k, s));
            }
            changed
        };
        if changed.is_empty() { return }
        let Some(jobs) = &self.jobs else {
            return done(changed.iter().map(|(k, s)| (*k, draw(s, size, &self.renderer, &self.store))).collect());
        };
        let (reply, rx) = oneshot::channel();
        let delivered = slint::spawn_local(async move { if let Ok(layers) = rx.await { done(layers) } });
        if delivered.is_err() { return }
        let _ = jobs.send(Job { layers: changed, size, reply });
    }

    /// Fetches the current frame of `grabber`, at most `max_w` wide, and hands it to `done` on the UI thread
    /// (`None`: no frame). False, and `done` dropped, while the previous grab is still on its way.
    pub fn grab(&self, grabber: FrameGrabber, max_w: u32, done: impl FnOnce(Option<VideoPixels>) + 'static) -> bool {
        if self.grabbing.get() { return false }
        let Some(grabs) = &self.grabs else { done(grabber.grab(max_w)); return true };
        let (reply, rx) = oneshot::channel();
        let busy = self.grabbing.clone();
        let delivered = slint::spawn_local(async move {
            let px = rx.await.ok().flatten();
            busy.set(false);
            done(px)
        });
        if delivered.is_err() { return false }
        self.grabbing.set(true);
        if grabs.send(Grab { grabber, max_w, reply }).is_err() { self.grabbing.set(false) }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::Inst;
    use crate::model::EndAction;

    fn frame() -> Frame {
        Frame {
            background: Some(Inst { inst: 1, v: Background::Color { color: "#112233".into() } }),
            slide: Some(SlideLayer::Text { text: "Amazing grace".into(), footer: None, reference: None, scripture: false }),
            ..Default::default()
        }
    }

    fn video(inst: u64) -> Option<Inst<MediaLayer>> { Some(Inst { inst, v: MediaLayer::Video { asset: "v.mp4".into(), end: EndAction::Stop, paused: false } }) }

    #[test]
    fn preview_hides_background_under_foreground_video() {
        let mut f = frame();
        assert!(background_visible(&f));
        assert_eq!(drawn_video(&f), None);
        f.media = video(5);
        assert!(!background_visible(&f), "a foreground video covers the background, as in the room");
        assert_eq!(drawn_video(&f), Some((5, false)));
        let s = sources(&f, &Theme::default(), None, (448, 252));
        assert_eq!(s[1], (Kind::Media, Source::Hidden), "the video is drawn under the layers, not as one");
        assert!(matches!(&s[2], (Kind::Slide, Source::Text(k)) if k.canvas == (448, 252) && k.text == "Amazing grace"));
        // A background video is the drawn one while no foreground video plays, and the background stays visible.
        let bgv = Frame { background: Some(Inst { inst: 9, v: Background::Video { asset: "bg.mp4".into() } }), ..Default::default() };
        assert_eq!((drawn_video(&bgv), background_visible(&bgv)), (Some((9, true)), true));
        assert_eq!(sources(&bgv, &Theme::default(), None, (64, 36))[0], (Kind::Background, Source::Hidden));
    }

    #[test]
    fn logo_layer_only_while_logo_is_on() {
        let mut f = Frame::default();
        assert_eq!(sources(&f, &Theme::default(), Some("logo.png"), (64, 36))[3].1, Source::Hidden);
        f.logo = true;
        assert_eq!(sources(&f, &Theme::default(), Some("logo.png"), (64, 36))[3].1, Source::Picture { asset: "logo.png".into(), cover: false, scale: 0.7 });
        assert_eq!(sources(&f, &Theme::default(), None, (64, 36))[3].1, Source::Fill("#000000".into()));
    }

    #[test]
    fn only_changed_layers_are_drawn_again() {
        let app = crate::app::test_app();
        let p = Preview::inline(app.store.root.clone());
        p.set_size((320, 180));
        let got: Rc<RefCell<Vec<Kind>>> = Rc::default();
        let record = |got: &Rc<RefCell<Vec<Kind>>>| { let g = got.clone(); move |l: Layers| g.borrow_mut().extend(l.into_iter().map(|(k, _)| k)) };
        let mut f = frame();
        p.show(&f, &Theme::default(), None, record(&got));
        assert_eq!(*got.borrow(), LAYERS);
        got.borrow_mut().clear();
        f.logo = true;
        p.show(&f, &Theme::default(), None, record(&got));
        assert_eq!(*got.borrow(), [Kind::Logo]);
        got.borrow_mut().clear();
        p.show(&f, &Theme::default(), None, record(&got));
        assert!(got.borrow().is_empty(), "nothing changed, nothing drawn");
        assert!(p.set_size((448, 252)));
        p.show(&f, &Theme::default(), None, record(&got));
        assert_eq!(got.borrow().len(), 4, "a new size draws every layer");
    }

    #[test]
    fn text_layer_renders_at_preview_size() {
        let app = crate::app::test_app();
        let p = Preview::inline(app.store.root.clone());
        p.set_size((320, 180));
        let size: Rc<Cell<(u32, u32)>> = Rc::default();
        let s = size.clone();
        p.show(&frame(), &Theme::default(), None, move |layers| for (k, d) in layers {
            if let (Kind::Slide, Drawn::Pic(Some(Px::Premul(px)), _)) = (k, d) { s.set((px.width(), px.height())) }
        });
        assert_eq!(size.get(), (320, 180));
    }
}
