//! Settings → Media → Livestream → Your scenes… (`ui/dialogs/stream-scenes.slint`, designs
//! `docs/superpowers/specs/2026-11-17-own-scenes-design.md` and `2026-11-17-scene-extras-design.md`): the church's own
//! livestream scenes, each a list of layers, with a picture of how the chosen one looks drawn by the stream's own
//! code. A layer can be dragged on the picture, and resized by its corner. Each scene can have its own places in the
//! tall (phone) picture, and its own way of changing to it. Save keeps them in Settings; closing goes back to
//! Settings → Media.
use super::{DialogKind, SettingsTab, confirm, prompt, strings as dlg};
use crate::{
    model::Theme,
    stream::{key_out::Key, own_scenes::{self, Crop, Layer, Now, OwnScene, Place, Shows, StandIns}, scene::Scene, transition::Transition},
    timers::TimeOfDay,
    tr,
    ui::{StreamScenesDlg, operator::window::Operator},
};
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};
use std::{cell::RefCell, rc::{Rc, Weak}};

pub fn title() -> String { tr!("Your livestream scenes") }

/// How near the chosen layer's bottom right corner (in % of the picture) a press resizes it instead of moving it.
const HANDLE: f32 = 4.0;

/// A layer being dragged on the picture: moved, or resized by its corner, from where the press was and the box it
/// had then (both in % of the picture).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Drag { resize: bool, from: (f32, f32), start: (f32, f32, f32, f32) }

/// The scenes being edited, the chosen scene and the chosen layer (its place in the scene's layers, back to front).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Editor {
    pub scenes: Vec<OwnScene>,
    pub scene: Option<usize>,
    pub layer: Option<usize>,
    /// The tall (phone) picture is shown and its places are edited.
    pub tall_view: bool,
    /// How the stream changes between scenes, unless a scene has its own way (Settings).
    pub default_transition: Transition,
    drag: Option<Drag>,
    /// As saved, to know whether anything changed.
    saved: (Vec<OwnScene>, Transition),
}

impl Editor {
    /// Opens on the first scene and its front layer.
    pub fn new(scenes: Vec<OwnScene>) -> Editor { Editor::with_default(scenes, Transition::default()) }

    pub fn with_default(scenes: Vec<OwnScene>, default_transition: Transition) -> Editor {
        let mut ed = Editor { saved: (scenes.clone(), default_transition.clone()), scenes, default_transition, ..Editor::default() };
        ed.choose(0);
        ed
    }

    pub fn changed(&self) -> bool { self.scenes != self.saved.0 || self.default_transition != self.saved.1 }

    pub fn current(&self) -> Option<&OwnScene> { self.scene.and_then(|i| self.scenes.get(i)) }

    fn current_mut(&mut self) -> Option<&mut OwnScene> { self.scene.and_then(|i| self.scenes.get_mut(i)) }

    pub fn current_layer(&self) -> Option<&Layer> { self.current().and_then(|s| s.layers.get(self.layer?)) }

    fn layer_mut(&mut self) -> Option<&mut Layer> {
        let i = self.layer?;
        self.current_mut().and_then(|s| s.layers.get_mut(i))
    }

    /// The picture shown: the tall one only when the scene has tall places of its own.
    pub fn editing_tall(&self) -> bool { self.tall_view && self.current().is_some_and(|s| s.tall) }

    /// The shape of the picture shown (the editor's preview size).
    pub fn view_size(&self) -> (u32, u32) {
        let (w, h) = own_scenes::PREVIEW;
        if self.tall_view { (h, w) } else { (w, h) }
    }

    /// The chosen layer's place in the picture being edited.
    pub fn place(&self) -> Option<Place> { self.current_layer().map(|l| l.place_in(self.editing_tall())) }

    fn set_place_of(&mut self, place: Place) {
        let tall = self.editing_tall();
        if let Some(l) = self.layer_mut() { if tall { l.tall = Some(place) } else { l.place = place } }
    }

    /// Chooses scene `i` (the last when past the end; none when there are none) and its front layer.
    pub fn choose(&mut self, i: usize) {
        self.scene = (!self.scenes.is_empty()).then(|| i.min(self.scenes.len() - 1));
        self.layer = self.current().and_then(|s| s.layers.len().checked_sub(1));
        self.drag = None;
    }

