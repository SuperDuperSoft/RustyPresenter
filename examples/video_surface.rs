//! Check C1 rebuilt on `video::surface`: `testsrc2` drawn under a transparent
//! Slint window with a white "Amazing grace" on top.
//!
//! Args: `--sw` (same as `RP_FORCE_SOFTWARE=1`), `--measure` (after the video
//! plays, warm up 2 s, measure `--secs` seconds (default 10), print a summary
//! and exit 0 on PASS / 1 on FAIL), `--src <url>`. libmpv comes from the usual
//! lookup (`RP_LIBMPV` first). `--switch` instead checks switching players: a
//! foreground video added undrawn must report `Playing`, is drawn for 2 s, then
//! removed, and the background video must move again (exit 0 on PASS / 1 on FAIL).
//!
//! Measurement (as the C1 spike): every drawn frame three rows are read back
//! (GL: `glReadPixels` after Slint drew; software: the frame image) and a frame
//! counts as distinct only if some byte moved by more than 4/255 (dither noise
//! is ±1). Once a second the whole window is read back and the text box checked:
//! the same pixels must stay white while the video under them changes.
use glow::HasContext;
use rustypresenter::model::EndAction;
use rustypresenter::video::{DrawMode, Options, Player, Role, VideoSurface, choose_backend, ffi::Mpv};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

slint::slint! {
    export component Stage inherits Window {
        in property <image> video;
        out property <length> text-x: label.x;
        out property <length> text-y: label.y;
        out property <length> text-w: label.width;
        out property <length> text-h: label.height;
        title: "video surface";
        width: 1280px;
        height: 720px;
        background: transparent;
        Image { x: 0px; y: 0px; width: root.width; height: root.height; source: root.video; image-fit: fill; }
        label := Text { x: 60px; y: 60px; text: "Amazing grace"; color: white; font-size: 110px; font-weight: 800; }
    }
}

/// One full-window readback, reduced to the text box and a video-only corner.
struct Sample {
    white: Vec<bool>,
    text: Vec<[u8; 3]>,
    video: Vec<[u8; 3]>,
}

#[derive(Default)]
struct Stats {
    frames: u64,
    distinct: u64,
    noise: u64,
    prev: Vec<u8>,
    samples: Vec<Sample>,
    next_sample: Option<Instant>,
}

impl Stats {
    /// Counts a drawn frame from three of its rows.
    fn frame(&mut self, rows: Vec<u8>) {
        self.frames += 1;
        let md = rows.iter().zip(&self.prev).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(255);
        if md > 4 {
            self.distinct += 1;
        } else if md > 0 {
            self.noise += 1;
        }
        self.prev = rows;
    }

    fn sample_due(&mut self) -> bool {
        match self.next_sample {
            Some(t) if Instant::now() >= t => {
                self.next_sample = Some(t + Duration::from_secs(1));
                true
            }
            _ => false,
        }
    }
}

/// Analyses a top-down RGBA image of the whole window.
fn analyse(stage: &Stage, rgba: &[u8], w: u32, h: u32) -> Sample {
    let s = stage.window().scale_factor();
    let (tx, ty, tw, th) = ((stage.get_text_x() * s) as u32, (stage.get_text_y() * s) as u32, (stage.get_text_w() * s) as u32, (stage.get_text_h() * s) as u32);
    let px = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2]]
    };
    let mut smp = Sample { white: vec![], text: vec![], video: vec![] };
    for y in ty..(ty + th).min(h) {
        for x in tx..(tx + tw).min(w) {
            let p = px(x, y);
            smp.white.push(p.iter().all(|c| *c >= 235));
            smp.text.push(p);
        }
    }
    for y in h - 240..h - 40 {
        for x in w - 340..w - 40 {
            smp.video.push(px(x, y));
        }
    }
    smp
}

