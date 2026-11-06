//! FEAT-1 tests of the live engine: overlay slots, the clear matrix, the clear epoch and failure reports (spec §5.2,
//! §5.3), kept apart from `live.rs`.
use std::collections::HashMap;

use uuid::Uuid;

use crate::live::{Command, Envelope, Live, LiveItem, Plan, RoomEvent};
use crate::model::{AssetId, Background, Slide, SlideContent, Theme};
use crate::theme::overlay::{ImageSource, OverlaySlot, OverlaySource, Overlay, Placement, saved_key};
use crate::theme::strings;

#[derive(Default)]
struct P {
    items: Vec<Uuid>,
    built: HashMap<Uuid, LiveItem>,
    overlays: Vec<Overlay>,
    logo: Option<AssetId>,
}

impl Plan for P {
    fn order(&self) -> Vec<Uuid> { self.items.clone() }
    fn build(&self, id: Uuid) -> Option<LiveItem> { self.built.get(&id).cloned() }
    fn saved_overlay(&self, id: Uuid) -> Option<(u32, Overlay)> {
        self.overlays.iter().position(|o| o.id == id).map(|i| (i as u32, self.overlays[i].clone()))
    }
    fn logo(&self) -> Option<AssetId> { self.logo.clone() }
}

fn sign(name: &str) -> Overlay {
    Overlay { id: Uuid::new_v4(), name: name.into(), source: OverlaySource::Text { text: name.into(), styles: vec![], look: Default::default() }, place: Placement::default() }
}

fn bug() -> Overlay {
    Overlay { id: Uuid::new_v4(), name: "Logo".into(), source: OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 100 }, place: Placement::default() }
}

fn clock() -> Overlay {
    Overlay { id: Uuid::new_v4(), name: "Clock".into(), source: OverlaySource::Clock { look: Default::default(), seconds: false, h24: false }, place: Placement::default() }
}

fn plan_with(overlays: Vec<Overlay>) -> P {
    let id = Uuid::new_v4();
    let slide = |t: &str| Slide { label: String::new(), content: SlideContent::Text { text: t.into(), footer: None, reference: None, size: 0, parallel: None }, notes: String::new() };
    let item = LiveItem { item_id: id, title: "Song".into(), kind: "song".into(), slides: vec![slide("a"), slide("b")],
        background: Background::Color { color: "#000".into() }, theme: Theme { size: 77, ..Theme::default() } };
    P { items: vec![id], built: HashMap::from([(id, item)]), overlays, logo: Some("logo.png".into()) }
}

fn run(l: &mut Live, p: &P, cmd: Command) -> Result<(), String> {
    let o = l.apply(Envelope { id: Uuid::new_v4().to_string(), base: Some(l.version), cmd }, p);
    if o.ok { Ok(()) } else { Err(o.error.unwrap_or_default()) }
}

fn on(l: &mut Live, p: &P, id: Uuid) -> Result<(), String> {
    let seen = Some(l.clear_epoch);
    run(l, p, Command::Overlay { id, on: true, seen_clear: seen })
}

fn keys(l: &Live) -> Vec<String> { l.frame.overlays.iter().map(|o| o.name.clone()).collect() }

#[test]
fn overlay_on_bumps_and_is_idempotent() {
    let p = plan_with(vec![sign("Wi-Fi")]);
    let mut l = Live::new();
    let v = l.version;
    on(&mut l, &p, p.overlays[0].id).unwrap();
    assert_eq!((l.version, keys(&l)), (v + 1, vec!["Wi-Fi".to_string()]));
    on(&mut l, &p, p.overlays[0].id).unwrap();
    assert_eq!(l.version, v + 1, "turning it on again changes nothing");
}

#[test]
fn overlay_on_changed_copy_replaces() {
    let mut p = plan_with(vec![sign("Wi-Fi")]);
    let mut l = Live::new();
    on(&mut l, &p, p.overlays[0].id).unwrap();
    p.overlays[0].name = "Wi-Fi 2".into();
    on(&mut l, &p, p.overlays[0].id).unwrap();
    assert_eq!(keys(&l), ["Wi-Fi 2"]);
}