    /// "Scene 1", or the first such name not taken.
    fn free_name(&self) -> String {
        (1..).map(|n| tr!("Scene {n}", n = n)).find(|n| !self.scenes.iter().any(|s| s.name == *n)).expect("a free name")
    }

    pub fn add(&mut self) {
        let s = OwnScene::new(self.free_name());
        self.scenes.push(s);
        self.choose(self.scenes.len() - 1);
    }

    /// A copy of the chosen scene right after it, with an id of its own. It does not stand in for anything.
    pub fn copy(&mut self) {
        let Some(i) = self.scene else { return };
        let mut s = self.scenes[i].clone();
        s.id = uuid::Uuid::new_v4();
        s.name = tr!("{name} copy", name = s.title());
        s.instead_of = None;
        self.scenes.insert(i + 1, s);
        self.choose(i + 1);
    }

    pub fn rename(&mut self, name: &str) {
        let name = name.trim().to_string();
        if name.is_empty() { return }
        if let Some(s) = self.current_mut() { s.name = name }
    }

    pub fn delete(&mut self) {
        let Some(i) = self.scene else { return };
        self.scenes.remove(i);
        self.choose(i.min(self.scenes.len().saturating_sub(1)));
    }

    /// The chosen scene one up (`up`) or down the list.
    pub fn move_scene(&mut self, up: bool) {
        let Some(i) = self.scene else { return };
        let j = if up { i.checked_sub(1) } else { (i + 1 < self.scenes.len()).then_some(i + 1) };
        let Some(j) = j else { return };
        self.scenes.swap(i, j);
        self.scene = Some(j);
    }

    /// "Use it by itself in place of": 0 never, else that built-in scene (`Scene::ALL`). Only one scene stands in for
    /// each built-in one: another that did no longer does.
    pub fn set_instead(&mut self, choice: usize) {
        let Some(i) = self.scene else { return };
        let built_in = choice.checked_sub(1).and_then(|c| Scene::ALL.get(c).copied());
        for (j, s) in self.scenes.iter_mut().enumerate() {
            if j == i { s.instead_of = built_in } else if built_in.is_some() && s.instead_of == built_in { s.instead_of = None }
        }
    }

    /// "When it comes in": 0 like the others (the default), else `Transition::choices()[choice - 1]`.
    pub fn set_transition(&mut self, choice: usize) {
        let Some(s) = self.current_mut() else { return };
        s.transition = match choice.checked_sub(1) {
            None => None,
            Some(c) => Some(s.transition.clone().unwrap_or_default().changed_to(c)),
        };
    }

    pub fn set_stinger(&mut self, path: &str) {
        if let Some(s) = self.current_mut() && let Some(Transition::Stinger { .. }) = s.transition { s.transition = Some(Transition::Stinger { path: path.to_string() }) }
    }

    /// The change between scenes for every scene without its own: `Transition::choices()[choice]`.
    pub fn set_default_transition(&mut self, choice: usize) { self.default_transition = self.default_transition.changed_to(choice) }

    pub fn set_default_stinger(&mut self, path: &str) {
        if let Transition::Stinger { .. } = self.default_transition { self.default_transition = Transition::Stinger { path: path.to_string() } }
    }

    /// Shows the wide (`tall` false) or the tall picture.
    pub fn set_view(&mut self, tall: bool) { self.tall_view = tall; self.drag = None }

    /// The chosen scene gets tall places of its own (each layer starting where it is in the wide picture), or goes
    /// back to the wide picture fitted across.
    pub fn arrange_tall(&mut self, on: bool) {
        if let Some(s) = self.current_mut() {
            s.tall = on;
            if !on { s.layers.iter_mut().for_each(|l| l.tall = None) }
        }
    }

    /// The layer list's row `row` (the front layer first).
    pub fn choose_layer(&mut self, row: usize) {
        let n = self.current().map_or(0, |s| s.layers.len());
        self.layer = n.checked_sub(row + 1);
    }

    /// The chosen layer's row in the list (the front layer first).
    pub fn layer_row(&self) -> Option<usize> { Some(self.current()?.layers.len() - 1 - self.layer?) }

    /// A new layer in front of everything: the church's words in a box near the top left.
    pub fn add_layer(&mut self) {
        let Some(s) = self.current_mut() else { return };
        s.layers.push(Layer { shows: Shows::Text { text: tr!("Welcome"), look: Default::default() }, place: Place::Box { x: 5, y: 5, w: 40, h: 20 }, ..Layer::default() });
        self.layer = Some(s.layers.len() - 1);
    }

