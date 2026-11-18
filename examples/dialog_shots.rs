//! Saves Settings → Themes and Settings → Language as PNGs at one window size, to check that every dialog starts at
//! the same height and its footer stays in the window.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example dialog_shots -- <folder> <width> <height>`.
use rustypresenter::{
    app::{App, LiveUpdate},
    store::Store,
    ui::{dialogs::{SettingsTab, settings}, operator::{view::OperatorSnapshot, window::Operator}, test_support},
};
use std::{rc::Rc, sync::Arc, time::Duration};
use uuid::Uuid;

type Step = Box<dyn Fn(&Rc<Operator>)>;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| ".".into());
    let w: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(1366);
    let h: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(768);
    let store = Store::open(std::env::temp_dir().join(format!("rp-dialog-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let op = test_support::operator_for_tests(app.clone());
    op.window().set_size(slint::PhysicalSize::new(w, h));
    sync(&op, &app);
    let steps: Vec<(String, Step)> = vec![
        (format!("themes-{w}x{h}"), Box::new(|op| settings::open(op, SettingsTab::Themes))),
        (format!("language-{w}x{h}"), Box::new(|op| settings::open(op, SettingsTab::Language))),
    ];
    run(op, app, steps, 0, out);
    slint::run_event_loop()?;
    Ok(())
}

fn sync(op: &Operator, app: &Arc<App>) {
    op.apply_snapshot(Arc::new(OperatorSnapshot::read(app)));
    op.apply_live(LiveUpdate::new(app.live_view()));
}

fn run(op: Rc<Operator>, app: Arc<App>, steps: Vec<(String, Step)>, i: usize, out: String) {
    let Some((name, step)) = steps.get(i) else { let _ = slint::quit_event_loop(); return };
    step(&op);
    sync(&op, &app);
    let path = format!("{out}/dlgfix-{name}.png");
    slint::Timer::single_shot(Duration::from_millis(900), move || {
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
