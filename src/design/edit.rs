//! The slide editor's logic (N26, N27), with no UI: the slides being edited, what is chosen, dragging with snap
//! guides, align and distribute, order, builds, undo, and copy / paste of a style. Reuses the stage layout editor's
//! geometry (`stage::geom`).
use super::{Design, HAlign, Look, Obj, ObjKind, Outline, PctRect, Shape, TextStyle, TransitionKind, VAlign, MAX_OBJECTS, MAX_SLIDES, strings as s};
use crate::stage::geom::{self, Guide, Handle, Nudge, UndoStack};
use uuid::Uuid;

/// The editor's grid step, the smallest object and how far an arrow moves (all in %).
pub const GRID: f32 = 1.0;
pub const MIN_OBJ: f32 = 2.0;
pub const STEP: f32 = 1.0;

/// Which setting was changed last, so a slider drag or typing is one undo step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field { Text, Size, Color, Font, Rotation, Opacity, Fill, OutlineColor, OutlineWidth, Shadow, Soft, Radius, Notes, TransitionMs, PathColor }

/// Align the chosen objects: to each other when there are several, else to the slide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignTo { Left, Center, Right, Top, Middle, Bottom }

/// Changes to the drawing order of the chosen object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order { Front, Forward, Backward, Back }

/// A text style and a look, copied from one object to paste on others (N27).
#[derive(Clone, Debug, PartialEq)]
pub struct StyleCopy { pub text: Option<TextStyle>, pub look: Look, pub opacity: u8 }

/// What is being dragged.
#[derive(Clone, Debug)]
struct Drag { handle: Option<Handle>, start: (f32, f32), orig: Vec<(Uuid, PctRect)>, before: Vec<Design>, moved: bool, free: bool }

/// The slides of one item while they are edited.
pub struct Editor {
    stored: Vec<Design>,
    pub slides: Vec<Design>,
    /// The slide shown.
    pub at: usize,
    /// The chosen objects; the first is the one the side panel shows.
    pub sel: Vec<Uuid>,
    undo: UndoStack<(Vec<Design>, usize)>,
    drag: Option<Drag>,
    pub guides: Vec<Guide>,
    last: Option<Field>,
    /// The line being drawn with the pen, and the slides before it.
    pen: Option<(Uuid, Vec<Design>)>,
}

impl Editor {
    /// Edits `slides`; an empty item starts with one blank slide.
    pub fn new(slides: Vec<Design>) -> Editor {
        let stored = slides.clone();
        let slides = if slides.is_empty() { vec![Design::default()] } else { slides };
        Editor { stored, slides, at: 0, sel: vec![], undo: UndoStack::new(100), drag: None, guides: vec![], last: None, pen: None }
    }

    pub fn changed(&self) -> bool { self.slides != self.stored && !(self.stored.is_empty() && self.slides == [Design::default()]) }
    pub fn saved(&mut self) { self.stored = self.slides.clone() }
    pub fn can_undo(&self) -> bool { self.undo.can_undo() }
    /// The setting changed last (a field being typed in is not overwritten while it doesn't read yet).
    pub fn last_field(&self) -> Option<Field> { self.last }
    pub fn can_redo(&self) -> bool { self.undo.can_redo() }

    pub fn slide(&self) -> &Design { &self.slides[self.at.min(self.slides.len() - 1)] }
    fn slide_mut(&mut self) -> &mut Design { let i = self.at.min(self.slides.len() - 1); &mut self.slides[i] }

    /// The object the side panel shows.
    pub fn primary(&self) -> Option<&Obj> { self.sel.first().and_then(|id| self.slide().objects.iter().find(|o| o.id == *id)) }

    fn chosen(&self) -> Vec<&Obj> { self.slide().objects.iter().filter(|o| self.sel.contains(&o.id)).collect() }

    /// One undo step around `f`; nothing is kept when nothing changed.
    fn edit(&mut self, f: impl FnOnce(&mut Editor)) {
        let before = (self.slides.clone(), self.at);
        f(self);
        self.last = None;
        if self.slides != before.0 { self.undo.push(before) }
    }

    /// Like `edit` on every chosen object, but repeated changes of one `field` make one undo step.
    fn edit_chosen(&mut self, field: Field, mut f: impl FnMut(&mut Obj)) {
        let before = (self.slides.clone(), self.at);
        let sel = self.sel.clone();
        for o in self.slide_mut().objects.iter_mut().filter(|o| sel.contains(&o.id)) { f(o) }
        if self.slides != before.0 && self.last != Some(field) { self.undo.push(before) }
        self.last = Some(field);
    }

