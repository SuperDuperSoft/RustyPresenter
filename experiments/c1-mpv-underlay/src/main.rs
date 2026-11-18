//! Check C1: libmpv video drawn underneath a Slint scene.
//!
//! GL mode (default): Slint winit backend + FemtoVG (OpenGL). On `RenderingSetup` an mpv render
//! context is created with `MPV_RENDER_API_TYPE_OPENGL` and Slint's `get_proc_address`; on
//! `BeforeRendering` mpv draws the current frame into Slint's framebuffer (flip_y = 1), the GL
//! state FemtoVG relies on is restored, and Slint then draws a transparent scene with a white
//! "Amazing grace" Text on top. mpv's update callback requests a Slint redraw.
//!
//! SW mode (`--sw`): Slint software renderer; mpv renders with `MPV_RENDER_API_TYPE_SW` into an
//! RGB buffer that is handed to Slint as an `Image` underneath the same Text.
//!
//! Verification is programmatic: once a second the composed window is read back (glReadPixels in
//! `AfterRendering`, or `take_snapshot` in SW mode) and the text area / a video-only area are
//! analysed; every rendered frame a few rows are hashed to count distinct video frames.
//! Once a render context exists it plays the source, warms up 2 s, measures `--secs`
//! (default 10), prints a summary and exits.
//!
//! Args: `--sw`, `--no-restore` (skip GL state restore, to see what breaks), `--no-block`
//! (MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME = 0), `--src <url>`,
//! `--secs <n>`, `--hwdec <mode>`. libmpv path: env `C1_LIBMPV`, default
//! `%LOCALAPPDATA%\RustyPresenter\deps\mpv\libmpv-2.dll`.
mod mpv;

use glow::HasContext;
use std::cell::RefCell;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::time::{Duration, Instant};

slint::slint! {
    export component Stage inherits Window {
        in property <image> video;
        out property <length> text-x: label.x;
        out property <length> text-y: label.y;
        out property <length> text-w: label.width;
        out property <length> text-h: label.height;
        title: "C1 mpv underlay";
        width: 1280px;
        height: 720px;
        background: transparent;
        Image { x: 0px; y: 0px; width: root.width; height: root.height; source: root.video; image-fit: fill; }
        label := Text { x: 60px; y: 60px; text: "Amazing grace"; color: white; font-size: 110px; font-weight: 800; }
    }
}

static LIB: OnceLock<mpv::Lib> = OnceLock::new();
static PENDING: AtomicBool = AtomicBool::new(false);
static SW_MODE: AtomicBool = AtomicBool::new(false);
static NO_BLOCK: AtomicBool = AtomicBool::new(false);

fn lib() -> &'static mpv::Lib {
    LIB.get().unwrap()
}

thread_local! {
    static UI: RefCell<Option<slint::Weak<Stage>>> = const { RefCell::new(None) };
    static SW: RefCell<Option<SwState>> = const { RefCell::new(None) };
    static STATS: Rc<RefCell<Stats>> = Rc::new(RefCell::new(Stats::default()));
}

/// One readback of the composed window, already analysed.
struct Sample {
    t: f64,
    white_mask: Vec<bool>,    // per pixel of the text rect: near-white?
    text_px: Vec<[u8; 3]>,    // text rect pixels
    video_px: Vec<[u8; 3]>,   // video-only rect pixels
    top_mid: [u8; 3],
    bottom_mid: [u8; 3],
}

#[derive(Default)]
struct Stats {
    frames: u64,     // composed frames (GL: AfterRendering count; SW: images handed to Slint)
    mpv_new: u64,    // render passes where mpv_render_context_update reported UPDATE_FRAME
    distinct: u64,   // frames whose sampled rows differ from the previous frame
    last_hash: u64,
    prev_hash: u64,          // hash before last_hash (to spot A-B-A flicker)
    prev_rows: Vec<u8>,
    noise_frames: u64,       // GL: frames that differ from the previous one only by <= 4 per byte
    aba: u64,                // frames equal to the one two frames back but not the previous
    samples: Vec<Sample>,
    render_errors: u64,
    max_render_ms: f64,
    total_render_ms: f64,
}

/// mpv's update callback: runs on an mpv thread. Must not call mpv; only hop to the UI thread.
unsafe extern "C" fn on_mpv_update(_: *mut c_void) {
    if PENDING.swap(true, SeqCst) {
        return; // a wake-up is already queued
    }
    let _ = slint::invoke_from_event_loop(|| {
        PENDING.store(false, SeqCst);
        if SW_MODE.load(SeqCst) {
            sw_frame();
        } else {
            UI.with(|u| {
                if let Some(s) = u.borrow().as_ref().and_then(|w| w.upgrade()) {
                    s.window().request_redraw();
                }
            });
        }
    });
}

