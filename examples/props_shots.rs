//! Saves N11/N12's screens as PNGs: the Shape row in Screen Configuration, the media library's Video inputs tab,
//! and a room window cut to rounded corners over a lyric.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example props_shots -- <folder>`.
use rustypresenter::{
    app::App,
    live::{Command, Envelope},
    model::*,
    render::{Renderer, mask::{Mask, MaskShape}},
    store::Store,
    ui::{VideoInputs, dialogs::screens, operator::window::Operator, ops::Ops, room::Room, test_support::window_of},
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{cell::RefCell, rc::Rc, time::Duration};
use uuid::Uuid;

fn save(w: &slint::Window, path: &str) {
    match w.take_snapshot() {
        Ok(px) => match image::RgbaImage::from_raw(px.width(), px.height(), px.as_bytes().to_vec()).map(|img| img.save(path)) {
            Some(Ok(())) => println!("saved {path}"),
            other => eprintln!("could not save {path}: {other:?}"),
        },
        Err(e) => eprintln!("could not draw {path}: {e}"),
    }
}

fn out(name: &str) -> String { format!("{}/v1-props-{name}.png", std::env::args().nth(1).unwrap_or_else(|| ".".into())) }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open(std::env::temp_dir().join(format!("rp-props-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let rt = tokio::runtime::Runtime::new()?;
    let item = Uuid::new_v4();
    {
        let mut d = app.data.lock().unwrap();
        let s = &mut d.settings.screens.audience.screens[0];
        s.output_mut().target = ScreenTarget::Window;
        s.mask = Mask { shape: MaskShape::Rounded, inset: 4, radius: 18 };
        d.backgrounds.push(BackgroundEntry::new("Integrated Camera", Background::Camera { device: "Integrated Camera".into(), dim: 0 }, MediaStatus::Ready, chrono::Utc::now()));
        d.services[0].items.push(ServiceItem {
            id: item,
            kind: ItemKind::Text {
                title: "Welcome".into(),
                body: "Welcome home\nWe're glad you're here".into(),
                layout: Default::default(),
            },
            background: Some(Background::Gradient { from: "#2b1530".into(), to: "#0d0f13".into(), angle: 135, dim: 0 }),
            notes: String::new(),
            theme: None,
        });
    }
    let op = Operator::open(app.clone(), Ops::new(app.clone(), rt.handle().clone()), Rc::new(NoScreens))?;
    op.window().set_size(slint::PhysicalSize::new(1440, 900));
    let room: Rc<RefCell<Option<Rc<Room>>>> = Rc::default();
    let (app2, room2, room3) = (app.clone(), room.clone(), room.clone());
    let steps: Vec<(u64, Box<dyn Fn(&Rc<Operator>)>)> = vec![
        (1200, Box::new(screens::open)),
        (700, Box::new(|op: &Rc<Operator>| save(op.window(), &out("screens-shape")))),
        (200, Box::new(|op: &Rc<Operator>| {
            op.close_dialog();
            let ui = window_of(op);
            let g = ui.global::<VideoInputs>();
            g.set_searching(true);
            ui.invoke_bin_set_tab("inputs".into());
            let found: Vec<SharedString> = vec!["Integrated Camera".into(), "USB Capture HDMI".into()];
            g.set_found(ModelRc::new(VecModel::from(found)));
            g.set_searching(false);
        })),
        (700, Box::new(|op: &Rc<Operator>| save(op.window(), &out("video-inputs")))),
        (200, Box::new(move |_op: &Rc<Operator>| {
            let r = Room::open(app2.clone(), Renderer::new(), None, None).expect("room");
            r.window().set_size(slint::PhysicalSize::new(960, 540));
            app2.command(Envelope { id: "shots".into(), base: None, cmd: Command::Show { item_id: item, slide: 0 } });
            *room2.borrow_mut() = Some(r);
        })),
        (2500, Box::new(move |_op: &Rc<Operator>| if let Some(r) = room3.borrow().as_ref() { save(r.window(), &out("room-rounded")) })),
    ];
    run(op.clone(), Rc::new(steps), 0);
    slint::run_event_loop()?;
    drop(room);
    Ok(())
}

fn run(op: Rc<Operator>, steps: Rc<Vec<(u64, Box<dyn Fn(&Rc<Operator>)>)>>, i: usize) {
    let Some((wait, _)) = steps.get(i) else { let _ = slint::quit_event_loop(); return };
    slint::Timer::single_shot(Duration::from_millis(*wait), move || {
        (steps[i].1)(&op);
        run(op, steps, i + 1);
    });
}

struct NoScreens;
impl rustypresenter::ui::operator::window::OperatorScreens for NoScreens {
    fn video_info(&self) -> rustypresenter::ui::operator::window::VideoInfo {
        rustypresenter::ui::operator::window::VideoInfo { draw_mode: rustypresenter::video::DrawMode::Software, mpv: None }
    }
    fn preview_grabber(&self) -> Option<rustypresenter::video::FrameGrabber> { None }
}