    /// Like `edit_chosen` for the slide itself.
    fn edit_slide(&mut self, field: Option<Field>, f: impl FnOnce(&mut Design)) {
        let before = (self.slides.clone(), self.at);
        f(self.slide_mut());
        if self.slides != before.0 && (field.is_none() || self.last != field) { self.undo.push(before) }
        self.last = field;
    }

    pub fn undo(&mut self) { if let Some(b) = self.undo.undo((self.slides.clone(), self.at)) { self.restore(b) } }
    pub fn redo(&mut self) { if let Some(b) = self.undo.redo((self.slides.clone(), self.at)) { self.restore(b) } }

    fn restore(&mut self, (slides, at): (Vec<Design>, usize)) {
        self.slides = slides;
        self.at = at.min(self.slides.len() - 1);
        let ids: Vec<Uuid> = self.slide().objects.iter().map(|o| o.id).collect();
        self.sel.retain(|id| ids.contains(id));
        self.last = None;
    }

    // ---- slides ----

    pub fn select_slide(&mut self, i: usize) {
        if i < self.slides.len() && i != self.at { self.at = i; self.sel.clear(); self.last = None; }
    }

    /// A new blank slide after this one (`copy`: a copy of this one), shown.
    pub fn add_slide(&mut self, copy: bool) -> Result<(), String> {
        if self.slides.len() >= MAX_SLIDES { return Err(s::too_many_slides()) }
        self.edit(|e| {
            let mut new = if copy { e.slide().clone() } else { Design { transition: e.slide().transition, ..Design::default() } };
            for o in &mut new.objects { o.id = Uuid::new_v4() }
            e.slides.insert(e.at + 1, new);
            e.at += 1;
            e.sel.clear();
        });
        Ok(())
    }

    /// Removes this slide; the last one left is emptied instead.
    pub fn delete_slide(&mut self) {
        self.edit(|e| {
            if e.slides.len() == 1 { e.slides[0] = Design::default() } else { e.slides.remove(e.at); }
            e.at = e.at.min(e.slides.len() - 1);
            e.sel.clear();
        });
    }

    /// Moves this slide one place earlier (`up`) or later.
    pub fn move_slide(&mut self, up: bool) {
        let (i, n) = (self.at, self.slides.len());
        let j = if up { i.checked_sub(1) } else { (i + 1 < n).then_some(i + 1) };
        let Some(j) = j else { return };
        self.edit(|e| { e.slides.swap(i, j); e.at = j });
    }

    pub fn set_notes(&mut self, notes: &str) { let n = notes.to_string(); self.edit_slide(Some(Field::Notes), |d| d.notes = n) }

    pub fn set_transition(&mut self, kind: TransitionKind) { self.edit_slide(None, |d| d.transition.kind = kind) }

    pub fn set_transition_ms(&mut self, ms: i32) {
        let ms = (ms.max(0) as u32).min(super::TRANSITION_MAX_MS);
        self.edit_slide(Some(Field::TransitionMs), |d| d.transition.ms = ms)
    }

    /// This slide's transition on every slide.
    pub fn transition_to_all(&mut self) {
        let t = self.slide().transition;
        self.edit(|e| for d in &mut e.slides { d.transition = t });
    }

    // ---- objects ----

    /// Adds an object of `kind` in the middle of the slide (a line or arrow: a thin box), chosen.
    pub fn add(&mut self, kind: ObjKind) -> Result<Uuid, String> {
        if self.slide().objects.len() >= MAX_OBJECTS { return Err(s::too_many_objects()) }
        let rect = match &kind {
            ObjKind::Text { .. } => PctRect::new(20.0, 40.0, 60.0, 20.0),
            ObjKind::Shape { shape: Shape::Line | Shape::Arrow } => PctRect::new(30.0, 47.5, 40.0, 5.0),
            ObjKind::Path { .. } => PctRect::new(0.0, 0.0, 100.0, 100.0),
            _ => PctRect::new(30.0, 30.0, 40.0, 40.0),
        };
        let mut o = Obj::new(kind, rect);
        if let ObjKind::Shape { shape } = &o.kind {
            o.look.fill = (!matches!(shape, Shape::Line | Shape::Arrow)).then(|| "#3b82f6".to_string());
            if matches!(shape, Shape::Line | Shape::Arrow) { o.look.outline = Some(Outline { color: "#ffffff".into(), width: 8.0 }) }
        }
        let id = o.id;
        self.edit(|e| { e.slide_mut().objects.push(o); e.sel = vec![id] });
        Ok(id)
    }

