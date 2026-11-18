//! Drawing on the slide from a phone, end to end: John 3:16 live in the room and the operator, the phone remote served
//! on `127.0.0.1:<port>` for a browser to pair and draw with. Once lines arrive (and a moment passes) it saves the room
//! (`draw-room.png`) and the operator (`draw-operator.png`); it quits after `secs` seconds.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example draw_shots -- <folder> <port> <secs>`.
use rustypresenter::{
    app::App,
    live::{Command, Envelope},
    model::*,
    store::Store,
    ui::{operator::window::Operator, ops::Ops},
};
use std::{cell::Cell, net::SocketAddr, rc::Rc, time::{Duration, Instant}};
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).cloned().unwrap_or_else(|| ".".into());
    let port: u16 = args.get(2).and_then(|p| p.parse().ok()).unwrap_or(7979);
    let secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(90);
    let store = Store::open(std::env::temp_dir().join(format!("rp-draw-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, port, None);
    let rt = tokio::runtime::Runtime::new()?;
    let item = Uuid::new_v4();
    {
        let mut d = app.data.lock().unwrap();
        let verses = vec![VerseText::new("John 3:16", "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.")];
        d.services[0].items.push(ServiceItem { id: item, kind: ItemKind::Scripture { reference: "John 3:16".into(), translation: "KJV".into(), verses, per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() }, background: None, notes: String::new(), theme: None });
    }
    let v = app.live.lock().unwrap().version;
    let o = app.command(Envelope { id: "show".into(), base: Some(v), cmd: Command::Show { item_id: item, slide: 0 } });
    assert!(o.ok, "{o:?}");
    let server = app.clone();
    rt.spawn(async move {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.expect("port");
        let svc = rustypresenter::server::router(server).into_make_service_with_connect_info::<SocketAddr>();
        let _ = axum::serve(listener, svc).await;
    });
    println!("serving on http://127.0.0.1:{port}/remote");

    let op = Operator::open(app.clone(), Ops::new(app.clone(), rt.handle().clone()), Rc::new(NoScreens))?;
    op.window().set_size(slint::PhysicalSize::new(1440, 900));
    let room = rustypresenter::ui::room::Room::open(app.clone(), rustypresenter::render::Renderer::new(), None, None)?;
    room.window().set_size(slint::PhysicalSize::new(1280, 720));
    let start = Instant::now();
    let first_lines = Rc::new(Cell::new(None::<Instant>));
    let saved = Rc::new(Cell::new(false));
    let timer = slint::Timer::default();
    let (a, o, r) = (app.clone(), op.clone(), room.clone());
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(250), move || {
        let strokes = a.live.lock().unwrap().drawing.strokes.len();
        if strokes >= 2 && first_lines.get().is_none() { first_lines.set(Some(Instant::now())) }
        // Once the phone has drawn a while (and stopped for a moment), the room and the operator.
        if !saved.get() && first_lines.get().is_some_and(|t| t.elapsed() > Duration::from_secs(6)) {
            save(r.window(), &format!("{out}/draw-room.png"));
            save(o.window(), &format!("{out}/draw-operator.png"));
            saved.set(true);
        }
        if start.elapsed() > Duration::from_secs(secs) { let _ = slint::quit_event_loop(); }
    });
    slint::run_event_loop()?;
    Ok(())
}

struct NoScreens;
impl rustypresenter::ui::operator::window::OperatorScreens for NoScreens {
    fn video_info(&self) -> rustypresenter::ui::operator::window::VideoInfo {
        rustypresenter::ui::operator::window::VideoInfo { draw_mode: rustypresenter::video::DrawMode::Software, mpv: None }
    }
    fn preview_grabber(&self) -> Option<rustypresenter::video::FrameGrabber> { None }
}
