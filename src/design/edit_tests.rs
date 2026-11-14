use super::*;
use crate::design::{Fit, TransitionKind};

fn r(x: f32, y: f32, w: f32, h: f32) -> PctRect { PctRect::new(x, y, w, h) }

fn shape_at(e: &mut Editor, rect: PctRect) -> Uuid {
    let id = e.add(ObjKind::Shape { shape: Shape::Rect }).unwrap();
    e.slide_mut().objects.iter_mut().find(|o| o.id == id).unwrap().rect = rect;
    id
}

fn rect_of(e: &Editor, id: Uuid) -> PctRect { e.slide().objects.iter().find(|o| o.id == id).unwrap().rect }

#[test]
fn a_new_editor_has_one_blank_slide_and_no_changes() {
    let e = Editor::new(vec![]);
    assert_eq!(e.slides, vec![Design::default()]);
    assert!(!e.changed() && !e.can_undo());
}

#[test]
fn adding_text_chooses_it_and_undo_takes_it_away() {
    let mut e = Editor::new(vec![]);
    let id = e.add_text().unwrap();
    assert_eq!(e.sel, vec![id]);
    assert!(e.changed());
    e.undo();
    assert!(e.slide().objects.is_empty() && e.sel.is_empty());
    e.redo();
    assert_eq!(e.slide().objects.len(), 1);
}

#[test]
fn dragging_moves_snaps_to_the_middle_and_is_one_undo_step() {
    let mut e = Editor::new(vec![]);
    let id = shape_at(&mut e, r(10.0, 10.0, 20.0, 20.0));
    let steps = e.undo.can_undo();
    e.press((15.0, 15.0), (0.5, 0.5), false, false);
    e.drag_to((30.0, 22.0));
    e.drag_to((45.4, 30.3));
    // Moved by 30.4 / 15.3: its middle lands near the slide's middle line (50) and snaps there.
    assert_eq!(rect_of(&e, id).x, 40.0);
    assert!(e.guides.contains(&Guide::V(50.0)));
    e.release();
    assert!(e.guides.is_empty());
    e.undo();
    assert_eq!(rect_of(&e, id), r(10.0, 10.0, 20.0, 20.0), "the whole drag is one step");
    assert!(steps);
}

#[test]
fn a_handle_resizes_and_alt_does_not_snap() {
    let mut e = Editor::new(vec![]);
    let id = shape_at(&mut e, r(10.0, 10.0, 20.0, 20.0));
    e.press((30.0, 30.0), (0.5, 0.5), false, true);
    e.drag_to((40.3, 35.0));
    let got = rect_of(&e, id);
    assert!((got.w - 30.3).abs() < 1e-3 && got.h == 25.0 && got.x == 10.0, "{got:?}");
    e.release();
    e.press((40.3, 35.0), (0.5, 0.5), false, false);
    e.drag_to((45.1, 35.0));
    assert_eq!(rect_of(&e, id).w, 35.0, "snapped to the grid");
}

#[test]
fn shift_click_chooses_several_and_they_move_together() {
    let mut e = Editor::new(vec![]);
    let a = shape_at(&mut e, r(10.0, 10.0, 10.0, 10.0));
    let b = shape_at(&mut e, r(50.0, 50.0, 10.0, 10.0));
    e.press((15.0, 15.0), (0.1, 0.1), false, false);
    e.release();
    e.press((55.0, 55.0), (0.1, 0.1), true, false);
    e.release();
    assert_eq!(e.sel, vec![a, b]);
    e.nudge(Nudge::Right, false);
    assert_eq!((rect_of(&e, a).x, rect_of(&e, b).x), (11.0, 51.0));
    e.press((1.0, 90.0), (0.1, 0.1), false, false);
    assert!(e.sel.is_empty(), "the bare slide chooses nothing");
}

