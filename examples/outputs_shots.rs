//! Saves N5/N33's screens as PNGs: the operator's preview showing the stage screen, with the performance line on,
//! and Settings → Video. Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example outputs_shots -- <folder>`.
use rustypresenter::{
    app::App,
    live::{Command, Envelope},
    model::*,
    store::Store,
    ui::{OutputsState, dialogs::{SettingsTab, settings}, operator::window::{OperatorScreens, Operator, VideoInfo}, ops::Ops, stage::Stage, test_support::window_of},
    video::{DrawMode, FrameGrabber},
};
use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer};
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

/// The stage window, open beside the operator.
struct StageOnly(Rc<Stage>);
impl OperatorScreens for StageOnly {
    fn video_info(&self) -> VideoInfo { VideoInfo { draw_mode: DrawMode::Software, mpv: None } }
    fn preview_grabber(&self) -> Option<FrameGrabber> { None }
    fn stage_snapshot(&self, max_w: u32) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
        self.0.window().take_snapshot().ok().map(|px| rustypresenter::ui::perf::shrink(&px, max_w))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-outputs-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let rt = tokio::runtime::Runtime::new()?;
    let item = Uuid::new_v4();
    {
        let mut d = app.data.lock().unwrap();
        d.settings.show_performance = true;
        let body = "Welcome to church!\n\nCoffee after the service".to_string();
        d.services[0].items.push(ServiceItem { id: item, kind: ItemKind::Text { title: "Announcements".into(), body }, background: None, notes: "Band: start the intro".into(), theme: None });
    }
    let v = app.live.lock().unwrap().version;
    let o = app.command(Envelope { id: Uuid::new_v4().to_string(), base: Some(v), cmd: Command::Show { item_id: item, slide: 0 } });
    assert!(o.ok, "{o:?}");
    let stage = Stage::open(app.clone(), None)?;
    stage.window().set_size(slint::PhysicalSize::new(1280, 720));
    let op = Operator::open(app.clone(), Ops::new(app.clone(), rt.handle().clone()), Rc::new(StageOnly(stage.clone())))?;
    op.window().set_size(slint::PhysicalSize::new(1440, 900));
    let o2 = op.clone();
    slint::Timer::single_shot(Duration::from_millis(1500), move || {
        window_of(&o2).global::<OutputsState>().invoke_view_changed(1);
        let o3 = o2.clone();
        let out = out.clone();
        slint::Timer::single_shot(Duration::from_millis(1500), move || {
            save(o3.window(), &format!("{out}/pp-outputs-stage-preview.png"));
            settings::open(&o3, SettingsTab::Video);
            let o4 = o3.clone();
            slint::Timer::single_shot(Duration::from_millis(800), move || {
                save(o4.window(), &format!("{out}/pp-outputs-settings-video.png"));
                let _ = slint::quit_event_loop();
            });
        });
    });
    slint::run_event_loop()?;
    drop(stage);
    Ok(())
}
