//! Saves the FEAT-5 dialogs as PNGs: Find a verse with results and a picked range, and Slide breaks with a break added.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example bibleext_shots -- <folder>`.
use rustypresenter::{
    app::{App, LiveUpdate},
    model::*,
    store::Store,
    ui::{FindVerseDlg, SlideBreaksDlg, dialogs::{find_verse, slide_breaks}, operator::{view::OperatorSnapshot, window::Operator}, test_support},
};
use slint::{ComponentHandle, Model};
use std::{rc::Rc, sync::Arc, time::Duration};
use uuid::Uuid;

type Step = Box<dyn Fn(&Rc<Operator>)>;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-bibleext-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let item_id = Uuid::new_v4();
    {
        let (_, verses) = rustypresenter::bible::kjv().lookup("Psalm 23:1-6").unwrap();
        let mut d = app.data.lock().unwrap();
        d.services[0].items.push(ServiceItem { id: item_id, kind: ItemKind::Scripture { reference: "Psalm 23:1-6".into(), translation: "KJV".into(), verses,
            per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() }, background: None, notes: String::new(), theme: None });
    }
    let op = test_support::operator_for_tests(app.clone());
    op.window().set_size(slint::PhysicalSize::new(1366, 820));
    sync(&op, &app);
    let steps: Vec<(&str, u64, Step)> = vec![
        ("find-verse", 6000, Box::new(|op| find_verse::open(op, "be still and know"))),
        ("find-verse-range", 1500, Box::new(|op| {
            let g = test_support::window_of(op).global::<FindVerseDlg>();
            g.invoke_hit_picked(0);
            g.invoke_line_clicked(10, true);
        })),
        ("slide-breaks", 1200, Box::new(move |op| { op.close_dialog(); slide_breaks::open(op, item_id) })),
        ("slide-breaks-added", 1200, Box::new(|op| {
            let g = test_support::window_of(op).global::<SlideBreaksDlg>();
            if let Some(i) = g.get_gaps().iter().position(|x| x.state == "none") { g.invoke_gap_clicked(i as i32) }
        })),
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
        let path = format!("{out}/bibleext-{name}.png");
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
