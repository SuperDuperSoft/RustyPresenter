//! Saves Settings → Video (with the Graphics picker) as a PNG.
//! Run: `xvfb-run -a cargo run --example gpu_shots -- <file.png>`.
use rustypresenter::{app::App, model::Graphics, store::Store, ui::{dialogs::{SettingsTab, settings}, operator::view::OperatorSnapshot, test_support}, video};
use std::{sync::Arc, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "gpu-settings.png".into());
    let chosen = video::choose_backend(Graphics::Auto);
    println!("chosen: {chosen:?}");
    let store = Store::open(std::env::temp_dir().join(format!("rp-gpu-shots-{}", uuid::Uuid::new_v4())))?;
    let data = store.load()?;
    let app = App::new(store, data, 0, None);
    let op = test_support::operator_for_tests(app.clone());
    op.window().set_size(slint::PhysicalSize::new(1366, 900));
    op.apply_snapshot(Arc::new(OperatorSnapshot::read(&app)));
    settings::open(&op, SettingsTab::Video);
    slint::Timer::single_shot(Duration::from_millis(1200), move || {
        match op.window().take_snapshot() {
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