    /// A new text box with the editor's sample words.
    pub fn add_text(&mut self) -> Result<Uuid, String> { self.add(ObjKind::Text { text: s::sample_text(), style: TextStyle::default() }) }

    /// A new web page, its address still to type.
    pub fn add_web(&mut self) -> Result<Uuid, String> { self.add(ObjKind::Web { url: String::new() }) }

    pub fn delete(&mut self) {
        let sel = std::mem::take(&mut self.sel);
        self.edit(|e| e.slide_mut().objects.retain(|o| !sel.contains(&o.id)));
    }

    /// Copies of the chosen objects, a little right and down, chosen instead.
    pub fn duplicate(&mut self) -> Result<(), String> {
        if self.sel.is_empty() { return Ok(()) }
        if self.slide().objects.len() + self.sel.len() > MAX_OBJECTS { return Err(s::too_many_objects()) }
        self.edit(|e| {
            let copies: Vec<Obj> = e.chosen().into_iter().cloned().map(|mut o| { o.id = Uuid::new_v4(); o.rect = geom::move_by(o.rect, 2.0, 2.0); o }).collect();
            e.sel = copies.iter().map(|o| o.id).collect();
            e.slide_mut().objects.extend(copies);
        });
        Ok(())
    }

    pub fn select(&mut self, id: Uuid, add: bool) {
        if !self.slide().objects.iter().any(|o| o.id == id) { return }
        if add {
            if let Some(i) = self.sel.iter().position(|s| *s == id) { self.sel.remove(i); } else { self.sel.push(id) }
        } else {
            self.sel = vec![id];
        }
        self.last = None;
    }

    pub fn select_all(&mut self) { self.sel = self.slide().objects.iter().map(|o| o.id).collect(); self.last = None }

    /// Tab / Shift+Tab: the next (or previous) object in drawing order, wrapping.
    pub fn next(&mut self, back: bool) {
        let ids: Vec<Uuid> = self.slide().objects.iter().map(|o| o.id).collect();
        if ids.is_empty() { return }
        let n = ids.len();
        let at = self.sel.first().and_then(|s| ids.iter().position(|i| i == s));
        let i = match (at, back) { (None, false) => 0, (None, true) => n - 1, (Some(i), false) => (i + 1) % n, (Some(i), true) => (i + n - 1) % n };
        self.sel = vec![ids[i]];
        self.last = None;
    }

    /// The topmost object under `p` (%).
    pub fn hit(&self, p: (f32, f32)) -> Option<Uuid> {
        self.slide().objects.iter().rev().find(|o| inside(o, p)).map(|o| o.id)
    }

    /// Pointer down at `p` (%): a handle of the chosen object resizes it, an object is chosen (`add`: Shift adds or
    /// takes it out) and moved with the others chosen, the bare slide chooses nothing. `tol`: how far a handle reaches
    /// (%); `free` (Alt) turns snapping off.
    pub fn press(&mut self, p: (f32, f32), tol: (f32, f32), add: bool, free: bool) {
        self.drag = None;
        self.last = None;
        let before = self.slides.clone();
        if self.sel.len() == 1 && let Some(o) = self.primary() && let Some(h) = handle_at(o.rect, p, tol) {
            self.drag = Some(Drag { handle: Some(h), start: p, orig: vec![(o.id, o.rect)], before, moved: false, free });
            return;
        }
        match self.hit(p) {
            Some(id) if add => { self.select(id, true); }
            Some(id) => { if !self.sel.contains(&id) { self.sel = vec![id] } else { self.sel.retain(|s| *s != id); self.sel.insert(0, id) } }
            None => { if !add { self.sel.clear() } return }
        }
        let orig = self.chosen().iter().map(|o| (o.id, o.rect)).collect();
        self.drag = Some(Drag { handle: None, start: p, orig, before, moved: false, free });
    }

