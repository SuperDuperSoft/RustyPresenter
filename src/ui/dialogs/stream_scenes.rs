//! Settings → Media → Livestream → Your scenes… (`ui/dialogs/stream-scenes.slint`, design
//! `docs/superpowers/specs/2026-11-17-own-scenes-design.md`): the church's own livestream scenes, each a list of
//! layers, with a picture of how the chosen one looks drawn by the stream's own code. Save keeps them in Settings;
//! closing goes back to Settings → Media.
use super::{DialogKind, SettingsTab, confirm, prompt, strings as dlg};
use crate::{
    model::Theme,
    stream::{own_scenes::{self, Layer, OwnScene, Place, Shows, StandIns}, scene::Scene},
    tr,
    ui::{StreamScenesDlg, operator::window::Operator},
};
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};
use std::{cell::RefCell, rc::{Rc, Weak}};

pub fn title() -> String { tr!("Your livestream scenes") }

/// The scenes being edited, the chosen scene and the chosen layer (its place in the scene's layers, back to front).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Editor {
    pub scenes: Vec<OwnScene>,
    pub scene: Option<usize>,
    pub layer: Option<usize>,
    /// As saved, to know whether anything changed.
    saved: Vec<OwnScene>,
}

impl Editor {
    /// Opens on the first scene and its front layer.
    pub fn new(scenes: Vec<OwnScene>) -> Editor {
        let mut ed = Editor { saved: scenes.clone(), scenes, scene: None, layer: None };
        ed.choose(0);
        ed
    }

    pub fn changed(&self) -> bool { self.scenes != self.saved }

    pub fn current(&self) -> Option<&OwnScene> { self.scene.and_then(|i| self.scenes.get(i)) }

    fn current_mut(&mut self) -> Option<&mut OwnScene> { self.scene.and_then(|i| self.scenes.get_mut(i)) }

    pub fn current_layer(&self) -> Option<&Layer> { self.current().and_then(|s| s.layers.get(self.layer?)) }

    fn layer_mut(&mut self) -> Option<&mut Layer> {
        let i = self.layer?;
        self.current_mut().and_then(|s| s.layers.get_mut(i))
    }

    /// Chooses scene `i` (the last when past the end; none when there are none) and its front layer.
    pub fn choose(&mut self, i: usize) {
        self.scene = (!self.scenes.is_empty()).then(|| i.min(self.scenes.len() - 1));
        self.layer = self.current().and_then(|s| s.layers.len().checked_sub(1));
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
        s.layers.push(Layer { shows: Shows::Text { text: tr!("Welcome") }, place: Place::Box { x: 5, y: 5, w: 40, h: 20 }, see_through: 0 });
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
        if let Some(l) = self.layer_mut() { l.place = if let Place::Box { .. } = c { l.place.as_box() } else { c } }
    }

    /// The chosen layer's box, in percent.
    pub fn set_box(&mut self, x: u8, y: u8, w: u8, h: u8) {
        let (x, y) = (x.min(99), y.min(99));
        if let Some(l) = self.layer_mut() { l.place = Place::Box { x, y, w: w.clamp(1, 100 - x), h: h.clamp(1, 100 - y) } }
    }

    pub fn set_see_through(&mut self, v: u8) { if let Some(l) = self.layer_mut() { l.see_through = v.min(100) } }

    pub fn set_picture(&mut self, path: &str) { if let Some(l) = self.layer_mut() && let Shows::Picture { .. } = l.shows { l.shows = Shows::Picture { path: path.to_string() } } }

    pub fn set_colour(&mut self, hex: &str) {
        let Some(hex) = super::background::parse_hex(hex) else { return };
        if let Some(l) = self.layer_mut() && let Shows::Colour { .. } = l.shows { l.shows = Shows::Colour { colour: hex } }
    }

    pub fn set_text(&mut self, text: &str) { if let Some(l) = self.layer_mut() && let Shows::Text { .. } = l.shows { l.shows = Shows::Text { text: text.to_string() } } }

    fn saved_now(&mut self) { self.saved = self.scenes.clone() }
}

/// The editor and what draws its picture.
struct State {
    ed: Editor,
    theme: Theme,
    fonts: crate::render::Fonts,
    stand: Option<StandIns>,
}

type St = Rc<RefCell<State>>;

fn g(op: &Operator) -> StreamScenesDlg<'_> { op.ui.global::<StreamScenesDlg>() }

