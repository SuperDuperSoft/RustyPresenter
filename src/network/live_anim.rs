//! The live designed slide's click group for the phone (1.2): the picture under it (the slide with its objects left
//! out), one picture per layer of `Play::layers`, and each moving layer's frames sampled from `Play::pose`, as the
//! room plays them. The phone plays them with Web Animations (`web/remote.js`, `playAnim`). Made once per live
//! version and width, and kept until the next.
use crate::{app::App, design::Play, live::SlideLayer, model::SlideContent, ui::room::anim::{self, SpriteImg}};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// One group, cut for the phone: its description and pictures (0: the picture under it as a JPEG, then each
/// layer as a PNG).
pub struct PhoneAnim { pub version: u64, pub width: u32, pub json: Value, pub pics: Vec<(Vec<u8>, &'static str)> }

static LAST: Mutex<Option<Arc<PhoneAnim>>> = Mutex::new(None);

/// The group made last, when it is for `version` at `width`.
pub fn cached(version: u64, width: u32) -> Option<Arc<PhoneAnim>> {
    LAST.lock().unwrap().clone().filter(|a| a.version == version && a.width == width)
}

/// Frames per group: about one each 60 ms, at least 2, at most 30.
fn frames(dur_ms: u32) -> usize { (dur_ms / 60).clamp(2, 30) as usize }

fn r3(v: f32) -> f32 { (v * 1000.0).round() / 1000.0 }

/// What the phone plays: each layer's place (% of the picture) and, for a moving one, its frames
/// `[offset, opacity, x %, y % (of its own size), turn, scale]` and its clip `[offset, top, right, bottom, left]` in
/// % of the picture (when it is cut). `lead`: the slide's transition first (ms), the pictures coming in over it.
pub fn describe(play: &Play, imgs: &[SpriteImg], canvas: (u32, u32), lead_ms: u32) -> Value {
    let area = anim::area(canvas);
    let dur = play.duration_ms();
    let n = frames(dur);
    let layers: Vec<Value> = imgs.iter().enumerate().map(|(k, img)| {
        let place = [r3(img.x * 100.0), r3(img.y * 100.0), r3(img.w * 100.0), r3(img.h * 100.0)];
        if img.obj.is_none() { return json!({ "i": k + 1, "at": place }) }
        let at = |f: usize| f as f32 / n as f32;
        let sprites: Vec<_> = (0..=n).map(|f| anim::sprite_at(img, play, at(f) * dur as f32, area)).collect();
        let s0 = &sprites[0];
        let f: Vec<[f32; 6]> = sprites.iter().enumerate().map(|(i, s)| {
            let (tx, ty) = if img.w > 0.0 && img.h > 0.0 { (s.dx / img.w * 100.0, s.dy / img.h * 100.0) } else { (0.0, 0.0) };
            [r3(at(i)), r3(s.op), r3(tx), r3(ty), r3(s.rot), r3(s.scale)]
        }).collect();
        let cut = sprites.iter().any(|s| s.cx != 0.0 || s.cy != 0.0 || s.cw != 1.0 || s.ch != 1.0);
        let c: Option<Vec<[f32; 5]>> = cut.then(|| sprites.iter().enumerate().map(|(i, s)| {
            [r3(at(i)), r3(s.cy * 100.0), r3((1.0 - s.cx - s.cw) * 100.0), r3((1.0 - s.cy - s.ch) * 100.0), r3(s.cx * 100.0)]
        }).collect());
        json!({ "i": k + 1, "at": place, "o": [r3(s0.ox * 100.0), r3(s0.oy * 100.0)], "f": f, "c": c })
    }).collect();
    json!({ "ms": dur, "lead": lead_ms, "layers": layers })
}

/// Premultiplied pixels as a PNG with plain alpha.
fn png(px: &slint::SharedPixelBuffer<slint::Rgba8Pixel>) -> Result<Vec<u8>, String> {
    let mut raw = px.as_bytes().to_vec();
    for p in raw.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a > 0 && a < 255 { for c in &mut p[..3] { *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8 } }
    }
    let mut out = Vec::new();
    let enc = image::codecs::png::PngEncoder::new_with_quality(&mut out, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::Adaptive);
    image::ImageEncoder::write_image(enc, &raw, px.width(), px.height(), image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
    Ok(out)
}

/// The live slide's click group for a phone `width` wide (the room screen's shape). None when no group plays.
pub fn phone_anim(app: &App, width: u32) -> Result<Option<Arc<PhoneAnim>>, String> {
    use crate::ui::thumbs;
    let size = crate::server::live_slide_size(width, app.room_canvas());
    let (version, job, play, lead, font) = {
        let data = app.data.lock().unwrap();
        let live = app.live.lock().unwrap();
        if let Some(a) = cached(live.version, size.0) { return Ok(Some(a)) }
        let Some(SlideLayer::Design { design, build, play: Some(play) }) = &live.frame.slide else { return Ok(None) };
        if play.duration_ms() == 0 { return Ok(None) }
        let Some((c, item)) = live.cursor.as_ref().zip(live.item.as_ref()) else { return Ok(None) };
        let Some(slide) = item.slides.get(c.slide) else { return Ok(None) };
        // The picture under the group: the slide with its objects left out (they are all in the layers).
        let mut under = slide.clone();
        if let SlideContent::Design { design: d, .. } = &mut under.content { d.objects.clear() }
        let settings = crate::model::Settings { theme: item.theme.clone(), background: item.background.clone(), ..data.settings.clone() };
        let passage = crate::model::passage(&item.kind, &item.slides);
        let job = thumbs::thumb(&under, &item.kind, &passage, Some(&item.background), &settings, &data.media, size).1;
        let t = design.transition;
        let lead = if *build || matches!(t.kind, crate::design::TransitionKind::Cut) { 0 } else { t.ms.min(crate::design::TRANSITION_MAX_MS) };
        (live.version, job, (**play).clone(), lead, item.theme.font.clone())
    };
    static RENDER: thumbs::TextRenderer = thumbs::TextRenderer::new();
    let base = thumbs::compose(&job, size, &RENDER, &app.store);
    let rgb: Vec<u8> = base.as_bytes().chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85).encode(&rgb, size.0, size.1, image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
    let layers = crate::ui::room::prepare::play_layers(&play, &font, size, &app.store.assets());
    let mut pics = vec![(jpeg, "image/jpeg")];
    for l in &layers { pics.push((png(&l.px)?, "image/png")) }
    let imgs = anim::Layers { canvas: size, layers }.images();
    let json = describe(&play, &imgs, size, lead);
    let made = Arc::new(PhoneAnim { version, width: size.0, json, pics });
    *LAST.lock().unwrap() = Some(made.clone());
    Ok(Some(made))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{Anim, AnimKind, Design, Effect, Obj, ObjKind, PctRect, Shape, Start};

    #[test]
    fn a_group_is_described_in_a_few_frames_per_moving_layer() {
        let still = Obj::new(ObjKind::Shape { shape: Shape::Rect }, PctRect::new(0.0, 0.0, 10.0, 10.0));
        let o = Obj::new(ObjKind::Shape { shape: Shape::Rect }, PctRect::new(50.0, 50.0, 20.0, 20.0));
        let mut wipe = Anim::new(o.id, AnimKind::Entrance, Effect::Wipe, Start::OnClick);
        wipe.dur_ms = 600;
        let play = Design { objects: vec![still, o], anims: vec![wipe], ..Default::default() }.play(1).unwrap();
        let img = |x, obj| SpriteImg { image: slint::Image::default(), x, y: x, w: 0.1, h: 0.1, obj };
        let v = describe(&play, &[img(0.0, None), img(0.5, Some(1))], (1600, 900), 0);
        assert_eq!(v["ms"], 600);
        assert_eq!(v["layers"][0], json!({ "i": 1, "at": [0.0, 0.0, 10.0, 10.0] }), "a still layer just sits");
        let f = v["layers"][1]["f"].as_array().unwrap();
        assert_eq!(f.len(), 11, "600 ms: 10 steps");
        assert_eq!(f[10], json!([1.0, 1.0, 0.0, 0.0, 0.0, 1.0]));
        let c = v["layers"][1]["c"].as_array().unwrap();
        assert_eq!(c[0], json!([0.0, 70.0, 30.0, 30.0, 50.0]), "wiped up from the bottom: all of it cut at first");
        let top = c[9][1].as_f64().unwrap();
        assert!(top > 50.0 && top < 51.0, "nearly all of it shows: {top}");
        assert_eq!(c[10], json!([1.0, 0.0, 0.0, 0.0, 0.0]), "no cut once it is in");
        assert!(v.to_string().len() < 2000, "small: {}", v.to_string().len());
    }
}
