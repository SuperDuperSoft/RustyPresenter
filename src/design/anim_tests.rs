use super::*;
use crate::design::{Design, Dir, Obj, ObjKind, PctRect, TextStyle};

fn text(t: &str) -> Obj { Obj::new(ObjKind::text(t, TextStyle::default()), PctRect::new(10.0, 10.0, 40.0, 20.0)) }

fn names(d: &Design) -> Vec<String> {
    d.objects.iter().map(|o| match &o.kind { ObjKind::Text { text, .. } => text.clone(), _ => String::new() }).collect()
}

fn step(obj: &Obj, kind: AnimKind, effect: Effect, start: Start) -> Anim { Anim::new(obj.id, kind, effect, start) }

#[test]
fn old_builds_load_as_appear_steps_and_are_not_written_again() {
    let json = r#"{"objects":[
        {"id":"00000000-0000-0000-0000-000000000001","kind":{"type":"text","text":"title"},"rect":{"x":0,"y":0,"w":10,"h":10}},
        {"id":"00000000-0000-0000-0000-000000000002","kind":{"type":"text","text":"two"},"rect":{"x":0,"y":0,"w":10,"h":10},"build":4},
        {"id":"00000000-0000-0000-0000-000000000003","kind":{"type":"text","text":"one"},"rect":{"x":0,"y":0,"w":10,"h":10},"build":1},
        {"id":"00000000-0000-0000-0000-000000000004","kind":{"type":"text","text":"two too"},"rect":{"x":0,"y":0,"w":10,"h":10},"build":4}]}"#;
    let d: Design = serde_json::from_str(json).unwrap();
    assert_eq!(d.steps(), 2);
    assert_eq!(d.anims.iter().map(|a| (a.kind, a.effect, a.start)).collect::<Vec<_>>(), [
        (AnimKind::Entrance, Effect::Appear, Start::OnClick),
        (AnimKind::Entrance, Effect::Appear, Start::OnClick),
        (AnimKind::Entrance, Effect::Appear, Start::WithPrevious),
    ]);
    assert_eq!(names(&d.at_step(0)), ["title"]);
    assert_eq!(names(&d.at_step(1)), ["title", "one"]);
    assert_eq!(names(&d.at_step(2)), ["title", "two", "one", "two too"], "drawn in their own order");
    assert!(d.objects.iter().all(|o| o.build == 0));
    let again = serde_json::to_value(&d).unwrap();
    assert!(again["objects"].as_array().unwrap().iter().all(|o| o.get("build").is_none()));
    assert_eq!(serde_json::from_value::<Design>(again).unwrap(), d, "round trips as steps");
}

#[test]
fn a_slide_without_steps_writes_none() {
    let d = Design { objects: vec![text("Hi")], ..Default::default() };
    assert!(serde_json::to_value(&d).unwrap().get("anims").is_none());
}