    pub fn remove_layer(&mut self) {
        let Some(i) = self.layer else { return };
        let Some(s) = self.current_mut() else { return };
        if i >= s.layers.len() { return }
        s.layers.remove(i);
        let n = s.layers.len();
        self.layer = n.checked_sub(1).map(|last| i.min(last));
    }

    /// The chosen layer one step in front of (`front`) or behind the next.
    pub fn move_layer(&mut self, front: bool) {
        let Some(i) = self.layer else { return };
        let Some(s) = self.current_mut() else { return };
        let j = if front { (i + 1 < s.layers.len()).then_some(i + 1) } else { i.checked_sub(1) };
        let Some(j) = j else { return };
        s.layers.swap(i, j);
        self.layer = Some(j);
    }

    /// What the chosen layer shows: `Shows::choices()[choice]`.
    pub fn set_shows(&mut self, choice: usize) {
        let Some(c) = Shows::choices().get(choice).cloned() else { return };
        if let Some(l) = self.layer_mut() { l.shows = l.shows.changed_to(&c) }
    }

    /// Where the chosen layer goes: `Place::choices()[choice]`. A box starts where the layer was.
    pub fn set_place(&mut self, choice: usize) {
        let Some(c) = Place::choices().get(choice).copied() else { return };
        let size = self.view_size();
        let Some(now) = self.place() else { return };
        self.set_place_of(if let Place::Box { .. } = c { now.as_box_in(size) } else { c });
    }

    /// The chosen layer's box, in percent.
    pub fn set_box(&mut self, x: u8, y: u8, w: u8, h: u8) {
        let (x, y) = (x.min(99), y.min(99));
        self.set_place_of(Place::Box { x, y, w: w.clamp(1, 100 - x), h: h.clamp(1, 100 - y) });
    }

    /// The chosen layer's place as a box in % of the picture shown: `(x, y, width, height)`.
    pub fn box_pct(&self) -> Option<(f32, f32, f32, f32)> {
        let (w, h) = self.view_size();
        let (x, y, rw, rh) = self.place()?.rect((w, h));
        let (fw, fh) = (w as f32 / 100.0, h as f32 / 100.0);
        Some((x as f32 / fw, y as f32 / fh, rw as f32 / fw, rh as f32 / fh))
    }

    /// A press on the picture at (`x`, `y`) % of it: near the chosen layer's bottom right corner it starts resizing
    /// it; otherwise the layer in front there is chosen (if any) and starts moving.
    pub fn press(&mut self, x: f32, y: f32) {
        self.drag = None;
        if let Some((bx, by, bw, bh)) = self.box_pct() && (x - (bx + bw)).abs() <= HANDLE && (y - (by + bh)).abs() <= HANDLE {
            self.drag = Some(Drag { resize: true, from: (x, y), start: (bx, by, bw, bh) });
            return;
        }
        let (w, h) = self.view_size();
        let tall = self.editing_tall();
        let Some(s) = self.current() else { return };
        let (px, py) = ((x / 100.0 * w as f32) as u32, (y / 100.0 * h as f32) as u32);
        let hit = s.layers.iter().enumerate().rev().find(|(_, l)| {
            let (lx, ly, lw, lh) = l.place_in(tall).rect((w, h));
            l.see_through < 100 && px >= lx && px < lx + lw && py >= ly && py < ly + lh
        }).map(|(i, _)| i);
        let Some(i) = hit else { return };
        self.layer = Some(i);
        if let Some(start) = self.box_pct() { self.drag = Some(Drag { resize: false, from: (x, y), start }) }
    }

    /// The pointer moved to (`x`, `y`) % while pressed: the layer follows (a box in percent, kept inside the
    /// picture). False when nothing is being dragged.
    pub fn drag_to(&mut self, x: f32, y: f32) -> bool {
        let Some(d) = self.drag else { return false };
        let (dx, dy) = (x - d.from.0, y - d.from.1);
        let (sx, sy, sw, sh) = d.start;
        let r = |v: f32| v.round().clamp(0.0, 100.0) as u8;
        let (bx, by, bw, bh) = if d.resize {
            (r(sx), r(sy), r((sw + dx).clamp(2.0, 100.0 - sx)), r((sh + dy).clamp(2.0, 100.0 - sy)))
        } else {
            (r((sx + dx).clamp(0.0, 100.0 - sw)), r((sy + dy).clamp(0.0, 100.0 - sh)), r(sw), r(sh))
        };
        let want = Place::Box { x: bx, y: by, w: bw.max(1), h: bh.max(1) };
        if self.place() == Some(want) { return false }
        self.set_box(bx, by, bw, bh);
        true
    }