#[test]
fn overlay_off_when_absent_is_noop() {
    let p = plan_with(vec![sign("A")]);
    let mut l = Live::new();
    let v = l.version;
    run(&mut l, &p, Command::Overlay { id: p.overlays[0].id, on: false, seen_clear: None }).unwrap();
    assert_eq!(l.version, v);
}

#[test]
fn ninth_saved_overlay_is_rejected() {
    let p = plan_with((0..9).map(|i| sign(&format!("s{i}"))).collect());
    let mut l = Live::new();
    for o in &p.overlays[..8] { on(&mut l, &p, o.id).unwrap() }
    assert_eq!(on(&mut l, &p, p.overlays[8].id).unwrap_err(), strings::OVERLAY_CAP);
    on(&mut l, &p, p.overlays[0].id).unwrap();
}

#[test]
fn cap_ignores_non_saved_slots() {
    let p = plan_with((0..8).map(|i| sign(&format!("s{i}"))).collect());
    let mut l = Live::new();
    let timer = OverlaySlot { key: "timer:x".into(), z: 1_000_000, inst: 99, name: "T".into(), source: OverlaySource::Unknown, place: Placement::default() };
    assert!(l.show_overlay(timer));
    for o in &p.overlays { on(&mut l, &p, o.id).unwrap() }
    assert_eq!(l.frame.overlays.len(), 9);
}

#[test]
fn overlays_survive_next_show_and_clears_except_all() {
    let p = plan_with(vec![sign("A"), clock()]);
    let mut l = Live::new();
    for o in &p.overlays { on(&mut l, &p, o.id).unwrap() }
    let item = p.items[0];
    for c in [Command::Show { item_id: item, slide: 0 }, Command::Next, Command::Prev, Command::ClearSlide, Command::ClearMedia, Command::ClearAudio, Command::Logo] {
        run(&mut l, &p, c.clone()).unwrap();
        assert_eq!(keys(&l), ["A", "Clock"], "after {c:?}");
    }
    run(&mut l, &p, Command::ClearAll).unwrap();
    assert!(l.frame.overlays.is_empty());
}

#[test]
fn logo_keeps_text_picture_and_clock_but_drops_unknown() {
    let p = plan_with(vec![sign("A"), bug(), clock()]);
    let mut l = Live::new();
    for o in &p.overlays { on(&mut l, &p, o.id).unwrap() }
    l.show_overlay(OverlaySlot { key: "timer:x".into(), z: 1_000_000, inst: 99, name: "T".into(), source: OverlaySource::Unknown, place: Placement::default() });
    run(&mut l, &p, Command::Logo).unwrap();
    assert_eq!(keys(&l), ["A", "Logo", "Clock"]);
    assert!(l.frame.logo);
}

#[test]
fn clear_overlays_removes_all_and_is_noop_when_none() {
    let p = plan_with(vec![sign("A")]);
    let mut l = Live::new();
    let v = l.version;
    run(&mut l, &p, Command::ClearOverlays).unwrap();
    assert_eq!(l.version, v, "nothing to remove: no new version");
    on(&mut l, &p, p.overlays[0].id).unwrap();
    run(&mut l, &p, Command::ClearOverlays).unwrap();
    assert!(l.frame.overlays.is_empty());
}

#[test]
fn overlay_commands_need_no_base() {
    let p = plan_with(vec![sign("A")]);
    let mut l = Live::new();
    let id = p.overlays[0].id;
    let o = l.apply(Envelope { id: "1".into(), base: None, cmd: Command::Overlay { id, on: true, seen_clear: Some(0) } }, &p);
    assert!(o.ok);
    let o = l.apply(Envelope { id: "2".into(), base: Some(9999), cmd: Command::Overlay { id, on: false, seen_clear: None } }, &p);
    assert!(o.ok, "a stale base does not matter");
}

