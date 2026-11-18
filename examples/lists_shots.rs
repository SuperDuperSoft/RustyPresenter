//! Saves the N8, N14 and N37 screens as PNGs: the service list filtered, the slides in Easy and Table views and the grid
//! with big pictures, Quick search, and the Services dialog filtered.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example lists_shots -- <folder>`.
use i_slint_backend_testing::ElementHandle;
use rustypresenter::{
    app::{App, LiveUpdate},
    model::*,
    store::Store,
    ui::{operator::{view::OperatorSnapshot, window::Operator}, test_support},
};
use std::{rc::Rc, sync::Arc, time::Duration};
use uuid::Uuid;

type Step = Box<dyn Fn(&Rc<Operator>)>;

const SONGS: [(&str, &str); 6] = [
    ("Amazing Grace", "Verse 1\nAmazing grace how sweet the sound\nThat saved a wretch like me\n\nVerse 2\n'Twas grace that taught my heart to fear\nAnd grace my fears relieved\n\nChorus\nMy chains are gone\nI've been set free\n\nVerse 3\nThe Lord has promised good to me\nHis word my hope secures"),
    ("Be Thou My Vision", "Verse 1\nBe Thou my vision O Lord of my heart\nNaught be all else to me save that Thou art"),
    ("Grace Alone", "Verse 1\nEvery promise we can make\nEvery prayer and step of faith\n\nChorus\nGrace alone"),
    ("Holy Holy Holy", "Verse 1\nHoly holy holy Lord God Almighty\nEarly in the morning our song shall rise to Thee"),
    ("How Great Thou Art", "Verse 1\nO Lord my God when I in awesome wonder\nConsider all the worlds Thy hands have made"),
    ("Great Is Thy Faithfulness", "Verse 1\nGreat is Thy faithfulness O God my Father\nThere is no shadow of turning with Thee"),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-lists-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let mut first = None;
    {
        let mut d = app.data.lock().unwrap();
        for (title, lyrics) in SONGS {
            let song = Song { id: Uuid::new_v4(), title: title.into(), lyrics: lyrics.into(), writers: "John Newton".into(), ..Default::default() };
            if title != "Holy Holy Holy" {
                let id = Uuid::new_v4();
                first.get_or_insert(id);
                d.services[0].items.push(ServiceItem { id, kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: None, notes: String::new(), theme: None });
            }
            d.songs.push(song);
        }
        d.services[0].items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome & announcements".into(), body: "Welcome!".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None });
        for w in 1..=10 {
            let mut s = Service::new(&format!("Sunday week {w}"));
            s.date = chrono::NaiveDate::from_ymd_opt(2026, 9, 6).map(|d| d + chrono::Duration::days(7 * i64::from(w)));
            d.services.push(s);
        }
        d.services.push(Service::new("Easter sunrise"));
    }
    let first = first.unwrap();
    let op = test_support::operator_for_tests(app.clone());
    op.window().set_size(slint::PhysicalSize::new(1366, 820));
    sync(&op, &app);
    let steps: Vec<(&str, u64, Step)> = vec![
        ("service-filter", 1200, Box::new(|op| set(op, "ServiceColumn::service-filter", "grace"))),
        ("slides-easy", 1200, Box::new(move |op| {
            set(op, "ServiceColumn::service-filter", "");
            test_support::window_of(op).invoke_service_select(first.to_string().into());
            test_support::window_of(op).invoke_slides_view_picked(1);
        })),
        ("slides-table", 1500, Box::new(|op| test_support::window_of(op).invoke_slides_view_picked(2))),
        ("slides-grid-big", 1500, Box::new(|op| {
            let w = test_support::window_of(op);
            w.invoke_slides_view_picked(0);
            w.set_slides_thumb_size(400);
            w.invoke_slides_thumb_size_changed(400);
        })),
        ("quick-search", 1200, Box::new(|op| {
            test_support::window_of(op).invoke_menu("find".into());
            set(op, "QuickSearchDialog::search", "grace");
        })),
        ("services-filter", 1200, Box::new(|op| {
            op.close_dialog();
            test_support::window_of(op).invoke_services();
            set(op, "ServicesDialog::services-filter", "week 1");
        })),
        ("services-filter-cleared", 1200, Box::new(|op| set(op, "ServicesDialog::services-filter", ""))),
    ];
    run(op, app, steps, 0, out);
    slint::run_event_loop()?;
    Ok(())
}

fn set(op: &Operator, id: &str, text: &str) {
    match ElementHandle::find_by_element_id(test_support::window_of(op), id).next() {
        Some(e) => e.set_accessible_value(text),
        None => eprintln!("{id} not found"),
    }
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
        let path = format!("{out}/pp-lists-{name}.png");
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