    /// The pointer moved to `p` (%) while down. Moving snaps the first chosen object to the others' edges and
    /// middles and the slide's (with a guide), else to the grid; resizing snaps the dragged edges to the grid.
    pub fn drag_to(&mut self, p: (f32, f32)) {
        let Some(d) = self.drag.clone() else { return };
        let (dx, dy) = (p.0 - d.start.0, p.1 - d.start.1);
        let ids: Vec<Uuid> = d.orig.iter().map(|(id, _)| *id).collect();
        let others: Vec<PctRect> = self.slide().objects.iter().filter(|o| !ids.contains(&o.id)).map(|o| o.rect).collect();
        let Some(&(_, first)) = d.orig.first() else { return };
        let mut rects: Vec<(Uuid, PctRect)> = Vec::new();
        self.guides.clear();
        match d.handle {
            Some(h) => {
                use Handle::*;
                let (mut dx, mut dy) = (dx, dy);
                if !d.free {
                    let ex = if matches!(h, W | NW | SW) { first.x } else { first.x + first.w };
                    let ey = if matches!(h, N | NW | NE) { first.y } else { first.y + first.h };
                    dx = geom::snap(ex + dx, GRID) - ex;
                    dy = geom::snap(ey + dy, GRID) - ey;
                }
                rects.push((d.orig[0].0, geom::resize(first, h, dx, dy, MIN_OBJ)));
            }
            None => {
                let moved = geom::move_by(first, dx, dy);
                let target = if d.free { moved } else {
                    let (mut out, guides) = geom::snap_to_guides(moved, &others, geom::GUIDE_TOL);
                    if !guides.iter().any(|g| matches!(g, Guide::V(_))) { out.x = geom::snap(out.x, GRID) }
                    if !guides.iter().any(|g| matches!(g, Guide::H(_))) { out.y = geom::snap(out.y, GRID) }
                    self.guides = guides;
                    geom::move_by(out, 0.0, 0.0)
                };
                let (sx, sy) = (target.x - first.x, target.y - first.y);
                rects.extend(d.orig.iter().map(|(id, r)| (*id, geom::move_by(*r, sx, sy))));
            }
        }
        for (id, r) in rects {
            if let Some(o) = self.slide_mut().objects.iter_mut().find(|o| o.id == id) { o.rect = r }
        }
        if let Some(dd) = self.drag.as_mut() { dd.moved = true }
    }

    /// The pointer let go: a drag that changed something is one undo step.
    pub fn release(&mut self) {
        if let Some(d) = self.drag.take() && d.moved && d.before != self.slides { self.undo.push((d.before, self.at)) }
        self.guides.clear();
    }

    /// An arrow: moves the chosen objects one step; with Shift it makes the first wider or taller.
    pub fn nudge(&mut self, key: Nudge, shift: bool) {
        let (dx, dy) = match key { Nudge::Left => (-STEP, 0.0), Nudge::Right => (STEP, 0.0), Nudge::Up => (0.0, -STEP), Nudge::Down => (0.0, STEP) };
        let sel = self.sel.clone();
        let first = sel.first().copied();
        self.edit(|e| for o in e.slide_mut().objects.iter_mut().filter(|o| sel.contains(&o.id)) {
            if shift {
                if Some(o.id) == first { o.rect = geom::resize(o.rect, if dx != 0.0 { Handle::E } else { Handle::S }, dx, dy, MIN_OBJ) }
            } else {
                o.rect = geom::move_by(o.rect, dx, dy)
            }
        });
    }

    /// Lines the chosen objects up: several to each other, one to the slide.
    pub fn align(&mut self, to: AlignTo) {
        let chosen = self.chosen();
        if chosen.is_empty() { return }
        let area = if chosen.len() == 1 { PctRect::new(0.0, 0.0, 100.0, 100.0) } else { bounds(chosen.iter().map(|o| o.rect)) };
        let sel = self.sel.clone();
        self.edit(|e| for o in e.slide_mut().objects.iter_mut().filter(|o| sel.contains(&o.id)) {
            let r = &mut o.rect;
            match to {
                AlignTo::Left => r.x = area.x,
                AlignTo::Center => r.x = area.x + (area.w - r.w) / 2.0,
                AlignTo::Right => r.x = area.x + area.w - r.w,
                AlignTo::Top => r.y = area.y,
                AlignTo::Middle => r.y = area.y + (area.h - r.h) / 2.0,
                AlignTo::Bottom => r.y = area.y + area.h - r.h,
            }
        });
    }