    pub fn release(&mut self) { self.drag = None }

    pub fn set_see_through(&mut self, v: u8) { if let Some(l) = self.layer_mut() { l.see_through = v.min(100) } }

    /// The edges cut off the chosen layer's picture, in percent.
    pub fn set_crop(&mut self, left: u8, top: u8, right: u8, bottom: u8) {
        if let Some(l) = self.layer_mut() { l.crop = Crop { left, top, right, bottom }.checked() }
    }

    /// What is taken out of the chosen layer: `Key::index` order.
    pub fn set_key_kind(&mut self, i: usize) { if let Some(l) = self.layer_mut() { l.key = l.key.changed_to(i) } }

    /// How close (or the level) and how soft the chosen layer's key is.
    pub fn set_key_amounts(&mut self, amount: u8, soft: u8) { if let Some(l) = self.layer_mut() { l.key = l.key.with_amounts(amount, soft) } }

    pub fn set_key_colour(&mut self, hex: &str) {
        let Some(hex) = super::background::parse_hex(hex) else { return };
        if let Some(l) = self.layer_mut() && let Key::Colour { closeness, soft, .. } = l.key { l.key = Key::Colour { colour: hex, closeness, soft } }
    }

    pub fn set_picture(&mut self, path: &str) { if let Some(l) = self.layer_mut() && let Shows::Picture { .. } = l.shows { l.shows = Shows::Picture { path: path.to_string() } } }

    pub fn set_video(&mut self, path: &str) { if let Some(l) = self.layer_mut() && let Shows::Video { .. } = l.shows { l.shows = Shows::Video { path: path.to_string() } } }

    pub fn set_web(&mut self, address: &str) { if let Some(l) = self.layer_mut() && let Shows::Web { .. } = l.shows { l.shows = Shows::Web { address: address.trim().to_string() } } }

    pub fn set_colour(&mut self, hex: &str) {
        let Some(hex) = super::background::parse_hex(hex) else { return };
        if let Some(l) = self.layer_mut() && let Shows::Colour { .. } = l.shows { l.shows = Shows::Colour { colour: hex } }
    }

    pub fn set_text(&mut self, text: &str) { if let Some(l) = self.layer_mut() && let Shows::Text { text: t, .. } = &mut l.shows { *t = text.to_string() } }

    /// The words' largest size (px on a 1080p stream; 0: the church's default) of a text, clock or countdown layer.
    pub fn set_look_size(&mut self, px: u16) { if let Some(l) = self.layer_mut() && let Some(look) = l.shows.look_mut() { look.size = px.min(400) } }

    /// Their colour; empty: the church's default.
    pub fn set_look_colour(&mut self, hex: &str) {
        let colour = if hex.trim().is_empty() { String::new() } else { match super::background::parse_hex(hex) { Some(h) => h, None => return } };
        if let Some(l) = self.layer_mut() && let Some(look) = l.shows.look_mut() { look.colour = colour }
    }

    pub fn set_clock(&mut self, seconds: bool, h24: bool) {
        if let Some(l) = self.layer_mut() && let Shows::Clock { seconds: s, h24: h, .. } = &mut l.shows { (*s, *h) = (seconds, h24) }
    }

    /// The time a countdown counts to (`H:MM`); empty: the service's start time. Anything else is not taken.
    pub fn set_countdown_to(&mut self, text: &str) {
        let to = if text.trim().is_empty() { None } else { match TimeOfDay::parse(text) { Some(t) => Some(t), None => return } };
        if let Some(l) = self.layer_mut() && let Shows::Countdown { to: t, .. } = &mut l.shows { *t = to }
    }

    fn saved_now(&mut self) { self.saved = (self.scenes.clone(), self.default_transition.clone()) }
}

/// The editor and what draws its picture.
struct State {
    ed: Editor,
    theme: Theme,
    fonts: crate::render::Fonts,
    stand: Option<StandIns>,
    /// The service's start time, for countdowns in the picture.
    starts_at: Option<TimeOfDay>,
}

type St = Rc<RefCell<State>>;