#[test]
fn align_one_to_the_slide_and_several_to_each_other() {
    let mut e = Editor::new(vec![]);
    let a = shape_at(&mut e, r(10.0, 10.0, 20.0, 10.0));
    e.align(AlignTo::Center);
    assert_eq!(rect_of(&e, a).x, 40.0);
    let b = shape_at(&mut e, r(70.0, 60.0, 10.0, 10.0));
    e.sel = vec![a, b];
    e.align(AlignTo::Top);
    assert_eq!((rect_of(&e, a).y, rect_of(&e, b).y), (10.0, 10.0));
    e.align(AlignTo::Right);
    assert_eq!((rect_of(&e, a).x + 20.0, rect_of(&e, b).x + 10.0), (80.0, 80.0));
}

#[test]
fn distribute_spaces_three_evenly() {
    let mut e = Editor::new(vec![]);
    let a = shape_at(&mut e, r(0.0, 0.0, 10.0, 10.0));
    let b = shape_at(&mut e, r(15.0, 0.0, 10.0, 10.0));
    let c = shape_at(&mut e, r(80.0, 0.0, 20.0, 10.0));
    e.sel = vec![c, a, b];
    e.distribute(true);
    assert_eq!((rect_of(&e, a).x, rect_of(&e, b).x, rect_of(&e, c).x), (0.0, 40.0, 80.0));
}

#[test]
fn order_moves_the_chosen_object() {
    let mut e = Editor::new(vec![]);
    let a = shape_at(&mut e, r(0.0, 0.0, 10.0, 10.0));
    let b = shape_at(&mut e, r(0.0, 0.0, 10.0, 10.0));
    assert_eq!(e.hit((5.0, 5.0)), Some(b), "the top one");
    e.sel = vec![a];
    e.order(Order::Front);
    assert_eq!(e.hit((5.0, 5.0)), Some(a));
    e.order(Order::Backward);
    assert_eq!(e.slide().objects[0].id, a);
}

#[test]
fn a_turned_object_is_hit_where_it_is_drawn() {
    let mut e = Editor::new(vec![]);
    let id = shape_at(&mut e, r(40.0, 45.0, 20.0, 10.0));
    assert_eq!(e.hit((58.0, 46.0)), Some(id));
    e.sel = vec![id];
    e.set_rotation(90.0);
    assert_eq!(e.hit((58.0, 46.0)), None, "turned upright, that corner is empty");
    assert_eq!(e.hit((50.0, 35.0)), Some(id));
}

#[test]
fn builds_number_in_order_and_move() {
    let mut e = Editor::new(vec![]);
    let a = e.add_text().unwrap();
    let b = e.add_text().unwrap();
    let c = e.add_text().unwrap();
    for id in [a, b, c] { e.sel = vec![id]; e.set_build(true); }
    let build = |e: &Editor, id| e.slide().click_of(id);
    assert_eq!((build(&e, a), build(&e, b), build(&e, c)), (1, 2, 3));
    e.sel = vec![a];
    e.set_build(false);
    assert_eq!((build(&e, a), build(&e, b), build(&e, c)), (0, 1, 2), "no gaps");
    e.sel = vec![c];
    e.move_build(true);
    assert_eq!((build(&e, b), build(&e, c)), (2, 1));
    e.move_build(true);
    assert_eq!(build(&e, c), 1, "already first");
    assert_eq!(e.slide().steps(), 2);
}

#[test]
fn copy_and_paste_style_onto_every_text_box_when_none_is_chosen() {
    let mut e = Editor::new(vec![]);
    let a = e.add_text().unwrap();
    e.set_size(120.0);
    e.set_bold(true);
    e.set_fill(Some("#112233"));
    let copied = e.copy_style().unwrap();
    e.add_slide(false).unwrap();
    let b = e.add_text().unwrap();
    e.set_text("Point two");
    let pic = e.add(ObjKind::Picture { asset: "p.png".into(), fit: Fit::Fill }).unwrap();
    e.sel.clear();
    e.paste_style(&copied);
    let o = e.slide().objects.iter().find(|o| o.id == b).unwrap();
    let st = o.text_style().unwrap();
    assert_eq!((st.size, st.bold, o.look.fill.as_deref()), (120.0, true, Some("#112233")));
    assert!(matches!(&o.kind, ObjKind::Text { text, .. } if text == "Point two"), "the words stay");
    assert_eq!(e.slide().objects.iter().find(|o| o.id == pic).unwrap().look.fill, None, "only text boxes");
    e.undo();
    assert_eq!(e.slide().objects.iter().find(|o| o.id == b).unwrap().text_style().unwrap().size, 72.0, "one step");
    assert!(a != b);
}

