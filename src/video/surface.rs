//! Draws one mpv player under a Slint scene (check C1). OpenGL: mpv renders into
//! the window's framebuffer from the rendering notifier, before Slint draws on
//! top. Software: mpv renders into a buffer that is handed out as an `Image`.
use super::ffi::*;
use super::player::{Player, RenderContext};
use glow::HasContext;
use slint::{ComponentHandle, GraphicsAPI, PhysicalSize, RenderingState};
use std::cell::RefCell;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawMode {
    OpenGl,
    Software,
}

/// Picks Slint's backend: winit + FemtoVG (OpenGL), or the software renderer when
/// `RP_FORCE_SOFTWARE=1` or OpenGL cannot be selected. Call once, before any window.
pub fn choose_backend() -> DrawMode {
    let select = |r: &str| slint::BackendSelector::new().backend_name("winit".into()).renderer_name(r.into()).select();
    if std::env::var("RP_FORCE_SOFTWARE").as_deref() != Ok("1") {
        match select("femtovg") {
            Ok(()) => return DrawMode::OpenGl,
            Err(e) => eprintln!("OpenGL drawing unavailable ({e}); drawing video in software"),
        }
    }
    if let Err(e) = select("software") {
        eprintln!("software renderer: {e}");
    }
    DrawMode::Software
}

/// What mpv's update callback reaches. Leaked once per surface, so a callback that
/// races a render context being freed never sees freed memory.
struct Wake {
    queued: AtomicBool,
    surface: usize,
}

thread_local! {
    /// Live surfaces on the UI thread, by `Wake::surface`.
    static SURFACES: RefCell<Vec<(usize, std::rc::Weak<VideoSurface>)>> = const { RefCell::new(Vec::new()) };
}

/// mpv's update callback, on an mpv thread: never calls mpv, only coalesces and hops to the UI thread.
unsafe extern "C" fn on_update(ctx: *mut c_void) {
    let wake = unsafe { &*(ctx as *const Wake) };
    if wake.queued.swap(true, Ordering::AcqRel) {
        return;
    }
    let id = wake.surface;
    let _ = slint::invoke_from_event_loop(move || {
        let s = SURFACES.with(|v| v.borrow().iter().find(|(i, _)| *i == id).and_then(|(_, w)| w.upgrade()));
        if let Some(s) = s {
            s.wake.queued.store(false, Ordering::Release);
            s.woken();
        }
    });
}

unsafe extern "C" fn gl_proc(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let f = unsafe { &*(ctx as *const &dyn Fn(&CStr) -> *const c_void) };
    f(unsafe { CStr::from_ptr(name) }) as *mut c_void
}

/// Draws the chosen player (the foreground video if any, else the background) under one window.
pub struct VideoSurface {
    mode: DrawMode,
    wake: &'static Wake,
    size: Box<dyn Fn() -> Option<PhysicalSize>>,
    redraw: Box<dyn Fn()>,
    on_frame: Box<dyn Fn(Option<slint::Image>)>,
    st: RefCell<State>,
    probe: RefCell<Option<Box<dyn FnMut(&glow::Context, PhysicalSize)>>>,
}

#[derive(Default)]
struct State {
    /// What `set_player` asked for.
    want: Option<Rc<Player>>,
    /// The player a render context was made for (or tried); differs from `want` until synced.
    bound: Option<Rc<Player>>,
    /// `bound` renders through mpv's software API (always in Software mode; after a GL failure in OpenGL mode).
    sw: bool,
    gl: Option<glow::Context>,
    buf: Vec<u8>,
    w: u32,
    h: u32,
}

