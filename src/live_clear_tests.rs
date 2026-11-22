//! N18: `Command::ClearLayers` (F7, F8 and clear groups).
use super::*;
use crate::theme::overlay::{OverlaySource, Placement};
use std::collections::HashMap;

struct P { order: Vec<Uuid>, items: HashMap<Uuid, LiveItem> }

impl Plan for P {
    fn order(&self) -> Vec<Uuid> { self.order.clone() }
    fn build(&self, id: Uuid) -> Option<LiveItem> { self.items.get(&id).cloned() }
}

fn plan() -> P {
    let id = Uuid::new_v4();
    let slide = Slide::new("a", SlideContent::Text { text: "a".into(), footer: None, reference: None, size: 0, parallel: None });
    let item = LiveItem { item_id: id, title: "A".into(), kind: "song".into(), slides: vec![slide], background: Background::Color { color: "#000".into() }, theme: Theme::default() };
    P { order: vec![id], items: HashMap::from([(id, item)]) }
}

fn slot(key: &str) -> OverlaySlot {
    OverlaySlot { key: key.into(), z: 0, inst: 0, name: key.into(), source: OverlaySource::Text { text: key.into(), styles: vec![], look: Default::default() }, place: Placement::default() }
}

/// A live screen with a slide, its background, audio, the logo flag and one overlay of each kind.
fn full(p: &P) -> Live {
    let mut l = Live::new();
    assert!(l.apply(Envelope { id: "show".into(), base: None, cmd: Command::Show { item_id: p.order[0], slide: 0 } }, p).ok);
    l.frame.logo = true;
    l.frame.audio = Some(Inst { inst: 99, v: AudioLayer { asset: "m.mp3".into(), title: "M".into(), looped: false, paused: false } });
    l.frame.overlays = vec![slot("saved:1"), slot("timer:00000000-0000-4000-8000-000000000001"), slot("message")];
    l
}

fn clear(l: &mut Live, p: &P, layers: &[ClearLayer]) -> Outcome {
    l.apply(Envelope::new(Command::ClearLayers { layers: layers.to_vec() }), p)
}

fn keys(l: &Live) -> Vec<&str> { l.frame.overlays.iter().map(|s| s.key.as_str()).collect() }

#[test]
fn all_but_the_background_keeps_only_the_background() {
    let p = plan();
    let mut l = full(&p);
    let epoch = l.clear_epoch;
    let o = clear(&mut l, &p, &[ClearLayer::Slide, ClearLayer::Audio, ClearLayer::Logo, ClearLayer::Overlays, ClearLayer::Message, ClearLayer::Timers]);
    assert!(o.ok, "{o:?}");
    assert!(l.frame.slide.is_none() && l.frame.audio.is_none() && !l.frame.logo && l.frame.overlays.is_empty());
    assert!(l.frame.background.is_some(), "the background stays");
    assert_eq!(l.clear_epoch, epoch + 1, "a clear group is a clear");
}

#[test]
fn each_overlay_layer_clears_only_its_own_kind() {
    let p = plan();
    let mut l = full(&p);
    clear(&mut l, &p, &[ClearLayer::Message]);
    assert_eq!(keys(&l), ["saved:1", "timer:00000000-0000-4000-8000-000000000001"]);
    let mut l = full(&p);
    clear(&mut l, &p, &[ClearLayer::Timers]);
    assert_eq!(keys(&l), ["saved:1", "message"]);
    let mut l = full(&p);
    clear(&mut l, &p, &[ClearLayer::Overlays]);
    assert_eq!(keys(&l), ["timer:00000000-0000-4000-8000-000000000001", "message"]);
    assert!(l.frame.slide.is_some() && l.frame.logo, "the slide and logo stay");
}

#[test]
fn media_takes_the_background_and_video_only() {
    let p = plan();
    let mut l = full(&p);
    clear(&mut l, &p, &[ClearLayer::Media]);
    assert!(l.frame.background.is_none() && l.frame.media.is_none());
    assert!(l.frame.slide.is_some() && l.frame.audio.is_some() && l.frame.overlays.len() == 3);
}

#[test]
fn an_overlay_only_clear_with_nothing_up_keeps_the_version() {
    let p = plan();
    let mut l = Live::new();
    let v = l.version;
    assert!(clear(&mut l, &p, &[ClearLayer::Message, ClearLayer::Timers]).ok);
    assert_eq!(l.version, v);
    assert!(clear(&mut l, &p, &[ClearLayer::Slide]).ok);
    assert_eq!(l.version, v + 1, "a screen layer always makes a new version, like F2");
}

#[test]
fn clear_layers_reads_and_writes_as_json() {
    let env: Envelope = serde_json::from_str(r#"{"id":"x","cmd":"clear_layers","layers":["slide","timers"]}"#).unwrap();
    assert_eq!(env.cmd, Command::ClearLayers { layers: vec![ClearLayer::Slide, ClearLayer::Timers] });
}

#[test]
fn clear_groups_take_the_phone_drawing_too() {
    let scribble = |l: &mut Live| {
        let c = drawing::DrawCmd::Start { stroke: 1, color: drawing::PALETTE[0].into(), width: 12.0, pen: drawing::Pen::Pen, points: vec![[0.2, 0.3], [0.6, 0.3]], ink: Default::default() };
        std::sync::Arc::make_mut(&mut l.drawing).apply(Uuid::new_v4(), &c).unwrap();
    };
    let p = plan();
    let mut l = full(&p);
    scribble(&mut l);
    let v = l.version;
    assert!(clear(&mut l, &p, &[ClearLayer::Drawing]).ok);
    assert!(l.drawing.is_empty() && l.frame.slide.is_some(), "only the drawing went");
    assert!(l.version > v, "the screens hear of it");
    scribble(&mut l);
    assert!(clear(&mut l, &p, &[ClearLayer::Slide]).ok);
    assert!(l.drawing.is_empty(), "the text takes its drawing with it, as F2 does");
    let mut l = full(&p);
    scribble(&mut l);
    assert!(clear(&mut l, &p, &crate::shortcuts::ClearGroup::builtin().layers).ok);
    assert!(l.drawing.is_empty(), "F9 clears drawings too");
}
