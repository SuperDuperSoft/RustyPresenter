//! Room output decisions, without any drawing: which layers a new live view needs
//! prepared, which to remove, and when a prepared frame may be committed.
//! Mirrors `web/output.js` (`keys`, `apply`, the `token` rule, `syncPause`).
use crate::app::LiveView;
use crate::live::{Frame, MediaLayer, RoomEvent, SlideLayer};
use crate::model::Background;
use std::time::Duration;

/// What a layer shows; a layer is re-prepared only when its key changes.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum LayerKey { Background(u64), Media(u64), Slide(String), Logo(String), Overlays(String), Audio(u64) }

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind { Background, Media, Slide, Logo, Overlays, Audio }

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Want { pub kind: Kind, pub key: LayerKey, pub timeout: Duration }

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Plan { pub token: u64, pub version: u64, pub prepare: Vec<Want>, pub remove: Vec<Kind>, pub fade_through_black: bool }

#[derive(Debug)]
pub enum Ready { Swap { commit: RoomEvent }, Stale }

/// Prepare timeouts, as in `web/output.js`.
const STILL: Duration = Duration::from_millis(2500);
const BACKGROUND_VIDEO: Duration = Duration::from_millis(6000);
const PLAYER: Duration = Duration::from_millis(8000);

/// Bottom → top, then audio. Also the index into `Layers::keys`.
const KINDS: [Kind; 6] = [Kind::Background, Kind::Media, Kind::Slide, Kind::Logo, Kind::Overlays, Kind::Audio];

/// The layer keys of one frame, plus what fading needs to know about it.
#[derive(Clone, Default)]
struct Layers { keys: [Option<LayerKey>; 6], video: bool, version: Option<u64>, theme: String }

impl Layers {
    fn of(view: &LiveView, theme: String) -> Self {
        let f = &view.frame;
        // A slide with its own theme (taken live with one) keys on it alone, so editing the church default never
        // prepares it again; one from an older `live-state.json` follows the view's theme.
        let slide = f.slide.as_ref().map(|s| {
            let own = matches!(s, SlideLayer::Text { theme: Some(_), .. });
            let json = serde_json::to_string(s).unwrap_or_default();
            LayerKey::Slide(if own { json } else { format!("{json}:{}", theme_json(view)) })
        });
        let keys = [
            f.background.as_ref().map(|b| LayerKey::Background(b.inst)),
            f.media.as_ref().map(|m| LayerKey::Media(m.inst)),
            slide,
            f.logo.then(|| LayerKey::Logo(view.logo.clone().unwrap_or_default())),
            (!f.overlays.is_empty()).then(|| LayerKey::Overlays(overlays_key(view))),
            f.audio.as_ref().map(|a| LayerKey::Audio(a.inst)),
        ];
        Self { keys, video: is_video(f), version: Some(view.version), theme }
    }
}

/// What the overlay layer shows: its slots without their instance numbers, the font they fall back to and the text
/// of the bound ones (a clock) right now, so a new minute prepares the overlays again and nothing else.
fn overlays_key(view: &LiveView) -> String {
    let slots: Vec<_> = view.frame.overlays.iter().map(|o| (&o.key, o.z, &o.source, &o.place)).collect();
    format!("{}:{}:{}", serde_json::to_string(&slots).unwrap_or_default(), view.theme.font, bound_now(view))
}

/// The text of every bound overlay (a clock) at this moment.
pub fn bound_now(view: &LiveView) -> String {
    if !view.frame.overlays.iter().any(|o| o.source.is_bound()) { return String::new() }
    let ctx = crate::theme::overlay::BoundCtx::at(chrono::Local::now());
    view.frame.overlays.iter().filter_map(|o| crate::theme::overlay::bound_text(&o.source, &ctx)).collect::<Vec<_>>().join("|")
}

fn theme_json(view: &LiveView) -> String { serde_json::to_string(&view.theme).unwrap_or_default() }
fn is_video(f: &Frame) -> bool { matches!(f.media.as_ref().map(|m| &m.v), Some(MediaLayer::Video { .. })) }