impl VideoSurface {
    /// Attaches to `ui`'s window. `on_frame` receives software frames (and `None`
    /// when they stop); show it as an `Image` under the scene. In OpenGL mode this
    /// takes the window's rendering notifier.
    pub fn attach<C: ComponentHandle + 'static>(ui: &C, mode: DrawMode, on_frame: impl Fn(Option<slint::Image>) + 'static) -> Rc<VideoSurface> {
        let (w1, w2) = (ui.as_weak(), ui.as_weak());
        static NEXT: AtomicUsize = AtomicUsize::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let wake: &'static Wake = Box::leak(Box::new(Wake { queued: AtomicBool::new(false), surface: id }));
        let mut s = VideoSurface {
            mode,
            wake,
            size: Box::new(move || w1.upgrade().map(|c| c.window().size())),
            redraw: Box::new(move || {
                if let Some(c) = w2.upgrade() {
                    c.window().request_redraw()
                }
            }),
            on_frame: Box::new(on_frame),
            st: RefCell::default(),
            probe: RefCell::new(None),
        };
        if mode == DrawMode::OpenGl {
            let slot: Rc<RefCell<Option<Rc<VideoSurface>>>> = Rc::default();
            let s2 = slot.clone();
            let notifier = ui.window().set_rendering_notifier(move |state, api| {
                let s = s2.borrow().clone();
                if let Some(s) = s {
                    s.notify(state, api)
                }
            });
            match notifier {
                Ok(()) => return VideoSurface::register(s, Some(slot)),
                Err(e) => {
                    eprintln!("no rendering notifier ({e}); drawing video in software");
                    s.mode = DrawMode::Software;
                }
            }
        }
        VideoSurface::register(s, None)
    }

    fn register(s: VideoSurface, slot: Option<Rc<RefCell<Option<Rc<VideoSurface>>>>>) -> Rc<VideoSurface> {
        let s = Rc::new(s);
        let id = s.wake.surface;
        SURFACES.with(|v| {
            let mut v = v.borrow_mut();
            v.retain(|(_, w)| w.strong_count() > 0);
            v.push((id, Rc::downgrade(&s)));
        });
        if let Some(slot) = slot {
            *slot.borrow_mut() = Some(s.clone());
        }
        s
    }

    pub fn mode(&self) -> DrawMode {
        self.mode
    }

    /// Chooses the player to draw (`None` = nothing). Its `load` may come before or
    /// after: with `vo=libmpv` the player waits for the render context made here.
    pub fn set_player(self: &Rc<Self>, player: Option<Rc<Player>>) {
        self.st.borrow_mut().want = player;
        match self.mode {
            DrawMode::Software => self.sync(None),
            DrawMode::OpenGl => (self.redraw)(),
        }
    }

    /// Whether a video is set to be drawn.
    pub fn active(&self) -> bool {
        self.st.borrow().want.is_some()
    }

    /// Diagnostics only: called after Slint has drawn each OpenGL frame (e.g. readback).
    pub fn set_gl_probe(&self, f: impl FnMut(&glow::Context, PhysicalSize) + 'static) {
        *self.probe.borrow_mut() = Some(Box::new(f));
    }

    /// Frees the old render context and makes one for the wanted player. In OpenGL
    /// mode this runs inside the notifier with the GL context current.
    fn sync(self: &Rc<Self>, api: Option<&GraphicsAPI>) {
        let mut st = self.st.borrow_mut();
        let same = match (&st.want, &st.bound) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        let cleared = st.sw;
        if let Some(old) = st.bound.take() {
            drop(old.take_render());
        }
        st.sw = false;
        if let Some(p) = st.want.clone() {
            let made = match (self.mode, api) {
                (DrawMode::OpenGl, Some(GraphicsAPI::NativeOpenGL { get_proc_address })) => create_gl(&p, *get_proc_address).or_else(|e| {
                    eprintln!("mpv OpenGL drawing failed ({e}); drawing this video in software");
                    create_sw(&p)
                }),
                _ => create_sw(&p),
            };
            match made {
                Ok((rc, sw)) => {
                    st.sw = sw;
                    unsafe { (p.mpv().render_context_set_update_callback)(rc.raw(), Some(on_update), self.wake as *const Wake as *mut c_void) };
                    p.set_render(rc); // starts a load that waited for it
                }
                Err(e) => eprintln!("video {} cannot be drawn: {e}", p.inst()),
            }
            st.bound = Some(p);
        }
        drop(st);
        if cleared {
            // Not inside the notifier: let Slint finish this frame first.
            let s = self.clone();
            slint::Timer::single_shot(Duration::ZERO, move || {
                if !s.st.borrow().sw {
                    (s.on_frame)(None)
                }
            });
        }
    }

    /// mpv has something new.
    fn woken(self: &Rc<Self>) {
        if self.st.borrow().sw {
            self.sw_frame();
        } else {
            (self.redraw)();
        }
    }

    fn notify(self: &Rc<Self>, state: RenderingState, api: &GraphicsAPI) {
        match state {
            RenderingState::RenderingSetup => {
                if let GraphicsAPI::NativeOpenGL { get_proc_address } = api {
                    self.st.borrow_mut().gl = Some(unsafe { glow::Context::from_loader_function_cstr(|n| get_proc_address(n)) });
                }
            }
            RenderingState::BeforeRendering => self.draw_gl(api),
            RenderingState::AfterRendering => {
                let st = self.st.borrow();
                let Some(gl) = st.gl.as_ref() else { return };
                if let (Some(f), Some(size)) = (self.probe.borrow_mut().as_mut(), (self.size)()) {
                    f(gl, size);
                }
                if let (false, Some(p)) = (st.sw, st.bound.as_ref()) {
                    if let Some(rc) = p.render_raw() {
                        unsafe { (p.mpv().render_context_report_swap)(rc) };
                    }
                }
            }
            RenderingState::RenderingTeardown => {
                let mut st = self.st.borrow_mut();
                if let Some(p) = st.bound.take() {
                    drop(p.take_render()); // GL is current here
                }
                st.sw = false;
                st.gl = None;
            }
            _ => {}
        }
    }

    /// Draws the current video frame into the framebuffer Slint is about to draw on.
    fn draw_gl(self: &Rc<Self>, api: &GraphicsAPI) {
        let Some(size) = (self.size)() else { return };
        let saved = {
            let st = self.st.borrow();
            let Some(gl) = st.gl.as_ref() else { return };
            if st.want.is_none() && st.bound.is_none() {
                return;
            }
            unsafe { save_gl(gl) }
        };
        self.sync(Some(api));
        let st = self.st.borrow();
        let Some(gl) = st.gl.as_ref() else { return };
        if let (false, Some(p)) = (st.sw, st.bound.as_ref()) {
            if let Some(rc) = p.render_raw() {
                let mpv = p.mpv();
                let mut fbo = mpv_opengl_fbo { fbo: saved.draw_fbo, w: size.width as c_int, h: size.height as c_int, internal_format: 0 };
                let (mut flip, mut block): (c_int, c_int) = (1, 0);
                let mut params = [
                    mpv_render_param { kind: PARAM_OPENGL_FBO, data: &mut fbo as *mut _ as *mut c_void },
                    mpv_render_param { kind: PARAM_FLIP_Y, data: &mut flip as *mut _ as *mut c_void },
                    mpv_render_param { kind: PARAM_BLOCK_FOR_TARGET_TIME, data: &mut block as *mut _ as *mut c_void },
                    mpv_render_param { kind: PARAM_INVALID, data: std::ptr::null_mut() },
                ];
                unsafe {
                    (mpv.render_context_update)(rc);
                    (mpv.render_context_render)(rc, params.as_mut_ptr());
                }
            }
        }
        unsafe { restore_gl(gl, &saved) };
    }

    /// Software: renders a new mpv frame and hands it out as an image.
    fn sw_frame(&self) {
        let img = {
            let mut st = self.st.borrow_mut();
            let Some((p, rc)) = st.bound.as_ref().and_then(|p| p.render_raw().map(|rc| (p.clone(), rc))) else { return };
            let mpv = p.mpv();
            if unsafe { (mpv.render_context_update)(rc) } & UPDATE_FRAME == 0 {
                return;
            }
            let Some(size) = (self.size)() else { return };
            // Width a multiple of 16, so the rgb0 stride is a multiple of 64.
            let (w, h) = (size.width & !15, size.height);
            if w == 0 || h == 0 {
                return;
            }
            if (w, h) != (st.w, st.h) {
                st.buf = vec![0u8; (w * h * 4) as usize + 64];
                (st.w, st.h) = (w, h);
            }
            let off = st.buf.as_ptr().align_offset(64);
            let ptr = unsafe { st.buf.as_mut_ptr().add(off) };
            let mut sz: [c_int; 2] = [w as c_int, h as c_int];
            let mut stride: usize = (w * 4) as usize;
            let mut block: c_int = 0;
            let mut params = [
                mpv_render_param { kind: PARAM_API_TYPE, data: RENDER_API_TYPE_SW.as_ptr() as *mut c_void },
                mpv_render_param { kind: PARAM_SW_SIZE, data: sz.as_mut_ptr() as *mut c_void },
                mpv_render_param { kind: PARAM_SW_FORMAT, data: c"rgb0".as_ptr() as *mut c_void },
                mpv_render_param { kind: PARAM_SW_STRIDE, data: &mut stride as *mut _ as *mut c_void },
                mpv_render_param { kind: PARAM_SW_POINTER, data: ptr as *mut c_void },
                mpv_render_param { kind: PARAM_BLOCK_FOR_TARGET_TIME, data: &mut block as *mut _ as *mut c_void },
                mpv_render_param { kind: PARAM_INVALID, data: std::ptr::null_mut() },
            ];
            if unsafe { (mpv.render_context_render)(rc, params.as_mut_ptr()) } < 0 {
                return;
            }
            let src = &st.buf[off..off + (w * h * 4) as usize];
            let mut pb = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(w, h);
            for (d, s) in pb.make_mut_slice().iter_mut().zip(src.chunks_exact(4)) {
                *d = slint::Rgb8Pixel { r: s[0], g: s[1], b: s[2] };
            }
            slint::Image::from_rgb8(pb)
        };
        (self.on_frame)(Some(img));
    }
}