/// Opens the editor on the church's scenes as last saved. Closing it goes back to Settings → Media.
pub fn open(op: &Rc<Operator>) {
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    let st: St = Rc::new(RefCell::new(State {
        ed: Editor::new(snap.settings.record.own_scenes.clone()), theme: snap.settings.theme.clone(), fonts: crate::render::Fonts::new(), stand: None,
    }));
    op.open_dialog(DialogKind::StreamScenes);
    let g = g(op);
    let names = |v: Vec<String>| ModelRc::new(VecModel::from(v.into_iter().map(SharedString::from).collect::<Vec<_>>()));
    g.set_shows_names(names(Shows::choices().iter().map(Shows::kind_label).collect()));
    g.set_place_names(names(Place::choices().iter().map(Place::label).collect()));
    g.set_instead_names(names(std::iter::once(tr!("Never")).chain(Scene::ALL.iter().map(|s| s.label())).collect()));
    wire(op, &st);
    show(op, &st);
    op.on_dialog_close(|op| super::settings::open(op, SettingsTab::Media));
}

/// Puts the editor into the dialog and draws the picture again.
fn show(op: &Operator, st: &St) {
    let g = g(op);
    let mut s = st.borrow_mut();
    let State { ed, theme, fonts, stand } = &mut *s;
    let names: Vec<SharedString> = ed.scenes.iter().map(|s| s.title().into()).collect();
    g.set_scenes(ModelRc::new(VecModel::from(names)));
    g.set_scene_index(ed.scene.map_or(-1, |i| i as i32));
    g.set_changed(ed.changed());
    let Some(scene) = ed.current() else {
        g.set_layers(ModelRc::default());
        g.set_layer_index(-1);
        g.set_has_layer(false);
        g.set_picture(Image::default());
        return;
    };
    g.set_instead_index(scene.instead_of.and_then(|b| Scene::ALL.iter().position(|s| *s == b)).map_or(0, |i| i as i32 + 1));
    let rows: Vec<SharedString> = scene.layers.iter().rev().map(|l| l.label().into()).collect();
    g.set_layers(ModelRc::new(VecModel::from(rows)));
    g.set_layer_index(ed.layer_row().map_or(-1, |r| r as i32));
    let layer = ed.current_layer();
    g.set_has_layer(layer.is_some());
    if let Some(l) = layer {
        g.set_layer_unknown(l.shows == Shows::Unknown);
        g.set_shows_index(l.shows.index().map_or(0, |i| i as i32));
        g.set_place_index(l.place.index() as i32);
        let (is_box, (x, y, w, h)) = match l.place { Place::Box { x, y, w, h } => (true, (x, y, w, h)), _ => (false, (0, 0, 0, 0)) };
        g.set_is_box(is_box);
        g.set_box_x(i32::from(x));
        g.set_box_y(i32::from(y));
        g.set_box_w(i32::from(w));
        g.set_box_h(i32::from(h));
        g.set_see_through(i32::from(l.see_through.min(100)));
        g.set_is_picture(matches!(l.shows, Shows::Picture { .. }));
        g.set_is_colour(matches!(l.shows, Shows::Colour { .. }));
        g.set_is_text(matches!(l.shows, Shows::Text { .. }));
        match &l.shows {
            Shows::Picture { path } => g.set_picture_name(path.as_str().into()),
            Shows::Colour { colour } => g.set_colour(colour.as_str().into()),
            // Not while typing: setting it would move the cursor.
            Shows::Text { text } if g.get_text() != text.as_str() => g.set_text(text.as_str().into()),
            _ => {}
        }
    }
    let stand = stand.get_or_insert_with(|| StandIns::new(own_scenes::PREVIEW, theme, fonts));
    let px = own_scenes::preview(scene, stand, theme, fonts);
    let (w, h) = own_scenes::PREVIEW;
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
    g.on_select_scene(on_i(Editor::choose));
    g.on_add_scene(on(|op, st| with(op, st, Editor::add)));
    g.on_copy_scene(on(|op, st| with(op, st, Editor::copy)));
    g.on_rename_scene(on(rename));
    g.on_delete_scene(on(delete));
    g.on_move_scene(on_i(|e, up| e.move_scene(up == 1)));
    g.on_instead_changed(on_i(Editor::set_instead));
    g.on_select_layer(on_i(Editor::choose_layer));
    g.on_add_layer(on(|op, st| with(op, st, Editor::add_layer)));
    g.on_remove_layer(on(|op, st| with(op, st, Editor::remove_layer)));
    g.on_move_layer(on_i(|e, front| e.move_layer(front == 1)));
    g.on_shows_changed(on_i(Editor::set_shows));
    g.on_place_changed(on_i(Editor::set_place));
    g.on_see_through_changed(on_i(|e, v| e.set_see_through(v.min(100) as u8)));
    let (me, s) = (Rc::downgrade(op), st.clone());
    g.on_box_changed(move |x, y, w, h| if let Some(op) = me.upgrade() {
        let pct = |v: i32| v.clamp(0, 100) as u8;
        with(&op, &s, |e| e.set_box(pct(x), pct(y), pct(w), pct(h)))
    });
    g.on_colour_changed(on_s(Editor::set_colour));
    g.on_text_edited(on_s(Editor::set_text));
    g.on_choose_picture(on(choose_picture));
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
    let question = tr!("Delete the scene “{name}”? Slide actions and macros that choose it will show the room screen instead.", name = scene.title());
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
