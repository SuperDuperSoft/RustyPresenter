//! Saves the operator with a song live and the busiest live column (performance line, phone drawing chip) at
//! 1280 × 720, 1366 × 768 and 1920 × 1080, to check Back / Next and the clears stay in view.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example live_fit_shots -- <folder> [prefix]`.
use rustypresenter::{
    app::App,
    live::{Command, Envelope},
    model::*,
    store::Store,
    ui::{operator::window::{Operator, OperatorScreens, VideoInfo}, ops::Ops, test_support::window_of},
    video::{DrawMode, FrameGrabber},
};
use std::{rc::Rc, time::Duration};
use uuid::Uuid;

const SIZES: [(u32, u32); 3] = [(1280, 720), (1366, 768), (1920, 1080)];

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
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let prefix = std::env::args().nth(2).unwrap_or_else(|| "live".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-live-fit-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let rt = tokio::runtime::Runtime::new()?;
    let item = Uuid::new_v4();
    {
        let mut d = app.data();
        d.settings.show_performance = true;
        let lyrics = "Verse 1\nAmazing grace how sweet the sound\nThat saved a wretch like me\n\nVerse 2\n'Twas grace that taught my heart to fear\nAnd grace my fears relieved\n\nChorus\nMy chains are gone, I've been set free";
        let song = Song { id: Uuid::new_v4(), title: "Amazing Grace".into(), lyrics: lyrics.into(), writers: "John Newton".into(), ..Default::default() };
        d.services[0].items.push(ServiceItem { id: item, kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: None, notes: String::new(), theme: None });
        d.songs.push(song);
    }
    let v = app.live().version;
    let o = app.command(Envelope { id: Uuid::new_v4().to_string(), base: Some(v), cmd: Command::Show { item_id: item, slide: 0 } });
    assert!(o.ok, "{o:?}");
    let op = Operator::open(app.clone(), Ops::new(app.clone(), rt.handle().clone()), Rc::new(NoScreens))?;
    shot(op, out, prefix, 0);
    slint::run_event_loop()?;
    Ok(())
}

/// Sizes the window to `SIZES[i]`, waits for it to settle, saves it, then the next size.
fn shot(op: Rc<Operator>, out: String, prefix: String, i: usize) {
    let Some(&(w, h)) = SIZES.get(i) else {
        let _ = slint::quit_event_loop();
        return;
    };
    op.window().set_size(slint::PhysicalSize::new(w, h));
    slint::Timer::single_shot(Duration::from_millis(1500), move || {
        window_of(&op).set_drawing_on(true);
        slint::Timer::single_shot(Duration::from_millis(300), move || {
            save(op.window(), &format!("{out}/{prefix}-{w}x{h}.png"));
            shot(op, out, prefix, i + 1);
        });
    });
}
