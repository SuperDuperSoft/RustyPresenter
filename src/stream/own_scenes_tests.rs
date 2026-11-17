//! Tests of `stream::own_scenes`.
use super::*;

const W: u32 = 40;
const H: u32 = 20;
const RED: [u8; 4] = [200, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 200, 255];
const GREEN: [u8; 4] = [0, 200, 0, 255];

fn solid(c: [u8; 4]) -> Vec<u8> { c.repeat((W * H) as usize) }
fn px(img: &[u8], x: u32, y: u32) -> [u8; 4] { let i = ((y * W + x) * 4) as usize; img[i..i + 4].try_into().unwrap() }
fn scene(layers: Vec<Layer>) -> OwnScene { OwnScene { id: Uuid::from_u128(7), name: "Ours".into(), layers, ..OwnScene::default() } }
fn layer(shows: Shows, place: Place) -> Layer { Layer { shows, place, ..Layer::default() } }
fn draw(s: &OwnScene, room: &[u8], camera: Option<&[u8]>, cameras: &[Option<&[u8]>], words: Option<&[u8]>) -> Vec<u8> {
    let mut fonts = crate::render::Fonts::bundled_only();
    let p = prepare(s, (W, H), &Theme::default(), &mut fonts);
    let mut out = vec![7u8; (W * H * 4) as usize];
    compose(&p, &Inputs { size: (W, H), room, camera, words, card: None, cameras, own: None, more: &[] }, &mut out);
    out
}

#[test]
fn layers_go_back_to_front() {
    let s = scene(vec![layer(Shows::Room, Place::Full), layer(Shows::Camera { number: 0 }, Place::RightHalf)]);
    let (room, cam) = (solid(BLUE), solid(RED));
    let out = draw(&s, &room, Some(&cam), &[], None);
    assert_eq!((px(&out, 5, 10), px(&out, 30, 10)), (BLUE, RED), "the room on the left, the camera in front on the right");
    let s = scene(s.layers.into_iter().rev().collect());
    assert_eq!(draw(&s, &room, Some(&cam), &[], None), room, "the room in front covers the camera");
}

#[test]
fn each_camera_by_its_number_and_a_missing_one_shows_the_room() {
    let s = scene(vec![layer(Shows::Camera { number: 1 }, Place::LeftHalf), layer(Shows::Camera { number: 2 }, Place::RightHalf)]);
    let (room, one, two) = (solid(BLUE), solid(RED), solid(GREEN));
    let out = draw(&s, &room, None, &[Some(&one), Some(&two)], None);
    assert_eq!((px(&out, 5, 10), px(&out, 30, 10)), (RED, GREEN));
    let out = draw(&s, &room, None, &[Some(&one), None], None);
    assert_eq!(px(&out, 30, 10), BLUE, "camera 2 sends nothing: the room in its place");
    let s = scene(vec![layer(Shows::Camera { number: 3 }, Place::Full)]);
    assert_eq!(draw(&s, &room, None, &[Some(&one)], None), room, "a camera that is not set up");
}

#[test]
fn see_through_lets_what_is_behind_show() {
    let s = scene(vec![layer(Shows::Room, Place::Full), Layer { see_through: 50, ..layer(Shows::Camera { number: 0 }, Place::Full) }]);
    let out = draw(&s, &solid([200, 0, 0, 255]), Some(&solid([0, 0, 200, 255])), &[], None);
    let p = px(&out, 10, 10);
    assert!((95..=105).contains(&p[0]) && (95..=105).contains(&p[2]) && p[3] == 255, "half of each: {p:?}");
    let gone = scene(vec![layer(Shows::Room, Place::Full), Layer { see_through: 100, ..layer(Shows::Colour { colour: "#ffffff".into() }, Place::Full) }]);
    assert_eq!(draw(&gone, &solid(RED), None, &[], None), solid(RED), "100% see-through is not seen");
}

#[test]
fn a_colour_fills_its_place_and_black_is_behind_everything() {
    let s = scene(vec![layer(Shows::Colour { colour: "#00c800".into() }, Place::Box { x: 50, y: 0, w: 50, h: 50 })]);
    let out = draw(&s, &solid(RED), None, &[], None);
    assert_eq!((px(&out, 30, 2), px(&out, 30, 15), px(&out, 5, 2)), (GREEN, [0, 0, 0, 255], [0, 0, 0, 255]));
}

