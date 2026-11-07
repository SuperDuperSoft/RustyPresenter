//! Saves the translation editor (Settings → Translations…) as PNGs: Spanish with a plural text chosen, a text with a
//! mistake, and the add-a-language picker.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example transedit_shots -- <folder>`.
use rustypresenter::{
    app::App,
    store::Store,
    ui::{TranslationEditorDlg, operator::window::Operator, ops::Ops},
};
use slint::{ComponentHandle, Model};
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

fn ui(op: &Operator) -> &rustypresenter::ui::OperatorWindow { rustypresenter::ui::test_support::window_of(op) }

fn out(name: &str) -> String { format!("{}/{name}", std::env::args().nth(1).unwrap_or_else(|| ".".into())) }

fn pick(op: &Operator, english: &str) {
    let g = ui(op).global::<TranslationEditorDlg>();
    g.set_search(english.into());
    g.invoke_search_changed(english.into());
    if let Some(i) = g.get_rows().iter().position(|r| r.english == english) { g.invoke_select(i as i32) }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open(std::env::temp_dir().join(format!("rp-transedit-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let rt = tokio::runtime::Runtime::new()?;
    rustypresenter::i18n::set_language("es")?;
    let op = Operator::open(app.clone(), Ops::new(app.clone(), rt.handle().clone()), Rc::new(NoScreens))?;
    op.window().set_size(slint::PhysicalSize::new(1440, 900));
    type Step = (u64, Box<dyn Fn(&Rc<Operator>)>);
    let steps: Vec<Step> = vec![
        (1200, Box::new(|op: &Rc<Operator>| {
            rustypresenter::ui::dialogs::translation_editor::open(op, None);
            let g = ui(op).global::<TranslationEditorDlg>();
            g.set_search("".into());
            g.invoke_search_changed("".into());
            g.invoke_select(3);
        })),
        (700, Box::new(|op: &Rc<Operator>| save(op.window(), &out("transedit-editor.png")))),
        (100, Box::new(|op: &Rc<Operator>| pick(op, "{n} song"))),
        (600, Box::new(|op: &Rc<Operator>| save(op.window(), &out("transedit-plural.png")))),
        (100, Box::new(|op: &Rc<Operator>| {
            pick(op, "Delete “{name}”?");
            ui(op).global::<TranslationEditorDlg>().invoke_form_edited(0, "¿Borrar?".into());
        })),
        (600, Box::new(|op: &Rc<Operator>| save(op.window(), &out("transedit-problem.png")))),
        (100, Box::new(|op: &Rc<Operator>| ui(op).global::<TranslationEditorDlg>().invoke_add_language())),
        (600, Box::new(|op: &Rc<Operator>| {
            let g = ui(op).global::<TranslationEditorDlg>();
            g.invoke_add_pick(2);
            save(op.window(), &out("transedit-add.png"));
            g.set_add_custom(true);
        })),
        (600, Box::new(|op: &Rc<Operator>| save(op.window(), &out("transedit-add-custom.png")))),
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
