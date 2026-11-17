//! The livestream's scene worker (1.3b): ten times a second it looks at what is live, picks the scene
//! (`scene::choose`), and draws what the scene needs besides the room and the camera: the words over the camera
//! (the live text slide in the lower-third look, `lyrics`) and the scene cards (Starting soon with the pre-service
//! countdown, Ending, Hold), each a picture the church chose or words. The recorder's writer reads the newest of
//! all three from `Slot` and puts the stream's picture together (`compose`).
use super::lyrics;
use super::own_scenes::{self, Now, OwnScene, Prepared};
use super::transition::Transition;
use super::scene::{AutoPrefs, Choice, LiveInfo, Scene, choose, effective, lit};
use crate::model::Theme;
use crate::render::cache::{RenderKey, draw_key};
use crate::tr;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How often what is live is looked at.
pub const TICK: Duration = Duration::from_millis(100);

/// One scene card in Settings → Livestream: words (empty: the default for its scene) or a picture file.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Card {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub words: String,
    /// A picture file; empty: the words.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub picture: String,
}

impl Card {
    fn is_default(&self) -> bool { *self == Card::default() }
}

/// The three cards.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Cards {
    #[serde(skip_serializing_if = "Card::is_default")]
    pub starting: Card,
    #[serde(skip_serializing_if = "Card::is_default")]
    pub ending: Card,
    #[serde(skip_serializing_if = "Card::is_default")]
    pub hold: Card,
}

impl Cards {
    pub fn is_default(&self) -> bool { *self == Cards::default() }

    /// The card of `scene` (`None` for a scene that is not a card).
    pub fn of(&self, scene: Scene) -> Option<&Card> {
        match scene {
            Scene::StartingSoon => Some(&self.starting),
            Scene::Ending => Some(&self.ending),
            Scene::Hold => Some(&self.hold),
            _ => None,
        }
    }
}

/// A card's words when the church has not written its own.
pub fn default_words(scene: Scene) -> String {
    match scene {
        Scene::StartingSoon => tr!("Starting soon"),
        Scene::Ending => tr!("Thanks for joining us"),
        _ => tr!("We'll be right back"),
    }
}

/// The words card of `scene` at `canvas`: the card's words (or the default) in the middle, in `theme`'s font and
/// colour, and the countdown under them.
pub fn card_key(card: &Card, scene: Scene, countdown: Option<&str>, theme: &Theme, canvas: (u32, u32)) -> RenderKey {
    let words = card.words.trim();
    let text = if words.is_empty() { default_words(scene) } else { words.to_string() };
    RenderKey { text, footer: countdown.map(str::to_string), reference: None, scripture: false, size: 0, passage: vec![], parallel: None,
        theme: Theme { position: "center".into(), band: false, ..theme.clone() }, canvas, design: None }
}

/// What is live (and the operator's choice, and the pre-service countdown as text while it runs), asked each tick.
pub type Info = Arc<dyn Fn() -> (LiveInfo, Choice, Option<String>) + Send + Sync>;

/// Everything the worker needs: where to look and the church's choices.
#[derive(Clone)]
pub struct Look {
    pub info: Info,
    /// The live slide's words picture to draw (`lyrics::key_for`), `None` when it has none.
    pub words: lyrics::Source,
    pub auto: AutoPrefs,
    pub cards: Cards,
    /// The church default's style, for the cards' words.
    pub theme: Theme,
    /// Where the corner camera and the words go (Settings).
    pub layout: super::compose::Layout,
    /// The church's own scenes (Settings → Livestream → Your scenes).
    pub own: Vec<OwnScene>,
    /// The scene changes, the tall picture and the service's start time (`More`).
    pub more: More,
}

/// More of the church's choices for the scenes (design `2026-11-17-scene-extras-design.md`).
#[derive(Clone, Default)]
pub struct More {
    /// How the stream changes between scenes, unless the scene it changes to has its own way.
    pub transition: Transition,
    /// The service's start time, for countdown layers (asked when one is shown).
    pub starts_at: Option<Arc<dyn Fn() -> Option<crate::timers::TimeOfDay> + Send + Sync>>,
    /// The tall (phone) picture's size, when the app puts it together (the church has tall scenes and a tall
    /// destination); `None`: FFmpeg fits the wide picture across, as before.
    pub tall: Option<(u32, u32)>,
}

impl std::fmt::Debug for Look {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("Look") }
}