#[test]
fn steps_round_trip_and_an_unknown_effect_loads_as_fade() {
    let o = text("x");
    let mut fly = step(&o, AnimKind::Entrance, Effect::Fly, Start::OnClick);
    fly.dir = Some(Dir::Left);
    let mut go = step(&o, AnimKind::Motion, Effect::Path, Start::AfterPrevious);
    go.path = Some(MotionPath::line((20.0, -5.0)));
    let d = Design { objects: vec![o.clone()], anims: vec![fly, go], ..Default::default() };
    let json = serde_json::to_value(&d).unwrap();
    assert_eq!(json["anims"][0]["dir"], "left");
    assert_eq!(json["anims"][1]["start"], "after_previous");
    assert_eq!(serde_json::from_value::<Design>(json).unwrap(), d);
    let odd: Anim = serde_json::from_str(&format!(r#"{{"obj":"{}","kind":"entrance","effect":"checkerboard"}}"#, o.id)).unwrap();
    assert_eq!((odd.effect, odd.start, odd.dur_ms), (Effect::Fade, Start::OnClick, 500));
}

#[test]
fn clicks_group_with_and_after_previous_and_time_them() {
    let (a, b, c, d) = (text("a"), text("b"), text("c"), text("d"));
    let mut anims = vec![
        step(&a, AnimKind::Entrance, Effect::Fade, Start::AfterPrevious), // group 0: plays by itself
        step(&b, AnimKind::Entrance, Effect::Fade, Start::OnClick),
        step(&c, AnimKind::Entrance, Effect::Fade, Start::WithPrevious),
        step(&d, AnimKind::Entrance, Effect::Fade, Start::AfterPrevious),
    ];
    anims[2].delay_ms = 200;
    anims[2].dur_ms = 1000;
    anims[3].delay_ms = 100;
    let s = Design { objects: vec![a, b, c, d], anims, ..Default::default() };
    assert_eq!(s.groups(), vec![vec![(0, 0)], vec![(1, 0), (2, 200), (3, 1300)]]);
    assert_eq!(s.steps(), 1);
    assert_eq!(names(&s.at_step(0)), ["a"], "group 0 is over before the first Next");
    assert_eq!(names(&s.at_step(1)), ["a", "b", "c", "d"]);
    assert_eq!(s.play(1).unwrap().duration_ms(), 1800);
    assert_eq!(s.play(0).unwrap().tracks.len(), 1);
    assert!(s.play(2).is_none());
}

#[test]
fn exits_hide_and_motion_spin_and_grow_stay() {
    let (a, b) = (text("a"), text("b"));
    let mut go = step(&a, AnimKind::Motion, Effect::Path, Start::OnClick);
    go.path = Some(MotionPath::line((30.0, 10.0)));
    let mut spin = step(&a, AnimKind::Emphasis, Effect::Spin, Start::WithPrevious);
    spin.amount = Some(90.0);
    let grow = step(&a, AnimKind::Emphasis, Effect::Grow, Start::OnClick);
    let out = step(&b, AnimKind::Exit, Effect::Fade, Start::OnClick);
    let s = Design { objects: vec![a, b], anims: vec![go, spin, grow, out], ..Default::default() };
    assert_eq!(names(&s.at_step(0)), ["a", "b"], "only an entrance hides an object at first");
    let moved = s.at_step(1).objects[0].clone();
    assert_eq!((moved.rect.x, moved.rect.y, moved.rotation), (40.0, 20.0, 90.0));
    let grown = s.at_step(2).objects[0].clone();
    assert_eq!((grown.rect.w, grown.rect.h, grown.rect.x), (60.0, 30.0, 30.0), "grown about its middle");
    assert_eq!(names(&s.at_step(3)), ["a"]);
    assert_eq!(s.at_step(9), s.at_step(3));
}

#[test]
fn a_play_poses_objects_over_time() {
    let (a, b) = (text("a"), text("b"));
    let mut fly = step(&a, AnimKind::Entrance, Effect::Fly, Start::OnClick);
    fly.dir = Some(Dir::Down);
    fly.dur_ms = 1000;
    let mut fade = step(&b, AnimKind::Exit, Effect::Fade, Start::AfterPrevious);
    fade.dur_ms = 1000;
    let s = Design { objects: vec![a, b], anims: vec![fly, fade], ..Default::default() };
    let p = s.play(1).unwrap();
    assert_eq!(p.shown, [false, true]);
    assert!(p.animated(0) && p.animated(1));
    let a0 = p.pose(0, 0.0);
    assert_eq!((a0.op, a0.dy), (1.0, 90.0), "starts below the slide's bottom edge");
    let mid = p.pose(0, 500.0);
    assert!((mid.dy - 45.0).abs() < 0.01, "halfway up: {mid:?}");
    assert_eq!(p.pose(0, 1000.0), Pose::SHOWN);
    assert_eq!(p.pose(1, 900.0), Pose::SHOWN, "waits for the fly");
    assert!((p.pose(1, 1500.0).op - 0.5).abs() < 0.01);
    assert_eq!(p.pose(1, 2000.0).op, 0.0);
}

#[test]
fn an_entrance_waits_hidden_and_wipe_split_zoom_cut_or_grow() {
    let a = text("a");
    let mut wipe = step(&a, AnimKind::Entrance, Effect::Wipe, Start::OnClick);
    wipe.delay_ms = 500;
    wipe.dir = Some(Dir::Left);
    let s = Design { objects: vec![a.clone()], anims: vec![wipe], ..Default::default() };
    let p = s.play(1).unwrap();
    assert_eq!(p.pose(0, 100.0).op, 0.0);
    assert_eq!(p.pose(0, 500.0).clip, [0.0, 1.0, 0.0, 0.0], "from the left: the right is cut away");
    let split = Design { anims: vec![step(&a, AnimKind::Entrance, Effect::Split, Start::OnClick)], ..s.clone() };
    assert_eq!(split.play(1).unwrap().pose(0, 0.0).clip, [0.5, 0.5, 0.0, 0.0]);
    let zoom = Design { anims: vec![step(&a, AnimKind::Exit, Effect::Zoom, Start::OnClick)], ..s.clone() };
    let z = zoom.play(1).unwrap().pose(0, 250.0);
    assert!((z.scale - 0.5).abs() < 0.01 && (z.op - 0.5).abs() < 0.01);
    let gone = Design { anims: vec![step(&a, AnimKind::Exit, Effect::Appear, Start::OnClick)], ..s };
    assert_eq!(gone.play(1).unwrap().pose(0, 0.0).op, 0.0, "Disappear is at once");
}

#[test]
fn powerpoint_paths_read_as_slide_percent() {
    let p = MotionPath::parse_ppt("M 0 0 L 0.25 0.1 E").unwrap();
    assert_eq!(p.segs, [Seg::Move { to: (0.0, 0.0) }, Seg::Line { to: (25.0, 10.0) }]);
    assert_eq!(p.moved(), (25.0, 10.0));
    let c = MotionPath::parse_ppt("M0,0 C 0.1,0 0.2,0.1 0.2,0.2 l -0.2 0 Z E").unwrap();
    assert_eq!(c.segs.len(), 4);
    assert_eq!(c.segs[2], Seg::Line { to: (0.0, 20.0) });
    let e = MotionPath::parse_ppt("M 0 0 L 1.5E-1 -2.5e-2 E").unwrap();
    let Seg::Line { to } = e.segs[1] else { panic!("a line: {:?}", e.segs) };
    assert!((to.0 - 15.0).abs() < 1e-4 && (to.1 + 2.5).abs() < 1e-4, "{to:?}");
    assert!(MotionPath::parse_ppt("E").is_none());
    let mid = MotionPath::line((10.0, 0.0)).at(0.5);
    assert_eq!(mid, (5.0, 0.0));
}

#[test]
fn sanitise_drops_steps_of_missing_objects_and_clamps_times() {
    let a = text("a");
    let mut long = step(&a, AnimKind::Entrance, Effect::Fade, Start::OnClick);
    long.dur_ms = u32::MAX;
    let mut d = Design { objects: vec![a.clone()], anims: vec![long, step(&text("gone"), AnimKind::Exit, Effect::Fade, Start::OnClick)], ..Default::default() };
    assert!(d.sanitise());
    assert_eq!((d.anims.len(), d.anims[0].dur_ms), (1, ANIM_MAX_MS));
    assert!(!d.sanitise());
}

#[test]
fn clicks_swap_whole_groups() {
    let (a, b, c) = (text("a"), text("b"), text("c"));
    let mut d = Design { objects: vec![a.clone(), b.clone(), c.clone()], anims: vec![
        step(&a, AnimKind::Entrance, Effect::Fade, Start::OnClick),
        step(&b, AnimKind::Entrance, Effect::Fade, Start::OnClick),
        step(&c, AnimKind::Entrance, Effect::Fade, Start::WithPrevious),
    ], ..Default::default() };
    d.swap_clicks(1);
    assert_eq!((d.click_of(a.id), d.click_of(b.id), d.click_of(c.id)), (2, 1, 1));
}

#[test]
fn a_play_draws_still_runs_together_and_each_moving_object_alone() {
    let (a, b, c, d, e) = (text("a"), text("b"), text("c"), text("d"), text("e"));
    let s = Design { objects: vec![a.clone(), b.clone(), c.clone(), d.clone(), e.clone()], anims: vec![
        step(&e, AnimKind::Entrance, Effect::Fade, Start::OnClick),
        step(&c, AnimKind::Entrance, Effect::Fade, Start::OnClick),
    ], ..Default::default() };
    // Click 2 brings c; e is already there.
    let p = s.play(2).unwrap();
    assert_eq!(p.layers(), vec![(vec![0, 1], None), (vec![2], Some(2)), (vec![3, 4], None)]);
    // Click 1 brings e; c is still hidden and left out.
    assert_eq!(s.play(1).unwrap().layers(), vec![(vec![0, 1, 3], None), (vec![4], Some(4))]);
}

#[test]
fn paragraph_steps_show_their_paragraphs_one_click_at_a_time() {
    use crate::design::{Para, Run};
    let paras = vec![Para { runs: vec![Run::plain("Title")], ..Para::default() }, Para { runs: vec![Run::plain("One")], bullet: "•".into(), ..Para::default() }, Para { runs: vec![Run::plain("Two")], bullet: "•".into(), ..Para::default() }];
    let mut o = Obj::new(ObjKind::rich(paras, TextStyle::default()), PctRect::new(10.0, 10.0, 40.0, 20.0));
    o.look.fill = Some("#000000".into());
    let id = o.id;
    let step = |n: u16| Anim { para: Some((n, n)), ..Anim::new(id, AnimKind::Entrance, Effect::Fade, Start::OnClick) };
    let d = Design { objects: vec![o], anims: vec![step(1), step(2)], ..Default::default() };
    assert_eq!(d.steps(), 2);
    let shown = |s: u16| d.at_step(s).objects.iter().map(|o| match &o.kind {
        ObjKind::Text { paras, .. } => paras.iter().filter(|p| !p.hidden).map(|p| p.words()).collect::<Vec<_>>().join("+"),
        _ => String::new(),
    }).collect::<Vec<_>>();
    assert_eq!(shown(0), ["Title"]);
    assert_eq!(shown(1), ["Title", "One"]);
    assert_eq!(shown(2), ["Title", "One", "Two"]);
    // The box keeps the fill; the pieces have none, and each plays as its own layer.
    let at2 = d.at_step(2);
    assert!(at2.objects[0].look.fill.is_some() && at2.objects[1].look.fill.is_none());
    let play = d.play(1).unwrap();
    assert_eq!(play.tracks.len(), 1);
    assert_eq!((play.objects.len(), play.tracks[0].obj, play.shown.as_slice()), (3, 1, &[true, false, false][..]));
    // Saved and loaded, the step keeps its paragraphs.
    let back: Design = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
    assert_eq!(back.anims[0].para, Some((1, 1)));
}
