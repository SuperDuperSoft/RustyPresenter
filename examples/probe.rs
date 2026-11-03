//! Check C6 probe: `cargo run --release --example probe -- <file or av://lavfi:...>`.
//! Prints what a headless mpv finds (with each track) and saves the thumbnail as `probe-thumb.png`.
use rustypresenter::import::probe::probe_blocking;
use rustypresenter::video::ffi::{Mpv, Node};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn main() {
    let path = std::env::args_os().nth(1).expect("usage: probe <file>");
    let mpv = Mpv::load().expect("libmpv");
    println!("libmpv {} (client API {:?})", mpv.path().display(), mpv.api_version());
    let probe = probe_blocking(&mpv, path.as_ref(), Duration::from_secs(20), &CancellationToken::new()).expect("probe");
    println!("kind {:?}, duration {:?}, size {:?}", probe.kind, probe.duration, probe.size);
    if let Node::Array(tracks) = &probe.tracks {
        for track in tracks {
            let field = |key| track.get(key).map_or_else(String::new, |v| format!("{key}={}", show(v)));
            let fields = ["id", "type", "codec", "demux-w", "demux-h", "demux-rotation", "albumart", "default"].map(field);
            println!("track {}", fields.iter().filter(|f| !f.is_empty()).cloned().collect::<Vec<_>>().join(" "));
        }
    }
    if let Some(thumb) = probe.thumb {
        let first = *thumb.get_pixel(0, 0);
        println!("thumbnail {}×{}, uniform: {}", thumb.width(), thumb.height(), thumb.pixels().all(|p| *p == first));
        thumb.save("probe-thumb.png").expect("save probe-thumb.png");
    }
}

fn show(v: &Node) -> String {
    match v {
        Node::Str(s) => s.clone(),
        Node::Flag(b) => b.to_string(),
        Node::Int(n) => n.to_string(),
        Node::Double(d) => d.to_string(),
        other => format!("{other:?}"),
    }
}
