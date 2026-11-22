//! Saves the N25 screens as PNGs: the theme list with the built-ins, the theme editor on a few layouts (one while
//! its box is dragged, with snap guides), several built-in themes, and Item edit's Layout picker.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example layouts_shots -- <folder>`.
use rustypresenter::{
    app::{App, LiveUpdate},
    model::*,
    store::Store,
    theme::LayoutKind,
    ui::{ThemesDlg, dialogs::{SettingsTab, item_edit, settings}, operator::{view::OperatorSnapshot, window::Operator}, test_support},
};
use slint::ComponentHandle;
use std::{rc::Rc, sync::Arc, time::Duration};
use uuid::Uuid;

type Step = Box<dyn Fn(&Rc<Operator>)>;

fn dlg(op: &Operator) -> ThemesDlg<'_> { test_support::window_of(op).global::<ThemesDlg>() }

fn pick(op: &Operator, k: LayoutKind) {
    dlg(op).set_layout_index(k.index() as i32);
    dlg(op).invoke_layout_picked();
}

/// Opens the built-in theme called `name` in the editor on layout `k`.
fn edit(op: &Operator, name: &str, k: LayoutKind) {
    let g = dlg(op);
    if g.get_editing() { g.invoke_cancel_edit() }
    let i = slint::Model::iter(&g.get_rows()).position(|r| r.name == name).unwrap_or(1);
    g.invoke_edit(i as i32);
    pick(op, k);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-layouts-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let item_id = Uuid::new_v4();
    {
        let mut d = app.data();
        d.settings.theme.font = "Noto Sans".into();
        d.services[0].items.push(ServiceItem { id: item_id, kind: ItemKind::Text { title: "Sermon".into(), body: "Pray every day\nRead the Bible\nServe your neighbour".into(), layout: LayoutKind::Points },
            background: None, notes: String::new(), theme: None });
    }
    let op = test_support::operator_for_tests(app.clone());
    op.window().set_size(slint::PhysicalSize::new(1366, 900));
    sync(&op, &app);
    let steps: Vec<(&str, Step)> = vec![
        ("list", Box::new(|op| settings::open(op, SettingsTab::Themes))),
        ("editor-points", Box::new(|op| edit(op, "Midnight", LayoutKind::Points))),
        ("editor-dragging", Box::new(|op| {
            pick(op, LayoutKind::MainPoint);
            dlg(op).invoke_drag_start(500.0, 500.0);
            dlg(op).invoke_drag_to(-40.0, -150.0);
        })),
        ("classic-quote", Box::new(|op| { dlg(op).invoke_drag_done(); edit(op, "Classic serif", LayoutKind::Quote) })),
        ("clean-white-title-subtitle", Box::new(|op| edit(op, "Clean white", LayoutKind::TitleSubtitle))),
        ("ocean-scripture", Box::new(|op| edit(op, "Ocean", LayoutKind::Scripture))),
        ("youth-title", Box::new(|op| edit(op, "Youth night", LayoutKind::Title))),
        ("sunrise-lower-third", Box::new(|op| edit(op, "Sunrise", LayoutKind::LowerThirdLyrics))),
        ("slate-main-point", Box::new(|op| edit(op, "Slate", LayoutKind::MainPoint))),
        ("item-edit", Box::new(move |op| { op.close_dialog(); item_edit::open(op, item_id) })),
    ];
    run(op, app, steps.into_iter().collect(), 0, out);
    slint::run_event_loop()?;
    Ok(())
}

fn sync(op: &Operator, app: &Arc<App>) {
    op.apply_snapshot(Arc::new(OperatorSnapshot::read(app)));
    op.apply_live(LiveUpdate::new(app.live_view()));
}

fn run(op: Rc<Operator>, app: Arc<App>, steps: Vec<(&'static str, Step)>, i: usize, out: String) {
    let Some((name, step)) = steps.get(i) else { let _ = slint::quit_event_loop(); return };
    step(&op);
    sync(&op, &app);
    let name = *name;
    slint::Timer::single_shot(Duration::from_millis(1200), move || {
        let path = format!("{out}/v1-themes-{name}.png");
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
