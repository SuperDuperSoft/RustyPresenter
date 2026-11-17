//! Stage layouts (FEAT-3 spec §3.6, `ui/dialogs/stage-layouts.slint`): the church's own stage layouts, edited as a
//! copy (`Editor`) and saved all at once with `stage/layouts/replace`. Built-in layouts are shown but locked; a
//! copy can be changed. Also the top bar's quick picker and the Settings choice of the stage layout.
use super::{DialogKind, confirm, prompt, strings as dlg};
use crate::{
    lenient::Lenient,
    render::{color::is_color, parse_color},
    stage::{
        Align, ElementKind, PctRect, ShapeKind, StageElement, StageLayout,
        geom::{self, Guide, Handle, Nudge, UndoStack},
        presets, strings as s,
    },
    ui::{SettingsDlg, StageBoxVm, StageGuideVm, StageLayoutRow, StageLayoutsDlg, operator::window::Operator},
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{cell::RefCell, rc::{Rc, Weak}};
use uuid::Uuid;

/// Which setting of a box was changed last, so a slider drag or typing is one undo step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field { Size, Color, Text }

/// A box being dragged: moved, or resized by a handle.
#[derive(Clone, Debug)]
struct Drag { id: Uuid, handle: Option<Handle>, start: (f32, f32), orig: PctRect, before: Vec<StageLayout>, moved: bool, free: bool }

/// The open editor: the stored layouts, the edited copy, what is chosen, and undo.
pub struct Editor {
    stored: Vec<StageLayout>,
    pub draft: Vec<StageLayout>,
    /// The stage default when the dialog opened (or was last changed from it).
    pub default: Uuid,
    /// The chosen layout (built-in or the church's).
    pub layout: Uuid,
    /// The chosen box.
    pub sel: Option<Uuid>,
    undo: UndoStack<Vec<StageLayout>>,
    drag: Option<Drag>,
    pub guides: Vec<Guide>,
    last: Option<(Uuid, Field)>,
}

impl Editor {
    pub fn new(stored: Vec<StageLayout>, default: Uuid) -> Self {
        let layout = if presets::find_layout(&stored, default).is_some() { default } else { presets::CLASSIC };
        Editor { draft: stored.clone(), stored, default, layout, sel: None, undo: UndoStack::new(50), drag: None, guides: vec![], last: None }
    }

    /// Every layout in list order: the built-ins, then the church's.
    pub fn all(&self) -> Vec<&StageLayout> { presets::all_layouts(&self.draft).collect() }

    pub fn current(&self) -> &StageLayout { presets::find_layout(&self.draft, self.layout).unwrap_or(presets::classic()) }

    pub fn locked(&self) -> bool { presets::is_builtin(self.layout) }

    /// Something differs from what is saved.
    pub fn changed(&self) -> bool { self.draft != self.stored }

    pub fn can_undo(&self) -> bool { self.undo.can_undo() }
    pub fn can_redo(&self) -> bool { self.undo.can_redo() }

    /// What Save sends: every church layout, and the saved ones that are gone.
    pub fn save_body(&self) -> serde_json::Value {
        let deleted: Vec<Uuid> = self.stored.iter().map(|l| l.id).filter(|id| !self.draft.iter().any(|l| l.id == *id)).collect();
        serde_json::json!({ "layouts": self.draft, "deleted": deleted })
    }

    /// The draft is now what is stored.
    pub fn saved(&mut self) { self.stored = self.draft.clone(); }

    /// Change the chosen church layout as one undo step. Built-ins never change.
    fn edit(&mut self, f: impl FnOnce(&mut StageLayout, &mut Option<Uuid>)) {
        let before = self.draft.clone();
        let (id, mut sel) = (self.layout, self.sel);
        let Some(l) = self.draft.iter_mut().find(|l| l.id == id) else { return };
        f(l, &mut sel);
        self.sel = sel;
        self.last = None;
        if self.draft != before { self.undo.push(before) }
    }

    /// Like `edit`, but repeated changes of one setting of one box make one undo step.
    fn edit_box(&mut self, field: Field, f: impl FnOnce(&mut StageElement)) {
        let Some(sel) = self.sel else { return };
        let before = self.draft.clone();
        let id = self.layout;
        let Some(e) = self.draft.iter_mut().find(|l| l.id == id)
            .and_then(|l| l.elements.iter_mut().filter_map(Lenient::known_mut).find(|e| e.id == sel)) else { return };
        f(e);
        if self.draft != before && self.last != Some((sel, field)) { self.undo.push(before) }
        self.last = Some((sel, field));
    }

    /// The chosen box, when this version knows it.
    pub fn selected_box(&self) -> Option<&StageElement> {
        let sel = self.sel?;
        self.current().elements.iter().filter_map(Lenient::known).find(|e| e.id == sel)
    }

    /// A name no other layout has: `base`, else "base 2", "base 3"…
    fn unique_name(&self, base: &str) -> String {
        let taken = |n: &str| self.all().iter().any(|l| l.name.trim().to_lowercase() == n.to_lowercase());
        if !taken(base) { return base.into() }
        (2..).map(|n| format!("{base} {n}")).find(|n| !taken(n)).unwrap_or_default()
    }

    pub fn select_layout(&mut self, index: usize) {
        if let Some(id) = self.all().get(index).map(|l| l.id) { self.layout = id; self.sel = None; self.last = None; }
    }

    /// A new empty layout, chosen.
    pub fn new_layout(&mut self) -> Uuid {
        let l = StageLayout { id: Uuid::new_v4(), name: self.unique_name(&s::new_layout_name()), background: crate::stage::BACKGROUND.into(), font: String::new(), elements: vec![] };
        self.add_layout(l)
    }

    /// A copy of the chosen layout (with new ids, so it is the church's own), chosen.
    pub fn duplicate(&mut self) -> Uuid {
        let mut l = self.current().clone();
        l.id = Uuid::new_v4();
        l.name = self.unique_name(&s::copy_name(&l.name));
        for e in l.elements.iter_mut().filter_map(Lenient::known_mut) { e.id = Uuid::new_v4() }
        self.add_layout(l)
    }

    fn add_layout(&mut self, l: StageLayout) -> Uuid {
        let id = l.id;
        self.undo.push(self.draft.clone());
        self.draft.push(l);
        (self.layout, self.sel, self.last) = (id, None, None);
        id
    }

    pub fn rename(&mut self, name: &str) { let name = name.trim().to_string(); self.edit(|l, _| l.name = name) }

    /// Remove the chosen church layout; the stage default (or Classic) is chosen next.
    pub fn delete(&mut self) {
        if self.locked() { return }
        self.undo.push(self.draft.clone());
        let id = self.layout;
        self.draft.retain(|l| l.id != id);
        self.layout = if presets::find_layout(&self.draft, self.default).is_some() { self.default } else { presets::CLASSIC };
        (self.sel, self.last) = (None, None);
    }

    /// Add a box of kind `index` (`presets::all_kinds`) in the middle, chosen.
    pub fn add_box(&mut self, index: usize) -> Result<(), String> {
        let Some(mut kind) = presets::all_kinds().get(index).cloned() else { return Ok(()) };
        if self.locked() { return Ok(()) }
        if self.current().elements.len() >= geom::MAX_BOXES { return Err(s::too_many_boxes()) }
        if let ElementKind::Text { text } = &mut kind { *text = s::your_words() }
        let el = presets::new_box(kind, Uuid::new_v4());
        let id = el.id;
        self.edit(|l, sel| { l.elements.push(Lenient::Known(el)); *sel = Some(id) });
        Ok(())
    }

    pub fn remove_box(&mut self) {
        let Some(id) = self.sel else { return };
        if self.selected_box().is_some_and(|e| e.locked) { return }
        self.edit(|l, sel| { if geom::delete_box(&mut l.elements, id) { *sel = None } });
    }

    pub fn select_box(&mut self, id: Uuid) {
        if self.current().elements.iter().any(|e| geom::element_id(e) == Some(id)) { self.sel = Some(id); self.last = None; }
    }

    /// Pointer down at `p` (%): a handle of the chosen box resizes it, a box is chosen and moved, the screen
    /// chooses nothing. `tol` is how far a handle reaches each way (%); `free` turns snapping off.
    pub fn press(&mut self, p: (f32, f32), tol: (f32, f32), free: bool) {
        self.drag = None;
        if self.locked() { return }
        let handle = self.selected_box().filter(|e| !e.locked).and_then(|e| handle_at(e.rect, p, tol).map(|h| (e.id, e.rect, h)));
        if let Some((id, orig, h)) = handle {
            self.drag = Some(Drag { id, handle: Some(h), start: p, orig, before: self.draft.clone(), moved: false, free });
            return;
        }
        self.sel = geom::hit_test(p, &self.current().elements);
        self.last = None;
        // N10: a locked box can be chosen but not moved.
        if let Some(e) = self.selected_box().filter(|e| !e.locked) {
            self.drag = Some(Drag { id: e.id, handle: None, start: p, orig: e.rect, before: self.draft.clone(), moved: false, free });
        }
    }

    /// The pointer moved to `p` (%) while down.
    pub fn drag_to(&mut self, p: (f32, f32)) {
        let Some((start, id, orig, handle, free)) = self.drag.as_ref().map(|d| (d.start, d.id, d.orig, d.handle, d.free)) else { return };
        let (dx, dy) = (p.0 - start.0, p.1 - start.1);
        let layout = self.layout;
        let others: Vec<PctRect> = self.current().elements.iter().filter_map(Lenient::known).filter(|e| e.id != id).map(|e| e.rect).collect();
        let (rect, guides) = drag_rect(orig, handle, dx, dy, free, &others);
        let Some(d) = self.drag.as_mut() else { return };
        if rect != d.orig { d.moved = true }
        self.guides = guides;
        if let Some(e) = self.draft.iter_mut().find(|l| l.id == layout)
            .and_then(|l| l.elements.iter_mut().filter_map(Lenient::known_mut).find(|e| e.id == id)) { e.rect = rect }
    }

    /// The pointer let go: a drag that changed something is one undo step.
    pub fn release(&mut self) {
        if let Some(d) = self.drag.take() { if d.moved && d.before != self.draft { self.undo.push(d.before) } }
        self.guides.clear();
        self.last = None;
    }

    pub fn nudge(&mut self, key: Nudge, shift: bool) {
        let Some(id) = self.sel else { return };
        if self.selected_box().is_some_and(|e| e.locked) { return }
        self.edit(|l, _| if let Some(e) = l.elements.iter_mut().filter_map(Lenient::known_mut).find(|e| e.id == id) { e.rect = geom::nudge(e.rect, key, shift) });
    }

    pub fn duplicate_box(&mut self) -> Result<(), String> {
        let Some(id) = self.sel else { return Ok(()) };
        let mut err = None;
        self.edit(|l, sel| match geom::duplicate_box(&mut l.elements, id, Uuid::new_v4()) {
            Ok(copy) => *sel = Some(copy),
            Err(e) => err = Some(e),
        });
        err.filter(|e| !e.is_empty()).map_or(Ok(()), Err)
    }

    pub fn next_box(&mut self, back: bool) { self.sel = geom::next_box(&self.current().elements, self.sel, back); self.last = None; }

    pub fn undo(&mut self) { if let Some(d) = self.undo.undo(self.draft.clone()) { self.draft = d; self.after_history() } }
    pub fn redo(&mut self) { if let Some(d) = self.undo.redo(self.draft.clone()) { self.draft = d; self.after_history() } }

    /// After undo or redo: a layout or box that is gone is no longer chosen.
    fn after_history(&mut self) {
        if presets::find_layout(&self.draft, self.layout).is_none() { self.layout = presets::CLASSIC }
        if self.sel.is_some_and(|id| !self.current().elements.iter().any(|e| geom::element_id(e) == Some(id))) { self.sel = None }
        self.last = None;
    }

    /// What the chosen box shows: kind `index` of `presets::all_kinds`. A Text box keeps its words.
    pub fn set_kind(&mut self, index: usize) {
        let Some(mut kind) = presets::all_kinds().get(index).cloned() else { return };
        let Some(id) = self.sel else { return };
        if let ElementKind::Text { text } = &mut kind { *text = s::your_words() }
        self.edit(|l, _| if let Some(e) = l.elements.iter_mut().filter_map(Lenient::known_mut).find(|e| e.id == id) {
            if presets::kind_index(&e.kind) != presets::kind_index(&kind) { e.kind = kind }
        });
    }

    pub fn set_size(&mut self, size: i32) {
        let v = (size as f32).clamp(geom::SIZE_MIN, geom::SIZE_MAX);
        self.edit_box(Field::Size, |e| e.style.max_size = Some(v));
    }

    /// A colour that doesn't read is ignored (the field keeps what was typed). A shape's colour is its fill
    /// (a line's is its text colour).
    pub fn set_color(&mut self, c: &str) {
        let c = c.trim().to_string();
        if !is_color(&c) { return }
        self.edit_box(Field::Color, |e| if fills(&e.kind) { e.style.fill = Some(c) } else { e.style.color = Some(c) });
    }

    /// N10: lock the chosen box so it can't be moved, resized or removed by mistake.
    pub fn set_locked(&mut self, on: bool) { self.edit_selected(|e| e.locked = on) }

    /// N10: keep the chosen box but don't draw it on the stage.
    pub fn set_hidden(&mut self, on: bool) { self.edit_selected(|e| e.hidden = on) }

    /// N10: a slide box shows the song's chords too.
    pub fn set_chords(&mut self, on: bool) {
        self.edit_selected(|e| match &mut e.kind {
            ElementKind::CurrentSlide { chords, .. } | ElementKind::NextSlide { chords, .. } => *chords = on,
            _ => {}
        })
    }

    /// N10: the chosen shape box's shape, `ShapeKind::ALL[i]`.
    pub fn set_shape(&mut self, i: usize) {
        let Some(k) = ShapeKind::ALL.get(i).copied() else { return };
        self.edit_selected(|e| if let ElementKind::Shape { shape } = &mut e.kind { *shape = k });
    }

    /// One undo step changing the chosen box (a known one).
    fn edit_selected(&mut self, f: impl FnOnce(&mut StageElement)) {
        let Some(id) = self.sel else { return };
        self.edit(|l, _| if let Some(e) = l.elements.iter_mut().filter_map(Lenient::known_mut).find(|e| e.id == id) { f(e) });
    }

    pub fn set_align(&mut self, i: i32) {
        let a = match i { 0 => Align::Left, 2 => Align::Right, _ => Align::Center };
        let Some(id) = self.sel else { return };
        self.edit(|l, _| if let Some(e) = l.elements.iter_mut().filter_map(Lenient::known_mut).find(|e| e.id == id) { e.style.align = Some(a) });
    }

    pub fn set_text(&mut self, t: &str) {
        let mut t = t.to_string();
        geom::truncate_chars(&mut t, 200);
        self.edit_box(Field::Text, |e| if let ElementKind::Text { text } = &mut e.kind { *text = t });
    }
}

/// A shape (but not a line) is coloured by its fill.
fn fills(kind: &ElementKind) -> bool { matches!(kind, ElementKind::Shape { shape } if *shape != ShapeKind::Line) }

/// The handle of `r` under `p`, corners first.
fn handle_at(r: PctRect, p: (f32, f32), tol: (f32, f32)) -> Option<Handle> {
    use Handle::*;
    let (l, t, rt, b, cx, cy) = (r.x, r.y, r.x + r.w, r.y + r.h, r.x + r.w / 2.0, r.y + r.h / 2.0);
    [(NW, l, t), (NE, rt, t), (SE, rt, b), (SW, l, b), (N, cx, t), (E, rt, cy), (S, cx, b), (W, l, cy)].into_iter()
        .find(|&(_, x, y)| (p.0 - x).abs() <= tol.0 && (p.1 - y).abs() <= tol.1).map(|(h, _, _)| h)
}

/// Where a dragged box goes. Moving snaps to other boxes' edges and middles (with a guide), else to the grid;
/// resizing snaps the dragged edges to the grid. `free` (Alt) snaps to nothing.
fn drag_rect(orig: PctRect, handle: Option<Handle>, dx: f32, dy: f32, free: bool, others: &[PctRect]) -> (PctRect, Vec<Guide>) {
    match handle {
        None => {
            let r = geom::move_by(orig, dx, dy);
            if free { return (r, vec![]) }
            let (mut out, guides) = geom::snap_to_guides(r, others, geom::GUIDE_TOL);
            if !guides.iter().any(|g| matches!(g, Guide::V(_))) { out.x = geom::snap(out.x, geom::GRID) }
            if !guides.iter().any(|g| matches!(g, Guide::H(_))) { out.y = geom::snap(out.y, geom::GRID) }
            (geom::move_by(out, 0.0, 0.0), guides)
        }
        Some(h) => {
            use Handle::*;
            let (mut dx, mut dy) = (dx, dy);
            if !free {
                let edge_x = if matches!(h, W | NW | SW) { orig.x } else { orig.x + orig.w };
                let edge_y = if matches!(h, N | NW | NE) { orig.y } else { orig.y + orig.h };
                dx = geom::snap(edge_x + dx, geom::GRID) - edge_x;
                dy = geom::snap(edge_y + dy, geom::GRID) - edge_y;
            }
            (geom::resize(orig, h, dx, dy, geom::MIN_BOX), vec![])
        }
    }
}

fn color(c: &str) -> slint::Color { let [r, g, b, a] = parse_color(c); slint::Color::from_argb_u8(a, r, g, b) }

/// The rect of a box this version doesn't know, from its JSON.
fn unknown_rect(v: &serde_json::Value) -> Option<PctRect> { v.get("rect").and_then(|r| serde_json::from_value(r.clone()).ok()) }

/// The boxes as the canvas draws them.
pub fn box_rows(ed: &Editor) -> Vec<StageBoxVm> {
    let (names, mut samples) = (s::kind_names(), s::sample_words());
    samples[7] = crate::stage::view::clock(chrono::Local::now().time(), false, false);
    ed.current().elements.iter().filter_map(|el| {
        let id = geom::element_id(el)?;
        let selected = ed.sel == Some(id);
        Some(match el {
            Lenient::Known(e) => {
                let st = presets::resolve_style(&e.kind, &e.style);
                let k = presets::kind_index(&e.kind);
                let sample = match &e.kind { ElementKind::Text { text } => text.clone(), _ => samples[k].clone() };
                let shape = match &e.kind { ElementKind::Shape { shape } => shape.index() as i32, _ => -1 };
                StageBoxVm {
                    id: id.to_string().into(), x: e.rect.x, y: e.rect.y, w: e.rect.w, h: e.rect.h,
                    name: names[k].as_str().into(), sample: sample.into(), color: color(&st.color),
                    fill: st.fill.as_deref().map_or(slint::Color::from_argb_u8(0, 0, 0, 0), color), size: st.max_size,
                    align: match st.align { Align::Left => 0, Align::Center => 1, Align::Right => 2 }, selected, unknown: false,
                    locked: e.locked, hidden: e.hidden, shape,
                }
            }
            Lenient::Unknown(v) => {
                let r = unknown_rect(v)?;
                StageBoxVm { id: id.to_string().into(), x: r.x, y: r.y, w: r.w, h: r.h, selected, unknown: true, shape: -1, ..Default::default() }
            }
        })
    }).collect()
}

/// Puts the editor into the dialog's global.
fn show(g: &StageLayoutsDlg, ed: &Editor) {
    let rows: Vec<StageLayoutRow> = ed.all().iter().map(|l| StageLayoutRow {
        name: l.name.as_str().into(), builtin: presets::is_builtin(l.id), on_stage: l.id == ed.default && !ed.changed_layout(l.id), selected: l.id == ed.layout,
    }).collect();
    g.set_layouts(ModelRc::new(VecModel::from(rows)));
    g.set_boxes(ModelRc::new(VecModel::from(box_rows(ed))));
    let guides: Vec<StageGuideVm> = ed.guides.iter().map(|g| match g {
        Guide::V(at) => StageGuideVm { vertical: true, at: *at },
        Guide::H(at) => StageGuideVm { vertical: false, at: *at },
    }).collect();
    g.set_guides(ModelRc::new(VecModel::from(guides)));
    g.set_screen_bg(color(&ed.current().background));
    g.set_locked(ed.locked());
    g.set_can_use(true);
    g.set_can_undo(ed.can_undo());
    g.set_can_redo(ed.can_redo());
    let unknown = ed.sel.is_some() && ed.selected_box().is_none();
    g.set_has_box(ed.sel.is_some());
    g.set_box_unknown(unknown);
    if let Some(e) = ed.selected_box() {
        let st = presets::resolve_style(&e.kind, &e.style);
        g.set_box_kind(presets::kind_index(&e.kind) as i32);
        g.set_box_size(st.max_size.round() as i32);
        // Typing a colour is not overwritten while it doesn't read yet.
        let shown = if fills(&e.kind) { st.fill.clone().unwrap_or_default() } else { st.color.clone() };
        if is_color(&g.get_box_color()) || !ed.typing_color() { g.set_box_color(shown.as_str().into()) }
        g.set_box_locked(e.locked);
        g.set_box_hidden(e.hidden);
        let chords = match &e.kind { ElementKind::CurrentSlide { chords, .. } | ElementKind::NextSlide { chords, .. } => Some(*chords), _ => None };
        g.set_box_is_slide(chords.is_some());
        g.set_box_chords(chords.unwrap_or(false));
        let shape = match &e.kind { ElementKind::Shape { shape } => Some(*shape), _ => None };
        g.set_box_is_shape(shape.is_some());
        g.set_box_shape(shape.map_or(0, |k| k.index() as i32));
        g.set_box_fills(fills(&e.kind));
        g.set_box_align(match st.align { Align::Left => 0, Align::Center => 1, Align::Right => 2 });
        let text = match &e.kind { ElementKind::Text { text } => Some(text.clone()), _ => None };
        g.set_box_is_text(text.is_some());
        if text.as_deref() != Some(g.get_box_text().as_str()) { g.set_box_text(text.unwrap_or_default().into()) }
    }
}

impl Editor {
    /// The chosen box's colour is being typed (the last change was to it).
    fn typing_color(&self) -> bool { matches!(self.last, Some((_, Field::Color))) }

    /// The layout `id` has changes not saved (it is not on stage as shown).
    fn changed_layout(&self, id: Uuid) -> bool {
        self.draft.iter().find(|l| l.id == id) != self.stored.iter().find(|l| l.id == id)
    }
}

/// Opens the editor on the stage default (Classic when it names no layout).
pub fn open(op: &Rc<Operator>) {
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    op.open_dialog(DialogKind::StageLayouts);
    let ed = Rc::new(RefCell::new(Editor::new(snap.data.stage_layouts.clone(), snap.data.settings.stage.layout)));
    let g = op.ui.global::<StageLayoutsDlg>();
    let names: Vec<SharedString> = s::kind_names().iter().map(|n| n.as_str().into()).collect();
    let help: Vec<SharedString> = s::kind_help().iter().map(|n| n.as_str().into()).collect();
    g.set_kind_names(ModelRc::new(VecModel::from(names)));
    g.set_kind_help(ModelRc::new(VecModel::from(help)));
    let shapes: Vec<SharedString> = s::shape_names().iter().map(|n| n.as_str().into()).collect();
    g.set_shape_names(ModelRc::new(VecModel::from(shapes)));
    g.set_error("".into());
    g.set_box_color("".into());
    g.set_box_text("".into());
    show(&g, &ed.borrow());
    connect(op, &ed);
}

/// Runs `f` on the editor, then redraws.
fn with(op: &Rc<Operator>, ed: &Rc<RefCell<Editor>>, f: impl FnOnce(&mut Editor)) {
    f(&mut ed.borrow_mut());
    show(&op.ui.global::<StageLayoutsDlg>(), &ed.borrow());
}

fn connect(op: &Rc<Operator>, ed: &Rc<RefCell<Editor>>) {
    let g = op.ui.global::<StageLayoutsDlg>();
    // A callback that changes the editor and redraws.
    macro_rules! on {
        ($set:ident, |$e:ident $(, $a:ident : $t:ty)*| $body:expr) => {{
            let (me, ed): (Weak<Operator>, Rc<RefCell<Editor>>) = (Rc::downgrade(op), ed.clone());
            g.$set(move |$($a: $t),*| if let Some(op) = me.upgrade() { with(&op, &ed, |$e| { $body; }) });
        }};
    }
    on!(on_select_layout, |e, i: i32| e.select_layout(i.max(0) as usize));
    on!(on_new_layout, |e| e.new_layout());
    on!(on_duplicate_layout, |e| e.duplicate());
    on!(on_select_box, |e, id: SharedString| if let Ok(id) = id.parse() { e.select_box(id) });
    on!(on_remove_box, |e| e.remove_box());
    on!(on_press, |e, x: f32, y: f32, tx: f32, ty: f32, free: bool| e.press((x, y), (tx, ty), free));
    on!(on_drag, |e, x: f32, y: f32| e.drag_to((x, y)));
    on!(on_release, |e| e.release());
    on!(on_nudge, |e, k: i32, shift: bool| e.nudge([Nudge::Left, Nudge::Right, Nudge::Up, Nudge::Down][k.clamp(0, 3) as usize], shift));
    on!(on_undo, |e| e.undo());
    on!(on_redo, |e| e.redo());
    on!(on_next_box, |e, back: bool| e.next_box(back));
    on!(on_kind_changed, |e, i: i32| e.set_kind(i.max(0) as usize));
    on!(on_size_changed, |e, v: i32| e.set_size(v));
    on!(on_color_changed, |e, c: SharedString| e.set_color(&c));
    on!(on_align_changed, |e, i: i32| e.set_align(i));
    on!(on_text_edited, |e, t: SharedString| e.set_text(&t));
    on!(on_locked_changed, |e, v: bool| e.set_locked(v));
    on!(on_hidden_changed, |e, v: bool| e.set_hidden(v));
    on!(on_chords_changed, |e, v: bool| e.set_chords(v));
    on!(on_shape_changed, |e, i: i32| e.set_shape(i.max(0) as usize));
    let toast_err = |op: &Rc<Operator>, r: Result<(), String>| if let Err(e) = r { op.toast(e, true) };
    let (me, ed2) = (Rc::downgrade(op), ed.clone());
    g.on_add_box(move |i| if let Some(op) = me.upgrade() {
        let r = ed2.borrow_mut().add_box(i.max(0) as usize);
        show(&op.ui.global::<StageLayoutsDlg>(), &ed2.borrow());
        toast_err(&op, r);
    });
    let (me, ed2) = (Rc::downgrade(op), ed.clone());
    g.on_duplicate_box(move || if let Some(op) = me.upgrade() {
        let r = ed2.borrow_mut().duplicate_box();
        show(&op.ui.global::<StageLayoutsDlg>(), &ed2.borrow());
        toast_err(&op, r);
    });
    let (me, ed2) = (Rc::downgrade(op), ed.clone());
    g.on_rename_layout(move || if let Some(op) = me.upgrade() { rename(&op, &ed2) });
    let (me, ed2) = (Rc::downgrade(op), ed.clone());
    g.on_delete_layout(move || if let Some(op) = me.upgrade() { delete(&op, &ed2) });
    let (me, ed2) = (Rc::downgrade(op), ed.clone());
    g.on_use_layout(move || if let Some(op) = me.upgrade() { use_layout(&op, &ed2) });
    let (me, ed2) = (Rc::downgrade(op), ed.clone());
    g.on_save(move || if let Some(op) = me.upgrade() { save(&op, &ed2, true, |_| {}) });
    let (me, ed2) = (Rc::downgrade(op), ed.clone());
    g.on_cancel(move || if let Some(op) = me.upgrade() { cancel(&op, &ed2) });
}

fn rename(op: &Rc<Operator>, ed: &Rc<RefCell<Editor>>) {
    if ed.borrow().locked() { return }
    let name = ed.borrow().current().name.clone();
    let (me, ed) = (Rc::downgrade(op), ed.clone());
    prompt(op, &s::rename_title(), &name, move |typed| {
        let (Some(op), Some(typed)) = (me.upgrade(), typed) else { return };
        if op.dialog() != Some(DialogKind::StageLayouts) { return }
        with(&op, &ed, |e| e.rename(&typed));
    });
}

/// Asks first, naming everything that uses the layout (`stage/layout/refs`).
fn delete(op: &Rc<Operator>, ed: &Rc<RefCell<Editor>>) {
    if ed.borrow().locked() { return }
    let (id, name) = { let e = ed.borrow(); (e.layout, e.current().name.clone()) };
    let (me, ed) = (Rc::downgrade(op), ed.clone());
    op.ops().call::<serde_json::Value>("stage/layout/refs", serde_json::json!({ "id": id, "name": name }), move |r| {
        let Some(op) = me.upgrade() else { return };
        let question = match r {
            Ok(v) => v["question"].as_str().unwrap_or_default().to_string(),
            Err(e) => return op.toast(e, true),
        };
        let me = Rc::downgrade(&op);
        confirm(&op, &question, &s::delete(), &dlg::cancel(), true, move |ok| {
            let Some(op) = me.upgrade().filter(|_| ok) else { return };
            if op.dialog() != Some(DialogKind::StageLayouts) || ed.borrow().layout != id { return }
            with(&op, &ed, Editor::delete);
        });
    });
}

/// Saves every change in one `stage/layouts/replace`. On an error nothing is saved and the dialog stays open with
/// the error under it; otherwise `close` closes it, then `then` runs.
fn save(op: &Rc<Operator>, ed: &Rc<RefCell<Editor>>, close: bool, then: impl FnOnce(&Rc<Operator>) + 'static) {
    let body = ed.borrow().save_body();
    let (me, ed) = (Rc::downgrade(op), ed.clone());
    op.ops().call::<serde_json::Value>("stage/layouts/replace", body, move |r| {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<StageLayoutsDlg>();
        match r {
            Ok(_) => {
                g.set_error("".into());
                ed.borrow_mut().saved();
                if close {
                    op.close_dialog();
                    op.toast(s::saved(), false);
                } else {
                    show(&g, &ed.borrow());
                }
                then(&op);
            }
            Err(e) => g.set_error(e.into()),
        }
    });
}

/// "Use on stage": saves first when anything changed, then makes the chosen layout the stage default.
fn use_layout(op: &Rc<Operator>, ed: &Rc<RefCell<Editor>>) {
    let (id, name) = { let e = ed.borrow(); (e.layout, e.current().name.clone()) };
    let ed2 = ed.clone();
    let set = move |op: &Rc<Operator>| {
        let (me, ed) = (Rc::downgrade(op), ed2.clone());
        set_default(op, id, &name, move |_| if let Some(op) = me.upgrade() { with(&op, &ed, |e| e.default = id) });
    };
    if ed.borrow().changed() { save(op, ed, false, set) } else { set(op) }
}

/// Sends `stage/layout/default` and says which layout the stage shows.
fn set_default(op: &Rc<Operator>, id: Uuid, name: &str, done: impl FnOnce(&Rc<Operator>) + 'static) {
    let (me, name) = (Rc::downgrade(op), name.to_string());
    op.ops().call::<serde_json::Value>("stage/layout/default", serde_json::json!({ "id": id }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => { op.toast(s::now_on_stage(&name), false); done(&op) }
            Err(e) => op.toast(e, true),
        }
    });
}