    /// Spreads three or more chosen objects evenly, across (`across`) or down, the outer two staying put.
    pub fn distribute(&mut self, across: bool) {
        let mut chosen: Vec<(Uuid, PctRect)> = self.chosen().iter().map(|o| (o.id, o.rect)).collect();
        if chosen.len() < 3 { return }
        let key = |r: &PctRect| if across { r.x } else { r.y };
        let len = |r: &PctRect| if across { r.w } else { r.h };
        chosen.sort_by(|a, b| key(&a.1).total_cmp(&key(&b.1)));
        let (first, last) = (chosen[0].1, chosen[chosen.len() - 1].1);
        let span = key(&last) + len(&last) - key(&first);
        let total: f32 = chosen.iter().map(|(_, r)| len(r)).sum();
        let gap = (span - total) / (chosen.len() - 1) as f32;
        let mut at = key(&first);
        let mut place = Vec::new();
        for (id, r) in &chosen { place.push((*id, at)); at += len(r) + gap; }
        self.edit(|e| for (id, v) in place {
            if let Some(o) = e.slide_mut().objects.iter_mut().find(|o| o.id == id) { if across { o.rect.x = v } else { o.rect.y = v } }
        });
    }

    /// Changes where the first chosen object is drawn among the others.
    pub fn order(&mut self, how: Order) {
        let Some(id) = self.sel.first().copied() else { return };
        self.edit(|e| {
            let objs = &mut e.slide_mut().objects;
            let Some(i) = objs.iter().position(|o| o.id == id) else { return };
            let n = objs.len();
            let j = match how { Order::Front => n - 1, Order::Back => 0, Order::Forward => (i + 1).min(n - 1), Order::Backward => i.saturating_sub(1) };
            let o = objs.remove(i);
            objs.insert(j, o);
        });
    }

    // ---- the chosen objects' settings ----

    pub fn set_text(&mut self, t: &str) {
        let t = t.to_string();
        self.edit_chosen(Field::Text, |o| match &mut o.kind {
            ObjKind::Text { text, .. } => *text = t.clone(),
            ObjKind::Web { url } => *url = t.clone(),
            _ => {}
        });
    }

    /// Changes the text style of the chosen text boxes.
    pub fn set_style(&mut self, field: Field, f: impl Fn(&mut TextStyle)) {
        self.edit_chosen(field, |o| if let ObjKind::Text { style, .. } = &mut o.kind { f(style) });
    }

    pub fn set_size(&mut self, px: f32) { let px = px.clamp(super::SIZE_MIN, super::SIZE_MAX); self.set_style(Field::Size, |s| s.size = px) }
    pub fn set_bold(&mut self, on: bool) { self.set_style_once(|s| s.bold = on) }
    pub fn set_italic(&mut self, on: bool) { self.set_style_once(|s| s.italic = on) }
    pub fn set_align(&mut self, a: HAlign) { self.set_style_once(|s| s.align = a) }
    pub fn set_valign(&mut self, a: VAlign) { self.set_style_once(|s| s.valign = a) }
    pub fn set_shrink(&mut self, on: bool) { self.set_style_once(|s| s.shrink = on) }

    fn set_style_once(&mut self, f: impl Fn(&mut TextStyle)) { self.last = None; self.set_style(Field::Text, f); self.last = None }

    pub fn set_rotation(&mut self, deg: f32) {
        let deg = ((deg + 180.0).rem_euclid(360.0)) - 180.0;
        self.edit_chosen(Field::Rotation, |o| o.rotation = deg);
    }

    pub fn set_opacity(&mut self, v: i32) { let v = v.clamp(0, 100) as u8; self.edit_chosen(Field::Opacity, |o| o.opacity = v) }

    /// A colour (`None`: no fill). A colour that doesn't read is ignored.
    pub fn set_fill(&mut self, c: Option<&str>) {
        let c = c.map(|c| c.trim().to_string());
        if c.as_deref().is_some_and(|c| !crate::render::color::is_color(c)) { return }
        self.edit_chosen(Field::Fill, |o| o.look.fill = c.clone());
    }

    pub fn set_outline(&mut self, on: bool) {
        self.last = None;
        self.edit_chosen(Field::OutlineWidth, |o| o.look.outline = on.then(|| o.look.outline.clone().unwrap_or(Outline { color: "#ffffff".into(), width: 4.0 })));
        self.last = None;
    }

