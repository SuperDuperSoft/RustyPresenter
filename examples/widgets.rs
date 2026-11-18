//! Every shared operator widget (`ui/widgets.slint`) in each state, for a person to compare beside the web
//! operator: colours, radii and font weights should match `web/operator.css`. A development aid only; the app
//! never opens this window.
//!
//! Args: `--snapshot <file.png>` draws the window once, saves it as a PNG and exits (no person needed).
use rustypresenter::ui::WidgetGallery;
use slint::ComponentHandle;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let snapshot = args.iter().position(|a| a == "--snapshot").and_then(|i| args.get(i + 1)).cloned();
    let w = WidgetGallery::new()?;
    w.show()?;
    if let Some(path) = snapshot {
        let weak = w.as_weak();
        slint::Timer::single_shot(Duration::from_millis(500), move || {
            let Some(w) = weak.upgrade() else { return };
            match w.window().take_snapshot() {
                Ok(px) => {
                    let saved = image::RgbaImage::from_raw(px.width(), px.height(), px.as_bytes().to_vec())
                        .map(|img| img.save(&path));
                    match saved {
                        Some(Ok(())) => println!("saved {path}"),
                        Some(Err(e)) => eprintln!("could not save {path}: {e}"),
                        None => eprintln!("snapshot has the wrong size"),
                    }
                }
                Err(e) => eprintln!("snapshot failed: {e}"),
            }
            let _ = slint::quit_event_loop();
        });
    }
    slint::run_event_loop()?;
    Ok(())
}