#[test]
fn overlay_on_with_stale_or_missing_seen_clear_is_refused() {
    let p = plan_with(vec![sign("A")]);
    let mut l = Live::new();
    run(&mut l, &p, Command::ClearAll).unwrap();
    let id = p.overlays[0].id;
    assert_eq!(run(&mut l, &p, Command::Overlay { id, on: true, seen_clear: Some(0) }).unwrap_err(), strings::SCREEN_CLEARED);
    assert_eq!(run(&mut l, &p, Command::Overlay { id, on: true, seen_clear: None }).unwrap_err(), strings::SCREEN_CLEARED);
    on(&mut l, &p, id).unwrap();
}

#[test]
fn overlay_off_and_clear_overlays_ignore_seen_clear() {
    let p = plan_with(vec![sign("A")]);
    let mut l = Live::new();
    on(&mut l, &p, p.overlays[0].id).unwrap();
    run(&mut l, &p, Command::ClearSlide).unwrap();
    run(&mut l, &p, Command::Overlay { id: p.overlays[0].id, on: false, seen_clear: Some(0) }).unwrap();
    run(&mut l, &p, Command::ClearOverlays).unwrap();
}

#[test]
fn every_clear_and_logo_bumps_clear_epoch_even_when_nothing_removed() {
    let p = plan_with(vec![]);
    let mut l = Live::new();
    for (i, c) in [Command::ClearAll, Command::ClearSlide, Command::ClearMedia, Command::ClearAudio, Command::Logo, Command::ClearOverlays].into_iter().enumerate() {
        run(&mut l, &p, c).unwrap();
        assert_eq!(l.clear_epoch, i as u64 + 1);
    }
    run(&mut l, &p, Command::Next).unwrap();
    assert_eq!(l.clear_epoch, 6, "showing is not a clear");
}

#[test]
fn clear_epoch_and_inst_survive_restore() {
    let p = plan_with(vec![sign("A"), sign("B")]);
    let mut l = Live::new();
    on(&mut l, &p, p.overlays[0].id).unwrap();
    run(&mut l, &p, Command::ClearSlide).unwrap();
    on(&mut l, &p, p.overlays[1].id).unwrap();
    let insts: Vec<u64> = l.frame.overlays.iter().map(|o| o.inst).collect();
    assert_eq!(insts[1], insts[0] + 1);
    let v = l.version;
    l.report(RoomEvent::Committed { version: v });
    let saved = crate::recovery::state::snapshot(&l, vec![], 1).unwrap();
    let json = serde_json::to_string(&saved).unwrap();
    let back: crate::recovery::state::LiveState = serde_json::from_str(&json).unwrap();
    let mut fresh = Live::new();
    fresh.restore(&back, true);
    assert_eq!(fresh.clear_epoch, 1);
    assert_eq!(fresh.frame.overlays, l.frame.overlays, "overlays come back");
    fresh.report(RoomEvent::Committed { version: fresh.version });
    run(&mut fresh, &p, Command::ClearOverlays).unwrap();
    on(&mut fresh, &p, p.overlays[0].id).unwrap();
    assert!(fresh.frame.overlays[0].inst > insts[1], "an instance number is never reused");
}

#[test]
fn deleted_overlay_stays_on_screen_until_off() {
    let mut p = plan_with(vec![sign("A")]);
    let mut l = Live::new();
    let id = p.overlays[0].id;
    on(&mut l, &p, id).unwrap();
    p.overlays.clear();
    run(&mut l, &p, Command::Next).unwrap();
    assert_eq!(keys(&l), ["A"]);
    assert_eq!(on(&mut l, &p, id).unwrap_err(), strings::OVERLAY_GONE);
    run(&mut l, &p, Command::Overlay { id, on: false, seen_clear: None }).unwrap();
    assert!(l.frame.overlays.is_empty());
}