unsafe extern "C" fn gl_get_proc_address(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let f = unsafe { &*(ctx as *const &dyn Fn(&CStr) -> *const c_void) };
    f(unsafe { CStr::from_ptr(name) }) as *mut c_void
}

fn hash_rows(rows: &[u8]) -> u64 {
    // FNV-1a over a strided subset; enough to tell frames apart.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in rows.iter().step_by(3) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Text rect and a video-only rect in physical, top-down pixel coordinates.
fn rects(stage: &Stage, w: u32, h: u32) -> ((u32, u32, u32, u32), (u32, u32, u32, u32)) {
    let s = stage.window().scale_factor();
    let tr = (
        (stage.get_text_x() * s) as u32,
        (stage.get_text_y() * s) as u32,
        (stage.get_text_w() * s) as u32,
        (stage.get_text_h() * s) as u32,
    );
    let vr = (w - 340, h - 240, 300, 200); // bottom-right corner, far from the text
    (tr, vr)
}

/// Analyse a top-down RGBA buffer of the whole window.
fn analyse(stage: &Stage, t: f64, rgba: &[u8], w: u32, h: u32) -> Sample {
    let (tr, vr) = rects(stage, w, h);
    let px = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2]]
    };
    let mut white_mask = Vec::new();
    let mut text_px = Vec::new();
    for y in tr.1..(tr.1 + tr.3).min(h) {
        for x in tr.0..(tr.0 + tr.2).min(w) {
            let p = px(x, y);
            white_mask.push(p.iter().all(|c| *c >= 235));
            text_px.push(p);
        }
    }
    let mut video_px = Vec::new();
    for y in vr.1..vr.1 + vr.3 {
        for x in vr.0..vr.0 + vr.2 {
            video_px.push(px(x, y));
        }
    }
    Sample { t, white_mask, text_px, video_px, top_mid: px(w / 2, h / 10), bottom_mid: px(w / 2, h * 9 / 10) }
}

fn diff(a: [u8; 3], b: [u8; 3]) -> u32 {
    a.iter().zip(b.iter()).map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs()).sum()
}

// ---------------------------------------------------------------- GL path

#[derive(Default)]
struct SavedGl {
    program: i32,
    vao: i32,
    array_buf: i32,
    draw_fbo: i32,
    read_fbo: i32,
    active_tex: i32,
    tex: [i32; 2],
    viewport: [i32; 4],
    caps: Vec<(u32, bool)>,
    blend: [i32; 6], // src rgb, dst rgb, src a, dst a, eq rgb, eq a
    unpack: [i32; 3], // alignment, row length, unpack buffer
}

const CAPS: [u32; 5] = [glow::SCISSOR_TEST, glow::BLEND, glow::CULL_FACE, glow::DEPTH_TEST, glow::STENCIL_TEST];

unsafe fn save_gl(gl: &glow::Context) -> SavedGl {
    unsafe {
        let mut s = SavedGl {
            program: gl.get_parameter_i32(glow::CURRENT_PROGRAM),
            vao: gl.get_parameter_i32(glow::VERTEX_ARRAY_BINDING),
            array_buf: gl.get_parameter_i32(glow::ARRAY_BUFFER_BINDING),
            draw_fbo: gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING),
            read_fbo: gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING),
            active_tex: gl.get_parameter_i32(glow::ACTIVE_TEXTURE),
            caps: CAPS.iter().map(|c| (*c, gl.is_enabled(*c))).collect(),
            ..Default::default()
        };
        for i in 0..2 {
            gl.active_texture(glow::TEXTURE0 + i);
            s.tex[i as usize] = gl.get_parameter_i32(glow::TEXTURE_BINDING_2D);
        }
        gl.active_texture(s.active_tex as u32);
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut s.viewport);
        for (i, p) in [glow::BLEND_SRC_RGB, glow::BLEND_DST_RGB, glow::BLEND_SRC_ALPHA, glow::BLEND_DST_ALPHA, glow::BLEND_EQUATION_RGB, glow::BLEND_EQUATION_ALPHA]
            .iter()
            .enumerate()
        {
            s.blend[i] = gl.get_parameter_i32(*p);
        }
        s.unpack = [
            gl.get_parameter_i32(glow::UNPACK_ALIGNMENT),
            gl.get_parameter_i32(glow::UNPACK_ROW_LENGTH),
            gl.get_parameter_i32(glow::PIXEL_UNPACK_BUFFER_BINDING),
        ];
        s
    }
}