fn diff(a: [u8; 3], b: [u8; 3]) -> u32 {
    a.iter().zip(b).map(|(x, y)| x.abs_diff(y) as u32).sum()
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--sw") {
        unsafe { std::env::set_var("RP_FORCE_SOFTWARE", "1") }; // still single-threaded here
    }
    let measure = args.iter().any(|a| a == "--measure");
    let switch = args.iter().any(|a| a == "--switch");
    let secs: f64 = arg(&args, "--secs").and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let src = arg(&args, "--src").unwrap_or_else(|| "av://lavfi:testsrc2=size=1920x1080:rate=30".into());
    let mpv = match Mpv::load() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("FAIL: {e}");
            std::process::exit(2);
        }
    };
    let mode = choose_backend(rustypresenter::model::Graphics::Auto).draw;
    println!("mode={mode:?} src={src} libmpv={}", mpv.path().display());

    let stage = Stage::new().unwrap();
    let stats: Rc<RefCell<Stats>> = Rc::default();
    let weak = stage.as_weak();
    let st = stats.clone();
    let surface = VideoSurface::attach(&stage, mode, true, move |img| {
        let Some(stage) = weak.upgrade() else { return };
        if let Some(pb) = img.as_ref().and_then(|i| i.to_rgb8()) {
            let (w, h) = (pb.width() as usize, pb.height() as usize);
            let mut rows = Vec::with_capacity(w * 9);
            for y in [h / 2, h * 7 / 10, h * 9 / 10] {
                rows.extend_from_slice(&pb.as_bytes()[y * w * 3..(y + 1) * w * 3]);
            }
            st.borrow_mut().frame(rows);
        }
        stage.set_video(img.unwrap_or_default());
    });
    let mode = surface.mode();
    println!("drawing: {mode:?}");
    if mode == DrawMode::OpenGl {
        let (weak, st) = (stage.as_weak(), stats.clone());
        surface.set_gl_probe(move |gl, size| {
            let (w, h) = (size.width, size.height);
            let mut rows = vec![0u8; (w * 4 * 3) as usize];
            for (i, y) in [h / 10, h * 3 / 10, h / 2].into_iter().enumerate() {
                let row = &mut rows[i * (w * 4) as usize..(i + 1) * (w * 4) as usize];
                unsafe { gl.read_pixels(0, y as i32, w as i32, 1, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(row))) };
            }
            let mut st = st.borrow_mut();
            st.frame(rows);
            if !st.sample_due() {
                return;
            }
            let mut buf = vec![0u8; (w * h * 4) as usize];
            unsafe { gl.read_pixels(0, 0, w as i32, h as i32, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(&mut buf))) };
            let stride = (w * 4) as usize;
            let td: Vec<u8> = (0..h as usize).rev().flat_map(|y| buf[y * stride..(y + 1) * stride].iter().copied()).collect();
            if let Some(stage) = weak.upgrade() {
                st.samples.push(analyse(&stage, &td, w, h));
            }
        });
    }

    let (tx, rx) = channel();
    let opts = Options { vo: "libmpv", ao: "null", audio_device: String::new(), extra: vec![] };
    let player = Rc::new(Player::with_options(mpv.clone(), Role::Background, 1, tx.clone(), &opts).expect("player"));
    player.load(Path::new(&src), 0.0); // waits for the render context
    surface.set_player(Some(player.clone()));
    let fg = switch.then(|| {
        let fg = Rc::new(Player::with_options(mpv.clone(), Role::Foreground(EndAction::Loop), 2, tx.clone(), &opts).expect("player"));
        fg.load(Path::new("av://lavfi:testsrc2=size=1280x720:rate=30"), 0.0);
        surface.add(fg.clone()); // plays before it is drawn
        fg
    });

    // Once the video plays: warm up, then measure.
    let started = Rc::new(RefCell::new(None::<Instant>));
    let poll = slint::Timer::default();
    {
        let (started, stats, player, weak, mpv) = (started.clone(), stats.clone(), player.clone(), stage.as_weak(), mpv.clone());
        let mut base: Option<(Instant, u64, u64, f64, f64)> = None;
        let (surface, mut fg, mut fg_playing, mut step) = (surface.clone(), fg, false, 0);
        poll.start(slint::TimerMode::Repeated, Duration::from_millis(50), move || {
            while let Ok(r) = rx.try_recv() {
                if r.state == rustypresenter::live::PlayState::Error {
                    eprintln!("FAIL: {:?}", r.error);
                    std::process::exit(2);
                }
                fg_playing |= r.inst == 2 && r.state == rustypresenter::live::PlayState::Playing;
                if started.borrow().is_none() && r.state == rustypresenter::live::PlayState::Playing {
                    println!("playing");
                    *started.borrow_mut() = Some(Instant::now());
                }
            }
            if switch {
                let Some(t0) = *started.borrow() else { return };
                let el = t0.elapsed().as_secs_f64();
                let distinct = stats.borrow().distinct;
                let at = [2.0, 2.5, 4.0, 4.5, 6.5];
                if step >= at.len() || el < at[step] {
                    return;
                }
                match step {
                    0 => {
                        println!("foreground playing before drawn: {fg_playing}");
                        if !fg_playing {
                            println!("RESULT switch: FAIL");
                            std::process::exit(1);
                        }
                        surface.set_player(fg.clone());
                    }
                    1 | 3 => base = Some((Instant::now(), 0, distinct, 0.0, 0.0)),
                    2 => {
                        let b = base.unwrap();
                        let rate = (distinct - b.2) as f64 / b.0.elapsed().as_secs_f64();
                        println!("foreground distinct frames/s: {rate:.1}");
                        surface.set_player(Some(player.clone()));
                        if let Some(f) = fg.take() {
                            surface.remove(&f);
                        }
                        if rate < 20.0 {
                            println!("RESULT switch {mode:?}: FAIL");
                            std::process::exit(1);
                        }
                    }
                    _ => {
                        let b = base.unwrap();
                        let rate = (distinct - b.2) as f64 / b.0.elapsed().as_secs_f64();
                        println!("background distinct frames/s after switching back: {rate:.1}");
                        let pass = rate >= 20.0;
                        println!("RESULT switch {mode:?}: {}", if pass { "PASS" } else { "FAIL" });
                        std::process::exit(if pass { 0 } else { 1 });
                    }
                }
                step += 1;
                return;
            }
            if !measure {
                return;
            }
            let Some(t0) = *started.borrow() else { return };
            let prop = |n: &str| unsafe { mpv.get_string(player.handle(), n) }.and_then(|s| s.parse::<f64>().ok()).unwrap_or(f64::NAN);
            let el = t0.elapsed().as_secs_f64();
            let Some(stage) = weak.upgrade() else { return };
            if base.is_none() && el >= 2.0 {
                let mut st = stats.borrow_mut();
                st.samples.clear();
                st.next_sample = Some(Instant::now());
                base = Some((Instant::now(), st.frames, st.distinct, prop("estimated-frame-number"), prop("frame-drop-count")));
                println!("MEASURE_START");
            }
            if let Some(b) = base {
                // Software: snapshot the composed window (OpenGL samples in the probe).
                if mode == DrawMode::Software && stats.borrow_mut().sample_due() {
                    if let Ok(pb) = stage.window().take_snapshot() {
                        let smp = analyse(&stage, pb.as_bytes(), pb.width(), pb.height());
                        stats.borrow_mut().samples.push(smp);
                    }
                }
                if b.0.elapsed().as_secs_f64() >= secs {
                    let el = b.0.elapsed().as_secs_f64();
                    let st = stats.borrow();
                    let pass = summary(&st, el, b, prop("estimated-frame-number"), prop("frame-drop-count"), mode);
                    std::process::exit(if pass { 0 } else { 1 });
                }
            }
        });
    }
    stage.run().unwrap();
    std::process::exit(0);
}

