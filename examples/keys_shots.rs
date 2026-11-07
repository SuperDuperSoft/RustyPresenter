//! Saves N18/N28/N35's screens as PNGs: Settings → Help with the shortcut editor and the clear groups, a key being
//! changed, a new clear group, and Find my mouse's rings.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example keys_shots -- <folder>`.
use rustypresenter::{
    app::App,
    store::Store,
    ui::{dialogs::{SettingsTab, settings, settings_keys}, operator::window::Operator, ops::Ops},
};
use std::{rc::Rc, time::Duration};
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

fn out(name: &str) -> String { format!("{}/{name}", std::env::args().nth(1).unwrap_or_else(|| ".".into())) }

type Step = (u64, Box<dyn Fn(&Rc<Operator>)>);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open(std::env::temp_dir().join(format!("rp-keys-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let rt = tokio::runtime::Runtime::new()?;
    let op = Operator::open(app.clone(), Ops::new(app.clone(), rt.handle().clone()), Rc::new(NoScreens))?;
    op.window().set_size(slint::PhysicalSize::new(1440, 900));
    let steps: Vec<Step> = vec![
        (1500, Box::new(|op| settings::open(op, SettingsTab::Help))),
        (600, Box::new(|op| save(op.window(), &out("pp-keys-settings-help.png")))),
        (100, Box::new(|op| settings_keys::press_change(op, 10))),
        (400, Box::new(|op| save(op.window(), &out("pp-keys-press-a-key.png")))),
        (100, Box::new(|op| { op.window().dispatch_event(slint::platform::WindowEvent::KeyPressed { text: char::from(slint::platform::Key::F2).into() }); })),
        (400, Box::new(|op| save(op.window(), &out("pp-keys-clash.png")))),
        (100, Box::new(|op| { op.close_dialog(); settings::open(op, SettingsTab::Help); settings_keys::press_add_group(op) })),
        (500, Box::new(|op| save(op.window(), &out("pp-keys-new-group.png")))),
        (100, Box::new(|op| { op.close_dialog(); op.find_mouse() })),
        (380, Box::new(|op| save(op.window(), &out("pp-keys-find-mouse.png")))),
    ];
    run(op.clone(), Rc::new(steps), 0);
    slint::run_event_loop()?;
    Ok(())
}

fn run(op: Rc<Operator>, steps: Rc<Vec<Step>>, i: usize) {
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