fn nz(v: i32) -> Option<NonZeroU32> {
    NonZeroU32::new(v as u32)
}

unsafe fn restore_gl(gl: &glow::Context, s: &SavedGl) {
    unsafe {
        gl.use_program(nz(s.program).map(glow::NativeProgram));
        gl.bind_vertex_array(nz(s.vao).map(glow::NativeVertexArray));
        gl.bind_buffer(glow::ARRAY_BUFFER, nz(s.array_buf).map(glow::NativeBuffer));
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, nz(s.draw_fbo).map(glow::NativeFramebuffer));
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, nz(s.read_fbo).map(glow::NativeFramebuffer));
        for i in 0..2 {
            gl.active_texture(glow::TEXTURE0 + i);
            gl.bind_texture(glow::TEXTURE_2D, nz(s.tex[i as usize]).map(glow::NativeTexture));
        }
        gl.active_texture(s.active_tex as u32);
        gl.viewport(s.viewport[0], s.viewport[1], s.viewport[2], s.viewport[3]);
        for (c, on) in &s.caps {
            if *on { gl.enable(*c) } else { gl.disable(*c) }
        }
        gl.blend_func_separate(s.blend[0] as u32, s.blend[1] as u32, s.blend[2] as u32, s.blend[3] as u32);
        gl.blend_equation_separate(s.blend[4] as u32, s.blend[5] as u32);
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, s.unpack[0]);
        gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, s.unpack[1]);
        gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, nz(s.unpack[2]).map(glow::NativeBuffer));
    }
}

fn diff_gl(a: &SavedGl, b: &SavedGl) -> Vec<String> {
    let mut out = vec![];
    macro_rules! cmp {
        ($f:ident) => {
            if a.$f != b.$f {
                out.push(format!("{}: {:?} -> {:?}", stringify!($f), a.$f, b.$f));
            }
        };
    }
    cmp!(program);
    cmp!(vao);
    cmp!(array_buf);
    cmp!(draw_fbo);
    cmp!(read_fbo);
    cmp!(active_tex);
    cmp!(tex);
    cmp!(viewport);
    cmp!(caps);
    cmp!(blend);
    cmp!(unpack);
    out
}

struct GlState {
    gl: glow::Context,
    rc: *mut mpv::mpv_render_context,
    next_sample: f64,
    reported_state_diff: bool,
}