/// Cancel: asks first only when something changed.
fn cancel(op: &Rc<Operator>, ed: &Rc<RefCell<Editor>>) {
    if !ed.borrow().changed() { return op.close_dialog() }
    let me = Rc::downgrade(op);
    confirm(op, &s::discard_question(), &s::discard(), &s::keep_editing(), true, move |ok| {
        if let Some(op) = me.upgrade().filter(|_| ok) { op.close_dialog() }
    });
}

/// The top bar's picker: every layout's name, the one on stage, and what the rows do. Connected once per window.
pub(crate) fn wire(op: &Rc<Operator>) {
    let g = op.ui.global::<StageLayoutsDlg>();
    let me = Rc::downgrade(op);
    g.on_fill_picker(move || if let Some(op) = me.upgrade() {
        let Some(snap) = op.state.borrow().snap.clone() else { return };
        let (names, current) = picker(&snap.data.stage_layouts, snap.data.settings.stage.layout);
        let g = op.ui.global::<StageLayoutsDlg>();
        g.set_picker_names(ModelRc::new(VecModel::from(names.into_iter().map(SharedString::from).collect::<Vec<_>>())));
        g.set_picker_current(current as i32);
    });
    let me = Rc::downgrade(op);
    g.on_pick(move |i| if let Some(op) = me.upgrade() {
        let Some(snap) = op.state.borrow().snap.clone() else { return };
        let Some(l) = presets::all_layouts(&snap.data.stage_layouts).nth(i.max(0) as usize) else { return };
        set_default(&op, l.id, &l.name, |_| {});
    });
    let me = Rc::downgrade(op);
    g.on_edit_layouts(move || if let Some(op) = me.upgrade() { open(&op) });
}