#[test]
fn the_words_go_over_what_is_behind() {
    let mut words = vec![0u8; (W * H * 4) as usize];
    words[(15 * W * 4) as usize..(16 * W * 4) as usize].fill(255);
    let s = scene(vec![layer(Shows::Camera { number: 0 }, Place::Full), layer(Shows::Words, Place::Full)]);
    let out = draw(&s, &solid(BLUE), Some(&solid(RED)), &[], Some(&words));
    assert_eq!((px(&out, 3, 15), px(&out, 3, 2)), ([255; 4], RED));
    assert_eq!(draw(&s, &solid(BLUE), Some(&solid(RED)), &[], None), solid(RED), "no words live: nothing over the camera");
}

#[test]
fn a_camera_in_a_half_keeps_its_shape() {
    // A picture with a red left half and a blue right half, in the left half of the stream: its middle shows.
    let mut cam = vec![0u8; (W * H * 4) as usize];
    for y in 0..H { for x in 0..W { let i = ((y * W + x) * 4) as usize; cam[i..i + 4].copy_from_slice(if x < W / 2 { &RED } else { &BLUE }) } }
    let s = scene(vec![layer(Shows::Camera { number: 0 }, Place::LeftHalf)]);
    let out = draw(&s, &solid(GREEN), Some(&cam), &[], None);
    assert_eq!((px(&out, 1, 10), px(&out, 18, 10)), (RED, BLUE), "the middle of the picture, not squeezed");
    assert_eq!(px(&out, 30, 10), [0, 0, 0, 255]);
}

#[test]
fn places_stay_inside_the_picture() {
    assert_eq!(Place::Full.rect((1920, 1080)), (0, 0, 1920, 1080));
    assert_eq!(Place::LeftHalf.rect((1920, 1080)), (0, 0, 960, 1080));
    assert_eq!(Place::RightHalf.rect((1921, 1080)), (960, 0, 961, 1080));
    assert_eq!(Place::Box { x: 90, y: 90, w: 50, h: 50 }.rect((1000, 500)), (900, 450, 100, 50));
    assert_eq!(Place::Box { x: 10, y: 20, w: 30, h: 40 }.rect((1000, 500)), (100, 100, 300, 200));
    let corner = Place::Corner { corner: Corner::TopLeft, size: CornerSize::Large };
    assert_eq!(corner.rect((1280, 720)), corner_rect((1280, 720), &Layout { corner: Corner::TopLeft, corner_size: CornerSize::Large, words_top: false }));
    assert_eq!(Place::RightHalf.as_box(), Place::Box { x: 50, y: 0, w: 50, h: 100 });
}

#[test]
fn every_choice_has_its_own_place_in_the_lists() {
    for (i, p) in Place::choices().iter().enumerate() { assert_eq!(p.index(), i, "{p:?}") }
    for (i, s) in Shows::choices().iter().enumerate() { assert_eq!(s.index(), Some(i), "{s:?}") }
    assert_eq!(Place::Box { x: 1, y: 2, w: 3, h: 4 }.index(), Place::choices().len() - 1);
    assert_eq!(Shows::Unknown.index(), None);
}

#[test]
fn changing_what_a_layer_shows_keeps_what_it_held() {
    let pic = Shows::Picture { path: "logo.png".into() };
    assert_eq!(pic.changed_to(&Shows::Picture { path: String::new() }), pic);
    assert_eq!(Shows::Room.changed_to(&Shows::Text { text: String::new(), look: TextLook::default() }), Shows::Text { text: "Welcome".into(), look: TextLook::default() });
    assert_eq!(Shows::Room.changed_to(&Shows::Colour { colour: String::new() }), Shows::Colour { colour: "#000000".into() });
    assert_eq!(pic.changed_to(&Shows::Camera { number: 2 }), Shows::Camera { number: 2 });
}

#[test]
fn a_text_layer_is_drawn_in_its_place_only() {
    let s = scene(vec![layer(Shows::Text { text: "Welcome".into(), look: TextLook::default() }, Place::Box { x: 0, y: 0, w: 50, h: 100 })]);
    let mut fonts = crate::render::Fonts::bundled_only();
    let size = (320, 180);
    let p = prepare(&s, size, &Theme::default(), &mut fonts);
    let room = [0u8, 0, 0, 255].repeat(320 * 180);
    let mut out = vec![0u8; 320 * 180 * 4];
    compose(&p, &Inputs { size, room: &room, camera: None, words: None, card: None, cameras: &[], own: None, more: &[] }, &mut out);
    let lit = |x0: usize, x1: usize| (0..180).any(|y| (x0..x1).any(|x| out[(y * 320 + x) * 4] > 100));
    assert!(lit(0, 160), "the words in the left half");
    assert!(!lit(160, 320), "nothing in the right half");
}