fn install_gl_notifier(stage: &Stage, start: Instant, no_restore: bool, mpvh: *mut mpv::mpv_handle) {
    let weak = stage.as_weak();
    let mut st: Option<GlState> = None;
    stage
        .window()
        .set_rendering_notifier(move |state, api| {
            let l = lib();
            match state {
                slint::RenderingState::RenderingSetup => {
                    let slint::GraphicsAPI::NativeOpenGL { get_proc_address } = api else {
                        eprintln!("FAIL: graphics API is not NativeOpenGL: {api:?}");
                        std::process::exit(2);
                    };
                    let gl = unsafe { glow::Context::from_loader_function_cstr(|n| get_proc_address(n)) };
                    unsafe {
                        println!(
                            "GL: version={:?} renderer={:?} vendor={:?}",
                            gl.get_parameter_string(glow::VERSION),
                            gl.get_parameter_string(glow::RENDERER),
                            gl.get_parameter_string(glow::VENDOR)
                        );
                    }
                    // mpv only calls get_proc_address during mpv_render_context_create, so a pointer
                    // to Slint's (callback-scoped) closure is valid for exactly as long as needed.
                    let gpa: &dyn Fn(&CStr) -> *const c_void = get_proc_address;
                    let mut init = mpv::mpv_opengl_init_params {
                        get_proc_address: Some(gl_get_proc_address),
                        get_proc_address_ctx: &gpa as *const _ as *mut c_void,
                    };
                    let mut params = [
                        mpv::mpv_render_param { kind: mpv::PARAM_API_TYPE, data: c"opengl".as_ptr() as *mut c_void },
                        mpv::mpv_render_param { kind: mpv::PARAM_OPENGL_INIT_PARAMS, data: &mut init as *mut _ as *mut c_void },
                        mpv::mpv_render_param { kind: mpv::PARAM_INVALID, data: std::ptr::null_mut() },
                    ];
                    let mut rc = std::ptr::null_mut();
                    let saved = unsafe { save_gl(&gl) };
                    let r = unsafe { (l.render_context_create)(&mut rc, mpvh, params.as_mut_ptr()) };
                    if r < 0 {
                        eprintln!("FAIL: mpv_render_context_create(opengl) = {} ({})", r, l.err(r));
                        std::process::exit(2);
                    }
                    unsafe {
                        restore_gl(&gl, &saved);
                        (l.render_context_set_update_callback)(rc, Some(on_mpv_update), std::ptr::null_mut());
                    }
                    println!("mpv OpenGL render context created at {:.2}s", start.elapsed().as_secs_f64());
                    st = Some(GlState { gl, rc, next_sample: start.elapsed().as_secs_f64() + 2.0, reported_state_diff: false });
                    start_playback(mpvh, weak.clone());
                }
                slint::RenderingState::BeforeRendering => {
                    let Some(s) = st.as_mut() else { return };
                    let Some(stage) = weak.upgrade() else { return };
                    let size = stage.window().size();
                    let gl = &s.gl;
                    let before = unsafe { save_gl(gl) };
                    let flags = unsafe { (l.render_context_update)(s.rc) };
                    let mut fbo = mpv::mpv_opengl_fbo {
                        fbo: before.draw_fbo,
                        w: size.width as c_int,
                        h: size.height as c_int,
                        internal_format: 0,
                    };
                    let mut flip: c_int = 1; // rendering to the window's default framebuffer
                    let mut block: c_int = if NO_BLOCK.load(SeqCst) { 0 } else { 1 };
                    let mut params = [
                        mpv::mpv_render_param { kind: mpv::PARAM_BLOCK_FOR_TARGET_TIME, data: &mut block as *mut _ as *mut c_void },
                        mpv::mpv_render_param { kind: mpv::PARAM_OPENGL_FBO, data: &mut fbo as *mut _ as *mut c_void },
                        mpv::mpv_render_param { kind: mpv::PARAM_FLIP_Y, data: &mut flip as *mut _ as *mut c_void },
                        mpv::mpv_render_param { kind: mpv::PARAM_INVALID, data: std::ptr::null_mut() },
                    ];
                    let t0 = Instant::now();
                    let r = unsafe { (l.render_context_render)(s.rc, params.as_mut_ptr()) };
                    let ms = t0.elapsed().as_secs_f64() * 1000.0;
                    let after = unsafe { save_gl(gl) };
                    if !s.reported_state_diff && flags & mpv::UPDATE_FRAME != 0 {
                        s.reported_state_diff = true;
                        println!("GL state changed by mpv_render_context_render: {:?}", diff_gl(&before, &after));
                    }
                    if !no_restore {
                        unsafe { restore_gl(gl, &before) };
                    }
                    STATS.with(|st| {
                        let mut st = st.borrow_mut();
                        if r < 0 {
                            st.render_errors += 1;
                        }
                        if flags & mpv::UPDATE_FRAME != 0 {
                            st.mpv_new += 1;
                        }
                        st.total_render_ms += ms;
                        st.max_render_ms = st.max_render_ms.max(ms);
                    });
                }
                slint::RenderingState::AfterRendering => {
                    let Some(s) = st.as_mut() else { return };
                    let Some(stage) = weak.upgrade() else { return };
                    let size = stage.window().size();
                    let (w, h) = (size.width, size.height);
                    let gl = &s.gl;
                    // Per frame: hash three rows in the lower half (GL rows count from the bottom).
                    let mut rows = vec![0u8; (w * 4 * 3) as usize];
                    for (i, y) in [h / 10, h * 3 / 10, h / 2].iter().enumerate() {
                        let slice = &mut rows[(i as u32 * w * 4) as usize..((i as u32 + 1) * w * 4) as usize];
                        unsafe {
                            gl.read_pixels(0, *y as i32, w as i32, 1, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(slice)));
                        }
                    }
                    let hsh = hash_rows(&rows);
                    let t = start.elapsed().as_secs_f64();
                    let want_sample = t >= s.next_sample;
                    if want_sample {
                        s.next_sample += 1.0;
                    }
                    STATS.with(|st| {
                        let mut st = st.borrow_mut();
                        st.frames += 1;
                        // A "distinct" frame must differ by more than dithering noise (+-1 LSB seen
                        // on redraws of the same hw-decoded frame).
                        let md = rows.iter().zip(&st.prev_rows).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(255);
                        if hsh != st.last_hash && md <= 4 {
                            st.noise_frames += 1;
                        }
                        if md > 4 {
                            if hsh == st.prev_hash {
                                st.aba += 1;
                            }
                            st.distinct += 1;
                            st.prev_hash = st.last_hash;
                        }
                        st.last_hash = hsh;
                        st.prev_rows = rows;
                    });
                    if want_sample {
                        let mut buf = vec![0u8; (w * h * 4) as usize];
                        unsafe {
                            gl.read_pixels(0, 0, w as i32, h as i32, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(&mut buf)));
                        }
                        // flip to top-down
                        let stride = (w * 4) as usize;
                        let mut td = vec![0u8; buf.len()];
                        for y in 0..h as usize {
                            td[y * stride..(y + 1) * stride].copy_from_slice(&buf[(h as usize - 1 - y) * stride..(h as usize - y) * stride]);
                        }
                        let smp = analyse(&stage, t, &td, w, h);
                        STATS.with(|st| st.borrow_mut().samples.push(smp));
                    }
                    // Tell mpv a swap is about to happen (present follows AfterRendering).
                    unsafe { (l.render_context_report_swap)(s.rc) };
                }
                slint::RenderingState::RenderingTeardown => {
                    if let Some(s) = st.take() {
                        unsafe { (l.render_context_free)(s.rc) };
                    }
                }
                _ => {}
            }
        })
        .expect("set_rendering_notifier failed (renderer without notifier support?)");
}

