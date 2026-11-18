//! Stage layout geometry: tidying a stored layout, and the editor's snapping, dragging, box order and undo.
use super::{strings as s, ElementKind, ElementStyle, PctRect, StageElement, StageLayout, BACKGROUND};
use crate::{lenient::Lenient, render::color::is_color};
use std::collections::VecDeque;
use uuid::Uuid;

/// The editor's grid step, how close an edge must be to snap to another box, and the smallest box (all in %).
pub const GRID: f32 = 2.5;
pub const GUIDE_TOL: f32 = 1.0;
pub const MIN_BOX: f32 = 5.0;
pub const MAX_BOXES: usize = 24;
/// Text sizes a box may ask for, in % of the window height.
pub const SIZE_MIN: f32 = 2.0;
pub const SIZE_MAX: f32 = 40.0;

/// Cut `s` to `max` characters (never inside one). True when it was cut.
pub fn truncate_chars(s: &mut String, max: usize) -> bool {
    match s.char_indices().nth(max) {
        Some((at, _)) => { s.truncate(at); true }
        None => false,
    }
}

/// Make a stored layout safe to draw: rects inside the window, sizes 2–40 with the floor at or below the top,
/// real colours, names and words cut to length, at most 24 boxes. Boxes this version doesn't know are left alone.
/// True when anything changed (so the caller saves once).
pub fn sanitise_layout(l: &mut StageLayout) -> bool {
    let mut changed = truncate_chars(&mut l.name, 60) | truncate_chars(&mut l.font, 100);
    if !is_color(&l.background) { l.background = BACKGROUND.into(); changed = true; }
    if l.elements.len() > MAX_BOXES { l.elements.truncate(MAX_BOXES); changed = true; }
    for e in l.elements.iter_mut().filter_map(Lenient::known_mut) {
        let rect = e.rect.sanitised();
        if rect != e.rect { e.rect = rect; changed = true; }
        changed |= sanitise_style(&mut e.style);
        if let ElementKind::Text { text } = &mut e.kind { changed |= truncate_chars(text, 200); }
    }
    changed
}

fn sanitise_style(st: &mut ElementStyle) -> bool {
    let mut changed = false;
    for size in [&mut st.max_size, &mut st.min_size] {
        let fixed = size.filter(|v| v.is_finite()).map(|v| v.clamp(SIZE_MIN, SIZE_MAX));
        if fixed != *size { *size = fixed; changed = true; }
    }
    if let (Some(max), Some(min)) = (st.max_size, st.min_size) {
        if min > max { st.min_size = Some(max); changed = true; }
    }
    for c in [&mut st.color, &mut st.fill] {
        if c.as_deref().is_some_and(|c| !is_color(c)) { *c = None; changed = true; }
    }
    if let Some(t) = &mut st.caption_text { changed |= truncate_chars(t, 40); }
    changed
}

/// The nearest multiple of `step`.
pub fn snap(v: f32, step: f32) -> f32 { (v / step).round() * step }

/// A line the editor draws while a box snaps: vertical at x, or horizontal at y (in %).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Guide { V(f32), H(f32) }

/// Line `r` up with the other boxes' (and the window's) edges and middles when within `tol`.
/// Each direction snaps to its closest line; one guide per direction that snapped.
pub fn snap_to_guides(r: PctRect, others: &[PctRect], tol: f32) -> (PctRect, Vec<Guide>) {
    fn axis(start: f32, len: f32, lines: &[f32], tol: f32) -> Option<(f32, f32)> {
        let mut best: Option<(f32, f32, f32)> = None; // (distance, shift, line)
        for edge in [start, start + len / 2.0, start + len] {
            for &line in lines {
                let d = (line - edge).abs();
                if d <= tol && best.is_none_or(|(bd, _, _)| d < bd) { best = Some((d, line - edge, line)); }
            }
        }
        best.map(|(_, shift, line)| (shift, line))
    }
    let lines = |pos: fn(&PctRect) -> (f32, f32)| -> Vec<f32> {
        others.iter().map(pos).flat_map(|(a, l)| [a, a + l / 2.0, a + l]).chain([0.0, 50.0, 100.0]).collect()
    };
    let (mut out, mut guides) = (r, vec![]);
    if let Some((shift, line)) = axis(r.x, r.w, &lines(|o| (o.x, o.w)), tol) { out.x += shift; guides.push(Guide::V(line)); }
    if let Some((shift, line)) = axis(r.y, r.h, &lines(|o| (o.y, o.h)), tol) { out.y += shift; guides.push(Guide::H(line)); }
    (out, guides)
}

