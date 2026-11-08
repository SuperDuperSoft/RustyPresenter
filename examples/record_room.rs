//! N22 check, a development aid: opens a real room window, records it for a few seconds while the slide changes,
//! and prints what `ffprobe` finds in the MP4, plus the colour in the middle of the first and the last frame.
//! Run: `xvfb-run -a cargo run --example record_room -- <folder>` (OpenGL through Mesa), or with
//! `RP_FORCE_SOFTWARE=1` for software drawing.
use rustypresenter::{
    app::App,
    live::{Command, Envelope},
    model::*,
    record::{Plan, Preset, Recording, Sound, run},
    render::Renderer,
    store::Store,
    ui::room::Room,
    video,
};
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Duration};
use uuid::Uuid;

thread_local! { static KEEP: RefCell<Option<(Rc<Room>, Recording)>> = const { RefCell::new(None) }; }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let folder = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&folder)?;
    let mode = video::choose_backend(Graphics::Auto).draw;
    println!("drawing: {mode:?}");
    let store = Store::open(std::env::temp_dir().join(format!("rp-record-room-{}", Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let (red, white) = (Uuid::new_v4(), Uuid::new_v4());
    {
        let mut d = app.data.lock().unwrap();
        for (id, color) in [(red, "#cc0000"), (white, "#ffffff")] {
            d.services[0].items.push(ServiceItem { id, kind: ItemKind::Text { title: "Recording test".into(), body: "Recording test".into() }, background: Some(Background::Color { color: color.into() }), notes: String::new(), theme: None });
        }
    }
    let ffmpeg = run::find_ffmpeg(&app.store.root).expect("ffmpeg");
    let out = folder.join("record-room.mp4");
    let _ = std::fs::remove_file(&out);
    let plan = Plan { ffmpeg, encoder: "libx264", preset: Preset::P720, sound: Sound::None, out: out.clone() };
    let app2 = app.clone();
    slint::Timer::single_shot(Duration::from_millis(200), move || {
        let room = Room::open(app2.clone(), Renderer::new(), None, None).expect("room window");
        room.window().set_size(slint::PhysicalSize::new(1024, 768));
        let rec = Recording::start(plan, |e| println!("ended: {e:?}")).expect("recording starts");
        room.record(Some(rec.sink().clone()));
        KEEP.with(|k| *k.borrow_mut() = Some((room, rec)));
    });
    let show = |app: &std::sync::Arc<App>, id| { let o = app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::Show { item_id: id, slide: 0 } }); assert!(o.ok, "{o:?}") };
    let a = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        show(&a, red);
        // The first slide takes a while in a debug build (fonts load).
        std::thread::sleep(Duration::from_millis(4000));
        show(&a, white);
        std::thread::sleep(Duration::from_millis(1500));
        let _ = slint::invoke_from_event_loop(|| {
            let kept = KEEP.with(|k| k.borrow_mut().take());
            if let Some((room, rec)) = kept {
                room.record(None);
                rec.stop();
                rec.wait(Duration::from_secs(60));
                drop(room);
            }
            let _ = slint::quit_event_loop();
        });
    });
    slint::run_event_loop_until_quit()?;
    let probe = std::process::Command::new("ffprobe").args(["-v", "error", "-show_entries", "stream=codec_name,width,height,nb_frames:format=duration", "-of", "compact"]).arg(&out).output()?;
    println!("{}", String::from_utf8_lossy(&probe.stdout));
    for (name, at) in [("first", "3.5"), ("last", "-0.1")] {
        let seek: Vec<&str> = if at.starts_with('-') { vec!["-sseof", "-0.2"] } else { vec!["-ss", at] };
        let px = std::process::Command::new("ffmpeg").args(["-v", "error"]).args(&seek).arg("-i").arg(&out)
            .args(["-frames:v", "1", "-vf", "crop=2:2:(iw-2)/2:20", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]).output()?;
        println!("{name} frame, near the top middle: {:?}", px.stdout.get(..3));
        let png = folder.join(format!("record-room-{name}.png"));
        let _ = std::process::Command::new("ffmpeg").args(["-v", "error", "-y"]).args(&seek).arg("-i").arg(&out).args(["-frames:v", "1"]).arg(&png).status();
    }
    Ok(())
}
