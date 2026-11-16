//! Putting the stream's picture together in the app (1.3b): the room screen, the camera, the words over the camera
//! and the scene cards, all RGBA pictures of the stream's size, into the one picture FFmpeg gets. Pure.
use super::scene::Scene;
use std::time::Duration;

/// How long a scene change fades.
pub const FADE: Duration = Duration::from_millis(500);

/// Which corner the small camera goes in (Screen + camera).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corner { TopLeft, TopRight, BottomLeft, #[default] BottomRight }

/// How big the small camera is: a fifth, a quarter or a third as wide as the stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CornerSize { Small, #[default] Medium, Large }

/// The church's arrangement of the built-in scenes (Settings → Media → Livestream).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Layout {
    pub corner: Corner,
    pub corner_size: CornerSize,
    /// The words over the camera at the top instead of the bottom.
    pub words_top: bool,
}

impl Layout {
    pub fn is_default(&self) -> bool { *self == Layout::default() }
}

/// The pictures a scene is made of, each RGBA of `size`. `camera` is opaque (its own FFmpeg kept its shape with
/// black bars); `words` and `card` are premultiplied and clear where they show nothing. A picture of the wrong
/// length counts as missing.
pub struct Inputs<'a> {
    pub size: (u32, u32),
    pub room: &'a [u8],
    pub camera: Option<&'a [u8]>,
    pub words: Option<&'a [u8]>,
    pub card: Option<&'a [u8]>,
}

impl Inputs<'_> {
    fn bytes(&self) -> usize { self.size.0 as usize * self.size.1 as usize * 4 }
    fn ok<'b>(&self, p: Option<&'b [u8]>) -> Option<&'b [u8]> { p.filter(|p| p.len() == self.bytes()) }
}

/// Draws `scene` into `out` (RGBA of `i.size`).
pub fn compose(scene: Scene, i: &Inputs, layout: &Layout, out: &mut [u8]) {
    if out.len() != i.bytes() { return }
    let room = i.ok(Some(i.room));
    let camera = i.ok(i.camera);
    let fill_room = |out: &mut [u8]| match room { Some(r) => out.copy_from_slice(r), None => black(out) };
    match scene {
        Scene::Camera | Scene::CameraWords => {
            match camera { Some(c) => out.copy_from_slice(c), None => fill_room(out) }
            if scene == Scene::CameraWords && let Some(w) = i.ok(i.words) { over(out, w) }
        }
        Scene::Screen => fill_room(out),
        Scene::ScreenCamera => {
            fill_room(out);
            if let Some(c) = camera { corner(c, i.size, layout, out) }
        }
        Scene::StartingSoon | Scene::Ending | Scene::Hold => {
            black(out);
            if let Some(c) = i.ok(i.card) { over(out, c) }
        }
    }
}

fn black(out: &mut [u8]) { out.chunks_exact_mut(4).for_each(|p| p.copy_from_slice(&[0, 0, 0, 255])) }

/// Where the small picture goes in a `w`×`h` frame: `(x, y, width, height)`, in `layout`'s corner and size, `w / 40`
/// from the edges.
pub fn corner_rect((w, h): (u32, u32), layout: &Layout) -> (u32, u32, u32, u32) {
    let part = match layout.corner_size { CornerSize::Small => 5, CornerSize::Medium => 4, CornerSize::Large => 3 };
    let cw = ((w / part) & !1).max(2);
    let ch = (((u64::from(cw) * u64::from(h) + u64::from(w) / 2) / u64::from(w.max(1))) as u32 & !1).max(2);
    let edge = w / 40;
    let (left, top) = match layout.corner { Corner::TopLeft => (true, true), Corner::TopRight => (false, true), Corner::BottomLeft => (true, false), Corner::BottomRight => (false, false) };
    let x = if left { edge } else { w.saturating_sub(cw + edge) };
    let y = if top { edge } else { h.saturating_sub(ch + edge) };
    (x, y, cw, ch)
}

/// `src` (the whole `size` picture) shrunk into the corner of `out`, nearest pixel.
fn corner(src: &[u8], (w, h): (u32, u32), layout: &Layout, out: &mut [u8]) {
    let (x0, y0, cw, ch) = corner_rect((w, h), layout);
    if x0 + cw > w || y0 + ch > h { return }
    let sx: Vec<usize> = (0..cw).map(|x| (u64::from(x) * u64::from(w) / u64::from(cw)) as usize * 4).collect();
    for dy in 0..ch {
        let sy = (u64::from(dy) * u64::from(h) / u64::from(ch)) as usize;
        let row = &src[sy * w as usize * 4..(sy + 1) * w as usize * 4];
        let d0 = ((y0 + dy) as usize * w as usize + x0 as usize) * 4;
        for (px, &o) in out[d0..d0 + cw as usize * 4].chunks_exact_mut(4).zip(&sx) { px.copy_from_slice(&row[o..o + 4]) }
    }
}

/// Lays the premultiplied `top` over `base` (both RGBA of the same size). Clear pixels are skipped.
pub fn over(base: &mut [u8], top: &[u8]) {
    for (b, t) in base.chunks_exact_mut(4).zip(top.chunks_exact(4)) {
        let a = u32::from(t[3]);
        if a == 0 { continue }
        if a == 255 { b.copy_from_slice(t); continue }
        for c in 0..3 { b[c] = (u32::from(t[c]) + (u32::from(b[c]) * (255 - a) + 127) / 255).min(255) as u8 }
        b[3] = 255;
    }
}

