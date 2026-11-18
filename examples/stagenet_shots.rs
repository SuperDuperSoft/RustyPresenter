//! Saves N10 and N31 screens as PNGs: the stage layout editor with a locked box, a shape and the grid; the stage
//! window with chords, the next slide's picture and shapes; and Settings → Network….
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example stagenet_shots -- <folder>`.
use rustypresenter::{
    app::App, lenient::Lenient, live::{Command, Envelope}, model::*, store::Store,
    stage::{ElementKind, ElementStyle, PctRect, ShapeKind, StageElement, StageLayout},
    ui::{NetworkDlg, StageLayoutsDlg, dialogs::{network, stage_layouts}, operator::view::OperatorSnapshot, stage::Stage, test_support::{operator_for_tests, window_of}},
};
use slint::{ComponentHandle, Model};
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};
use uuid::Uuid;

fn el(kind: ElementKind, (x, y, w, h): (f32, f32, f32, f32), f: impl FnOnce(&mut ElementStyle)) -> Lenient<StageElement> {
    let mut style = ElementStyle::default();
    f(&mut style);
    Lenient::Known(StageElement { id: Uuid::new_v4(), kind, rect: PctRect::new(x, y, w, h), style, locked: false, hidden: false })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-stagenet-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, Some("http://192.168.1.20:4318/remote".into()));
    let layout = StageLayout {
        id: Uuid::new_v4(), name: "Band with chords".into(), background: "#07080a".into(), font: String::new(),
        elements: vec![
            el(ElementKind::Shape { shape: ShapeKind::Rounded }, (2.0, 2.0, 96.0, 58.0), |s| s.fill = Some("#141922".into())),
            el(ElementKind::CurrentSlide { pictures: false, keep_after_clear: false, chords: true }, (4.0, 4.0, 92.0, 54.0), |s| s.max_size = Some(8.0)),
            el(ElementKind::Shape { shape: ShapeKind::Line }, (2.0, 61.0, 96.0, 2.0), |s| s.color = Some("#f5c66b".into())),
            el(ElementKind::NextSlide { pictures: false, chords: true }, (2.0, 64.0, 58.0, 34.0), |s| { s.color = Some("#f5c66b".into()); s.max_size = Some(5.0); }),
            el(ElementKind::NextPicture, (62.0, 64.0, 36.0, 34.0), |_| {}),
        ],
    };
    {
        let mut d = app.data.lock().unwrap();
        d.settings.screens.stage.on = true;
        let song = Song {
            id: Uuid::new_v4(), title: "Amazing Grace".into(),
            lyrics: "Verse 1\n[G]Amazing [C]grace, how [G]sweet the sound\nThat [G]saved a wretch like [D]me\n\nChorus\n[G]My chains are [C]gone, I've been set [G]free".into(),
            ..Default::default()
        };
        d.services[0].items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: Some(Background::Color { color: "#1d3557".into() }), notes: String::new(), theme: None });
        d.songs.push(song);
        d.settings.stage.layout = layout.id;
        d.stage_layouts.push(layout);
    }
    assert!(app.command(Envelope { id: "n1".into(), base: Some(0), cmd: Command::Next }).ok);

    let stage = Stage::open(app.clone(), None).expect("stage window");
    stage.window().set_size(slint::PhysicalSize::new(1280, 720));
    let keep: Rc<RefCell<Vec<Rc<Stage>>>> = Rc::new(RefCell::new(vec![stage.clone()]));
    let op = operator_for_tests(app.clone());
    op.window().set_size(slint::PhysicalSize::new(1440, 900));
    op.window().show()?;
    op.apply_snapshot(Arc::new(OperatorSnapshot::read(&app)));
    let steps: Vec<(&str, Box<dyn Fn(&Op)>)> = vec![
        ("editor", Box::new(|op| {
            stage_layouts::open(op);
            let g = window_of(op).global::<StageLayoutsDlg>();
            g.invoke_select_layout(5);
            g.set_show_grid(true);
            g.invoke_select_box(g.get_boxes().iter().nth(3).unwrap().id);
            g.invoke_locked_changed(true);
        })),
        ("editor-shape", Box::new(|op| {
            let g = window_of(op).global::<StageLayoutsDlg>();
            g.invoke_select_box(g.get_boxes().iter().next().unwrap().id);
        })),
        ("network", Box::new(|op| {
            op.close_dialog();
            network::open(op);
            let g = window_of(op).global::<NetworkDlg>();
            g.set_remote_pw_on(true);
            g.set_stage_on(true);
            g.set_stage_pw_on(true);
            g.set_stage_url("http://192.168.1.20:4318/stage-view".into());
            g.set_tcp_on(true);
            g.set_tcp_address("192.168.1.20:4319".into());
        })),
    ];
    snap_stage(&stage, &out);
    run(op, steps, 0, out, keep);
    slint::run_event_loop()?;
    Ok(())
}

fn save(w: &slint::Window, path: &str) {
    match w.take_snapshot() {
        Ok(px) => match image::RgbaImage::from_raw(px.width(), px.height(), px.as_bytes().to_vec()).map(|img| img.save(path)) {
            Some(Ok(())) => println!("saved {path}"),
            other => eprintln!("could not save {path}: {other:?}"),
        },
        Err(e) => eprintln!("could not draw {path}: {e}"),
    }
}

fn snap_stage(stage: &Rc<Stage>, out: &str) {
    let (s, path) = (stage.clone(), format!("{out}/v1-stagenet-stage.png"));
    slint::Timer::single_shot(Duration::from_millis(900), move || save(s.window(), &path));
}

type Op = Rc<rustypresenter::ui::operator::window::Operator>;

fn run(op: Op, steps: Vec<(&'static str, Box<dyn Fn(&Op)>)>, i: usize, out: String, keep: Rc<RefCell<Vec<Rc<Stage>>>>) {
    let Some(name) = steps.get(i).map(|s| s.0.to_string()) else {
        slint::Timer::single_shot(Duration::from_millis(300), || { let _ = slint::quit_event_loop(); });
        return;
    };
    let delay = if i == 0 { 1500 } else { 700 };
    slint::Timer::single_shot(Duration::from_millis(delay / 2), move || {
        (steps[i].1)(&op);
        slint::Timer::single_shot(Duration::from_millis(delay / 2), move || {
            save(op.window(), &format!("{out}/v1-stagenet-{name}.png"));
            run(op, steps, i + 1, out, keep);
        });
    });
}