#[test]
fn slider_changes_of_one_setting_are_one_undo_step() {
    let mut e = Editor::new(vec![]);
    e.add_text().unwrap();
    for v in [80.0, 90.0, 100.0] { e.set_size(v) }
    e.undo();
    assert_eq!(e.primary().unwrap().text_style().unwrap().size, 72.0);
}

#[test]
fn slides_add_copy_move_and_delete() {
    let mut e = Editor::new(vec![]);
    e.add_text().unwrap();
    e.add_slide(true).unwrap();
    assert_eq!((e.slides.len(), e.at), (2, 1));
    assert_ne!(e.slides[0].objects[0].id, e.slides[1].objects[0].id, "a copy gets its own ids");
    e.set_transition(TransitionKind::Cut);
    e.move_slide(true);
    assert_eq!((e.at, e.slides[0].transition.kind), (0, TransitionKind::Cut));
    e.transition_to_all();
    assert!(e.slides.iter().all(|d| d.transition.kind == TransitionKind::Cut));
    e.set_transition(TransitionKind::Push);
    e.set_transition_dir(crate::design::Dir::Up);
    e.transition_to_all();
    assert!(e.slides.iter().all(|d| (d.transition.kind, d.transition.dir) == (TransitionKind::Push, crate::design::Dir::Up)));
    e.set_transition(TransitionKind::Cut);
    e.transition_to_all();
    e.delete_slide();
    assert_eq!(e.slides.len(), 1);
    e.delete_slide();
    assert_eq!(e.slides, vec![Design::default()], "the last slide is emptied");
}

#[test]
fn duplicate_and_delete_the_chosen() {
    let mut e = Editor::new(vec![]);
    let a = shape_at(&mut e, r(10.0, 10.0, 10.0, 10.0));
    e.duplicate().unwrap();
    assert_eq!(e.slide().objects.len(), 2);
    assert_ne!(e.sel, vec![a]);
    assert_eq!(rect_of(&e, e.sel[0]), r(12.0, 12.0, 10.0, 10.0));
    e.delete();
    assert_eq!(e.slide().objects.iter().map(|o| o.id).collect::<Vec<_>>(), vec![a]);
}

#[test]
fn the_pen_draws_a_line_in_a_box_of_its_own_and_a_dot_is_dropped() {
    let mut e = Editor::new(vec![]);
    e.pen_down((10.0, 20.0), "#ff0000", 8.0).unwrap();
    e.pen_to((30.0, 20.0));
    e.pen_to((30.0, 60.0));
    e.pen_up();
    let o = &e.slide().objects[0];
    let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
    assert!(near(o.rect.x, 10.0) && near(o.rect.y, 20.0) && near(o.rect.w, 20.0) && near(o.rect.h, 40.0), "{:?}", o.rect);
    let ObjKind::Path { points, .. } = &o.kind else { panic!() };
    let want = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)];
    assert!(points.len() == 3 && points.iter().zip(want).all(|(p, w)| near(p.0, w.0) && near(p.1, w.1)), "{points:?}");
    e.undo();
    assert!(e.slide().objects.is_empty(), "one undo step");
    e.pen_down((50.0, 50.0), "#ff0000", 8.0).unwrap();
    e.pen_up();
    assert!(e.slide().objects.is_empty(), "a dot is no line");
}

#[test]
fn a_web_page_takes_its_address_from_the_words_box() {
    let mut e = Editor::new(vec![]);
    e.add_web().unwrap();
    e.set_text("https://church.example/give");
    assert_eq!(e.primary().unwrap().kind, ObjKind::Web { url: "https://church.example/give".into() });
}