fn g(op: &Operator) -> StreamScenesDlg<'_> { op.ui.global::<StreamScenesDlg>() }

/// "When it comes in": like the others (naming the default), then each way.
fn transition_names(default: &Transition) -> Vec<String> {
    std::iter::once(tr!("Like the other scenes ({way})", way = default.label())).chain(Transition::choices().iter().map(Transition::label)).collect()
}

/// Opens the editor on the church's scenes as last saved. Closing it goes back to Settings → Media.
pub fn open(op: &Rc<Operator>) {
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    let starts_at = crate::automation::actions::starts_at(&op.app().data.lock().unwrap());
    let st: St = Rc::new(RefCell::new(State {
        ed: Editor::with_default(snap.settings.record.own_scenes.clone(), snap.settings.record.scene_transition.clone()),
        theme: snap.settings.theme.clone(), fonts: crate::render::Fonts::new(), stand: None, starts_at,
    }));
    op.open_dialog(DialogKind::StreamScenes);
    let g = g(op);
    let names = |v: Vec<String>| ModelRc::new(VecModel::from(v.into_iter().map(SharedString::from).collect::<Vec<_>>()));
    g.set_shows_names(names(Shows::choices().iter().map(Shows::kind_label).collect()));
    g.set_place_names(names(Place::choices().iter().map(Place::label).collect()));
    g.set_instead_names(names(std::iter::once(tr!("Never")).chain(Scene::ALL.iter().map(|s| s.label())).collect()));
    g.set_key_names(names(vec![tr!("Nothing"), tr!("A colour (green screen)"), tr!("The dark parts"), tr!("The light parts")]));
    g.set_default_transition_names(names(Transition::choices().iter().map(Transition::label).collect()));
    wire(op, &st);
    show(op, &st);
    op.on_dialog_close(|op| super::settings::open(op, SettingsTab::Media));
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path.trim()).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// Puts the editor into the dialog and draws the picture again.
fn show(op: &Operator, st: &St) {
    let g = g(op);
    let mut s = st.borrow_mut();
    let State { ed, theme, fonts, stand, starts_at } = &mut *s;
    let names: Vec<SharedString> = ed.scenes.iter().map(|s| s.title().into()).collect();
    g.set_scenes(ModelRc::new(VecModel::from(names)));
    g.set_scene_index(ed.scene.map_or(-1, |i| i as i32));
    g.set_changed(ed.changed());
    g.set_default_transition_index(ed.default_transition.index() as i32);
    g.set_default_is_stinger(matches!(ed.default_transition, Transition::Stinger { .. }));
    g.set_default_stinger_name(ed.default_transition.stinger().map(file_name).unwrap_or_default().into());
    g.set_tall_view(ed.tall_view);
    let Some(scene) = ed.current() else {
        g.set_layers(ModelRc::default());
        g.set_layer_index(-1);
        g.set_has_layer(false);
        g.set_has_box(false);
        g.set_picture(Image::default());
        return;
    };
    g.set_scene_tall(scene.tall);
    g.set_instead_index(scene.instead_of.and_then(|b| Scene::ALL.iter().position(|s| *s == b)).map_or(0, |i| i as i32 + 1));
    let names: Vec<SharedString> = transition_names(&ed.default_transition).into_iter().map(SharedString::from).collect();
    g.set_transition_names(ModelRc::new(VecModel::from(names)));
    g.set_transition_index(scene.transition.as_ref().map_or(0, |t| t.index() as i32 + 1));
    g.set_is_stinger(matches!(scene.transition, Some(Transition::Stinger { .. })));
    g.set_stinger_name(scene.transition.as_ref().and_then(Transition::stinger).map(file_name).unwrap_or_default().into());
    let tall = ed.editing_tall();
    let rows: Vec<SharedString> = scene.layers.iter().rev().map(|l| if tall { Layer { place: l.place_in(true), ..l.clone() }.label() } else { l.label() }.into()).collect();
    g.set_layers(ModelRc::new(VecModel::from(rows)));
    g.set_layer_index(ed.layer_row().map_or(-1, |r| r as i32));
    let layer = ed.current_layer();
    g.set_has_layer(layer.is_some());
    // The chosen layer's frame on the picture (only where the picture shown is the one being edited).
    let editable = !ed.tall_view || scene.tall;
    match ed.box_pct().filter(|_| editable) {
        Some((x, y, w, h)) => { g.set_has_box(true); g.set_box_left(x); g.set_box_top(y); g.set_box_width(w); g.set_box_height(h) }
        None => g.set_has_box(false),
    }
    if let Some(l) = layer {
        g.set_layer_unknown(l.shows == Shows::Unknown);
        g.set_shows_index(l.shows.index().map_or(0, |i| i as i32));
        let place = l.place_in(tall);
        g.set_place_index(place.index() as i32);
        let (is_box, (x, y, w, h)) = match place { Place::Box { x, y, w, h } => (true, (x, y, w, h)), _ => (false, (0, 0, 0, 0)) };
        g.set_is_box(is_box);
        g.set_box_x(i32::from(x));
        g.set_box_y(i32::from(y));
        g.set_box_w(i32::from(w));
        g.set_box_h(i32::from(h));
        g.set_see_through(i32::from(l.see_through.min(100)));
        g.set_can_crop(l.shows.can_crop());
        g.set_crop_left(i32::from(l.crop.left));
        g.set_crop_top(i32::from(l.crop.top));
        g.set_crop_right(i32::from(l.crop.right));
        g.set_crop_bottom(i32::from(l.crop.bottom));
        g.set_can_key(l.shows.can_key());
        g.set_key_index(l.key.index() as i32);
        let (amount, soft) = l.key.amounts();
        g.set_key_amount(i32::from(amount));
        g.set_key_soft(i32::from(soft));
        if let Key::Colour { colour, .. } = &l.key { g.set_key_colour(colour.as_str().into()) }
        g.set_is_picture(matches!(l.shows, Shows::Picture { .. }));
        g.set_is_colour(matches!(l.shows, Shows::Colour { .. }));
        g.set_is_text(matches!(l.shows, Shows::Text { .. }));
        g.set_is_video(matches!(l.shows, Shows::Video { .. }));
        g.set_is_clock(matches!(l.shows, Shows::Clock { .. }));
        g.set_is_countdown(matches!(l.shows, Shows::Countdown { .. }));
        g.set_is_web(matches!(l.shows, Shows::Web { .. }));
        g.set_has_look(l.shows.look().is_some());
        if let Some(look) = l.shows.look() {
            g.set_look_size(i32::from(look.size));
            g.set_look_colour(look.colour.as_str().into());
        }
        match &l.shows {
            Shows::Picture { path } => g.set_picture_name(path.as_str().into()),
            Shows::Video { path } => g.set_video_name(path.as_str().into()),
            Shows::Colour { colour } => g.set_colour(colour.as_str().into()),
            // Not while typing: setting it would move the cursor.
            Shows::Text { text, .. } if g.get_text() != text.as_str() => g.set_text(text.as_str().into()),
            Shows::Web { address } if g.get_web_address().trim() != address.as_str() => g.set_web_address(address.as_str().into()),
            Shows::Clock { seconds, h24, .. } => { g.set_clock_seconds(*seconds); g.set_clock_h24(*h24) }
            Shows::Countdown { to, .. } => {
                let typed = g.get_countdown_to();
                let parsed = if typed.trim().is_empty() { None } else { TimeOfDay::parse(&typed) };
                if parsed != *to || (to.is_some() && typed.trim().is_empty()) { g.set_countdown_to(to.map(|t| format!("{}:{:02}", t.hour, t.minute)).unwrap_or_default().into()) }
            }
            _ => {}
        }
    }
    let stand = stand.get_or_insert_with(|| StandIns::new(own_scenes::PREVIEW, theme, fonts));
    // The tall picture without places of its own is the wide one fitted across, as the stream sends it.
    let (w, h) = ed.view_size();
    let px = if ed.tall_view && !scene.tall {
        let wide = own_scenes::preview_at(scene, stand, false, theme, fonts, &Now::local(*starts_at));
        let mut out = vec![0u8; w as usize * h as usize * 4];
        crate::stream::tall::fit(&wide, own_scenes::PREVIEW, &mut out, (w, h));
        out
    } else {
        own_scenes::preview_at(scene, stand, ed.tall_view, theme, fonts, &Now::local(*starts_at))
    };
    g.set_picture(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&px, w, h)));
}