/// `r` moved by (dx, dy), kept inside the window.
pub fn move_by(r: PctRect, dx: f32, dy: f32) -> PctRect {
    PctRect { x: (r.x + dx).clamp(0.0, 100.0 - r.w), y: (r.y + dy).clamp(0.0, 100.0 - r.h), ..r }
}

/// A box's resize handles: the sides and corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle { N, NE, E, SE, S, SW, W, NW }

/// `r` with handle `h` dragged by (dx, dy). The opposite edges stay put; never smaller than `min` or outside the window.
pub fn resize(r: PctRect, h: Handle, dx: f32, dy: f32, min: f32) -> PctRect {
    use Handle::*;
    let (mut left, mut top, mut right, mut bottom) = (r.x, r.y, r.x + r.w, r.y + r.h);
    if matches!(h, W | NW | SW) { left = (left + dx).clamp(0.0, right - min); }
    if matches!(h, E | NE | SE) { right = (right + dx).clamp(left + min, 100.0); }
    if matches!(h, N | NW | NE) { top = (top + dy).clamp(0.0, bottom - min); }
    if matches!(h, S | SW | SE) { bottom = (bottom + dy).clamp(top + min, 100.0); }
    PctRect { x: left, y: top, w: right - left, h: bottom - top }
}

/// A box's id; for a box this version doesn't know, the id in its JSON if there is one.
pub fn element_id(e: &Lenient<StageElement>) -> Option<Uuid> {
    match e {
        Lenient::Known(e) => Some(e.id),
        Lenient::Unknown(v) => v.get("id").and_then(|id| serde_json::from_value(id.clone()).ok()),
    }
}

fn element_rect(e: &Lenient<StageElement>) -> Option<PctRect> {
    match e {
        Lenient::Known(e) => Some(e.rect),
        Lenient::Unknown(v) => v.get("rect").and_then(|r| serde_json::from_value(r.clone()).ok()),
    }
}

/// The topmost (last) box under point `p` (in %).
pub fn hit_test(p: (f32, f32), elements: &[Lenient<StageElement>]) -> Option<Uuid> {
    elements.iter().rev().find_map(|e| {
        let r = element_rect(e)?;
        (p.0 >= r.x && p.0 <= r.x + r.w && p.1 >= r.y && p.1 <= r.y + r.h).then_some(())?;
        element_id(e)
    })
}

/// An arrow key in the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nudge { Left, Right, Up, Down }

/// One grid step: an arrow moves the box; with Shift it moves the right or bottom edge (no smaller than `MIN_BOX`).
pub fn nudge(r: PctRect, key: Nudge, shift: bool) -> PctRect {
    let (dx, dy) = match key { Nudge::Left => (-GRID, 0.0), Nudge::Right => (GRID, 0.0), Nudge::Up => (0.0, -GRID), Nudge::Down => (0.0, GRID) };
    if shift {
        let h = if dx != 0.0 { Handle::E } else { Handle::S };
        resize(r, h, dx, dy, MIN_BOX)
    } else {
        move_by(r, dx, dy)
    }
}

/// Where a new box goes: 30 % × 15 %, in the middle.
pub fn new_box_rect() -> PctRect { PctRect::new(35.0, 42.5, 30.0, 15.0) }