fn summary(st: &Stats, el: f64, b: (Instant, u64, u64, f64, f64), est: f64, drops: f64, mode: DrawMode) -> bool {
    let fps = (st.frames - b.1) as f64 / el;
    let distinct = (st.distinct - b.2) as f64 / el;
    println!("--- video_surface {mode:?} summary ({el:.2}s window) ---");
    println!("frames={fps:.2}/s distinct_video_frames={distinct:.2}/s est_frame_number_rate={:.2}/s frame_drops_in_window={} noise_only_frames={}", (est - b.3) / el, drops - b.4, st.noise);
    let n = st.samples.len();
    if n < 3 {
        println!("FAIL: only {n} readback samples");
        return false;
    }
    let area = st.samples[0].white.len();
    let counts: Vec<usize> = st.samples.iter().map(|s| s.white.iter().filter(|b| **b).count()).collect();
    let always = (0..area).filter(|i| st.samples.iter().all(|s| s.white.get(*i) == Some(&true))).count();
    let mean = counts.iter().sum::<usize>() as f64 / n as f64;
    let never: Vec<usize> = (0..area).filter(|i| st.samples.iter().all(|s| s.white.get(*i) == Some(&false))).collect();
    let (mut under, mut vid) = (0.0, 0.0);
    for w in st.samples.windows(2) {
        under += never.iter().filter(|i| diff(w[0].text[**i], w[1].text[**i]) > 24).count() as f64 / never.len().max(1) as f64;
        vid += w[0].video.iter().zip(&w[1].video).filter(|(a, b)| diff(**a, **b) > 24).count() as f64 / w[0].video.len() as f64;
    }
    (under, vid) = (under / (n - 1) as f64, vid / (n - 1) as f64);
    let luma = |v: &[[u8; 3]]| v.iter().map(|p| (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.0).sum::<f64>() / v.len().max(1) as f64;
    let vid_luma = st.samples.iter().map(|s| luma(&s.video)).sum::<f64>() / n as f64;
    println!("readback: {n} samples; text box {area} px; white px per sample {counts:?}; white in every sample {always} ({:.1}% of mean {mean:.0})", 100.0 * always as f64 / mean.max(1.0));
    println!("readback: under-text changed/s {:.1}%; video corner mean luma {vid_luma:.1}, changed/s {:.1}%", under * 100.0, vid * 100.0);
    let text_ok = always as f64 >= 0.9 * mean && always as f64 > 0.03 * area as f64;
    let video_ok = vid_luma > 15.0 && vid > 0.05 && under > 0.01;
    let rate_ok = distinct >= 28.5;
    println!("checks: text_white_over_video={text_ok} video_present_and_changing={video_ok} distinct_fps>=28.5={rate_ok}");
    let pass = text_ok && video_ok && rate_ok;
    println!("RESULT {mode:?}: {}", if pass { "PASS" } else { "FAIL" });
    pass
}