/// Runs `f` on the editor, then shows it.
fn with(op: &Operator, st: &St, f: impl FnOnce(&mut Editor)) {
    f(&mut st.borrow_mut().ed);
    show(op, st);
}

fn wire(op: &Rc<Operator>, st: &St) {
    let g = g(op);
    let on = |f: fn(&Rc<Operator>, &St)| {
        let (me, st): (Weak<Operator>, St) = (Rc::downgrade(op), st.clone());
        move || if let Some(op) = me.upgrade() { f(&op, &st) }
    };
    let on_i = |f: fn(&mut Editor, usize)| {
        let (me, st): (Weak<Operator>, St) = (Rc::downgrade(op), st.clone());
        move |i: i32| if let Some(op) = me.upgrade() { with(&op, &st, |e| f(e, i.max(0) as usize)) }
    };
    let on_s = |f: fn(&mut Editor, &str)| {
        let (me, st): (Weak<Operator>, St) = (Rc::downgrade(op), st.clone());
        move |t: SharedString| if let Some(op) = me.upgrade() { with(&op, &st, |e| f(e, &t)) }
    };
    let on_b = |f: fn(&mut Editor, bool)| {
        let (me, st): (Weak<Operator>, St) = (Rc::downgrade(op), st.clone());
        move |b: bool| if let Some(op) = me.upgrade() { with(&op, &st, |e| f(e, b)) }
    };
    let pct = |v: i32| v.clamp(0, 100) as u8;
    g.on_select_scene(on_i(Editor::choose));
    g.on_add_scene(on(|op, st| with(op, st, Editor::add)));
    g.on_copy_scene(on(|op, st| with(op, st, Editor::copy)));
    g.on_rename_scene(on(rename));
    g.on_delete_scene(on(delete));
    g.on_move_scene(on_i(|e, up| e.move_scene(up == 1)));
    g.on_instead_changed(on_i(Editor::set_instead));
    g.on_transition_changed(on_i(Editor::set_transition));
    g.on_default_transition_changed(on_i(Editor::set_default_transition));
    g.on_choose_stinger(on(|op, st| choose_video(op, st, Editor::set_stinger)));
    g.on_choose_default_stinger(on(|op, st| choose_video(op, st, Editor::set_default_stinger)));
    g.on_view_changed(on_b(Editor::set_view));
    g.on_arrange_tall(on_b(Editor::arrange_tall));
    g.on_select_layer(on_i(Editor::choose_layer));
    g.on_add_layer(on(|op, st| with(op, st, Editor::add_layer)));
    g.on_remove_layer(on(|op, st| with(op, st, Editor::remove_layer)));
    g.on_move_layer(on_i(|e, front| e.move_layer(front == 1)));
    g.on_shows_changed(on_i(Editor::set_shows));
    g.on_place_changed(on_i(Editor::set_place));
    g.on_see_through_changed(on_i(|e, v| e.set_see_through(v.min(100) as u8)));
    let (me, s) = (Rc::downgrade(op), st.clone());
    g.on_box_changed(move |x, y, w, h| if let Some(op) = me.upgrade() { with(&op, &s, |e| e.set_box(pct(x), pct(y), pct(w), pct(h))) });
    let (me, s) = (Rc::downgrade(op), st.clone());
    g.on_crop_changed(move |l, t, r, b| if let Some(op) = me.upgrade() { with(&op, &s, |e| e.set_crop(pct(l), pct(t), pct(r), pct(b))) });
    g.on_key_kind_changed(on_i(Editor::set_key_kind));
    let (me, s) = (Rc::downgrade(op), st.clone());
    g.on_key_amounts_changed(move |a, b| if let Some(op) = me.upgrade() { with(&op, &s, |e| e.set_key_amounts(pct(a), pct(b))) });
    g.on_key_colour_changed(on_s(Editor::set_key_colour));
    // Dragging on the picture: a press chooses the layer there (or its corner), moving draws it again.
    let (me, s) = (Rc::downgrade(op), st.clone());
    g.on_press(move |x, y| if let Some(op) = me.upgrade() { with(&op, &s, |e| e.press(x, y)) });
    let (me, s) = (Rc::downgrade(op), st.clone());
    g.on_drag(move |x, y| if let Some(op) = me.upgrade() && s.borrow_mut().ed.drag_to(x, y) { show(&op, &s) });
    let (me, s) = (Rc::downgrade(op), st.clone());
    g.on_release(move || if let Some(op) = me.upgrade() { s.borrow_mut().ed.release(); show(&op, &s) });
    g.on_colour_changed(on_s(Editor::set_colour));
    g.on_text_edited(on_s(Editor::set_text));
    g.on_look_size_changed(on_i(|e, v| e.set_look_size(v.min(400) as u16)));
    g.on_look_colour_changed(on_s(Editor::set_look_colour));
    let (me, s) = (Rc::downgrade(op), st.clone());
    g.on_clock_changed(move |sec, h24| if let Some(op) = me.upgrade() { with(&op, &s, |e| e.set_clock(sec, h24)) });
    g.on_countdown_edited(on_s(Editor::set_countdown_to));
    g.on_web_edited(on_s(Editor::set_web));
    g.on_choose_picture(on(choose_picture));
    g.on_choose_video(on(|op, st| choose_video(op, st, Editor::set_video)));
    g.on_cancel(on(cancel));
    g.on_save(on(save));
}