fn known_at(els: &[Lenient<StageElement>], id: Uuid) -> Option<usize> {
    els.iter().position(|e| e.known().is_some_and(|e| e.id == id))
}

/// Draw the box last (on top). False when it is not found, already there, or a box this version doesn't know.
pub fn bring_to_front(els: &mut Vec<Lenient<StageElement>>, id: Uuid) -> bool {
    match known_at(els, id) {
        Some(i) if i + 1 < els.len() => { let e = els.remove(i); els.push(e); true }
        _ => false,
    }
}

/// Draw the box first (underneath). False when it is not found, already there, or unknown.
pub fn send_to_back(els: &mut Vec<Lenient<StageElement>>, id: Uuid) -> bool {
    match known_at(els, id) {
        Some(i) if i > 0 => { let e = els.remove(i); els.insert(0, e); true }
        _ => false,
    }
}

/// Copy a box, one grid step right and down (kept inside the window), right after the original.
/// An unknown or missing box gives an empty error (the menu item is off for those).
pub fn duplicate_box(els: &mut Vec<Lenient<StageElement>>, id: Uuid, new_id: Uuid) -> Result<Uuid, String> {
    let i = known_at(els, id).ok_or_else(String::new)?;
    if els.len() >= MAX_BOXES { return Err(s::too_many_boxes().into()); }
    let mut copy = els[i].known().cloned().ok_or_else(String::new)?;
    (copy.id, copy.rect, copy.locked) = (new_id, move_by(copy.rect, GRID, GRID), false);
    els.insert(i + 1, Lenient::Known(copy));
    Ok(new_id)
}

/// Remove a box, known or not. False when no box has that id.
pub fn delete_box(els: &mut Vec<Lenient<StageElement>>, id: Uuid) -> bool {
    let before = els.len();
    els.retain(|e| element_id(e) != Some(id));
    els.len() != before
}

/// Tab / Shift+Tab: the next (or previous) box in layout order, wrapping. Unknown boxes count, so they can be deleted.
pub fn next_box(els: &[Lenient<StageElement>], sel: Option<Uuid>, back: bool) -> Option<Uuid> {
    let ids: Vec<Uuid> = els.iter().filter_map(element_id).collect();
    if ids.is_empty() { return None; }
    let n = ids.len();
    let at = sel.and_then(|s| ids.iter().position(|i| *i == s));
    Some(match (at, back) {
        (None, false) => ids[0],
        (None, true) => ids[n - 1],
        (Some(i), false) => ids[(i + 1) % n],
        (Some(i), true) => ids[(i + n - 1) % n],
    })
}

/// Undo and redo for the editor: `push` the state before each change.
#[derive(Clone, Debug)]
pub struct UndoStack<T: Clone> { cap: usize, undo: VecDeque<T>, redo: Vec<T> }

impl<T: Clone> UndoStack<T> {
    pub fn new(cap: usize) -> Self { UndoStack { cap, undo: VecDeque::new(), redo: Vec::new() } }

    /// Remember `before`; a new change ends any redo. The oldest step goes past `cap`.
    pub fn push(&mut self, before: T) {
        self.redo.clear();
        self.undo.push_back(before);
        while self.undo.len() > self.cap { self.undo.pop_front(); }
    }

    /// The state to go back to, given the current one.
    pub fn undo(&mut self, current: T) -> Option<T> {
        let prev = self.undo.pop_back()?;
        self.redo.push(current);
        Some(prev)
    }

    pub fn redo(&mut self, current: T) -> Option<T> {
        let next = self.redo.pop()?;
        self.undo.push_back(current);
        Some(next)
    }

    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::presets;
    use serde_json::json;

