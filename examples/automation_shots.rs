//! Saves FEAT-2's screens as PNGs: the operator with the automation strip (a running timer, a message up, macros),
//! the Automation dialog's tabs, Show message, and the room with a timer and a message banner.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example automation_shots -- <folder>`.
use rustypresenter::{
    app::App,
    automation::{macros::{Macro, MacroStep}, messages::NURSERY_ID},
    lenient::Lenient,
    live::{Command, Envelope},
    model::*,
    store::Store,
    timers::{SERMON_ID, STOPWATCH_ID},
    ui::{operator::window::Operator, ops::Ops},
};
use std::{collections::BTreeMap, rc::Rc, sync::Arc, time::Duration};
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

fn cmd(app: &App, cmd: Command, base: Option<u64>) { let o = app.command(Envelope { id: Uuid::new_v4().to_string(), base, cmd }); assert!(o.ok, "{o:?}"); }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-auto-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let rt = tokio::runtime::Runtime::new()?;
    let item = Uuid::new_v4();
    {
        let mut d = app.data.lock().unwrap();
        let body = "Welcome to church!\n\nCoffee after the service\n\nYouth group Friday 7 PM".to_string();
        d.services[0].items.push(ServiceItem { id: item, kind: ItemKind::Text { title: "Announcements".into(), body }, background: None, notes: String::new(), theme: None });
        d.automation.advance.insert(item, rustypresenter::automation::advance::AutoAdvance { secs: 8, loop_item: true });
        for (name, steps) in [
            ("Walk-in", vec![MacroStep::TimerRestart { timer: STOPWATCH_ID }]),
            ("Sermon", vec![MacroStep::TimerStart { timer: SERMON_ID }, MacroStep::TimerShow { timer: SERMON_ID }]),
            ("Nursery 12", vec![MacroStep::MessageShow { template: NURSERY_ID, values: BTreeMap::from([("Number".into(), "12".into())]) }]),
            ("Offering", vec![MacroStep::Logo]),
        ] {
            d.automation.macros.push(Macro { id: Uuid::new_v4(), name: name.into(), steps: steps.into_iter().map(Lenient::Known).collect(), ..Macro::default() });
        }
    }
    rt.block_on(app.do_action("timer/start", format!(r#"{{"id":"{SERMON_ID}"}}"#).as_bytes())).unwrap();
    cmd(&app, Command::Show { item_id: item, slide: 0 }, Some(app.live.lock().unwrap().version));
    let epoch = Some(app.live.lock().unwrap().clear_epoch);
    cmd(&app, Command::ShowTimer { timer: SERMON_ID, seen_clear: epoch }, None);
    cmd(&app, Command::ShowMessage { template: NURSERY_ID, values: BTreeMap::from([("Number".into(), "27".into())]), seen_clear: epoch }, None);
    app.automation_tick(chrono::Utc::now());

    let op = Operator::open(app.clone(), Ops::new(app.clone(), rt.handle().clone()), Rc::new(NoScreens))?;
    let _ = Arc::clone(&app);
    op.window().set_size(slint::PhysicalSize::new(1440, 900));
    let room = rustypresenter::ui::room::Room::open(app.clone(), rustypresenter::render::Renderer::new(), None, None).ok();
    if let Some(r) = &room { r.window().set_size(slint::PhysicalSize::new(1280, 720)); }
    let steps: Vec<(u64, Box<dyn Fn(&Rc<Operator>)>)> = vec![
        (1500, Box::new(|op: &Rc<Operator>| save(op.window(), &format!("{}/auto-operator.png", std::env::args().nth(1).unwrap_or(".".into()))))),
        (300, Box::new(|op: &Rc<Operator>| rustypresenter::ui::dialogs::automation::open(op))),
        (700, Box::new(|op: &Rc<Operator>| { rustypresenter::ui::dialogs::automation::goto(op, 0, 1); save(op.window(), &format!("{}/auto-dialog-timers.png", std::env::args().nth(1).unwrap_or(".".into()))) })),
        (300, Box::new(|op: &Rc<Operator>| rustypresenter::ui::dialogs::automation::goto(op, 1, 0))),
        (500, Box::new(|op: &Rc<Operator>| save(op.window(), &format!("{}/auto-dialog-messages.png", std::env::args().nth(1).unwrap_or(".".into()))))),
        (300, Box::new(|op: &Rc<Operator>| rustypresenter::ui::dialogs::automation::goto(op, 2, 1))),
        (500, Box::new(|op: &Rc<Operator>| save(op.window(), &format!("{}/auto-dialog-macros.png", std::env::args().nth(1).unwrap_or(".".into()))))),
        (300, Box::new(|op: &Rc<Operator>| rustypresenter::ui::dialogs::automation::goto(op, 3, -1))),
        (500, Box::new(|op: &Rc<Operator>| save(op.window(), &format!("{}/auto-dialog-preservice.png", std::env::args().nth(1).unwrap_or(".".into()))))),
        (300, Box::new(|op: &Rc<Operator>| { op.close_dialog(); rustypresenter::ui::dialogs::show_message::open(op) })),
        (500, Box::new(|op: &Rc<Operator>| save(op.window(), &format!("{}/auto-show-message.png", std::env::args().nth(1).unwrap_or(".".into()))))),
    ];
    run(op.clone(), Rc::new(steps), 0, room.map(|r| (r, out.clone())));
    // Keep the ticker going while the shots are taken.
    let a = app.clone();
    rt.spawn(async move { loop { a.automation_tick(chrono::Utc::now()); tokio::time::sleep(Duration::from_millis(200)).await } });
    slint::run_event_loop()?;
    Ok(())
}

type Room = (Rc<rustypresenter::ui::room::Room>, String);

fn run(op: Rc<Operator>, steps: Rc<Vec<(u64, Box<dyn Fn(&Rc<Operator>)>)>>, i: usize, room: Option<Room>) {
    let Some((wait, _)) = steps.get(i) else {
        if let Some((r, out)) = &room { save(r.window(), &format!("{out}/auto-room.png")) }
        let _ = slint::quit_event_loop();
        return;
    };
    slint::Timer::single_shot(Duration::from_millis(*wait), move || {
        (steps[i].1)(&op);
        run(op, steps, i + 1, room);
    });
}

struct NoScreens;
impl rustypresenter::ui::operator::window::OperatorScreens for NoScreens {
    fn video_info(&self) -> rustypresenter::ui::operator::window::VideoInfo {
        rustypresenter::ui::operator::window::VideoInfo { draw_mode: rustypresenter::video::DrawMode::Software, mpv: None }
    }
    fn preview_grabber(&self) -> Option<rustypresenter::video::FrameGrabber> { None }
}