/// Every layout's name, and the index of the one on stage (Classic when the default names no layout).
pub fn picker(church: &[StageLayout], default: Uuid) -> (Vec<String>, usize) {
    let all: Vec<&StageLayout> = presets::all_layouts(church).collect();
    let current = all.iter().position(|l| l.id == default).unwrap_or(0);
    (all.iter().map(|l| l.name.clone()).collect(), current)
}

/// Settings → General → Screens and graphics: the stage layout choice, filled from the stored data.
pub(crate) fn fill_settings(g: &SettingsDlg, church: &[StageLayout], default: Uuid) {
    let (names, current) = picker(church, default);
    let ids: Vec<SharedString> = presets::all_layouts(church).map(|l| l.id.to_string().into()).collect();
    g.set_stage_layout_names(ModelRc::new(VecModel::from(names.into_iter().map(SharedString::from).collect::<Vec<_>>())));
    g.set_stage_layout_ids(ModelRc::new(VecModel::from(ids)));
    g.set_stage_layout(current as i32);
}

/// The stage layout chosen in Settings → General → Screens and graphics.
pub(crate) fn read_settings(g: &SettingsDlg, s: &mut crate::model::Settings) {
    use slint::Model;
    if let Some(id) = g.get_stage_layout_ids().row_data(g.get_stage_layout().max(0) as usize).and_then(|id| id.parse().ok()) { s.stage.layout = id }
}

#[cfg(test)]
#[path = "stage_layouts_tests.rs"]
mod tests;