#[test]
fn copy_cut_and_paste_objects() {
    let mut e = Editor::new(vec![]);
    let a = shape_at(&mut e, r(10.0, 10.0, 10.0, 10.0));
    let copied = e.copy_objects();
    assert_eq!(copied.len(), 1);
    e.paste_objects(&copied).unwrap();
    assert_eq!(e.slide().objects.len(), 2);
    assert_ne!(e.sel, vec![a], "the copy is chosen");
    assert_eq!(rect_of(&e, e.sel[0]), r(12.0, 12.0, 10.0, 10.0), "a little right and down, beside the original");
    // On another slide it goes in the same place.
    e.add_slide(false).unwrap();
    e.paste_objects(&copied).unwrap();
    assert_eq!(rect_of(&e, e.sel[0]), r(10.0, 10.0, 10.0, 10.0));
    e.undo();
    assert!(e.slide().objects.is_empty(), "one undo step");
    // Cut: copied, then gone.
    e.select_slide(0);
    e.select(a, false);
    let cut = e.cut_objects();
    assert_eq!(cut.len(), 1);
    assert_eq!(e.slide().objects.len(), 1);
    assert!(e.copy_objects().is_empty(), "nothing chosen now");
    assert!(e.paste_objects(&[]).is_ok());
}

#[test]
fn animations_are_added_changed_moved_and_removed_with_undo() {
    use crate::design::{AnimKind, Effect, Seg, Start};
    let mut e = Editor::new(vec![]);
    let a = e.add_text().unwrap();
    assert_eq!(e.add_anim(), Some(0));
    assert_eq!(e.add_anim(), Some(1));
    let s = e.slide();
    assert_eq!((s.anims[0].obj, s.anims[0].kind, s.anims[0].effect, s.anims[0].start), (a, AnimKind::Entrance, Effect::Fade, Start::OnClick));
    assert_eq!(s.anims[1].kind, AnimKind::Exit, "once it comes in, the next step takes it out");
    e.set_anim_kind(1, AnimKind::Motion);
    assert_eq!(e.slide().anims[1].effect, Effect::Path);
    assert_eq!(e.slide().anims[1].path.as_ref().unwrap().segs, [Seg::Line { to: (0.0, -25.0) }]);
    e.set_anim_kind(1, AnimKind::Emphasis);
    assert_eq!(e.slide().anims[1].effect, Effect::Spin);
    e.set_anim_effect(0, Effect::Appear);
    assert_eq!(e.slide().anims[0].dur_ms, 0);
    for ms in [100, 200, 300] { e.set_anim(0, Some(Field::AnimDelay), |x| x.delay_ms = ms) }
    e.undo();
    assert_eq!(e.slide().anims[0].delay_ms, 0, "a dragged slider is one undo step");
    e.redo();
    let b = e.add_text().unwrap();
    e.add_anim();
    assert_eq!(e.anims_of_primary(), [2]);
    assert_eq!(e.move_anim(2, true), 1);
    assert_eq!(e.slide().anims[1].obj, b);
    assert_eq!(e.move_anim(0, true), 0, "already first");
    e.remove_anim(1);
    assert!(e.anims_of_primary().is_empty());
    e.sel = vec![a];
    e.delete();
    assert!(e.slide().anims.is_empty(), "a deleted object's steps go with it");
}