// ---------------------------------------------------------------- SW path

struct SwState {
    rc: *mut mpv::mpv_render_context,
    buf: Vec<u8>, // over-allocated so the target can start on a 64-byte boundary
    w: u32,
    h: u32,
}

fn sw_frame() {
    let Some(stage) = UI.with(|u| u.borrow().as_ref().and_then(|w| w.upgrade())) else { return };
    SW.with(|s| {
        let mut s = s.borrow_mut();
        let Some(s) = s.as_mut() else { return };
        let l = lib();
        let flags = unsafe { (l.render_context_update)(s.rc) };
        let size = stage.window().size();
        if std::env::var_os("C1_DEBUG").is_some() {
            eprintln!("sw_frame flags={flags} size={}x{}", size.width, size.height);
        }
        if flags & mpv::UPDATE_FRAME == 0 {
            return;
        }
        let (w, h) = (size.width & !15, size.height); // width multiple of 16 -> stride multiple of 64
        if (w, h) != (s.w, s.h) {
            s.buf = vec![0u8; (w * h * 4) as usize + 64];
            (s.w, s.h) = (w, h);
        }
        let off = s.buf.as_ptr().align_offset(64);
        let ptr = unsafe { s.buf.as_mut_ptr().add(off) };
        let mut sz: [c_int; 2] = [w as c_int, h as c_int];
        let mut stride: usize = (w * 4) as usize;
        let mut block: c_int = if NO_BLOCK.load(SeqCst) { 0 } else { 1 };
        let mut params = [
            mpv::mpv_render_param { kind: mpv::PARAM_API_TYPE, data: c"sw".as_ptr() as *mut c_void },
            mpv::mpv_render_param { kind: mpv::PARAM_SW_SIZE, data: sz.as_mut_ptr() as *mut c_void },
            mpv::mpv_render_param { kind: mpv::PARAM_SW_FORMAT, data: c"rgb0".as_ptr() as *mut c_void },
            mpv::mpv_render_param { kind: mpv::PARAM_SW_STRIDE, data: &mut stride as *mut _ as *mut c_void },
            mpv::mpv_render_param { kind: mpv::PARAM_SW_POINTER, data: ptr as *mut c_void },
            mpv::mpv_render_param { kind: mpv::PARAM_BLOCK_FOR_TARGET_TIME, data: &mut block as *mut _ as *mut c_void },
            mpv::mpv_render_param { kind: mpv::PARAM_INVALID, data: std::ptr::null_mut() },
        ];
        let t0 = Instant::now();
        let r = unsafe { (l.render_context_render)(s.rc, params.as_mut_ptr()) };
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let src = &s.buf[off..off + (w * h * 4) as usize];
        let mut pb = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(w, h);
        for (d, sp) in pb.make_mut_slice().iter_mut().zip(src.chunks_exact(4)) {
            *d = slint::Rgb8Pixel { r: sp[0], g: sp[1], b: sp[2] };
        }
        // per-frame hash of three rows in the lower half (top-down: rows at 50%, 70%, 90%)
        let mut rows = Vec::with_capacity((w * 4 * 3) as usize);
        for y in [h / 2, h * 7 / 10, h * 9 / 10] {
            rows.extend_from_slice(&src[(y * w * 4) as usize..((y + 1) * w * 4) as usize]);
        }
        let hsh = hash_rows(&rows);
        stage.set_video(slint::Image::from_rgb8(pb));
        STATS.with(|st| {
            let mut st = st.borrow_mut();
            st.frames += 1;
            st.mpv_new += 1;
            if r < 0 {
                st.render_errors += 1;
            }
            if hsh != st.last_hash {
                st.distinct += 1;
                st.last_hash = hsh;
            }
            st.total_render_ms += ms;
            st.max_render_ms = st.max_render_ms.max(ms);
        });
    });
}

