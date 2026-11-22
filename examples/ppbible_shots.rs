//! Saves the N6 / N9 / N29 UI as PNGs: a passage with Prev/Next verse, Save and Stage only; the history and picker
//! pop-ups; About this Bible; the stage-only bar; Settings → Look's fade slider; and the Classic stage with a
//! stage-only slide. Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example ppbible_shots -- <folder>`.
use rustypresenter::{
    app::{App, LiveUpdate},
    live::{Command, Envelope},
    store::Store,
    ui::{PanelExtras, dialogs::{SettingsTab, settings}, operator::{bible, bible_extras, view::OperatorSnapshot, window::Operator}, stage::Stage, test_support},
};
use slint::ComponentHandle;
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};
use uuid::Uuid;

type Step = Box<dyn Fn(&Rc<Operator>)>;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-ppbible-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let op = test_support::operator_for_tests(app.clone());
    op.window().set_size(slint::PhysicalSize::new(1440, 860));
    sync(&op, &app);
    let a = app.clone();
    let steps: Vec<(&str, u64, Step)> = vec![
        ("passage", 2500, Box::new(|op| { bible::submit(op, "John 3:16".into()); bible::submit(op, "Ruth 4:5-22".into()) })),
        ("history", 800, Box::new(|op| test_support::window_of(op).invoke_open_bible_history())),
        ("picker", 800, Box::new(|op| { close_popups(op); test_support::window_of(op).invoke_open_bible_picker() })),
        ("stage-armed", 800, Box::new(|op| { close_popups(op); test_support::window_of(op).global::<PanelExtras>().invoke_toggle_stage_mode() })),
        ("stage-up", 1200, Box::new(move |op| {
            let id = a.data().services[0].items.last().map(|i| i.id).expect("the passage");
            a.command(Envelope { id: "s".into(), base: None, cmd: Command::StageOnly { item_id: Some(id), slide: 2 } });
            test_support::window_of(op).global::<PanelExtras>().invoke_toggle_stage_mode();
        })),
        ("settings-look", 1500, Box::new(|op| settings::open(op, SettingsTab::Look))),
        ("about-bible", 800, Box::new(|op| { op.close_dialog(); bible_extras::info(op, "KJV") })),
    ];
    let stage: Rc<RefCell<Option<Rc<Stage>>>> = Rc::default();
    run(op, app, steps, 0, out, stage);
    slint::run_event_loop()?;
    Ok(())
}

/// A click on an empty spot closes the open pop-up.
fn close_popups(op: &Operator) {
    use slint::platform::{PointerEventButton, WindowEvent};
    let position = slint::LogicalPosition::new(1100.0, 770.0);
    op.window().dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
    op.window().dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
}

fn sync(op: &Operator, app: &Arc<App>) {
    op.apply_snapshot(Arc::new(OperatorSnapshot::read(app)));
    op.apply_live(LiveUpdate::new(app.live_view()));
}

fn save(w: &slint::Window, path: &str) {
    match w.take_snapshot() {
        Ok(px) => match image::RgbaImage::from_raw(px.width(), px.height(), px.as_bytes().to_vec()).map(|img| img.save(path)) {
            Some(Ok(())) => println!("saved {path}"),
            other => eprintln!("could not save {path}: {other:?}"),
        },
        Err(e) => eprintln!("could not draw {path}: {e}"),
    }
}

fn run(op: Rc<Operator>, app: Arc<App>, steps: Vec<(&'static str, u64, Step)>, i: usize, out: String, stage: Rc<RefCell<Option<Rc<Stage>>>>) {
    let Some((name, wait, step)) = steps.get(i) else {
        // Last, the Classic stage with the stage-only slide up.
        let s = Stage::open(app.clone(), None).expect("stage window");
        s.window().set_size(slint::PhysicalSize::new(1280, 720));
        *stage.borrow_mut() = Some(s);
        slint::Timer::single_shot(Duration::from_millis(1000), move || {
            if let Some(s) = stage.borrow().as_ref() { save(s.window(), &format!("{out}/pp-bible-stage-classic.png")) }
            let _ = slint::quit_event_loop();
        });
        return;
    };
    step(&op);
    sync(&op, &app);
    let name = *name;
    slint::Timer::single_shot(Duration::from_millis(*wait), move || {
        save(op.window(), &format!("{out}/pp-bible-{name}.png"));
        run(op, app, steps, i + 1, out, stage);
    });
}