#[test]
fn show_overlay_and_hide_overlay_do_not_bump() {
    let mut l = Live::new();
    let v = l.version;
    let slot = OverlaySlot { key: saved_key(Uuid::nil()), z: 0, inst: 1, name: "x".into(), source: OverlaySource::Unknown, place: Placement::default() };
    assert!(l.show_overlay(slot.clone()));
    assert!(!l.show_overlay(OverlaySlot { inst: 2, ..slot.clone() }), "an equal copy");
    assert!(l.hide_overlay(&slot.key));
    assert!(!l.retain_overlays(|_| false));
    assert_eq!(l.version, v);
}

#[test]
fn logo_bug_follows_logo_change_on_next_toggle_and_needs_a_logo() {
    let mut p = plan_with(vec![bug()]);
    let mut l = Live::new();
    let id = p.overlays[0].id;
    on(&mut l, &p, id).unwrap();
    let asset = |l: &Live| match &l.frame.overlays[0].source { OverlaySource::Image { source: ImageSource::Asset { asset }, .. } => asset.clone(), s => panic!("{s:?}") };
    assert_eq!(asset(&l), "logo.png");
    p.logo = Some("new.png".into());
    run(&mut l, &p, Command::Overlay { id, on: false, seen_clear: None }).unwrap();
    on(&mut l, &p, id).unwrap();
    assert_eq!(asset(&l), "new.png");
    p.logo = None;
    run(&mut l, &p, Command::ClearOverlays).unwrap();
    assert_eq!(on(&mut l, &p, id).unwrap_err(), strings::LOGO_FIRST);
}

#[test]
fn overlay_failed_removes_slot_and_sets_notice_unless_stale() {
    let p = plan_with(vec![bug()]);
    let mut l = Live::new();
    on(&mut l, &p, p.overlays[0].id).unwrap();
    let slot = l.frame.overlays[0].clone();
    assert!(!l.report(RoomEvent::OverlayFailed { key: slot.key.clone(), inst: slot.inst + 7, reason: "x".into() }), "stale report");
    assert_eq!(l.frame.overlays.len(), 1);
    let v = l.version;
    assert!(l.report(RoomEvent::OverlayFailed { key: slot.key.clone(), inst: slot.inst, reason: strings::PICTURE_UNREADABLE.into() }));
    assert!(l.frame.overlays.is_empty());
    assert_eq!(l.version, v + 1);
    assert_eq!(l.notice.as_deref(), Some(strings::overlay_failed("Logo", strings::PICTURE_UNREADABLE).as_str()));
    run(&mut l, &p, Command::Next).unwrap();
    assert_eq!(l.notice, None, "short-lived: the next slide clears it");
}

#[test]
fn failed_slide_reverts_without_touching_overlays() {
    let p = plan_with(vec![sign("A")]);
    let mut l = Live::new();
    run(&mut l, &p, Command::Next).unwrap();
    l.report(RoomEvent::Committed { version: l.version });
    on(&mut l, &p, p.overlays[0].id).unwrap();
    run(&mut l, &p, Command::Next).unwrap();
    assert!(l.report(RoomEvent::Failed { version: l.version, reason: "x".into() }));
    assert_eq!(keys(&l), ["A"], "the overlay asked for stays");
}

#[test]
fn live_item_theme_reaches_the_slide_layer() {
    let p = plan_with(vec![]);
    let mut l = Live::new();
    run(&mut l, &p, Command::Next).unwrap();
    assert!(matches!(&l.frame.slide, Some(crate::live::SlideLayer::Text { theme: Some(t), .. }) if t.size == 77));
}

#[test]
fn showing_lights_each_layer() {
    let p = plan_with(vec![sign("A")]);
    let mut l = Live::new();
    assert_eq!(l.frame.showing(), Default::default());
    run(&mut l, &p, Command::Next).unwrap();
    on(&mut l, &p, p.overlays[0].id).unwrap();
    let s = l.frame.showing();
    assert!(s.text && s.background && s.overlays && !s.media && !s.logo && !s.audio);
}
