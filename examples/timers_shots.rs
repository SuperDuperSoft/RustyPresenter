//! Saves N3/N13/N17's screens as PNGs: the live column's walk-in music player, the walk-in music dialog, the timer
//! form with the five kinds, and the Calendar tab.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example timers_shots -- <folder>`.
use rustypresenter::{
    app::App,
    automation::{calendar::{CalendarEntry, CalendarRun, day_bit}, macros::{Macro, MacroStep}, music::{MusicList, Track}},
    lenient::Lenient,
    live::PlayState,
    model::*,
    store::Store,
    timers::{PRE_SERVICE_TIMER_ID, SERMON_ID, TimeOfDay, TimerDef, TimerKind},
    ui::{dialogs::automation, operator::window::Operator, ops::Ops},
    video::PlayerReport,
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-timers-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let rt = tokio::runtime::Runtime::new()?;
    let walk_in = Uuid::new_v4();
    {
        let mut d = app.data.lock().unwrap();
        let songs = ["Great Is Thy Faithfulness (piano)", "Come Thou Fount (acoustic)", "It Is Well (strings)", "Be Thou My Vision"];
        d.automation.music = MusicList {
            tracks: songs.iter().enumerate().map(|(i, t)| Track { id: Uuid::new_v4(), asset: format!("{i}.mp3"), title: t.to_string(), duration: Some(180.0 + 17.0 * i as f64) }).collect(),
            shuffle: true,
            ..MusicList::default()
        };
        for s in songs { d.media.push(MediaFile { id: Uuid::new_v4(), name: s.into(), kind: MediaKind::Audio, asset: "x.mp3".into(), status: MediaStatus::Ready, original_asset: None, duration: Some(200.0), thumb: None, size: None, folder: String::new() }) }
        d.automation.macros.push(Macro { id: walk_in, name: "Walk-in".into(), steps: vec![Lenient::Known(MacroStep::Music { play: true }), Lenient::Known(MacroStep::TimerRestart { timer: PRE_SERVICE_TIMER_ID })], ..Macro::default() });
        d.automation.timers.push(TimerDef { id: Uuid::new_v4(), name: "Video left".into(), kind: Lenient::Known(TimerKind::VideoLeft), on_stage: true, on_remote: true, ..TimerDef::default() });
        d.automation.calendar.push(CalendarEntry { id: Uuid::new_v4(), name: "Sunday walk-in".into(), at: TimeOfDay::new(10, 45), days: day_bit(chrono::Weekday::Sun), run: Lenient::Known(CalendarRun::Macro { macro_id: walk_in }), on: true });
        d.automation.calendar.push(CalendarEntry { id: Uuid::new_v4(), name: "Wednesday music".into(), at: TimeOfDay::new(18, 30), days: day_bit(chrono::Weekday::Wed), run: Lenient::Known(CalendarRun::Step { step: MacroStep::Music { play: true } }), on: true });
    }
    app.music(rustypresenter::automation::music::MusicOp::Play)?;
    let inst = app.automation.lock().unwrap().music.inst;
    app.music_report(PlayerReport { inst, player: 1, state: PlayState::Playing, position: 41.0, duration: 212.0, error: None });
    rt.block_on(app.do_action("timer/start", format!(r#"{{"id":"{SERMON_ID}"}}"#).as_bytes())).unwrap();
    app.automation_tick(chrono::Utc::now());

    let op = Operator::open(app.clone(), Ops::new(app.clone(), rt.handle().clone()), Rc::new(NoScreens))?;
    op.window().set_size(slint::PhysicalSize::new(1440, 900));
    let o = out.clone();
    let p = move |name: &str| format!("{o}/v1-timers-{name}.png");
    let steps: Vec<(u64, Box<dyn Fn(&Rc<Operator>)>)> = vec![
        (1500, Box::new({ let p = p.clone(); move |op: &Rc<Operator>| save(op.window(), &p("operator")) })),
        (300, Box::new(|op: &Rc<Operator>| rustypresenter::ui::dialogs::music::open(op))),
        (600, Box::new({ let p = p.clone(); move |op: &Rc<Operator>| save(op.window(), &p("music-dialog")) })),
        (300, Box::new(|op: &Rc<Operator>| { op.close_dialog(); automation::open(op); automation::goto(op, 0, 3) })),
        (600, Box::new({ let p = p.clone(); move |op: &Rc<Operator>| save(op.window(), &p("timer-kinds")) })),
        (300, Box::new(|op: &Rc<Operator>| automation::goto(op, automation::CALENDAR_TAB, 0))),
        (600, Box::new({ let p = p.clone(); move |op: &Rc<Operator>| save(op.window(), &p("calendar")) })),
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