/// What the stream shows now: its scene, and the words and card pictures (premultiplied RGBA of the stream's size).
/// For the church's own scene, `own` is it made ready for the stream's size, and `own_tall` for the tall picture
/// when it has places of its own there. `transition`: how the stream changes to this scene.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    pub scene: Scene,
    pub words: Option<Arc<Vec<u8>>>,
    pub card: Option<Arc<Vec<u8>>>,
    pub own: Option<Arc<Prepared>>,
    pub own_tall: Option<Arc<Prepared>>,
    pub transition: Transition,
}

pub type Slot = Arc<Mutex<Shown>>;

/// The worker. Dropping it stops it.
pub struct Worker {
    pub slot: Slot,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() { let _ = t.join(); }
    }
}

impl Drop for Worker {
    fn drop(&mut self) { self.stop() }
}

/// Starts the worker for a stream of `size`. `camera`: whether camera pictures are coming (camera scenes fall back
/// to the room screen without). While `hold` (Freeze) is set nothing changes.
pub fn start(look: Look, size: (u32, u32), camera: Arc<AtomicBool>, hold: Arc<AtomicBool>) -> Result<Worker, String> {
    let slot: Slot = Arc::default();
    let stop = Arc::new(AtomicBool::new(false));
    let (sl, st) = (slot.clone(), stop.clone());
    let thread = std::thread::Builder::new().name("stream-scenes".into()).spawn(move || {
        let mut w = State::new(size);
        while !st.load(Ordering::Relaxed) {
            if !hold.load(Ordering::Relaxed) { w.tick(&look, camera.load(Ordering::Relaxed), &sl) }
            std::thread::sleep(TICK);
        }
    }).map_err(|e| e.to_string())?;
    Ok(Worker { slot, stop, thread: Some(thread) })
}

/// What the worker drew last, so it draws again only when something changed.
struct State {
    size: (u32, u32),
    fonts: crate::render::Fonts,
    last: Scene,
    words: Option<(RenderKey, Option<Arc<Vec<u8>>>)>,
    card: Option<(Scene, Card, Option<String>, Option<Arc<Vec<u8>>>)>,
    /// The church's scene made ready last (wide and tall, its clocks as last drawn), and what it was made from.
    own: Option<(OwnScene, Arc<Prepared>, Option<Arc<Prepared>>)>,
}

impl State {
    fn new(size: (u32, u32)) -> State { State { size, fonts: crate::render::Fonts::new(), last: Scene::Screen, words: None, card: None, own: None } }

    fn tick(&mut self, look: &Look, camera: bool, slot: &Slot) {
        let (info, choice, countdown) = (look.info)();
        let wanted = choose(&info, look.auto, choice, self.last);
        self.last = wanted;
        let mut scene = effective(wanted, camera);
        // Chosen by itself: the church's scene that stands in for it, if any. A scene that was deleted since it was
        // chosen shows the room.
        if lit(choice, &info).is_none() { scene = own_scenes::stand_in(scene, &look.own) }
        let ours = match scene { Scene::Own(id) => own_scenes::find(&look.own, id), _ => None };
        if matches!(scene, Scene::Own(_)) && ours.is_none() { scene = Scene::Screen }
        let (own, own_tall) = match ours { Some(s) => { let (w, t) = self.prepared(s, look); (Some(w), t) } None => (None, None) };
        let wants_words = scene == Scene::CameraWords || wanted == Scene::CameraWords || ours.is_some_and(OwnScene::shows_words);
        let words = if wants_words { self.words_picture(look) } else { None };
        let card = look.cards.of(scene).map(|c| self.card_picture(c, scene, countdown.filter(|_| scene == Scene::StartingSoon), &look.theme));
        let card = card.flatten();
        let transition = own_scenes::transition_to(scene, &look.own, &look.more.transition);
        *slot.lock().unwrap_or_else(|e| e.into_inner()) = Shown { scene, words, card, own, own_tall, transition };
    }

    /// The church's scene made ready for the stream (and its tall picture), again only when it changed; its clocks
    /// and countdowns drawn again when their words change.
    fn prepared(&mut self, scene: &OwnScene, look: &Look) -> (Arc<Prepared>, Option<Arc<Prepared>>) {
        let now = || Now::local(look.more.starts_at.as_ref().and_then(|f| f()));
        if let Some((s, p, t)) = &mut self.own && s == scene {
            if p.ticks() || t.as_ref().is_some_and(|t| t.ticks()) {
                let now = now();
                if let Some(q) = p.retick(&now, &mut self.fonts) { *p = Arc::new(q) }
                if let Some(t) = t && let Some(q) = t.retick(&now, &mut self.fonts) { *t = Arc::new(q) }
            }
            return (p.clone(), t.clone());
        }
        let now = now();
        let p = Arc::new(own_scenes::prepare_at(scene, self.size, false, &look.theme, &mut self.fonts, &now));
        let t = look.more.tall.filter(|_| scene.tall).map(|size| Arc::new(own_scenes::prepare_at(scene, size, true, &look.theme, &mut self.fonts, &now)));
        self.own = Some((scene.clone(), p.clone(), t.clone()));
        (p, t)
    }

