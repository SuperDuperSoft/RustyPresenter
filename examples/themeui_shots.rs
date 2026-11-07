//! Saves the FEAT-1 screens as PNGs: Settings → Themes, the theme editor, the overlays strip, the Overlays dialog
//! and its editor, and Item edit's Theme box.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example themeui_shots -- <folder>`.
use rustypresenter::{
    app::{App, LiveUpdate},
    live::{Command, Envelope},
    model::*,
    store::Store,
    theme::{SavedTheme, overlay::{ImageSource, Overlay, OverlayKind, OverlayLook, OverlaySource, Placement}},
    ui::{OverlaysDlg, ThemesDlg, dialogs::{SettingsTab, item_edit, overlays, settings}, operator::{view::OperatorSnapshot, window::Operator}, test_support},
};
use slint::ComponentHandle;
use std::{rc::Rc, sync::Arc, time::Duration};
use uuid::Uuid;

type Step = Box<dyn Fn(&Rc<Operator>)>;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-themeui-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let item_id = Uuid::new_v4();
    {
        let mut d = app.data.lock().unwrap();
        let christmas = SavedTheme { id: Uuid::new_v4(), name: "Christmas".into(),
            style: Theme { font: "Noto Sans".into(), weight: 700, color: "#f5e6c8".into(), band: true, position: "lower".into(), ..Default::default() },
            background: Some(Background::Gradient { from: "#3a2412".into(), to: "#0d0806".into(), angle: 135, dim: 0 }) };
        let youth = SavedTheme { id: Uuid::new_v4(), name: "Youth night".into(), style: Theme { uppercase: true, outline: 6, ..Default::default() }, background: None };
        d.default_theme = Some(christmas.id);
        d.themes = vec![christmas, youth];
        let now = chrono::Utc::now();
        d.backgrounds = vec![
            BackgroundEntry::new("Sunrise", Background::Gradient { from: "#ff8a3d".into(), to: "#2b1530".into(), angle: 180, dim: 0 }, MediaStatus::Ready, now),
            BackgroundEntry::new("Deep blue", Background::Color { color: "#0b1a33".into() }, MediaStatus::Ready, now),
        ];
        let sign = |name: &str, text: &str| Overlay { id: Uuid::new_v4(), name: name.into(),
            source: OverlaySource::Text { text: text.into(), styles: vec![], look: OverlayLook { box_kind: rustypresenter::theme::overlay::BoxKind::Bar, ..Default::default() } },
            place: Placement::default_for(OverlayKind::Text) };
        d.overlays = vec![
            sign("Welcome", "Welcome! Coffee is in the hall"),
            sign("Wi-Fi", "Wi-Fi: GraceGuest"),
            Overlay { id: Uuid::new_v4(), name: "Clock".into(), source: OverlaySource::Clock { look: OverlayLook::default(), seconds: false, h24: false }, place: Placement::default_for(OverlayKind::Clock) },
            Overlay { id: Uuid::new_v4(), name: "Logo".into(), source: OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 100 }, place: Placement::default_for(OverlayKind::Picture) },
        ];
        let song = Song { id: Uuid::new_v4(), title: "Amazing Grace".into(), lyrics: "Verse 1\nAmazing grace, how sweet the sound\nThat saved a wretch like me\n\nChorus\nMy chains are gone".into(), ..Default::default() };
        d.services[0].items.push(ServiceItem { id: item_id, kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: None, notes: String::new(), theme: None });
        d.services[0].items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Pancake breakfast".into(), body: "Sunday 9am".into() }, background: None, notes: String::new(), theme: None });
        d.songs.push(song);
    }
    let first_overlay = app.data.lock().unwrap().overlays[1].id;
    app.command(Envelope { id: "s".into(), base: None, cmd: Command::Show { item_id, slide: 0 } });
    app.command(Envelope { id: "o".into(), base: None, cmd: Command::Overlay { id: first_overlay, on: true, seen_clear: Some(0) } });

    let op = test_support::operator_for_tests(app.clone());
    op.window().set_size(slint::PhysicalSize::new(1366, 768));
    sync(&op, &app);
    let steps: Vec<(&str, Step)> = vec![
        ("operator", Box::new(|_| {})),
        ("themes-list", Box::new(|op| settings::open(op, SettingsTab::Themes))),
        ("themes-library-add", Box::new(|op| {
            let g = test_support::window_of(op).global::<ThemesDlg>();
            g.set_adding(true);
            g.set_add_kind(1);
            g.invoke_add_changed();
        })),
        ("theme-editor", Box::new(|op| test_support::window_of(op).global::<ThemesDlg>().invoke_edit(2))),
        ("overlays", Box::new(|op| { op.close_dialog(); overlays::open(op) })),
        ("overlay-editor", Box::new(|op| test_support::window_of(op).global::<OverlaysDlg>().invoke_edit(0))),
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
    slint::Timer::single_shot(Duration::from_millis(900), move || {
        let path = format!("{out}/themeui-{name}.png");
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