// ---------------------------------------------------------------- main

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let sw = args.iter().any(|a| a == "--sw");
    let no_restore = args.iter().any(|a| a == "--no-restore");
    let src = arg_value(&args, "--src").unwrap_or_else(|| "av://lavfi:testsrc2=size=1920x1080:rate=30".into());
    let secs: f64 = arg_value(&args, "--secs").map(|s| s.parse().unwrap()).unwrap_or(10.0);
    let hwdec = arg_value(&args, "--hwdec").unwrap_or_else(|| "auto-safe".into());
    let dll = std::env::var("C1_LIBMPV").unwrap_or_else(|_| {
        format!("{}\\RustyPresenter\\deps\\mpv\\libmpv-2.dll", std::env::var("LOCALAPPDATA").unwrap())
    });
    SW_MODE.store(sw, SeqCst);
    NO_BLOCK.store(args.iter().any(|a| a == "--no-block"), SeqCst);
    RUN.set((src.clone(), secs, sw)).unwrap();
    println!("mode={} src={src} hwdec={hwdec} restore={} block_for_target_time={} pid={}", if sw { "software" } else { "opengl" }, !no_restore, !NO_BLOCK.load(SeqCst), std::process::id());

    let l = match mpv::Lib::load(&dll) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("FAIL: cannot load libmpv from {dll}: {e}");
            std::process::exit(2);
        }
    };
    let ver = unsafe { (l.client_api_version)() };
    println!("libmpv loaded: client API {}.{}", ver >> 16, ver & 0xffff);
    LIB.set(l).ok().unwrap();
    let l = lib();

    if let Some(out) = arg_value(&args, "--encode") {
        encode(&out, &arg_value(&args, "--ovc").unwrap_or_else(|| "h264_mf".into()), secs);
        return;
    }

    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name(if sw { "software" } else { "femtovg" }.into())
        .select()
        .expect("backend select");

    let h = unsafe { (l.create)() };
    assert!(!h.is_null(), "mpv_create failed");
    for (k, v) in [
        ("vo", "libmpv"),
        ("hwdec", hwdec.as_str()),
        ("loop-file", "inf"),
        ("terminal", "yes"),
        ("msg-level", "all=warn"),
        ("ao", "null"),
    ] {
        l.set_option(h, k, v);
    }
    let r = unsafe { (l.initialize)(h) };
    assert!(r >= 0, "mpv_initialize: {}", l.err(r));

    let stage = Stage::new().unwrap();
    UI.with(|u| *u.borrow_mut() = Some(stage.as_weak()));
    let start = Instant::now();

    if sw {
        let mut params = [
            mpv::mpv_render_param { kind: mpv::PARAM_API_TYPE, data: c"sw".as_ptr() as *mut c_void },
            mpv::mpv_render_param { kind: mpv::PARAM_INVALID, data: std::ptr::null_mut() },
        ];
        let mut rc = std::ptr::null_mut();
        let r = unsafe { (l.render_context_create)(&mut rc, h, params.as_mut_ptr()) };
        if r < 0 {
            eprintln!("FAIL: mpv_render_context_create(sw) = {} ({})", r, l.err(r));
            std::process::exit(2);
        }
        SW.with(|s| *s.borrow_mut() = Some(SwState { rc, buf: vec![], w: 0, h: 0 }));
        unsafe { (l.render_context_set_update_callback)(rc, Some(on_mpv_update), std::ptr::null_mut()) };
        // Snapshot the composed window once a second for verification.
        let weak = stage.as_weak();
        let timer = Box::leak(Box::new(slint::Timer::default()));
        timer.start(slint::TimerMode::Repeated, Duration::from_secs(1), move || {
            let t = start.elapsed().as_secs_f64();
            if STATS.with(|st| st.borrow().frames) < 30 {
                return; // wait until video has been playing for about a second
            }
            let s = weak.upgrade().unwrap();
            match s.window().take_snapshot() {
                Ok(pb) => {
                    let smp = analyse(&s, t, pb.as_bytes(), pb.width(), pb.height());
                    STATS.with(|st| st.borrow_mut().samples.push(smp));
                }
                Err(e) => eprintln!("take_snapshot failed: {e}"),
            }
        });
        start_playback(h, stage.as_weak());
    } else {
        install_gl_notifier(&stage, start, no_restore, h);
    }


    stage.run().unwrap();
    // Exit without mpv_terminate_destroy: it would block while a render context exists, and the
    // GL render context must be freed with the GL context current (RenderingTeardown).
    std::process::exit(0);
}