    fn words_picture(&mut self, look: &Look) -> Option<Arc<Vec<u8>>> {
        let key = (look.words)()?;
        if let Some((k, p)) = &self.words && *k == key { return p.clone() }
        let p = self.draw(&key);
        self.words = Some((key, p.clone()));
        p
    }

    fn card_picture(&mut self, card: &Card, scene: Scene, countdown: Option<String>, theme: &Theme) -> Option<Arc<Vec<u8>>> {
        if let Some((s, c, t, p)) = &self.card && *s == scene && c == card && *t == countdown { return p.clone() }
        let p = picture_file(&card.picture, self.size).or_else(|| self.draw(&card_key(card, scene, countdown.as_deref(), theme, self.size)));
        self.card = Some((scene, card.clone(), countdown, p.clone()));
        p
    }

    fn draw(&mut self, key: &RenderKey) -> Option<Arc<Vec<u8>>> {
        let r = draw_key(&mut self.fonts, key);
        ((r.width, r.height) == self.size).then(|| Arc::new(r.rgba_premul.buffer().as_bytes().to_vec()))
    }
}

/// The picture file `path` fitted into `size` (black bars), opaque; `None` when there is none or it cannot be read.
fn picture_file(path: &str, size: (u32, u32)) -> Option<Arc<Vec<u8>>> {
    if path.trim().is_empty() { return None }
    let img = image::open(path.trim()).map_err(|e| eprintln!("stream card: could not read {path}: {e}")).ok()?.to_rgba8();
    let mut out = vec![0u8; size.0 as usize * size.1 as usize * 4];
    crate::record::pump::fit(&crate::record::pump::Raw { w: img.width(), h: img.height(), bottom_up: false, rgba: img.as_raw() }, &mut out, size.0, size.1);
    Some(Arc::new(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    const SIZE: (u32, u32) = (320, 180);

    #[test]
    fn the_starting_card_has_the_countdown_under_it() {
        let k = card_key(&Card::default(), Scene::StartingSoon, Some("4:32"), &Theme::default(), SIZE);
        assert_eq!((k.text.as_str(), k.footer.as_deref(), k.canvas), ("Starting soon", Some("4:32"), SIZE));
        let k = card_key(&Card::default(), Scene::Ending, None, &Theme::default(), SIZE);
        assert_eq!((k.text.as_str(), k.footer), ("Thanks for joining us", None));
    }

    #[test]
    fn own_words_replace_the_default() {
        let card = Card { words: "Welcome!".into(), picture: String::new() };
        assert_eq!(card_key(&card, Scene::Hold, None, &Theme::default(), SIZE).text, "Welcome!");
    }

    fn words_slide() -> lyrics::Source {
        let slide = crate::live::SlideLayer::Text { text: "Amazing grace".into(), footer: None, reference: None, scripture: false, size: 0, passage: vec![], parallel: None, theme: None };
        Arc::new(move || lyrics::key_for(&slide, &Theme::default(), SIZE))
    }

    fn look(live: Arc<Mutex<LiveInfo>>, cards: Cards) -> Look {
        Look { info: Arc::new(move || (live.lock().unwrap().clone(), Choice::default(), None)), words: words_slide(), auto: AutoPrefs::default(), cards, theme: Theme::default(), layout: Default::default(), own: vec![], more: More::default() }
    }

    fn wait_for(slot: &Slot, scene: Scene) -> Shown {
        for _ in 0..50 {
            let s = slot.lock().unwrap().clone();
            if s.scene == scene { return s }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the scene never became {scene:?}: {:?}", slot.lock().unwrap().scene)
    }

    #[test]
    fn the_worker_follows_what_is_live() {
        let live = Arc::new(Mutex::new(LiveInfo { cursor_item: Some(Uuid::from_u128(1)), slide: crate::stream::scene::SlideKind::Words, ..LiveInfo::default() }));
        let w = start(look(live.clone(), Cards::default()), SIZE, Arc::new(AtomicBool::new(true)), Arc::default()).unwrap();
        let s = wait_for(&w.slot, Scene::CameraWords);
        assert!(s.words.as_ref().is_some_and(|p| p.len() == 320 * 180 * 4 && p.iter().any(|b| *b != 0)), "the words are drawn");
        *live.lock().unwrap() = LiveInfo::default();
        let s = wait_for(&w.slot, Scene::StartingSoon);
        assert!(s.card.is_some(), "the Starting soon card is drawn");
    }

    #[test]
    fn without_a_camera_words_go_out_as_the_room() {
        let live = Arc::new(Mutex::new(LiveInfo { cursor_item: Some(Uuid::from_u128(1)), slide: crate::stream::scene::SlideKind::Words, ..LiveInfo::default() }));
        let w = start(look(live, Cards::default()), SIZE, Arc::new(AtomicBool::new(false)), Arc::default()).unwrap();
        wait_for(&w.slot, Scene::Screen);
    }

    #[test]
    fn a_card_picture_that_cannot_load_shows_its_words() {
        let mut s = State::new(SIZE);
        let words = s.card_picture(&Card::default(), Scene::Hold, None, &Theme::default()).expect("a words card");
        let mut s = State::new(SIZE);
        let missing = Card { words: String::new(), picture: "C:/no/such/picture.png".into() };
        assert_eq!(s.card_picture(&missing, Scene::Hold, None, &Theme::default()), Some(words));
    }

    #[test]
    fn a_card_picture_fills_the_card() {
        let dir = std::env::temp_dir().join(format!("rp-test-card-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("red.png");
        image::RgbaImage::from_pixel(16, 9, image::Rgba([200, 0, 0, 255])).save(&path).unwrap();
        let p = picture_file(path.to_str().unwrap(), SIZE).unwrap();
        assert_eq!(&p[..4], &[200, 0, 0, 255]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn freeze_keeps_the_scene() {
        let live = Arc::new(Mutex::new(LiveInfo { cursor_item: Some(Uuid::from_u128(1)), slide: crate::stream::scene::SlideKind::Picture, ..LiveInfo::default() }));
        let hold = Arc::new(AtomicBool::new(false));
        let w = start(look(live.clone(), Cards::default()), SIZE, Arc::new(AtomicBool::new(true)), hold.clone()).unwrap();
        wait_for(&w.slot, Scene::Screen);
        hold.store(true, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(150));
        *live.lock().unwrap() = LiveInfo::default();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(w.slot.lock().unwrap().scene, Scene::Screen, "frozen");
    }

    #[test]
    fn a_church_scene_stands_in_when_chosen_by_itself_and_has_the_words() {
        use crate::stream::own_scenes::{Layer, Shows};
        let mut ours = OwnScene::new("Songs".into());
        ours.layers.push(Layer { shows: Shows::Words, ..Layer::default() });
        ours.instead_of = Some(Scene::CameraWords);
        let live = Arc::new(Mutex::new(LiveInfo { cursor_item: Some(Uuid::from_u128(1)), slide: crate::stream::scene::SlideKind::Words, ..LiveInfo::default() }));
        let w = start(Look { own: vec![ours.clone()], ..look(live, Cards::default()) }, SIZE, Arc::new(AtomicBool::new(true)), Arc::default()).unwrap();
        let s = wait_for(&w.slot, Scene::Own(ours.id));
        assert_eq!(s.own.as_ref().map(|p| p.id), Some(ours.id), "made ready for the writer");
        assert!(s.words.is_some(), "its words layer gets the words");
        std::thread::sleep(Duration::from_millis(250));
        let again = w.slot.lock().unwrap().clone();
        assert!(Arc::ptr_eq(s.own.as_ref().unwrap(), again.own.as_ref().unwrap()), "made ready once, not every tick");
    }

    #[test]
    fn a_church_scene_chosen_by_hand_is_kept_and_a_deleted_one_shows_the_room() {
        let ours = OwnScene::new("Welcome".into());
        let gone = Uuid::from_u128(99);
        let pick = Arc::new(Mutex::new(Some(Scene::Own(ours.id))));
        let p2 = pick.clone();
        let info: Info = Arc::new(move || (LiveInfo::default(), Choice::pick(*p2.lock().unwrap(), None), None));
        let look = Look { info, own: vec![ours.clone()], ..look(Arc::default(), Cards::default()) };
        let w = start(look, SIZE, Arc::new(AtomicBool::new(true)), Arc::default()).unwrap();
        wait_for(&w.slot, Scene::Own(ours.id));
        *pick.lock().unwrap() = Some(Scene::Own(gone));
        let s = wait_for(&w.slot, Scene::Screen);
        assert!(s.own.is_none());
    }
}