#[test]
fn a_picture_layer_keeps_its_see_through_parts() {
    let dir = std::env::temp_dir().join(format!("rp-test-own-scene-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("logo.png");
    // Left half red, right half clear.
    let img = image::RgbaImage::from_fn(20, 20, |x, _| if x < 10 { image::Rgba([200, 0, 0, 255]) } else { image::Rgba([0, 0, 0, 0]) });
    img.save(&path).unwrap();
    let s = scene(vec![layer(Shows::Room, Place::Full), layer(Shows::Picture { path: path.to_string_lossy().into() }, Place::Box { x: 0, y: 0, w: 50, h: 100 })]);
    let out = draw(&s, &solid(BLUE), None, &[], None);
    assert_eq!(px(&out, 7, 10), RED, "the picture's solid part");
    assert_eq!(px(&out, 13, 10), BLUE, "its clear part shows the room");
    assert_eq!(px(&out, 30, 10), BLUE);
    let missing = scene(vec![layer(Shows::Room, Place::Full), layer(Shows::Picture { path: "C:/no/such.png".into() }, Place::Full)]);
    assert_eq!(draw(&missing, &solid(BLUE), None, &[], None), solid(BLUE), "a picture that cannot be read is left out");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_church_scene_stands_in_only_where_it_is_asked_to() {
    let mut ours = OwnScene::new("Songs".into());
    ours.instead_of = Some(Scene::CameraWords);
    let list = vec![ours.clone()];
    assert_eq!(stand_in(Scene::CameraWords, &list), Scene::Own(ours.id));
    assert_eq!(stand_in(Scene::Camera, &list), Scene::Camera);
    assert_eq!(name(Scene::Own(ours.id), &list), "Songs");
    assert_eq!(name(Scene::Own(Uuid::from_u128(9)), &list), "A scene that was deleted");
    assert_eq!(all(&list).last(), Some(&Scene::Own(ours.id)));
    assert_eq!(all(&list).len(), Scene::ALL.len() + 1);
}

#[test]
fn which_camera_a_scene_shows() {
    let s = scene(vec![layer(Shows::Camera { number: 2 }, Place::Full)]);
    assert!(s.shows_camera(1) && !s.shows_camera(0));
    let list = vec![s.clone()];
    assert!(shows_camera(Scene::Own(s.id), 1, &list));
    assert!(!shows_camera(Scene::Own(s.id), 0, &list));
    assert!(shows_camera(Scene::Camera, 0, &list));
    assert!(OwnScene::new("x".into()).shows_camera(3), "the camera on air, whichever it is");
}

#[test]
fn scenes_are_kept_and_read_back_and_newer_kinds_do_not_break_them() {
    let s = OwnScene::new("Welcome".into());
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["layers"][1], serde_json::json!({ "shows": { "kind": "camera", "number": 0 }, "place": { "at": "corner", "corner": "bottom_right", "size": "medium" } }));
    assert_eq!(serde_json::from_value::<OwnScene>(v).unwrap(), s);
    let newer = serde_json::json!({ "id": Uuid::from_u128(1), "name": "x", "layers": [{ "shows": { "kind": "hologram" }, "place": { "at": "spiral" }, "see_through": 10 }] });
    let s: OwnScene = serde_json::from_value(newer).unwrap();
    assert_eq!((s.layers[0].shows.clone(), s.layers[0].place), (Shows::Unknown, Place::Full));
}

#[test]
fn the_preview_draws_the_stand_ins() {
    let mut fonts = crate::render::Fonts::bundled_only();
    let stand = StandIns::new((160, 90), &Theme::default(), &mut fonts);
    let out = preview(&OwnScene::new("x".into()), &stand, &Theme::default(), &mut fonts);
    assert_eq!(out.len(), 160 * 90 * 4);
    assert_eq!(&out[..3], &stand.room[..3], "the room screen behind");
    let (x, y, _, ch) = corner_rect((160, 90), &Layout::default());
    let i = (((y + ch - 1) * 160 + x + 1) * 4) as usize;
    assert_eq!(&out[i..i + 3], &[92, 92, 92], "the grey camera in the corner");
}


/// A camera picture with a red left half and a blue right half.
fn halves() -> Vec<u8> {
    let mut cam = vec![0u8; (W * H * 4) as usize];
    for y in 0..H { for x in 0..W { let i = ((y * W + x) * 4) as usize; cam[i..i + 4].copy_from_slice(if x < W / 2 { &RED } else { &BLUE }) } }
    cam
}

#[test]
fn a_cropped_camera_shows_only_what_is_left() {
    // Its left half cut off: the blue half fills the picture.
    let s = scene(vec![Layer { crop: Crop { left: 50, ..Crop::default() }, ..layer(Shows::Camera { number: 0 }, Place::Full) }]);
    let out = draw(&s, &solid(GREEN), Some(&halves()), &[], None);
    assert_eq!((px(&out, 1, 10), px(&out, 38, 10)), (BLUE, BLUE));
    assert_eq!(Crop { left: 80, right: 80, top: 0, bottom: 0 }.checked(), Crop { left: 80, right: 19, top: 0, bottom: 0 }, "always something left");
    assert_eq!(Crop { left: 25, top: 10, right: 25, bottom: 0 }.rect((100, 50)), (25, 5, 50, 45));
}

#[test]
fn a_green_screen_camera_shows_what_is_behind_it() {
    let mut cam = solid([0, 177, 64, 255]);
    cam[..4].copy_from_slice(&RED);
    let key = crate::stream::key_out::Key::Colour { colour: "#00b140".into(), closeness: 40, soft: 10 };
    let s = scene(vec![layer(Shows::Room, Place::Full), Layer { key: key.clone(), ..layer(Shows::Camera { number: 0 }, Place::Full) }]);
    let out = draw(&s, &solid(BLUE), Some(&cam), &[], None);
    assert_eq!((px(&out, 0, 0), px(&out, 10, 10)), (RED, BLUE), "the person stays, the green goes");
    // The room standing in for a camera with no picture is never keyed.
    let out = draw(&s, &solid([0, 177, 64, 255]), None, &[], None);
    assert_eq!(px(&out, 10, 10), [0, 177, 64, 255]);
    // A key on a colour layer does nothing.
    let s = scene(vec![Layer { key, ..layer(Shows::Colour { colour: "#00b140".into() }, Place::Full) }]);
    assert_eq!(px(&draw(&s, &solid(BLUE), None, &[], None), 3, 3), [0, 177, 64, 255]);
}

#[test]
fn the_tall_picture_uses_the_tall_places() {
    let s = OwnScene { tall: true, ..scene(vec![
        layer(Shows::Room, Place::Full),
        Layer { tall: Some(Place::Box { x: 0, y: 50, w: 100, h: 50 }), ..layer(Shows::Camera { number: 0 }, Place::RightHalf) },
    ]) };
    let mut fonts = crate::render::Fonts::bundled_only();
    let tall = (20, 40);
    let p = prepare_at(&s, tall, true, &Theme::default(), &mut fonts, &Now::local(None));
    let (room, cam) = (solid(BLUE), solid(RED));
    let mut out = vec![0u8; 20 * 40 * 4];
    compose(&p, &Inputs { size: (W, H), room: &room, camera: Some(&cam), words: None, card: None, cameras: &[], own: None, more: &[] }, &mut out);
    let at = |x: usize, y: usize| out[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4].to_vec();
    assert_eq!((at(10, 5), at(10, 35), at(2, 35)), (BLUE.to_vec(), RED.to_vec(), RED.to_vec()), "the room on top, the camera across the bottom half");
    assert_eq!(s.layers[0].place_in(true), Place::Full, "a layer with no tall place keeps its wide one");
    assert_eq!(Place::LeftHalf.as_box_in((1080, 1920)), Place::Box { x: 0, y: 0, w: 50, h: 100 });
}

#[test]
fn a_clock_is_drawn_again_only_when_its_words_change() {
    let s = scene(vec![layer(Shows::Clock { seconds: true, h24: true, look: TextLook { size: 60, colour: "#ff0000".into() } }, Place::Full)]);
    let mut fonts = crate::render::Fonts::bundled_only();
    let at = |h, m, sec| Now { at: chrono::NaiveDate::from_ymd_opt(2026, 11, 22).unwrap().and_hms_opt(h, m, sec).unwrap(), starts_at: None };
    let p = prepare_at(&s, (320, 180), false, &Theme::default(), &mut fonts, &at(10, 15, 0));
    assert!(p.ticks());
    assert_eq!(p.tick_texts(&at(10, 15, 0)), vec!["10:15:00"]);
    assert!(p.retick(&at(10, 15, 0), &mut fonts).is_none(), "the same second: nothing to draw");
    let q = p.retick(&at(10, 15, 1), &mut fonts).expect("a new second");
    assert_eq!(q.tick_texts(&at(10, 15, 1)), q.texts);
    let room = [0u8, 0, 0, 255].repeat(320 * 180);
    let mut out = vec![0u8; 320 * 180 * 4];
    compose(&q, &Inputs { size: (320, 180), room: &room, camera: None, words: None, card: None, cameras: &[], own: None, more: &[] }, &mut out);
    assert!(out.chunks_exact(4).any(|p| p[0] > 150 && p[1] < 60), "red words");
}

#[test]
fn a_countdown_counts_to_its_time_or_the_service_start() {
    let now = |starts_at| Now { at: chrono::NaiveDate::from_ymd_opt(2026, 11, 22).unwrap().and_hms_opt(10, 25, 30).unwrap(), starts_at };
    let count = |to| tick_text(&Tick::Countdown { to }, &now(Some(TimeOfDay::new(10, 30))));
    assert_eq!(count(None), "4:30", "to the service start");
    assert_eq!(count(Some(TimeOfDay::new(11, 0))), "34:30");
    assert_eq!(count(Some(TimeOfDay::new(9, 0))), "0:00", "the time has passed");
    assert_eq!(tick_text(&Tick::Countdown { to: None }, &now(None)), "\u{2013}:\u{2013}\u{2013}", "no service start time");
}

#[test]
fn video_and_web_page_layers_take_their_pictures_from_the_writer() {
    let s = scene(vec![
        layer(Shows::Video { path: "C:/loop.mp4".into() }, Place::LeftHalf),
        layer(Shows::Web { address: "https://example.org/lower-third".into() }, Place::Full),
        layer(Shows::Video { path: "C:/loop.mp4".into() }, Place::RightHalf),
    ]);
    let mut fonts = crate::render::Fonts::bundled_only();
    let p = prepare(&s, (W, H), &Theme::default(), &mut fonts);
    assert_eq!(p.moving(), &[Moving::Video("C:/loop.mp4".into()), Moving::Web("https://example.org/lower-third".into())], "each once");
    let (video, mut web) = (solid(RED), vec![0u8; (W * H * 4) as usize]);
    web[..4].copy_from_slice(&[255; 4]);
    let more = [Some(video.as_slice()), Some(web.as_slice())];
    let room = solid(BLUE);
    let mut out = vec![0u8; (W * H * 4) as usize];
    compose(&p, &Inputs { size: (W, H), room: &room, camera: None, words: None, card: None, cameras: &[], own: None, more: &more }, &mut out);
    assert_eq!((px(&out, 5, 10), px(&out, 30, 10), px(&out, 0, 0)), (RED, RED, [255; 4]), "the video in both halves, the page over it");
    let more = [None, None];
    compose(&p, &Inputs { size: (W, H), room: &room, camera: None, words: None, card: None, cameras: &[], own: None, more: &more }, &mut out);
    assert_eq!(px(&out, 5, 10), [0, 0, 0, 255], "no picture yet: nothing drawn");
    assert_eq!(videos(std::slice::from_ref(&s)), vec!["C:/loop.mp4".to_string()]);
    assert_eq!(web_pages(&[s]), vec!["https://example.org/lower-third".to_string()]);
}

#[test]
fn new_layer_kinds_are_kept_and_read_back() {
    let s = scene(vec![
        Layer { crop: Crop { left: 10, ..Crop::default() }, tall: Some(Place::LeftHalf), ..layer(Shows::Countdown { to: Some(TimeOfDay::new(10, 30)), look: TextLook { size: 90, colour: "#ffffff".into() } }, Place::Full) },
        layer(Shows::Clock { seconds: false, h24: true, look: TextLook::default() }, Place::Full),
        layer(Shows::Text { text: "Hi".into(), look: TextLook { size: 0, colour: "#ff0000".into() } }, Place::Full),
    ]);
    let s = OwnScene { tall: true, transition: Some(Transition::Slide), ..s };
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["layers"][0]["shows"], serde_json::json!({ "kind": "countdown", "to": "10:30", "size": 90, "colour": "#ffffff" }));
    assert_eq!(v["layers"][1]["shows"], serde_json::json!({ "kind": "clock", "h24": true }));
    assert_eq!(v["layers"][2]["shows"], serde_json::json!({ "kind": "text", "text": "Hi", "colour": "#ff0000" }));
    assert_eq!(serde_json::from_value::<OwnScene>(v).unwrap(), s);
    let old = serde_json::json!({ "id": Uuid::from_u128(2), "layers": [{ "shows": { "kind": "text", "text": "Welcome" } }] });
    let old: OwnScene = serde_json::from_value(old).unwrap();
    assert_eq!((old.tall, old.transition.clone(), old.layers[0].shows.clone()), (false, None, Shows::Text { text: "Welcome".into(), look: TextLook::default() }));
    assert_eq!(transition_to(Scene::Own(s.id), std::slice::from_ref(&s), &Transition::Cut), Transition::Slide);
    assert_eq!(transition_to(Scene::Own(old.id), &[old], &Transition::Cut), Transition::Cut, "no transition of its own: the default");
}
