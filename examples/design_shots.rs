//! Saves the slide editor (N26) as PNGs: a sermon slide being made, a text box chosen, and the room's picture of
//! the finished slide.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example design_shots -- <folder>`.
use rustypresenter::{
    app::App, store::Store,
    ui::{DesignDlg, dialogs::design_editor, operator::view::OperatorSnapshot, test_support::{operator_for_tests, window_of}},
};
use slint::ComponentHandle;
use std::{rc::Rc, sync::Arc, time::Duration};
use uuid::Uuid;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-design-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let op = operator_for_tests(app.clone());
    op.window().set_size(slint::PhysicalSize::new(1440, 900));
    op.window().show()?;
    op.apply_snapshot(Arc::new(OperatorSnapshot::read(&app)));
    design_editor::open(&op, None);
    let steps: Vec<(&str, Box<dyn Fn(&Op)>)> = vec![
        ("slide", Box::new(|op| {
            let g = window_of(op).global::<DesignDlg>();
            g.invoke_add_shape(0);
            g.invoke_press(50.0, 50.0, 1.0, 1.5, false, false);
            g.invoke_drag(50.0, 70.0);
            g.invoke_release();
            g.invoke_radius_changed(30);
            g.invoke_opacity_changed(70);
            g.invoke_add_text();
            g.invoke_text_edited("Three things Jesus said".into());
            g.invoke_size_changed(110);
            g.invoke_bold_changed(true);
            g.invoke_shadow_on(true);
            g.invoke_add_text();
            g.invoke_text_edited("1. Come and see".into());
            g.invoke_press(50.0, 50.0, 1.0, 1.5, false, false);
            g.invoke_drag(50.0, 80.0);
            g.invoke_release();
            g.invoke_build_on(true);
        })),
        ("nothing-chosen", Box::new(|op| {
            let g = window_of(op).global::<DesignDlg>();
            g.invoke_press(2.0, 2.0, 1.0, 1.5, false, false);
            g.invoke_release();
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
        let path = format!("{out}/v1-editor-{name}.png");
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
