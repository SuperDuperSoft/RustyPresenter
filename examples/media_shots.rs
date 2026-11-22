//! Saves N21/N23/N24's screens as PNGs: Settings → Media, the "Clean up unused media" question, and a video's
//! Item edit with "Go to the next slide".
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example media_shots -- <folder>`.
use rustypresenter::{
    app::App,
    model::*,
    store::Store,
    ui::{dialogs::{SettingsTab, item_edit, settings, settings_media}, operator::window::Operator, ops::Ops},
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

fn out(name: &str) -> String { format!("{}/pp-media-{name}.png", std::env::args().nth(1).unwrap_or_else(|| ".".into())) }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open(std::env::temp_dir().join(format!("rp-media-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let rt = tokio::runtime::Runtime::new()?;
    let video = Uuid::new_v4();
    let item = Uuid::new_v4();
    {
        let mut d = app.data();
        let file = |id, name: &str| MediaFile { id, name: name.into(), kind: MediaKind::Video, asset: format!("{name}.mp4"), status: MediaStatus::Ready, original_asset: None, duration: Some(42.0), thumb: None, size: None, folder: String::new() };
        d.media.push(file(video, "Welcome bumper"));
        for n in ["Easter 2024 promo", "Old countdown", "Christmas loop", "VBS recap"] { d.media.push(file(Uuid::new_v4(), n)) }
        d.services[0].items.push(ServiceItem { id: item, kind: ItemKind::Media { media_id: video, autoplay: true, end: EndAction::Next, looped: false }, background: None, notes: String::new(), theme: None });
        d.settings.media.scale = rustypresenter::media_prefs::Scale::Fill;
        d.settings.media.picture_secs = 8;
        d.settings.media.delay_ms = 80;
    }
    let op = Operator::open(app.clone(), Ops::new(app.clone(), rt.handle().clone()), Rc::new(NoScreens))?;
    op.window().set_size(slint::PhysicalSize::new(1440, 900));
    let steps: Vec<(u64, Box<dyn Fn(&Rc<Operator>)>)> = vec![
        (1200, Box::new(|op: &Rc<Operator>| settings::open(op, SettingsTab::Media))),
        (700, Box::new(|op: &Rc<Operator>| save(op.window(), &out("settings")))),
        (200, Box::new(|op: &Rc<Operator>| { op.close_dialog(); settings_media::clean_up(op) })),
        (900, Box::new(|op: &Rc<Operator>| save(op.window(), &out("cleanup")))),
        (200, Box::new(|op: &Rc<Operator>| { let _ = op.window().dispatch_event(slint::platform::WindowEvent::KeyPressed { text: slint::platform::Key::Escape.into() }); })),
        (300, Box::new(move |op: &Rc<Operator>| item_edit::open(op, item))),
        (700, Box::new(|op: &Rc<Operator>| save(op.window(), &out("item-edit")))),
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

struct NoScreens;
impl rustypresenter::ui::operator::window::OperatorScreens for NoScreens {
    fn video_info(&self) -> rustypresenter::ui::operator::window::VideoInfo {
        rustypresenter::ui::operator::window::VideoInfo { draw_mode: rustypresenter::video::DrawMode::Software, mpv: None }
    }
    fn preview_grabber(&self) -> Option<rustypresenter::video::FrameGrabber> { None }
}
