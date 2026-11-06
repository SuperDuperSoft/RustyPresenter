//! Preparing the room's new layers off the UI thread: slides drawn by the shared renderer,
//! pictures decoded at display size, and players created and loaded (paused) for video and audio.
use super::Msg;
use crate::{app::App, live::{Frame, MediaLayer, SlideLayer}, model::{Background, Theme}, render::{RenderKey, Renderer, parse_color}};
use crate::room::{Kind, Want};
use crate::video::{Player, Players, Role};
use slint::{Color, Image, Rgba8Pixel, SharedPixelBuffer};
use crate::ui::block_on;
use std::{path::{Path, PathBuf}, sync::Arc};

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

/// What the prepare threads share with the room.
pub(super) struct Work { pub(super) app: Arc<App>, pub(super) renderer: Arc<Renderer>, pub(super) video: Players, pub(super) audio: Players }

/// One plan's layers to prepare, with what they need from the live view.
pub(super) struct Job { pub(super) token: u64, pub(super) wants: Vec<Want>, pub(super) frame: Frame, pub(super) theme: Theme, pub(super) logo: Option<String>, pub(super) canvas: (u32, u32) }

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
            SlideLayer::Text { text, footer, reference, scripture } => Source::Text(RenderKey {
                text: text.clone(), footer: footer.clone(), reference: reference.clone(), scripture: *scripture, theme: job.theme.clone(), canvas: job.canvas,
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
pub(super) fn prepare(work: &Work, job: Job, send: impl Fn(Msg)) {
    let mut sources: Vec<_> = job.wants.iter().map(|w| (w.kind, source(w.kind, &job, &work.app).unwrap_or(Source::Fill(Color::default())))).collect();
    sources.sort_by_key(|(_, s)| !matches!(s, Source::Player { .. }));
    let token = job.token;
    for (kind, s) in sources {
        let made = match s {
            Source::Fill(c) => Ok(Made::Pic(None, c)),
            Source::Picture { path, cover, scale } => {
                let canvas = ((job.canvas.0 as f32 * scale) as u32, (job.canvas.1 as f32 * scale) as u32);
                // Pictures sit on black, as in the web room; image backgrounds cover the screen.
                picture(&path, canvas, cover).map(|px| Made::Pic(Some(px), BLACK))
            }
            Source::Text(key) => {
                let r = block_on(work.renderer.render(key));
                Ok(Made::Pic(Some(Px::Premul(SharedPixelBuffer::clone_from_slice(&r.rgba_premul, r.width, r.height))), Color::default()))
            }
            Source::Player { role, inst, path } => {
                let players = if matches!(role, Role::Audio { .. }) { &work.audio } else { &work.video };
                let Some(p) = players.create(role, inst) else { continue };
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