fn timeout(kind: Kind, f: &Frame) -> Duration {
    match kind {
        Kind::Background if matches!(f.background.as_ref().map(|b| &b.v), Some(Background::Video { .. })) => BACKGROUND_VIDEO,
        Kind::Media if is_video(f) => PLAYER,
        Kind::Audio => PLAYER,
        _ => STILL,
    }
}

/// Tracks what is on air and which preparation is the newest (the `token` rule).
#[derive(Default)]
pub struct RoomState {
    shown: Layers,
    /// The newest plan still preparing; older tokens are stale.
    pending: Option<(u64, Layers)>,
    token: u64,
}

impl RoomState {
    pub fn new() -> Self { Self::default() }

    /// What to change for `view`; `None` when neither the version nor the theme/logo changed
    /// since the newest plan (only pause state may differ, see `pause_sync`).
    pub fn plan(&mut self, view: &LiveView) -> Option<Plan> {
        let theme = format!("{}:{}:{}", theme_json(view), serde_json::to_string(&view.logo).unwrap_or_default(), bound_now(view));
        let last = self.pending.as_ref().map_or(&self.shown, |(_, l)| l);
        if last.version == Some(view.version) && last.theme == theme { return None; }
        let next = Layers::of(view, theme);
        let mut prepare = vec![];
        let mut remove = vec![];
        for (i, kind) in KINDS.into_iter().enumerate() {
            match (&next.keys[i], &self.shown.keys[i]) {
                (Some(want), have) if have.as_ref() != Some(want) => prepare.push(Want { kind, key: want.clone(), timeout: timeout(kind, &view.frame) }),
                (None, Some(_)) => remove.push(kind),
                _ => {}
            }
        }
        let fade_through_black = next.keys[1] != self.shown.keys[1] && (next.video || self.shown.video);
        self.token += 1;
        self.pending = Some((self.token, next));
        Some(Plan { token: self.token, version: view.version, prepare, remove, fade_through_black })
    }

    /// Everything for `token` is prepared: swap it in if it is still the newest.
    pub fn ready(&mut self, token: u64) -> Ready {
        match self.pending.take() {
            Some((t, layers)) if t == token => {
                let version = layers.version.unwrap_or_default();
                self.shown = layers;
                Ready::Swap { commit: RoomEvent::Committed { version } }
            }
            other => { self.pending = other; Ready::Stale }
        }
    }

    /// Preparing `token` failed. Only the newest plan reports; the shown layers stay.
    pub fn failed(&mut self, token: u64, reason: String) -> Option<RoomEvent> {
        match self.pending.take() {
            Some((t, layers)) if t == token => Some(RoomEvent::Failed { version: layers.version.unwrap_or_default(), reason }),
            other => { self.pending = other; None }
        }
    }
}

/// (instance, paused) for the foreground video and the audio item.
pub fn pause_sync(view: &LiveView) -> Vec<(u64, bool)> {
    let f = &view.frame;
    let media = f.media.as_ref().and_then(|m| match m.v { MediaLayer::Video { paused, .. } => Some((m.inst, paused)), _ => None });
    media.into_iter().chain(f.audio.as_ref().map(|a| (a.inst, a.v.paused))).collect()
}

