//! Saves N19/N34/N36 as PNGs: slide labels on thumbnails, Settings → Songs (credits) and Sections and labels, and
//! the song editor with its new buttons and fields.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example songs_shots -- <folder>`.
use rustypresenter::{
    app::{App, LiveUpdate},
    model::*,
    store::Store,
    ui::{SectionsDlg, dialogs::{SettingsTab, settings, song_editor}, operator::{view::OperatorSnapshot, window::Operator}, test_support},
};
use slint::ComponentHandle;
use std::{rc::Rc, sync::Arc, time::Duration};
use uuid::Uuid;

type Step = Box<dyn Fn(&Rc<Operator>)>;

const LYRICS: &str = "Verse 1\nAmazing grace how sweet the sound\nThat saved a wretch like me\n\n{Instrumental}\n\nChorus\nMy chains are gone\nI've been set free\n\nBridge\n{Spoken Word}\nThe earth shall soon dissolve like snow\n\n{Repeat}\nThe sun forbear to shine";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-songs-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let (item_id, song_id) = (Uuid::new_v4(), Uuid::new_v4());
    {
        let mut d = app.data.lock().unwrap();
        d.settings.church_ccli = "1234567".into();
        d.songs.push(Song { id: song_id, title: "Amazing Grace (My Chains Are Gone)".into(), writers: "John Newton, Chris Tomlin".into(), artist: "Chris Tomlin".into(),
            publisher: "sixsteps Music".into(), year: "2006".into(), ccli: "4768151".into(), lyrics: LYRICS.into(), ..Default::default() });
        d.services[0].items.push(ServiceItem { id: item_id, kind: ItemKind::Song { song_id, arrangement: None }, background: None, notes: String::new(), theme: None });
    }
    let op = test_support::operator_for_tests(app.clone());
    op.window().set_size(slint::PhysicalSize::new(1366, 820));
    sync(&op, &app);
    let steps: Vec<(&str, u64, Step)> = vec![
        ("thumbnails", 2500, Box::new(move |op| test_support::window_of(op).invoke_service_select(item_id.to_string().into()))),
        ("settings-songs", 1200, Box::new(|op| settings::open(op, SettingsTab::Songs))),
        ("settings-sections", 1200, Box::new(|op| { op.close_dialog(); settings::open(op, SettingsTab::Sections) })),
        ("settings-sections-picker", 1200, Box::new(|op| test_support::window_of(op).global::<SectionsDlg>().set_picking(1))),
        ("song-editor", 2000, Box::new(move |op| { op.close_dialog(); song_editor::open_song_editor(op, Some(song_id), false) })),
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
        let path = format!("{out}/pp-songs-{name}.png");
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
