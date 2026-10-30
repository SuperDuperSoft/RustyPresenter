//! Check C2: list monitors through Slint's winit backend, go borderless fullscreen on the last
//! one, and report whether the window covers exactly that monitor. Prints PASS or FAIL, then exits.
use slint::winit_030::{WinitWindowAccessor, winit};
use std::time::Duration;

slint::slint! {
    export component Probe inherits Window {
        background: black;
        Text { text: "C2 monitor check"; color: white; font-size: 48px; }
    }
}

fn main() {
    let probe = Probe::new().unwrap();
    let weak = probe.as_weak();
    slint::Timer::single_shot(Duration::from_millis(500), move || {
        let p = weak.upgrade().unwrap();
        let target = p.window().with_winit_window(|w| {
            let mons: Vec<_> = w.available_monitors().collect();
            for (i, m) in mons.iter().enumerate() {
                println!("monitor {i}: name={:?} pos={:?} size={:?} scale={}", m.name(), m.position(), m.size(), m.scale_factor());
            }
            let m = mons.last().cloned()?;
            w.set_fullscreen(Some(winit::window::Fullscreen::Borderless(Some(m.clone()))));
            Some((m.position(), m.size()))
        }).flatten();
        let weak = p.as_weak();
        slint::Timer::single_shot(Duration::from_millis(1500), move || {
            let p = weak.upgrade().unwrap();
            let got = p.window().with_winit_window(|w| (w.outer_position().ok(), w.outer_size()));
            match (target, got) {
                (Some((pos, size)), Some((Some(wpos), wsize))) => {
                    let ok = wpos == pos && wsize == size;
                    println!("window pos={wpos:?} size={wsize:?}; target pos={pos:?} size={size:?}");
                    println!("{}", if ok { "PASS" } else { "FAIL" });
                }
                other => println!("FAIL: could not read monitors or window geometry: {other:?}"),
            }
            slint::quit_event_loop().unwrap();
        });
    });
    probe.run().unwrap();
}