fn rename(op: &Rc<Operator>, st: &St) {
    let Some(name) = st.borrow().ed.current().map(OwnScene::title) else { return };
    let (me, st) = (Rc::downgrade(op), st.clone());
    prompt(op, &tr!("Scene name"), &name, move |typed| {
        let (Some(op), Some(typed)) = (me.upgrade(), typed) else { return };
        if op.dialog() == Some(DialogKind::StreamScenes) { with(&op, &st, |e| e.rename(&typed)) }
    });
}

fn delete(op: &Rc<Operator>, st: &St) {
    let Some(scene) = st.borrow().ed.current().cloned() else { return };
    let (me, st) = (Rc::downgrade(op), st.clone());
    let question = tr!("Delete the scene “{name}”? Slide actions and macros that choose it will show the audience screen instead.", name = scene.title());
    confirm(op, &question, &tr!("Delete"), &dlg::cancel(), true, move |ok| {
        let Some(op) = me.upgrade().filter(|_| ok) else { return };
        if op.dialog() != Some(DialogKind::StreamScenes) || st.borrow().ed.current().map(|s| s.id) != Some(scene.id) { return }
        with(&op, &st, Editor::delete);
    });
}

fn choose_picture(op: &Rc<Operator>, st: &St) {
    let (me, st) = (Rc::downgrade(op), st.clone());
    crate::ui::operator::files::pick(op, crate::ui::operator::file_input::Filter::Pictures, false, move |paths| {
        let (Some(op), Some(path)) = (me.upgrade(), paths.first()) else { return };
        if op.dialog() == Some(DialogKind::StreamScenes) { with(&op, &st, |e| e.set_picture(&path.display().to_string())) }
    });
}