    const R10: PctRect = PctRect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 };
    fn r(x: f32, y: f32, w: f32, h: f32) -> PctRect { PctRect::new(x, y, w, h) }
    fn id(n: u128) -> Uuid { Uuid::from_u128(n) }
    fn boxed(n: u128, rect: PctRect, style: ElementStyle) -> Lenient<StageElement> {
        Lenient::Known(StageElement { id: id(n), kind: ElementKind::ItemTitle, rect, style, locked: false, hidden: false })
    }
    fn el(rect: PctRect, style: ElementStyle) -> Lenient<StageElement> { boxed(1, rect, style) }
    fn layout_with(elements: Vec<Lenient<StageElement>>) -> StageLayout {
        StageLayout { id: id(500), name: "Ours".into(), background: BACKGROUND.into(), font: String::new(), elements }
    }
    fn known(l: &StageLayout, i: usize) -> &StageElement { l.elements[i].known().unwrap() }
    fn unknown(n: u128) -> Lenient<StageElement> {
        Lenient::Unknown(json!({"id": id(n), "kind": {"type": "hologram"}, "rect": {"x": 0, "y": 0, "w": 50, "h": 50}}))
    }
    fn ids(els: &[Lenient<StageElement>]) -> Vec<Uuid> { els.iter().filter_map(element_id).collect() }

    #[test]
    fn every_builtin_passes_sanitise_unchanged() {
        for l in presets::builtins() {
            let mut c = l.clone();
            assert!(!sanitise_layout(&mut c), "{}", l.name);
            assert_eq!(&c, l);
        }
    }

    #[test]
    fn sanitise_clamps_and_fixes_nan() {
        let mut l = layout_with(vec![
            el(r(f32::NAN, -5.0, 150.0, 1.0), ElementStyle { max_size: Some(80.0), min_size: Some(50.0), ..Default::default() }),
            el(r(90.0, 0.0, 20.0, 10.0), ElementStyle { max_size: Some(1.0), ..Default::default() }),
            el(r(0.0, 0.0, f32::INFINITY, f32::NAN), ElementStyle { min_size: Some(f32::NAN), ..Default::default() })]);
        assert!(sanitise_layout(&mut l));
        assert_eq!(known(&l, 0).rect, r(0.0, 0.0, 100.0, 2.0));
        assert_eq!((known(&l, 0).style.max_size, known(&l, 0).style.min_size), (Some(40.0), Some(40.0)));
        assert_eq!(known(&l, 1).rect.x, 80.0);
        assert_eq!(known(&l, 1).style.max_size, Some(2.0));
        assert_eq!((known(&l, 2).rect.w, known(&l, 2).rect.h), (30.0, 15.0));
        assert_eq!(known(&l, 2).style.min_size, None, "a size that isn't a number goes back to the default");
    }

    #[test]
    fn sanitise_drops_bad_colours_keeps_white() {
        let mut l = layout_with(vec![el(R10, ElementStyle { color: Some("blurple".into()), fill: Some("white".into()), ..Default::default() })]);
        l.background = "nope".into();
        assert!(sanitise_layout(&mut l));
        assert_eq!((known(&l, 0).style.color.clone(), known(&l, 0).style.fill.as_deref(), l.background.as_str()), (None, Some("white"), "#07080a"));
    }

    #[test]
    fn sanitise_truncates_on_char_boundaries() {
        let mut l = layout_with(vec![Lenient::Known(StageElement {
            id: id(1), kind: ElementKind::Text { text: "🙏".repeat(250) }, rect: R10,
            style: ElementStyle { caption_text: Some("é".repeat(50)), ..Default::default() }, locked: false, hidden: false })]);
        l.name = "é".repeat(300);
        assert!(sanitise_layout(&mut l));
        assert_eq!(l.name.chars().count(), 60);
        let ElementKind::Text { text } = &known(&l, 0).kind else { panic!("kind changed") };
        assert_eq!(text.chars().count(), 200);
        assert_eq!(known(&l, 0).style.caption_text.as_ref().unwrap().chars().count(), 40);
    }

    #[test]
    fn sanitise_keeps_first_24_boxes_and_leaves_unknown_untouched() {
        let holo = json!({"id": id(1), "kind": {"type": "hologram"}, "rect": {"x": 0, "y": 0, "w": 500, "h": 10}});
        let mut els = vec![Lenient::Unknown(holo.clone())];
        els.extend((0..29).map(|_| el(r(0.0, 0.0, f32::NAN, 10.0), ElementStyle::default())));
        let mut l = layout_with(els);
        assert!(sanitise_layout(&mut l));
        assert_eq!(l.elements.len(), 24);
        assert_eq!(l.elements[0], Lenient::Unknown(holo));
        assert_eq!(known(&l, 1).rect.w, 30.0);
    }

    #[test]
    fn snap_to_grid() {
        assert_eq!(snap(3.7, 2.5), 2.5);
        assert_eq!(snap(3.8, 2.5), 5.0);
        assert_eq!(snap(0.0, 2.5), 0.0);
    }

    #[test]
    fn snap_to_other_box_edges_and_centres_returns_guides() {
        let other = [r(10.0, 0.0, 30.0, 10.0)];
        assert_eq!(snap_to_guides(r(10.6, 60.0, 20.0, 10.0), &other, GUIDE_TOL), (r(10.0, 60.0, 20.0, 10.0), vec![Guide::V(10.0)]));
        assert_eq!(snap_to_guides(r(11.5, 60.0, 20.0, 10.0), &other, GUIDE_TOL), (r(11.5, 60.0, 20.0, 10.0), vec![]));
        // Middles line up too: a box whose middle is 0.5 below the other's.
        assert_eq!(snap_to_guides(r(60.0, 0.5, 20.0, 10.0), &other, GUIDE_TOL), (r(60.0, 0.0, 20.0, 10.0), vec![Guide::H(0.0)]));
    }

    #[test]
    fn snaps_to_window_centre() {
        assert_eq!(snap_to_guides(r(34.4, 70.0, 30.0, 10.0), &[], GUIDE_TOL), (r(35.0, 70.0, 30.0, 10.0), vec![Guide::V(50.0)]));
    }

    #[test]
    fn move_by_stays_inside() {
        assert_eq!(move_by(r(70.0, 70.0, 20.0, 20.0), 50.0, 50.0), r(80.0, 80.0, 20.0, 20.0));
        assert_eq!(move_by(r(70.0, 70.0, 20.0, 20.0), -200.0, 0.0).x, 0.0);
    }

    #[test]
    fn resize_keeps_minimum_size_and_inside_window() {
        assert_eq!(resize(r(10.0, 10.0, 20.0, 20.0), Handle::NW, 40.0, 40.0, MIN_BOX), r(25.0, 25.0, 5.0, 5.0));
        let big = resize(r(10.0, 10.0, 20.0, 20.0), Handle::SE, 500.0, 500.0, MIN_BOX);
        assert_eq!((big.x + big.w, big.y + big.h), (100.0, 100.0));
        assert_eq!(resize(r(10.0, 10.0, 20.0, 20.0), Handle::E, 5.0, 99.0, MIN_BOX), r(10.0, 10.0, 25.0, 20.0), "E moves only the right edge");
        assert_eq!(resize(r(10.0, 10.0, 20.0, 20.0), Handle::N, 0.0, -50.0, MIN_BOX), r(10.0, 0.0, 20.0, 30.0));
    }

    #[test]
    fn hit_test_returns_topmost() {
        let els = vec![boxed(1, R10, ElementStyle::default()), boxed(2, R10, ElementStyle::default())];
        assert_eq!(hit_test((5.0, 5.0), &els), Some(id(2)));
        assert_eq!(hit_test((50.0, 50.0), &els), None);
    }

    #[test]
    fn hit_test_finds_unknown_box() {
        assert_eq!(hit_test((10.0, 10.0), &[unknown(7)]), Some(id(7)));
        assert_eq!(hit_test((10.0, 10.0), &[Lenient::Unknown(json!({"id": id(7)}))]), None);
    }

    #[test]
    fn nudge_and_shift_nudge() {
        let a = r(10.0, 10.0, 20.0, 20.0);
        assert_eq!(nudge(a, Nudge::Right, false).x, 12.5);
        assert_eq!(nudge(a, Nudge::Left, true).w, 17.5);
        assert_eq!(nudge(r(10.0, 10.0, 20.0, 5.0), Nudge::Up, true).h, 5.0);
        assert_eq!(nudge(r(0.0, 10.0, 20.0, 5.0), Nudge::Left, false).x, 0.0);
    }

    #[test]
    fn bring_to_front_and_send_to_back() {
        let mut els = vec![boxed(1, R10, ElementStyle::default()), boxed(2, R10, ElementStyle::default()), boxed(3, R10, ElementStyle::default())];
        assert!(bring_to_front(&mut els, id(1)));
        assert_eq!(ids(&els), [id(2), id(3), id(1)]);
        assert!(!bring_to_front(&mut els, id(1)));
        assert!(send_to_back(&mut els, id(3)));
        assert_eq!(ids(&els), [id(3), id(2), id(1)]);
        els.push(unknown(9));
        assert!(!send_to_back(&mut els, id(9)));
        assert_eq!(ids(&els), [id(3), id(2), id(1), id(9)]);
    }

    #[test]
    fn duplicate_box_offsets_by_one_grid_step_and_clamps() {
        let mut els = vec![boxed(1, r(10.0, 10.0, 20.0, 20.0), ElementStyle::default()), boxed(2, r(80.0, 85.0, 20.0, 15.0), ElementStyle::default())];
        assert_eq!(duplicate_box(&mut els, id(1), id(11)), Ok(id(11)));
        assert_eq!((element_id(&els[1]), els[1].known().unwrap().rect), (Some(id(11)), r(12.5, 12.5, 20.0, 20.0)));
        assert_eq!(duplicate_box(&mut els, id(2), id(12)), Ok(id(12)));
        assert_eq!(els[3].known().unwrap().rect, r(80.0, 85.0, 20.0, 15.0));
        els.push(unknown(9));
        assert!(duplicate_box(&mut els, id(9), id(13)).is_err());
        assert_eq!(els.len(), 5);
        let mut full: Vec<_> = (0..24).map(|n| boxed(100 + n, R10, ElementStyle::default())).collect();
        assert_eq!(duplicate_box(&mut full, id(100), id(1)), Err("A stage layout can have at most 24 boxes.".into()));
        assert_eq!(full.len(), 24);
    }

    #[test]
    fn delete_box_removes_known_or_unknown() {
        let mut els = vec![boxed(1, R10, ElementStyle::default()), unknown(9)];
        assert!(delete_box(&mut els, id(9)));
        assert!(!delete_box(&mut els, id(9)));
        assert_eq!(ids(&els), [id(1)]);
    }

    #[test]
    fn next_box_tabs_in_layout_order_and_wraps() {
        let els = vec![boxed(1, R10, ElementStyle::default()), unknown(9), boxed(2, R10, ElementStyle::default())];
        assert_eq!(next_box(&els, None, false), Some(id(1)));
        assert_eq!(next_box(&els, Some(id(1)), false), Some(id(9)));
        assert_eq!(next_box(&els, Some(id(9)), false), Some(id(2)));
        assert_eq!(next_box(&els, Some(id(2)), false), Some(id(1)));
        assert_eq!(next_box(&els, Some(id(1)), true), Some(id(2)));
        assert_eq!(next_box(&els, None, true), Some(id(2)));
        assert_eq!(next_box(&[], None, false), None);
    }

    #[test]
    fn undo_redo_cap_50() {
        let mut u = UndoStack::new(50);
        for n in 0..60 { u.push(n); }
        let mut current = 60;
        for want in (10..60).rev() {
            current = u.undo(current).unwrap();
            assert_eq!(current, want);
        }
        assert_eq!(u.undo(current), None);
        assert!(u.can_redo());
        assert_eq!(u.redo(10), Some(11));
        u.push(5);
        assert!(!u.can_redo() && u.can_undo());
    }
}