static RUN: OnceLock<(String, f64, bool)> = OnceLock::new(); // src, measured seconds, sw

/// Called once a render context exists: load the source and schedule the measurement window
/// (2 s warm-up, then `secs` measured), the summary and exit.
fn start_playback(h: *mut mpv::mpv_handle, weak: slint::Weak<Stage>) {
    let (src, secs, sw) = RUN.get().unwrap().clone();
    let l = lib();
    let r = l.command(h, &["loadfile", &src]);
    if r < 0 {
        eprintln!("FAIL: loadfile: {}", l.err(r));
        std::process::exit(2);
    }

    // Measurement window: from 2 s to `secs`.
    let warm = 2.0f64;
    let base: Rc<RefCell<Option<(Instant, u64, u64, u64, f64, String, String, String, String)>>> = Rc::new(RefCell::new(None));
    let snapshot = move |h| {
        STATS.with(|st| {
            let st = st.borrow();
            (
                Instant::now(),
                st.frames,
                st.mpv_new,
                st.distinct,
                st.total_render_ms,
                lib().get(h, "estimated-frame-number"),
                lib().get(h, "frame-drop-count"),
                lib().get(h, "vo-delayed-frame-count"),
                lib().get(h, "time-pos"),
            )
        })
    };
    {
        let base = base.clone();
        slint::Timer::single_shot(Duration::from_secs_f64(warm), move || {
            println!("MEASURE_START");
            *base.borrow_mut() = Some(snapshot(h));
        });
    }

    slint::Timer::single_shot(Duration::from_secs_f64(warm + secs), move || {
        let end = snapshot(h);
        println!("MEASURE_END");
        let b = base.borrow().clone().unwrap();
        let el = end.0.duration_since(b.0).as_secs_f64();
        let fps = (end.1 - b.1) as f64 / el;
        let mpv_fps = (end.2 - b.2) as f64 / el;
        let distinct_fps = (end.3 - b.3) as f64 / el;
        let avg_render = (end.4 - b.4) / ((end.1 - b.1).max(1) as f64);
        let ef = |s: &str| s.parse::<f64>().unwrap_or(f64::NAN);
        let est_fps = (ef(&end.5) - ef(&b.5)) / el;
        let tp_rate = (ef(&end.8) - ef(&b.8)) / el;
        let l = lib();
        println!("--- C1 {} summary ({el:.2}s window) ---", if sw { "software" } else { "opengl" });
        println!(
            "frames={fps:.2}/s mpv_update_frame={mpv_fps:.2}/s distinct_video_frames={distinct_fps:.2}/s est_frame_number_rate={est_fps:.2}/s time_pos_rate={tp_rate:.3}"
        );
        println!(
            "mpv: estimated-vf-fps={} container-fps={} frame-drop-count={} (window start {}) vo-delayed-frame-count={} (window start {}) mistimed-frame-count={} decoder-frame-drop-count={} hwdec-current={} current-vo={} video-params/pixelformat={} width={} height={}",
            l.get(h, "estimated-vf-fps"),
            l.get(h, "container-fps"),
            end.6,
            b.6,
            end.7,
            b.7,
            l.get(h, "mistimed-frame-count"),
            l.get(h, "decoder-frame-drop-count"),
            l.get(h, "hwdec-current"),
            l.get(h, "current-vo"),
            l.get(h, "video-params/pixelformat"),
            l.get(h, "width"),
            l.get(h, "height"),
        );
        let s = weak.upgrade().unwrap();
        let sz = s.window().size();
        println!("window physical={}x{} scale={} text rect(logical)=({},{} {}x{})", sz.width, sz.height, s.window().scale_factor(), s.get_text_x(), s.get_text_y(), s.get_text_w(), s.get_text_h());
        let pass = STATS.with(|st| {
            let st = st.borrow();
            println!("mpv render: avg {avg_render:.2} ms, max {:.2} ms, errors {}", st.max_render_ms, st.render_errors);
            println!(
                "frames differing from the previous only by <=4/255 (dither noise, not counted as distinct): {}; A-B-A flicker frames: {}",
                st.noise_frames, st.aba
            );
            let n = st.samples.len();
            if n < 3 {
                println!("FAIL: only {n} readback samples");
                return false;
            }
            let area = st.samples[0].white_mask.len();
            let counts: Vec<usize> = st.samples.iter().map(|s| s.white_mask.iter().filter(|b| **b).count()).collect();
            let always_white = (0..area).filter(|i| st.samples.iter().all(|s| s.white_mask[*i])).count();
            let mean_white = counts.iter().sum::<usize>() as f64 / n as f64;
            // Under-text video: pixels never white, changed between consecutive samples?
            let never_white: Vec<usize> = (0..area).filter(|i| st.samples.iter().all(|s| !s.white_mask[*i])).collect();
            let mut under_changed = 0.0;
            let mut vid_changed = 0.0;
            for w2 in st.samples.windows(2) {
                under_changed += never_white.iter().filter(|i| diff(w2[0].text_px[**i], w2[1].text_px[**i]) > 24).count() as f64 / never_white.len().max(1) as f64;
                vid_changed += w2[0].video_px.iter().zip(&w2[1].video_px).filter(|(a, b)| diff(**a, **b) > 24).count() as f64 / w2[0].video_px.len() as f64;
            }
            under_changed /= (n - 1) as f64;
            vid_changed /= (n - 1) as f64;
            let luma = |v: &[[u8; 3]]| v.iter().map(|p| (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.0).sum::<f64>() / v.len() as f64;
            let vid_luma = st.samples.iter().map(|s| luma(&s.video_px)).sum::<f64>() / n as f64;
            let under_luma = st.samples.iter().map(|s| luma(&never_white.iter().map(|i| s.text_px[*i]).collect::<Vec<_>>())).sum::<f64>() / n as f64;
            println!(
                "readback: {n} samples; text rect {area} px; white px per sample {counts:?}; white in every sample {always_white} ({:.1}% of mean {mean_white:.0})",
                100.0 * always_white as f64 / mean_white.max(1.0)
            );
            println!(
                "readback: under-text non-white px mean luma {under_luma:.1}, changed/s {:.1}%; video-only rect mean luma {vid_luma:.1}, changed/s {:.1}%",
                under_changed * 100.0,
                vid_changed * 100.0
            );
            println!("readback: sample times {:?}", st.samples.iter().map(|s| (s.t * 10.0).round() / 10.0).collect::<Vec<_>>());
            println!("readback: first sample top-mid {:?} bottom-mid {:?}", st.samples[0].top_mid, st.samples[0].bottom_mid);
            let text_ok = always_white as f64 >= 0.9 * mean_white && always_white as f64 > 0.03 * area as f64;
            let video_ok = vid_luma > 15.0 && vid_changed > 0.05 && under_luma > 15.0 && under_changed > 0.01;
            let rate_ok = distinct_fps >= 28.5;
            println!("checks: text_white_over_video={text_ok} video_present_and_changing={video_ok} distinct_fps>=28.5={rate_ok}");
            text_ok && video_ok && rate_ok
        });
        println!("RESULT {}: {}", if sw { "software" } else { "opengl" }, if pass { "PASS" } else { "FAIL" });
        slint::quit_event_loop().unwrap();
    });
}

/// Helper (not part of the check itself): make a local H.264 test file from testsrc2 with
/// libmpv's encoding mode, so the hardware-decode path can be exercised without downloads.
fn encode(out: &str, ovc: &str, secs: f64) {
    let l = lib();
    let h = unsafe { (l.create)() };
    let end = format!("{secs}");
    for (k, v) in [
        ("o", out),
        ("ovc", ovc),
        ("ovcopts", "b=8M"),
        ("of", "mp4"),
        ("end", end.as_str()),
        ("terminal", "yes"),
        ("msg-level", "all=warn,encode=info"),
        ("vo", "lavc"),
        ("ao", "null"),
    ] {
        l.set_option(h, k, v);
    }
    let r = unsafe { (l.initialize)(h) };
    assert!(r >= 0, "mpv_initialize: {}", l.err(r));
    println!("encoder-list has h264: {}", l.get(h, "encoder-list").matches("h264").count());
    let r = l.command(h, &["loadfile", "av://lavfi:testsrc2=size=1920x1080:rate=30"]);
    assert!(r >= 0, "loadfile: {}", l.err(r));
    loop {
        let ev = unsafe { &*(l.wait_event)(h, 60.0) };
        if ev.event_id == mpv::EVENT_END_FILE || ev.event_id == mpv::EVENT_SHUTDOWN {
            println!("encode finished (event {}, error {})", ev.event_id, ev.error);
            break;
        }
    }
    unsafe { (l.terminate_destroy)(h) }; // finalises the muxer (writes the mp4 index)
}