/// Picks a video file for `set` (a video layer, or a stinger).
fn choose_video(op: &Rc<Operator>, st: &St, set: fn(&mut Editor, &str)) {
    let (me, st) = (Rc::downgrade(op), st.clone());
    crate::ui::operator::files::pick(op, crate::ui::operator::file_input::Filter::Videos, false, move |paths| {
        let (Some(op), Some(path)) = (me.upgrade(), paths.first()) else { return };
        if op.dialog() == Some(DialogKind::StreamScenes) { with(&op, &st, |e| set(e, &path.display().to_string())) }
    });
}

/// Leaves; with changes not saved, asks first.
fn cancel(op: &Rc<Operator>, st: &St) {
    if !st.borrow().ed.changed() { return op.close_dialog() }
    let me = Rc::downgrade(op);
    confirm(op, &tr!("Leave without saving your scene changes?"), &tr!("Leave"), &tr!("Keep editing"), true, move |ok| {
        if let Some(op) = me.upgrade().filter(|_| ok) { op.close_dialog() }
    });
}

/// Keeps the scenes in Settings (the rest of the settings as they are now), then goes back to Settings.
fn save(op: &Rc<Operator>, st: &St) {
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    let mut s = snap.settings.clone();
    s.record.own_scenes = st.borrow().ed.scenes.clone();
    s.record.scene_transition = st.borrow().ed.default_transition.clone();
    let body = serde_json::to_value(&s).unwrap_or_default();
    let (me, st) = (Rc::downgrade(op), st.clone());
    op.ops().call::<serde_json::Value>("settings", body, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => {
                st.borrow_mut().ed.saved_now();
                op.toast(tr!("Scenes saved."), false);
                if op.dialog() == Some(DialogKind::StreamScenes) { op.close_dialog() }
            }
            Err(e) => op.toast(e, true),
        }
    });
}

#[cfg(test)]
#[path = "stream_scenes_tests.rs"]
mod tests;
