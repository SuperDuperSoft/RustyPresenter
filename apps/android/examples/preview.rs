//! Pictures of the phone's screens, drawn on a computer with Slint's software renderer (the same one the phone uses),
//! filled with a made-up Sunday: a way to check the screens without a phone.
//!
//!   cargo run --example preview --features preview [-- <language>]
//!
//! The PNGs go to `target/preview/` (or `CARGO_TARGET_DIR/preview`).
use remote::{fit, protocol, screens::*};
use slint::{
    ComponentHandle, ModelRc, VecModel,
    platform::{Platform, WindowAdapter, software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType}},
};
use std::{collections::HashMap, rc::Rc};

struct Preview(Rc<MinimalSoftwareWindow>);

impl Platform for Preview {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> { Ok(self.0.clone()) }
}

/// A stage layout as the computer sends it: the song's chords, the next slide, a clock and a message.
const STAGE: &str = r##"{"type":"stage","background":"#000000","banner":null,"follow":"Auto: following — Chorus","version":3,"cells":[
 {"x":3,"y":4,"w":94,"h":58,"color":"#ffffff","fill":null,"max":9,"min":3,"bold":true,"align":"left","valign":"top","kind":"chords","caption":"Now","chord_color":"#f5b94a",
  "text":"[G]Amazing [C]grace","lines":[[["G","Amazing "],["C","grace, how "],["G","sweet the sound"]],[["","That "],["D","saved a wretch like "],["G","me"]],[["G","I once was "],["C","lost, but "],["G","now am found"]],[["Em","Was blind but "],["D","now I "],["G","see"]]]},
 {"x":3,"y":66,"w":60,"h":30,"color":"#f2d08f","fill":"#15181e","max":6,"min":2,"bold":false,"align":"left","valign":"top","kind":"text","caption":"Up next","text":"’Twas grace that taught my heart to fear, and grace my fears relieved"},
 {"x":66,"y":66,"w":31,"h":14,"color":"#ffffff","fill":null,"max":10,"min":3,"bold":true,"align":"center","valign":"middle","kind":"text","caption":null,"text":"10:42"},
 {"x":66,"y":82,"w":31,"h":14,"color":"#1a1405","fill":"#f5b94a","max":5,"min":2,"bold":true,"align":"center","valign":"middle","kind":"message","caption":null,"text":"Slow down a little"}]}"##;

fn main() {
    let lang = std::env::args().nth(1);
    let (w, h, scale) = (412u32, 892u32, 2.0f32);
    let (pw, ph) = ((w as f32 * scale) as u32, (h as f32 * scale) as u32);
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Preview(window.clone()))).unwrap();
    window.dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: scale });
    window.set_size(slint::PhysicalSize::new(pw, ph));
    let ui = AppWindow::new().unwrap();
    // After the window is made: its words are bundled with it.
    if let Some(l) = &lang { slint::select_bundled_translation(l).expect("a language in translations/") }
    ui.show().unwrap();
    let st = ui.global::<S>();

    // A made-up Sunday.
    st.set_found(ModelRc::new(VecModel::from(vec![Found { name: s("SANCTUARY-PC"), address: s("192.168.1.20") }])));
    st.set_find_note(ui.global::<Say>().invoke_none_found());
    st.set_typed(s("192.168.1.20"));
    st.set_computer(s("SANCTUARY-PC · 192.168.1.20"));
    st.set_needs_password(true);
    st.set_connected(true);
    st.set_service(s("Sunday morning"));
    st.set_now_label(s("Chorus"));
    st.set_now_text(s("My chains are gone, I've been set free\nMy God, my Saviour has ransomed me"));
    st.set_notes(s("Band: build into the bridge."));
    st.set_sections(ModelRc::new(VecModel::from(vec![
        SectionChip { short: s("V1"), label: s("Verse 1"), color: color("#6aa8ff", slint::Color::default()), live: false },
        SectionChip { short: s("C"), label: s("Chorus"), color: color("#ff8f93", slint::Color::default()), live: true },
        SectionChip { short: s("V2"), label: s("Verse 2"), color: color("#6aa8ff", slint::Color::default()), live: false },
        SectionChip { short: s("B"), label: s("Bridge"), color: color("#3ccf91", slint::Color::default()), live: false },
    ])));
    st.set_next_label(s("Amazing Grace · Verse 2"));
    st.set_next_text(s("’Twas grace that taught my heart to fear"));
    st.set_items(ModelRc::new(VecModel::from(vec![
        ItemRow { title: s("Welcome"), count: s("1 slide"), live: false },
        ItemRow { title: s("Amazing Grace (My Chains Are Gone)"), count: s("6 slides"), live: true },
        ItemRow { title: s("John 3:16–17"), count: s("2 slides"), live: false },
        ItemRow { title: s("Announcements"), count: s("4 slides"), live: false },
    ])));
    st.set_light(s("on_air"));
    st.set_light_text(s("Camera 2: on air"));
    st.set_tally_camera(2);
    st.set_mic_on(true);
    st.set_mic_level(0.45);
    st.set_mic_note(s("Sending to the stream. Keep this page open and the phone awake."));
    st.set_cam_name(s("Balcony"));
    st.set_scan_note(ui.global::<Say>().invoke_point_at_qr());

    let stage = protocol::stage_view(STAGE).unwrap();
    let out = std::path::PathBuf::from(std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into())).join("preview");
    std::fs::create_dir_all(&out).unwrap();
    let shots: &[(&str, &str, i32, bool)] = &[
        ("1-find", "find", 0, false), ("2-pair", "pair", 0, false), ("3-remote", "main", 0, false), ("4-stage", "main", 1, true),
        ("5-stage-full", "main", 1, false), ("6-tally", "main", 2, false), ("7-microphone", "main", 3, false), ("8-camera", "main", 4, false),
        ("9-scan", "scan", 0, false),
    ];
    for (name, page, tab, bar) in shots {
        st.set_page(s(*page));
        st.set_tab(*tab);
        st.set_stage_bar(*bar);
        if *tab == 1 {
            // The stage tab's own size: the whole window without the tabs when they show.
            let sh = if *bar { h as f32 - 60.0 } else { h as f32 };
            st.set_cells(ModelRc::new(VecModel::from(stage_cells(fit::place(&stage.cells, w as f32, sh), &HashMap::new()))));
            st.set_follow(s(stage.follow.clone().unwrap_or_default()));
        }
        let mut buf = vec![PremultipliedRgbaColor::default(); (pw * ph) as usize];
        // Twice: the first draw lays out, the second shows what the layout settled on.
        for _ in 0..2 {
            slint::platform::update_timers_and_animations();
            window.request_redraw();
            window.draw_if_needed(|r| { r.render(&mut buf, pw as usize); });
        }
        let px: Vec<u8> = buf.iter().flat_map(|p| [p.red, p.green, p.blue, 255]).collect();
        let file = out.join(format!("{name}{}.png", lang.as_deref().map(|l| format!("-{l}")).unwrap_or_default()));
        image::save_buffer(&file, &px, pw, ph, image::ExtendedColorType::Rgba8).unwrap();
        println!("{}", file.display());
    }
}