impl Drop for VideoSurface {
    fn drop(&mut self) {
        // Normally already done in RenderingTeardown; a context must not outlive its callback target.
        if let Some(p) = self.st.get_mut().bound.take() {
            drop(p.take_render());
        }
    }
}

/// An OpenGL render context; `gpa` is only valid during this call, which is all mpv needs.
fn create_gl(p: &Player, gpa: &dyn Fn(&CStr) -> *const c_void) -> Result<(RenderContext, bool), String> {
    let mut init = mpv_opengl_init_params { get_proc_address: Some(gl_proc), get_proc_address_ctx: &gpa as *const _ as *mut c_void };
    let mut params = [
        mpv_render_param { kind: PARAM_API_TYPE, data: RENDER_API_TYPE_OPENGL.as_ptr() as *mut c_void },
        mpv_render_param { kind: PARAM_OPENGL_INIT_PARAMS, data: &mut init as *mut _ as *mut c_void },
        mpv_render_param { kind: PARAM_INVALID, data: std::ptr::null_mut() },
    ];
    create(p, &mut params).map(|rc| (rc, false))
}

fn create_sw(p: &Player) -> Result<(RenderContext, bool), String> {
    let mut params = [
        mpv_render_param { kind: PARAM_API_TYPE, data: RENDER_API_TYPE_SW.as_ptr() as *mut c_void },
        mpv_render_param { kind: PARAM_INVALID, data: std::ptr::null_mut() },
    ];
    create(p, &mut params).map(|rc| (rc, true))
}

fn create(p: &Player, params: &mut [mpv_render_param]) -> Result<RenderContext, String> {
    let mut ctx = std::ptr::null_mut();
    let r = unsafe { (p.mpv().render_context_create)(&mut ctx, p.handle(), params.as_mut_ptr()) };
    if r < 0 || ctx.is_null() {
        return Err(p.mpv().err(r));
    }
    Ok(unsafe { RenderContext::from_raw(p, ctx) })
}

/// GL state mpv may change and FemtoVG relies on (C1 gotcha 3).
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
const BLEND: [u32; 6] = [glow::BLEND_SRC_RGB, glow::BLEND_DST_RGB, glow::BLEND_SRC_ALPHA, glow::BLEND_DST_ALPHA, glow::BLEND_EQUATION_RGB, glow::BLEND_EQUATION_ALPHA];

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
        for (i, p) in BLEND.iter().enumerate() {
            s.blend[i] = gl.get_parameter_i32(*p);
        }
        s.unpack = [gl.get_parameter_i32(glow::UNPACK_ALIGNMENT), gl.get_parameter_i32(glow::UNPACK_ROW_LENGTH), gl.get_parameter_i32(glow::PIXEL_UNPACK_BUFFER_BINDING)];
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
