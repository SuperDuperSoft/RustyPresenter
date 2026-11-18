//! Saves the stage layout editor as PNGs: a built-in (locked), a copy being edited with a box chosen, the Add menu
//! and the top bar's layout picker.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example stage_editor_shots -- <folder>`.
use rustypresenter::{
    app::App, store::Store,
    ui::{StageLayoutsDlg, dialogs::stage_layouts, operator::view::OperatorSnapshot, test_support::{operator_for_tests, window_of}},
};
use slint::ComponentHandle;
use std::{rc::Rc, sync::Arc, time::Duration};
use uuid::Uuid;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-stage-editor-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    app.data.lock().unwrap().settings.screens.stage.on = true;
    let op = operator_for_tests(app.clone());
    op.window().set_size(slint::PhysicalSize::new(1440, 900));
    op.window().show()?;
    op.apply_snapshot(Arc::new(OperatorSnapshot::read(&app)));
    stage_layouts::open(&op);
    let steps: Vec<(&str, Box<dyn Fn(&Rc<rustypresenter::ui::operator::window::Operator>)>)> = vec![
        ("builtin", Box::new(|_| {})),
        ("edit", Box::new(|op| {
            let g = window_of(op).global::<StageLayoutsDlg>();
            g.invoke_duplicate_layout();
            g.invoke_add_box(7);
            // Drag the new clock towards the top right, near another box's edge.
            g.invoke_press(50.0, 50.0, 1.0, 1.5, false);
            g.invoke_drag(80.0, 20.6);
        })),
        ("drop", Box::new(|op| { window_of(op).global::<StageLayoutsDlg>().invoke_release(); })),
        ("text", Box::new(|op| {
            let g = window_of(op).global::<StageLayoutsDlg>();
            g.invoke_add_box(11);
            g.invoke_size_changed(8);
            g.invoke_color_changed("#f5c66b".into());
        })),
    ];
    run(op, steps, 0, out);
    slint::run_event_loop()?;
    Ok(())
}

type Op = Rc<rustypresenter::ui::operator::window::Operator>;

fn run(op: Op, steps: Vec<(&'static str, Box<dyn Fn(&Op)>)>, i: usize, out: String) {
    let Some((name, step)) = steps.get(i) else { let _ = slint::quit_event_loop(); return };
    step(&op);
    let name = name.to_string();
    slint::Timer::single_shot(Duration::from_millis(600), move || {
        let path = format!("{out}/stageedit-{name}.png");
        match op.window().take_snapshot() {
            Ok(px) => match image::RgbaImage::from_raw(px.width(), px.height(), px.as_bytes().to_vec()).map(|img| img.save(&path)) {
                Some(Ok(())) => println!("saved {path}"),
                other => eprintln!("could not save {path}: {other:?}"),
            },
            Err(e) => eprintln!("could not draw {path}: {e}"),
        }
        run(op, steps, i + 1, out);
    });
}