/// The background is not drawn while a foreground video is live (spec §5).
pub fn draw_background(frame: &Frame) -> bool { !is_video(frame) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::{AudioLayer, Inst, SlideLayer};
    use crate::model::{EndAction, Theme};

    fn view(version: u64, frame: Frame) -> LiveView {
        LiveView { version, committed: 0, pending: true, frame, cursor: None, item: None, slide: None, next: None, sections: vec![], changed: None, media: None,
            audio: None, notice: None, theme: Theme::default(), logo: Some("logo.png".into()), clear_epoch: 0, stage_message: String::new() }
    }
    fn text(t: &str) -> Option<SlideLayer> { Some(SlideLayer::Text { text: t.into(), footer: None, reference: None, scripture: false, size: 0, passage: vec![], parallel: None, theme: None }) }
    fn bg(inst: u64) -> Option<Inst<Background>> { Some(Inst { inst, v: Background::Color { color: "#000".into() } }) }
    fn video(inst: u64, paused: bool) -> Option<Inst<MediaLayer>> { Some(Inst { inst, v: MediaLayer::Video { asset: "v.mp4".into(), end: EndAction::Stop, paused } }) }
    fn still(inst: u64) -> Option<Inst<MediaLayer>> { Some(Inst { inst, v: MediaLayer::Still { asset: "s.png".into() } }) }
    fn audio(inst: u64, paused: bool) -> Option<Inst<AudioLayer>> {
        Some(Inst { inst, v: AudioLayer { asset: "a.mp3".into(), title: "A".into(), looped: false, paused } })
    }
    fn kinds(p: &Plan) -> Vec<Kind> { p.prepare.iter().map(|w| w.kind).collect() }
    /// Plan and commit at once.
    fn show(r: &mut RoomState, v: &LiveView) -> Plan {
        let p = r.plan(v).expect("plan");
        assert!(matches!(r.ready(p.token), Ready::Swap { .. }));
        p
    }

    #[test]
    fn first_view_prepares_every_present_layer() {
        let mut r = RoomState::new();
        let f = Frame { background: bg(1), media: video(2, false), slide: text("a"), logo: true, audio: audio(3, false), overlays: vec![] };
        let p = r.plan(&view(1, f)).unwrap();
        assert_eq!(p.version, 1);
        assert_eq!(kinds(&p), vec![Kind::Background, Kind::Media, Kind::Slide, Kind::Logo, Kind::Audio]);
        assert_eq!(p.prepare[0].key, LayerKey::Background(1));
        assert_eq!(p.prepare[1].key, LayerKey::Media(2));
        assert_eq!(p.prepare[3].key, LayerKey::Logo("logo.png".into()));
        assert_eq!(p.prepare[4].key, LayerKey::Audio(3));
        assert!(p.remove.is_empty());
        // An empty frame prepares nothing.
        let mut r = RoomState::new();
        let p = r.plan(&view(1, Frame::default())).unwrap();
        assert!(p.prepare.is_empty() && p.remove.is_empty());
    }

    #[test]
    fn unchanged_frame_new_version_commits_without_prepare() {
        // FEAT-4 quiet adoption: a new version with the same frame prepares and removes nothing.
        let mut r = RoomState::new();
        let f = Frame { background: bg(1), slide: text("a"), ..Default::default() };
        show(&mut r, &view(1, f.clone()));
        let p = r.plan(&view(2, f)).expect("a plan, so the version commits");
        assert!(p.prepare.is_empty() && p.remove.is_empty());
        assert!(matches!(r.ready(p.token), Ready::Swap { commit: RoomEvent::Committed { version: 2 } }));
    }

    #[test]
    fn same_version_same_theme_is_no_plan() {
        let mut r = RoomState::new();
        let v = view(1, Frame { slide: text("a"), ..Default::default() });
        show(&mut r, &v);
        assert!(r.plan(&v).is_none());
        // A repeat while the plan is still preparing does not restart it either.
        let v2 = view(2, Frame { slide: text("b"), ..Default::default() });
        let p = r.plan(&v2).unwrap();
        assert!(r.plan(&v2).is_none());
        assert!(matches!(r.ready(p.token), Ready::Swap { .. }));
    }

    #[test]
    fn theme_change_reprepares_only_slide() {
        let mut r = RoomState::new();
        let f = Frame { background: bg(1), slide: text("a"), logo: true, ..Default::default() };
        let mut v = view(1, f);
        show(&mut r, &v);
        v.theme.size += 10;
        let p = r.plan(&v).expect("theme change is a plan");
        assert_eq!(kinds(&p), vec![Kind::Slide]);
        assert!(p.remove.is_empty());
    }

    fn sign(text: &str) -> crate::theme::overlay::OverlaySlot {
        use crate::theme::overlay::{OverlaySlot, OverlaySource, Placement, saved_key};
        let source = OverlaySource::Text { text: text.into(), styles: vec![], look: Default::default() };
        OverlaySlot { key: saved_key(uuid::Uuid::nil()), z: 0, inst: 1, name: "Sign".into(), source, place: Placement::default() }
    }

    #[test]
    fn overlay_change_prepares_only_overlays() {
        let mut r = RoomState::new();
        let f = Frame { background: bg(1), slide: text("a"), logo: true, ..Default::default() };
        show(&mut r, &view(1, f.clone()));
        let on = Frame { overlays: vec![sign("Welcome")], ..f.clone() };
        let p = r.plan(&view(2, on.clone())).unwrap();
        assert_eq!(kinds(&p), vec![Kind::Overlays]);
        assert!(matches!(r.ready(p.token), Ready::Swap { .. }));
        // The same sign shown again (a new instance number) is not drawn again.
        let mut again = on.clone();
        again.overlays[0].inst = 7;
        assert!(r.plan(&view(3, again)).unwrap().prepare.is_empty());
        let p = r.plan(&view(4, f)).unwrap();
        assert!(p.prepare.is_empty());
        assert_eq!(p.remove, vec![Kind::Overlays]);
    }

    #[test]
    fn slide_layer_keys_on_its_own_theme_else_view_theme() {
        let own = |t: &str| {
            let mut s = text(t);
            if let Some(SlideLayer::Text { theme, .. }) = &mut s { *theme = Some(Theme::default()) }
            s
        };
        // A slide carrying its own theme: editing the church default does not prepare it again.
        let mut r = RoomState::new();
        let mut v = view(1, Frame { slide: own("a"), ..Default::default() });
        show(&mut r, &v);
        v.theme.size += 10;
        assert!(r.plan(&v).unwrap().prepare.is_empty(), "church default edit leaves a snapshot slide alone");
        // One from an older live-state.json follows the view's theme.
        let mut r = RoomState::new();
        let mut v = view(1, Frame { slide: text("a"), ..Default::default() });
        show(&mut r, &v);
        v.theme.size += 10;
        assert_eq!(kinds(&r.plan(&v).unwrap()), vec![Kind::Slide]);
    }

    #[test]
    fn new_media_instance_reprepares_media_only() {
        let mut r = RoomState::new();
        let f = Frame { background: bg(1), media: still(2), audio: audio(3, false), ..Default::default() };
        show(&mut r, &view(1, f.clone()));
        let p = r.plan(&view(2, Frame { media: still(4), ..f })).unwrap();
        assert_eq!(kinds(&p), vec![Kind::Media]);
        assert_eq!(p.prepare[0].key, LayerKey::Media(4));
    }

    #[test]
    fn clear_slide_removes_slide_without_preparing() {
        let mut r = RoomState::new();
        let f = Frame { background: bg(1), slide: text("a"), ..Default::default() };
        show(&mut r, &view(1, f.clone()));
        let p = r.plan(&view(2, Frame { slide: None, ..f })).unwrap();
        assert!(p.prepare.is_empty());
        assert_eq!(p.remove, vec![Kind::Slide]);
    }

    #[test]
    fn rapid_versions_only_newest_commits() {
        let mut r = RoomState::new();
        let p1 = r.plan(&view(1, Frame { slide: text("1"), ..Default::default() })).unwrap();
        let p2 = r.plan(&view(2, Frame { slide: text("2"), ..Default::default() })).unwrap();
        assert_ne!(p1.token, p2.token);
        assert!(matches!(r.ready(p1.token), Ready::Stale));
        match r.ready(p2.token) { Ready::Swap { commit: RoomEvent::Committed { version } } => assert_eq!(version, 2), o => panic!("{o:?}") }
        assert!(matches!(r.ready(p2.token), Ready::Stale), "a token commits once");
        // Five in a row while the first prepares: only the fifth commits.
        let toks: Vec<u64> = (3..8).map(|v| r.plan(&view(v, Frame { slide: text(&v.to_string()), ..Default::default() })).unwrap().token).collect();
        for t in &toks[..4] { assert!(matches!(r.ready(*t), Ready::Stale)); }
        assert!(matches!(r.ready(toks[4]), Ready::Swap { commit: RoomEvent::Committed { version: 7 } }));
    }

    #[test]
    fn failed_old_token_is_silent() {
        let mut r = RoomState::new();
        let p1 = r.plan(&view(1, Frame { slide: text("1"), ..Default::default() })).unwrap();
        let _p2 = r.plan(&view(2, Frame { slide: text("2"), ..Default::default() })).unwrap();
        assert!(r.failed(p1.token, "late".into()).is_none());
    }

    #[test]
    fn failed_newest_reports_failed_with_reason() {
        let mut r = RoomState::new();
        let f = Frame { slide: text("1"), ..Default::default() };
        show(&mut r, &view(1, f.clone()));
        let p = r.plan(&view(2, Frame { media: still(5), ..f.clone() })).unwrap();
        match r.failed(p.token, "picture could not be loaded".into()) {
            Some(RoomEvent::Failed { version, reason }) => { assert_eq!(version, 2); assert_eq!(reason, "picture could not be loaded"); }
            o => panic!("{o:?}"),
        }
        assert!(matches!(r.ready(p.token), Ready::Stale), "a failed token never commits");
        // The committed layers are kept: going back to version 3 = committed content needs nothing new.
        let p = r.plan(&view(3, f)).unwrap();
        assert!(p.prepare.is_empty() && p.remove.is_empty());
    }

    #[test]
    fn video_start_fades_through_black() {
        let mut r = RoomState::new();
        let f = Frame { slide: text("a"), ..Default::default() };
        show(&mut r, &view(1, f));
        let p = show(&mut r, &view(2, Frame { media: video(2, false), ..Default::default() }));
        assert!(p.fade_through_black, "start");
        let p = r.plan(&view(3, Frame { slide: text("b"), ..Default::default() })).unwrap();
        assert!(p.fade_through_black, "stop");
        assert_eq!(p.remove, vec![Kind::Media]);
    }

    #[test]
    fn still_does_not_fade_through_black() {
        let mut r = RoomState::new();
        let p = show(&mut r, &view(1, Frame { media: still(1), ..Default::default() }));
        assert!(!p.fade_through_black);
        let p = show(&mut r, &view(2, Frame { slide: text("a"), ..Default::default() }));
        assert!(!p.fade_through_black);
        // Pausing a live video changes the version but not the layer: no fade.
        show(&mut r, &view(3, Frame { media: video(2, false), ..Default::default() }));
        let p = r.plan(&view(4, Frame { media: video(2, true), ..Default::default() })).unwrap();
        assert!(!p.fade_through_black && p.prepare.is_empty());
    }

    #[test]
    fn background_hidden_under_foreground_video() {
        assert!(draw_background(&Frame { background: bg(1), ..Default::default() }));
        assert!(draw_background(&Frame { background: bg(1), media: still(2), ..Default::default() }));
        assert!(!draw_background(&Frame { background: bg(1), media: video(2, true), ..Default::default() }));
    }

    #[test]
    fn timeouts_match_web_room() {
        let mut r = RoomState::new();
        let bgv = Some(Inst { inst: 1, v: Background::Video { asset: "b.mp4".into(), dim: 0 } });
        let f = Frame { background: bgv, media: video(2, false), slide: text("a"), logo: true, audio: audio(3, false), overlays: vec![] };
        let p = r.plan(&view(1, f)).unwrap();
        let ms: Vec<u128> = p.prepare.iter().map(|w| w.timeout.as_millis()).collect();
        assert_eq!(ms, vec![6000, 8000, 2500, 2500, 8000]);
        let bgi = Some(Inst { inst: 4, v: Background::Image { asset: "b.png".into(), dim: 0 } });
        let p = r.plan(&view(2, Frame { background: bgi, media: still(5), ..Default::default() })).unwrap();
        let ms: Vec<u128> = p.prepare.iter().map(|w| w.timeout.as_millis()).collect();
        assert_eq!(ms, vec![2500, 2500]);
    }

    #[test]
    fn pause_sync_lists_media_and_audio() {
        let v = view(1, Frame { media: video(2, true), audio: audio(3, false), ..Default::default() });
        assert_eq!(pause_sync(&v), vec![(2, true), (3, false)]);
        assert!(pause_sync(&view(1, Frame { media: still(2), ..Default::default() })).is_empty());
    }
}