    pub fn set_outline_color(&mut self, c: &str) {
        let c = c.trim().to_string();
        if !crate::render::color::is_color(&c) { return }
        self.edit_chosen(Field::OutlineColor, |o| if let Some(ol) = &mut o.look.outline { ol.color = c.clone() });
    }

    pub fn set_outline_width(&mut self, w: f32) {
        let w = w.clamp(0.0, 100.0);
        self.edit_chosen(Field::OutlineWidth, |o| if let Some(ol) = &mut o.look.outline { ol.width = w });
    }

    pub fn set_shadow(&mut self, on: bool) {
        self.last = None;
        self.edit_chosen(Field::Shadow, |o| o.look.shadow = on.then(|| o.look.shadow.clone().unwrap_or_default()));
        self.last = None;
    }

    pub fn set_shadow_blur(&mut self, b: f32) {
        let b = b.clamp(0.0, 200.0);
        self.edit_chosen(Field::Shadow, |o| if let Some(sh) = &mut o.look.shadow { sh.blur = b });
    }

    pub fn set_soft(&mut self, px: f32) { let px = px.clamp(0.0, 400.0); self.edit_chosen(Field::Soft, |o| o.look.soft = px) }
    pub fn set_radius(&mut self, px: f32) { let px = px.clamp(0.0, 1000.0); self.edit_chosen(Field::Radius, |o| o.look.radius = px) }

    pub fn set_shape(&mut self, shape: Shape) {
        self.last = None;
        self.edit_chosen(Field::Fill, |o| if let ObjKind::Shape { shape: s } = &mut o.kind { *s = shape });
        self.last = None;
    }

    pub fn set_fit(&mut self, fit: super::Fit) {
        self.last = None;
        self.edit_chosen(Field::Fill, |o| if let ObjKind::Picture { fit: f, .. } = &mut o.kind { *f = fit });
        self.last = None;
    }

    // ---- the pen ----

    /// Starts a drawn line at `p` (%), in `color`, `width` 1080p pixels wide.
    pub fn pen_down(&mut self, p: (f32, f32), color: &str, width: f32) -> Result<(), String> {
        if self.slide().objects.len() >= MAX_OBJECTS { return Err(s::too_many_objects()) }
        let before = self.slides.clone();
        let o = Obj::new(ObjKind::Path { points: vec![(p.0 / 100.0, p.1 / 100.0)], width, color: color.to_string() }, PctRect::new(0.0, 0.0, 100.0, 100.0));
        let id = o.id;
        self.slide_mut().objects.push(o);
        self.sel = vec![id];
        self.pen = Some((id, before));
        Ok(())
    }

    /// The pen moved to `p` (%).
    pub fn pen_to(&mut self, p: (f32, f32)) {
        let Some(id) = self.pen.as_ref().map(|(id, _)| *id) else { return };
        let q = ((p.0 / 100.0).clamp(0.0, 1.0), (p.1 / 100.0).clamp(0.0, 1.0));
        if let Some(ObjKind::Path { points, .. }) = self.slide_mut().objects.iter_mut().find(|o| o.id == id).map(|o| &mut o.kind) {
            let far = points.last().is_none_or(|l| (l.0 - q.0).abs() + (l.1 - q.1).abs() > 0.002);
            if far && points.len() < 2000 { points.push(q) }
        }
    }

    /// The pen lifted: the line's box shrinks to what was drawn, and the line is one undo step. A dot is dropped.
    pub fn pen_up(&mut self) {
        let Some((id, before)) = self.pen.take() else { return };
        let objs = &mut self.slide_mut().objects;
        let Some(i) = objs.iter().position(|o| o.id == id) else { return };
        let ObjKind::Path { points, .. } = &mut objs[i].kind else { return };
        if points.len() < 2 { objs.remove(i); self.sel.clear(); return }
        let (mut l, mut t, mut r, mut b) = (1.0f32, 1.0f32, 0.0f32, 0.0f32);
        for p in points.iter() { l = l.min(p.0); t = t.min(p.1); r = r.max(p.0); b = b.max(p.1); }
        let (w, h) = ((r - l).max(0.01), (b - t).max(0.01));
        for p in points.iter_mut() { *p = ((p.0 - l) / w, (p.1 - t) / h) }
        objs[i].rect = PctRect::new(l * 100.0, t * 100.0, w * 100.0, h * 100.0);
        self.last = None;
        self.undo.push((before, self.at));
    }

    // ---- builds ----