#[test]
fn marked_words_take_a_look_of_their_own() {
    use crate::design::{Para, Run};
    let paras = vec![Para { runs: vec![Run::plain("Joy to the world")], ..Para::default() }, Para { runs: vec![Run::plain("the Lord is come")], ..Para::default() }];
    let mut ed = Editor::new(vec![Design { objects: vec![Obj::new(ObjKind::rich(paras, TextStyle::default()), PctRect::new(0.0, 0.0, 50.0, 50.0))], ..Default::default() }]);
    let id = ed.slide().objects[0].id;
    ed.select(id, false);
    let runs = |ed: &Editor| -> Vec<Vec<Run>> {
        let ObjKind::Text { paras, .. } = &ed.slide().objects[0].kind else { panic!() };
        paras.iter().map(|p| p.runs.clone()).collect()
    };
    // "Joy" marked, then Bold: that word only. The box's own style is as it was.
    ed.mark_words(0, 3);
    ed.set_bold(true);
    let bold = |t: &str| Run { text: t.into(), bold: Some(true), ..Run::default() };
    assert_eq!(runs(&ed), [vec![bold("Joy"), Run::plain(" to the world")], vec![Run::plain("the Lord is come")]]);
    let ObjKind::Text { style, text, .. } = &ed.slide().objects[0].kind else { panic!() };
    assert!(!style.bold);
    assert_eq!(text, "Joy to the world\nthe Lord is come");
    // The panel shows the marked words' look, so Bold can be taken off again.
    assert!(ed.shown_look().unwrap().bold);
    // Marked from the caret backwards, across the line break: "world" and "the Lord" in red.
    ed.mark_words(25, 11);
    ed.set_color("#ff0000");
    let red = |t: &str| Run { text: t.into(), color: Some("#ff0000".into()), ..Run::default() };
    assert_eq!(runs(&ed), [vec![bold("Joy"), Run::plain(" to the "), red("world")], vec![red("the Lord"), Run::plain(" is come")]]);
    // One undo step each.
    ed.undo();
    assert_eq!(runs(&ed)[1], [Run::plain("the Lord is come")]);
    // Bold off the same word: nothing differs from the box any more, so no looks are kept.
    ed.mark_words(0, 3);
    ed.set_bold(false);
    let ObjKind::Text { paras, .. } = &ed.slide().objects[0].kind else { panic!() };
    assert!(paras.is_empty(), "{paras:?}");
    // Nothing marked, or another box chosen: the whole box, as before.
    ed.mark_words(4, 4);
    assert_eq!(ed.marked_words(), None);
    ed.set_italic(true);
    let ObjKind::Text { style, paras, .. } = &ed.slide().objects[0].kind else { panic!() };
    assert!(style.italic && paras.is_empty());
    ed.mark_words(0, 3);
    ed.sel.clear();
    assert_eq!(ed.marked_words(), None, "the words were another choice's");
    // A mark in the middle of a letter of several bytes is moved to the letter's start.
    assert_eq!(crate::design::rich::style_range("né", &[], &TextStyle::default(), 0..2, &|s| s.bold = true)[0].runs,
        [bold("n"), Run::plain("é")]);
}

#[test]
fn typing_and_restyling_a_box_with_mixed_looks_keeps_the_looks_that_stay() {
    use crate::design::{Para, Run};
    let paras = vec![Para { runs: vec![Run { text: "Joy".into(), bold: Some(true), ..Run::default() }, Run::plain(" to the world")], ..Para::default() }];
    let mut ed = Editor::new(vec![Design { objects: vec![Obj::new(ObjKind::rich(paras, TextStyle::default()), PctRect::new(0.0, 0.0, 50.0, 50.0))], ..Default::default() }]);
    let id = ed.slide().objects[0].id;
    ed.select(id, false);
    ed.set_text("Joy to all the world");
    let ObjKind::Text { text, paras, .. } = &ed.slide().objects[0].kind else { panic!() };
    assert_eq!(text, "Joy to all the world");
    assert_eq!(paras[0].runs, vec![Run { text: "Joy".into(), bold: Some(true), ..Run::default() }, Run::plain(" to all the world")]);
    // Making the whole box bold clears the run's own bold: every word is bold now.
    ed.set_bold(true);
    ed.set_underline(true);
    let ObjKind::Text { style, paras, .. } = &ed.slide().objects[0].kind else { panic!() };
    assert!(style.bold && style.underline);
    assert!(paras.iter().flat_map(|p| &p.runs).all(|r| r.bold.is_none()));
}
