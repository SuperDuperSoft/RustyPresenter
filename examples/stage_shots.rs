//! Saves the stage window as a PNG for every built-in stage layout, with a song live, notes and a message up.
//! Run: `SLINT_BACKEND=winit-software xvfb-run -a cargo run --example stage_shots -- <folder>`.
use rustypresenter::{app::App, live::{Command, Envelope}, model::*, stage::presets, store::Store, ui::stage::Stage};
use std::{cell::RefCell, rc::Rc, time::Duration};
use uuid::Uuid;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let store = Store::open(std::env::temp_dir().join(format!("rp-stage-shots-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    {
        let mut d = app.data.lock().unwrap();
        let song = Song {
            id: Uuid::new_v4(), title: "Amazing Grace".into(),
            lyrics: "Verse 1\nAmazing grace, how sweet the sound\nThat saved a wretch like me\n\nChorus\nMy chains are gone, I've been set free\n\nVerse 2\n'Twas grace that taught my heart to fear".into(),
            ..Default::default()
        };
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: None, notes: "Build on the last line".into(), theme: None };
        d.songs.push(song);
        d.services[0].items.push(item);
    }
    assert!(app.command(Envelope { id: "n1".into(), base: Some(0), cmd: Command::Next }).ok);
    let layouts: Vec<(Uuid, String)> = presets::builtins().iter().map(|l| (l.id, l.name.to_lowercase().replace(' ', "-"))).collect();
    let open: Rc<RefCell<Option<Rc<Stage>>>> = Rc::default();
    shot(app, layouts, 0, out, open);
    slint::run_event_loop()?;
    Ok(())
}

/// Open the stage with layout `i`, wait for it to draw, save it, then go on to the next one.
fn shot(app: std::sync::Arc<App>, layouts: Vec<(Uuid, String)>, i: usize, out: String, open: Rc<RefCell<Option<Rc<Stage>>>>) {
    let Some((id, name)) = layouts.get(i).cloned() else { let _ = slint::quit_event_loop(); return };
    app.data.lock().unwrap().settings.stage.layout = id;
    let stage = Stage::open(app.clone(), None).expect("stage window");
    stage.window().set_size(slint::PhysicalSize::new(1280, 720));
    // The new window opens before the old one closes, or the event loop would end with the last window.
    *open.borrow_mut() = Some(stage);
    slint::Timer::single_shot(Duration::from_millis(800), move || {
        let path = format!("{out}/stage-{name}.png");
        if let Some(s) = open.borrow().as_ref() {
            match s.window().take_snapshot() {
                Ok(px) => match image::RgbaImage::from_raw(px.width(), px.height(), px.as_bytes().to_vec()).map(|img| img.save(&path)) {
                    Some(Ok(())) => println!("saved {path}"),
                    other => eprintln!("could not save {path}: {other:?}"),
                },
                Err(e) => eprintln!("could not draw {path}: {e}"),
            }
        }
        shot(app, layouts, i + 1, out, open);
    });
}