    /// The chosen objects appear on a Next of their own (after the slide's other builds), or with the slide.
    pub fn set_build(&mut self, on: bool) {
        let next = self.slide().objects.iter().map(|o| o.build).max().unwrap_or(0) + 1;
        let sel = self.sel.clone();
        self.edit(|e| {
            for o in e.slide_mut().objects.iter_mut().filter(|o| sel.contains(&o.id)) {
                if !on { o.build = 0 } else if o.build == 0 { o.build = next }
            }
            tidy_builds(e.slide_mut());
        });
    }

    /// Swaps the first chosen object's build with the one before (`earlier`) or after it.
    pub fn move_build(&mut self, earlier: bool) {
        let Some(b) = self.primary().map(|o| o.build).filter(|b| *b > 0) else { return };
        let other = if earlier { b.checked_sub(1).filter(|v| *v > 0) } else { Some(b + 1).filter(|v| *v <= self.slide().steps()) };
        let Some(other) = other else { return };
        self.edit(|e| for o in &mut e.slide_mut().objects {
            if o.build == b { o.build = other } else if o.build == other { o.build = b }
        });
    }

    // ---- copy and paste a style (N27) ----

    /// The first chosen object's style; with nothing chosen, the slide's first text box's.
    pub fn copy_style(&self) -> Option<StyleCopy> {
        let o = self.primary().or_else(|| self.slide().objects.iter().find(|o| o.text_style().is_some()))?;
        Some(StyleCopy { text: o.text_style().cloned(), look: o.look.clone(), opacity: o.opacity })
    }

    /// Puts `style` on the chosen objects; with nothing chosen, on every text box of the slide. The words and
    /// places stay; a text style only goes on text boxes. One undo step.
    pub fn paste_style(&mut self, style: &StyleCopy) {
        let targets: Vec<Uuid> = if self.sel.is_empty() {
            self.slide().objects.iter().filter(|o| o.text_style().is_some()).map(|o| o.id).collect()
        } else { self.sel.clone() };
        self.edit(|e| for o in e.slide_mut().objects.iter_mut().filter(|o| targets.contains(&o.id)) {
            o.look = style.look.clone();
            o.opacity = style.opacity;
            if let (ObjKind::Text { style: own, .. }, Some(t)) = (&mut o.kind, &style.text) { *own = t.clone() }
        });
    }
}

/// Build numbers made 1, 2, 3… in their order, so there are no gaps.
fn tidy_builds(d: &mut Design) {
    let mut b: Vec<u16> = d.objects.iter().map(|o| o.build).filter(|b| *b > 0).collect();
    b.sort_unstable();
    b.dedup();
    for o in &mut d.objects {
        if o.build > 0 { o.build = b.iter().position(|v| *v == o.build).map_or(0, |i| i as u16 + 1) }
    }
}

/// Whether `p` is on the object (its box before turning, turned by its rotation).
fn inside(o: &Obj, p: (f32, f32)) -> bool {
    let r = o.rect;
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    // Back into the box's own frame. % across and down differ (16:9), so turn in screen proportions.
    let (px, py) = ((p.0 - cx) * 16.0, (p.1 - cy) * 9.0);
    let a = -o.rotation.to_radians();
    let (x, y) = (px * a.cos() - py * a.sin(), px * a.sin() + py * a.cos());
    (x / 16.0).abs() <= r.w / 2.0 && (y / 9.0).abs() <= r.h / 2.0
}

fn bounds(rects: impl Iterator<Item = PctRect>) -> PctRect {
    let (mut l, mut t, mut r, mut b) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for x in rects { l = l.min(x.x); t = t.min(x.y); r = r.max(x.x + x.w); b = b.max(x.y + x.h); }
    PctRect::new(l, t, r - l, b - t)
}

/// The handle of `r` under `p`, corners first.
pub fn handle_at(r: PctRect, p: (f32, f32), tol: (f32, f32)) -> Option<Handle> {
    use Handle::*;
    let (l, t, rt, b, cx, cy) = (r.x, r.y, r.x + r.w, r.y + r.h, r.x + r.w / 2.0, r.y + r.h / 2.0);
    [(NW, l, t), (NE, rt, t), (SE, rt, b), (SW, l, b), (N, cx, t), (E, rt, cy), (S, cx, b), (W, l, cy)].into_iter()
        .find(|&(_, x, y)| (p.0 - x).abs() <= tol.0 && (p.1 - y).abs() <= tol.1).map(|(h, _, _)| h)
}

#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
