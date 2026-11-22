//! Saves the stage screen's chord sheet as a PNG: a SongSelect ChordPro song file imported as the SongSelect inbox
//! does, its first slide live on the Band layout. Run:
//! `SLINT_BACKEND=winit-software cargo run --example chord_shots -- <song file> <picture.png> [slide number] [key +/-]`.
use rustypresenter::{
    app::App,
    live::{Command, Envelope},
    model::*,
    stage::presets,
    store::Store,
    ui::stage::Stage,
};
use slint::ComponentHandle;
use std::time::Duration;
use uuid::Uuid;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let file = args.next().ok_or("the song file")?;
    let out = args.next().ok_or("where to save the picture")?;
    let slide: usize = args.next().map_or(Ok(0), |s| s.parse())?;
    let transpose: i8 = args.next().map_or(Ok(0), |s| s.parse())?;
    let bytes = std::fs::read(&file)?;
    let mut song = rustypresenter::songselect::inbox::read_song_file(&file, &bytes)?;
    song.transpose = transpose;
    let store = Store::open(std::env::temp_dir().join(format!("rp-chord-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let item = Uuid::new_v4();
    {
        let mut d = app.data();
        d.settings.stage.layout = presets::BAND;
        d.settings.credits = "none".into();
        let song_id = song.id;
        d.songs.push(song);
        d.services[0].items.push(ServiceItem { id: item, kind: ItemKind::Song { song_id, arrangement: None }, background: None, notes: String::new(), theme: None });
    }
    let r = app.command(Envelope { id: "show".into(), base: None, cmd: Command::Show { item_id: item, slide } });
    if !r.ok { return Err(format!("could not show the slide: {r:?}").into()) }
    let stage = Stage::open(app.clone(), None)?;
    stage.window().set_size(slint::PhysicalSize::new(1920, 1080));
    let s = stage.clone();
    slint::Timer::single_shot(Duration::from_millis(1500), move || {
        match s.window().take_snapshot() {
            Ok(px) => match image::RgbaImage::from_raw(px.width(), px.height(), px.as_bytes().to_vec()).map(|img| img.save(&out)) {
                Some(Ok(())) => println!("saved {out}"),
                other => eprintln!("could not save {out}: {other:?}"),
            },
            Err(e) => eprintln!("could not draw {out}: {e}"),
        }
        let _ = slint::quit_event_loop();
    });
    slint::run_event_loop()?;
    Ok(())
}
