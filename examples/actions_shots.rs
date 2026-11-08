//! Saves N15 and N16 as PNGs: the action palette over a song's slides with badges, and the Pre-service tab.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example actions_shots -- <folder>`.
use rustypresenter::{
    app::{App, LiveUpdate},
    automation::{macros::MacroStep, slide_actions},
    model::*,
    store::Store,
    timers::SERMON_ID,
    ui::{dialogs::automation, operator::{action_palette, view::OperatorSnapshot, window::Operator}, test_support},
};
use slint::ComponentHandle;
use std::{rc::Rc, sync::Arc, time::Duration};
use uuid::Uuid;

type Step = Box<dyn Fn(&Rc<Operator>)>;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-actions-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let item_id = Uuid::new_v4();
    {
        let mut d = app.data.lock().unwrap();
        let body = "Welcome to church\n\nPlease find a seat\n\nWe begin in a few minutes\n\nTurn your phone to silent";
        d.services[0].items.push(ServiceItem { id: item_id, kind: ItemKind::Text { title: "Welcome".into(), body: body.into() }, background: None, notes: String::new(), theme: None });
        let a = &mut d.automation;
        slide_actions::add(a, item_id, 0, vec![MacroStep::TimerRestart { timer: SERMON_ID }, MacroStep::TimerShow { timer: SERMON_ID }]).unwrap();
        slide_actions::add(a, item_id, 2, vec![MacroStep::GoToSlide { slide: 0 }]).unwrap();
    }
    let op = test_support::operator_for_tests(app.clone());
    op.window().set_size(slint::PhysicalSize::new(1366, 820));
    sync(&op, &app);
    let steps: Vec<(&str, u64, Step)> = vec![
        ("palette", 2500, Box::new(move |op| {
            test_support::window_of(op).invoke_service_select(item_id.to_string().into());
            action_palette::toggle(op);
        })),
        ("preservice", 1200, Box::new(|op| { automation::open(op); automation::goto(op, 3, -1) })),
    ];
    run(op, app, steps, 0, out);
    slint::run_event_loop()?;
    Ok(())
}

fn sync(op: &Operator, app: &Arc<App>) {
    op.apply_snapshot(Arc::new(OperatorSnapshot::read(app)));
    op.apply_live(LiveUpdate::new(app.live_view()));
}

fn run(op: Rc<Operator>, app: Arc<App>, steps: Vec<(&'static str, u64, Step)>, i: usize, out: String) {
    let Some((name, wait, step)) = steps.get(i) else { let _ = slint::quit_event_loop(); return };
    step(&op);
    sync(&op, &app);
    let name = *name;
    slint::Timer::single_shot(Duration::from_millis(*wait), move || {
        let path = format!("{out}/v1-actions-{name}.png");
        match op.window().take_snapshot() {
            Ok(px) => match image::RgbaImage::from_raw(px.width(), px.height(), px.as_bytes().to_vec()).map(|img| img.save(&path)) {
                Some(Ok(())) => println!("saved {path}"),
                other => eprintln!("could not save {path}: {other:?}"),
            },
            Err(e) => eprintln!("could not draw {path}: {e}"),
        }
        run(op, app, steps, i + 1, out);
    });
}
