//! Saves N20's screens as PNGs: Screen Configuration's Screen and Picture tabs, and a room window showing a test
//! pattern. Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example screens_shots -- <folder>`.
use rustypresenter::{
    app::App,
    model::*,
    screen_fix::TestPattern,
    store::Store,
    ui::{ScreensDialogState, dialogs::screens, operator::window::{OperatorScreens, Operator, VideoInfo}, ops::Ops, room::Room, test_support::window_of},
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

struct NoScreens;
impl OperatorScreens for NoScreens {
    fn video_info(&self) -> VideoInfo { VideoInfo { draw_mode: DrawMode::Software, mpv: None } }
    fn preview_grabber(&self) -> Option<FrameGrabber> { None }
    fn stage_snapshot(&self, _: u32) -> Option<SharedPixelBuffer<Rgba8Pixel>> { None }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-screens-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let rt = tokio::runtime::Runtime::new()?;
    {
        let mut d = app.data();
        d.settings.screens.assign("audience", ScreenTarget::Window);
        let s = &mut d.settings.screens.audience.screens[0];
        s.fix.red = 0.92;
        s.fix.gamma = 1.15;
        s.fix.black = 0.03;
        s.fix.corners[0] = [0.025, 0.01];
        s.fix.delay = 2;
        s.test = TestPattern::Grid;
        let mut lobby = Screen::new(Uuid::new_v4(), "Lobby TV");
        lobby.output_mut().target = ScreenTarget::Placeholder;
        d.settings.screens.audience.screens.push(lobby);
    }
    let room = Room::open(app.clone(), rustypresenter::render::Renderer::new(), None, None)?;
    room.window().set_size(slint::PhysicalSize::new(1280, 720));
    let op = Operator::open(app.clone(), Ops::new(app.clone(), rt.handle().clone()), Rc::new(NoScreens))?;
    op.window().set_size(slint::PhysicalSize::new(1440, 900));
    let o2 = op.clone();
    let room2 = room.clone();
    slint::Timer::single_shot(Duration::from_millis(1500), move || {
        save(room2.window(), &format!("{out}/v1-screens-room-grid.png"));
        screens::open(&o2);
        let o3 = o2.clone();
        let out = out.clone();
        slint::Timer::single_shot(Duration::from_millis(800), move || {
            save(o3.window(), &format!("{out}/v1-screens-screen-tab.png"));
            window_of(&o3).global::<ScreensDialogState>().set_tab(1);
            let o4 = o3.clone();
            slint::Timer::single_shot(Duration::from_millis(800), move || {
                save(o4.window(), &format!("{out}/v1-screens-picture-tab.png"));
                let _ = slint::quit_event_loop();
            });
        });
    });
    slint::run_event_loop()?;
    drop(room);
    Ok(())
}
