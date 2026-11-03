//! Check C6 probe: `cargo run --release --example probe -- <file or av://lavfi:...>`.
//! Prints what a headless mpv finds and saves the thumbnail as `probe-thumb.png`.
use rustypresenter::import::probe::probe_blocking;
use rustypresenter::video::ffi::Mpv;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn main() {
    let path = std::env::args_os().nth(1).expect("usage: probe <file>");
    let mpv = Mpv::load().expect("libmpv");
    println!("libmpv {} (client API {:?})", mpv.path().display(), mpv.api_version());
    let probe = probe_blocking(&mpv, path.as_ref(), Duration::from_secs(20), &CancellationToken::new()).expect("probe");
    println!("kind {:?}, duration {:?}, size {:?}", probe.kind, probe.duration, probe.size);
    if let Some(thumb) = probe.thumb {
        let first = *thumb.get_pixel(0, 0);
        println!("thumbnail {}×{}, uniform: {}", thumb.width(), thumb.height(), thumb.pixels().all(|p| *p == first));
        thumb.save("probe-thumb.png").expect("save probe-thumb.png");
    }
}
