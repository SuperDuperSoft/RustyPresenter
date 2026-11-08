//! Saves N22's screens as PNGs: the Record button off and while recording, Settings → Media's Recording group,
//! and the toast once the file is saved. Needs FFmpeg (it really records, from a fake room that sends nothing).
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example record_shots -- <folder>`.
use rustypresenter::{
    app::App,
    record::{FrameSink, Preset, RecordPrefs},
    store::Store,
    ui::{dialogs::{SettingsTab, settings}, operator::window::{Operator, OperatorScreens, VideoInfo}, ops::Ops},
    video::{DrawMode, FrameGrabber},
};
use std::{rc::Rc, sync::Arc, time::Duration};
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

fn out(name: &str) -> String { format!("{}/v1-record-{name}.png", std::env::args().nth(1).unwrap_or_else(|| ".".into())) }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open(std::env::temp_dir().join(format!("rp-record-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let folder = std::env::temp_dir().join("RustyPresenter recordings");
    app.data.lock().unwrap().settings.record = RecordPrefs { size: Preset::P720, folder: folder.display().to_string(), sound: "app".into() };
    let rt = tokio::runtime::Runtime::new()?;
    let op = Operator::open(app.clone(), Ops::new(app.clone(), rt.handle().clone()), Rc::new(FakeRoom))?;
    op.window().set_size(slint::PhysicalSize::new(1440, 900));
    let steps: Vec<(u64, Box<dyn Fn(&Rc<Operator>)>)> = vec![
        (1500, Box::new(|op: &Rc<Operator>| save(op.window(), &out("off")))),
        (100, Box::new(|op: &Rc<Operator>| op.toggle_recording())),
        (3300, Box::new(|op: &Rc<Operator>| save(op.window(), &out("on")))),
        (100, Box::new(|op: &Rc<Operator>| settings::open(op, SettingsTab::Media))),
        (900, Box::new(|op: &Rc<Operator>| save(op.window(), &out("settings")))),
        (100, Box::new(|op: &Rc<Operator>| {
            use slint::{LogicalPosition, platform::WindowEvent};
            let at = LogicalPosition::new(700.0, 500.0);
            op.window().dispatch_event(WindowEvent::PointerMoved { position: at });
            op.window().dispatch_event(WindowEvent::PointerScrolled { position: at, delta_x: 0.0, delta_y: -2000.0 });
        })),
        (600, Box::new(|op: &Rc<Operator>| save(op.window(), &out("settings-recording")))),
        (100, Box::new(|op: &Rc<Operator>| { op.close_dialog(); op.toggle_recording() })),
        (1500, Box::new(|op: &Rc<Operator>| save(op.window(), &out("saved")))),
    ];
    run(op.clone(), Rc::new(steps), 0);
    slint::run_event_loop()?;
    Ok(())
}

fn run(op: Rc<Operator>, steps: Rc<Vec<(u64, Box<dyn Fn(&Rc<Operator>)>)>>, i: usize) {
    let Some((wait, _)) = steps.get(i) else { let _ = slint::quit_event_loop(); return };
    slint::Timer::single_shot(Duration::from_millis(*wait), move || {
        (steps[i].1)(&op);
        run(op, steps, i + 1);
    });
}

/// A room window that is "open" but sends no pictures: the recording is black.
struct FakeRoom;
impl OperatorScreens for FakeRoom {
    fn video_info(&self) -> VideoInfo { VideoInfo { draw_mode: DrawMode::Software, mpv: None } }
    fn preview_grabber(&self) -> Option<FrameGrabber> { None }
    fn record_to(&self, _: Option<Arc<FrameSink>>) -> bool { true }
}