/// `from` faded `t` (0 to 1) of the way to `to`, into `out`.
pub fn blend(from: &[u8], to: &[u8], t: f32, out: &mut [u8]) {
    let k = (t.clamp(0.0, 1.0) * 256.0) as u32;
    for ((o, f), n) in out.chunks_exact_mut(4).zip(from.chunks_exact(4)).zip(to.chunks_exact(4)) {
        for c in 0..3 { o[c] = ((u32::from(f[c]) * (256 - k) + u32::from(n[c]) * k) / 256) as u8 }
        o[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_corner_camera_goes_where_the_church_put_it() {
        let size = (1280, 720);
        let at = |corner, big| corner_rect(size, &Layout { corner, corner_size: big, ..Layout::default() });
        // As before: bottom right, a quarter as wide.
        assert_eq!(at(Corner::BottomRight, CornerSize::Medium), corner_rect(size, &Layout::default()));
        assert_eq!(at(Corner::BottomRight, CornerSize::Medium), (1280 - 320 - 32, 720 - 180 - 32, 320, 180));
        assert_eq!(at(Corner::TopLeft, CornerSize::Medium), (32, 32, 320, 180));
        assert_eq!(at(Corner::TopRight, CornerSize::Small), (1280 - 256 - 32, 32, 256, 144));
        assert_eq!(at(Corner::BottomLeft, CornerSize::Large), (32, 720 - 240 - 32, 426, 240));
    }

    #[test]
    fn screen_and_camera_puts_the_camera_in_the_chosen_corner() {
        let size = (40u32, 40u32);
        let n = 40 * 40 * 4;
        let room = vec![255u8; n];
        let camera: Vec<u8> = [0, 0, 255, 255].repeat(40 * 40);
        let mut out = vec![0u8; n];
        let i = Inputs { size, room: &room, camera: Some(&camera), words: None, card: None };
        compose(Scene::ScreenCamera, &i, &Layout { corner: Corner::TopLeft, ..Layout::default() }, &mut out);
        let px = |x: usize, y: usize| out[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4].to_vec();
        assert_eq!(px(2, 2), [0, 0, 255, 255], "the camera top left");
        assert_eq!(px(37, 37), [255, 255, 255, 255], "the room bottom right");
    }

    const W: u32 = 40;
    const H: u32 = 20;
    const RED: [u8; 4] = [200, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 200, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    fn solid(c: [u8; 4]) -> Vec<u8> { c.repeat((W * H) as usize) }
    fn px(img: &[u8], x: u32, y: u32) -> [u8; 4] { let i = ((y * W + x) * 4) as usize; img[i..i + 4].try_into().unwrap() }
    fn run(scene: Scene, room: &[u8], camera: Option<&[u8]>, words: Option<&[u8]>, card: Option<&[u8]>) -> Vec<u8> {
        let mut out = vec![7u8; (W * H * 4) as usize];
        compose(scene, &Inputs { size: (W, H), room, camera, words, card }, &Layout::default(), &mut out);
        out
    }
    /// Clear everywhere but one opaque white row.
    fn words_row(y: u32) -> Vec<u8> {
        let mut w = vec![0u8; (W * H * 4) as usize];
        w[(y * W * 4) as usize..((y + 1) * W * 4) as usize].fill(255);
        w
    }

    #[test]
    fn camera_fills_and_words_go_over_it() {
        let (room, cam, words) = (solid(BLUE), solid(RED), words_row(15));
        let out = run(Scene::CameraWords, &room, Some(&cam), Some(&words), None);
        assert_eq!(px(&out, 3, 15), [255; 4]);
        assert_eq!(px(&out, 3, 2), RED);
        assert_eq!(run(Scene::Camera, &room, Some(&cam), Some(&words), None), solid(RED), "Camera has no words");
    }

    #[test]
    fn a_missing_camera_shows_the_room() {
        let (room, words) = (solid(BLUE), words_row(15));
        let out = run(Scene::CameraWords, &room, None, Some(&words), None);
        assert_eq!((px(&out, 3, 2), px(&out, 3, 15)), (BLUE, [255; 4]));
        assert_eq!(run(Scene::Screen, &room, Some(&solid(RED)), Some(&words), None), room, "Full screen is the room alone");
    }

    #[test]
    fn the_corner_is_a_quarter_wide_bottom_right() {
        assert_eq!(corner_rect((1920, 1080), &Layout::default()), (1392, 762, 480, 270));
        let (room, cam) = (solid(BLUE), solid(RED));
        let out = run(Scene::ScreenCamera, &room, Some(&cam), None, None);
        let (x, y, cw, ch) = corner_rect((W, H), &Layout::default());
        assert!(cw > 0 && ch > 0);
        assert_eq!(px(&out, x + cw / 2, y + ch / 2), RED);
        assert_eq!(px(&out, 0, 0), BLUE);
    }

    #[test]
    fn cards_go_over_black() {
        let room = solid(BLUE);
        assert_eq!(run(Scene::Ending, &room, None, None, None), solid(BLACK));
        let card = words_row(5);
        let out = run(Scene::Hold, &room, None, None, Some(&card));
        assert_eq!((px(&out, 1, 5), px(&out, 1, 6)), ([255; 4], BLACK));
    }

    #[test]
    fn inputs_of_the_wrong_size_are_left_out() {
        let (room, short) = (solid(BLUE), vec![9u8; (W * H * 4 - 3) as usize]);
        assert_eq!(run(Scene::Camera, &room, Some(&short), Some(&short), None), room);
    }

    #[test]
    fn half_way_is_half_of_each() {
        let mut out = vec![0u8; 4];
        blend(&[200, 0, 0, 255], &[0, 100, 0, 255], 0.5, &mut out);
        assert_eq!(out, [100, 50, 0, 255]);
    }
}
