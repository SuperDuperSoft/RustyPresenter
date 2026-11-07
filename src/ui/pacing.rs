//! Frame pacing per window: only the room waits for its monitor's refresh.
//!
//! All windows paint on the one UI thread, one window per event-loop turn, and Slint 1.18's FemtoVG backend
//! makes every window's buffer swap wait for its monitor's vertical blank (`SwapInterval::Wait(1)` in
//! i-slint-backend-winit `glcontext.rs`). So each operator or stage frame could hold the thread for up to a whole
//! refresh while the room's next frame waited in line, and with two monitors out of step the waits add up
//! (`docs/superpowers/specs/2026-11-03-gpu-rendering-design.md`, root cause 1). Here the operator and stage get
//! swap interval 0: their swaps return at once (Windows and macOS composite every window, so they do not tear),
//! and Slint's frame throttle still caps how often they draw. The room keeps interval 1
//! (`VideoSurface::attach`), so the projector never tears.
//!
//! The swap interval belongs to the OpenGL context, which Slint makes current before the `RenderingSetup`
//! notification; the call is found through the context's `get_proc_address`. A platform without one keeps
//! waiting for vsync, and says so once.
use slint::{GraphicsAPI, RenderingState};
use std::{cell::RefCell, ffi::{CStr, c_void}, rc::Rc};

type Probe = Rc<dyn Fn(&'static str, &RenderingState, &GraphicsAPI)>;

thread_local! {
    static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) };
}

/// Diagnostics only (`examples/slide_bench.rs`): `f` hears every rendering-notifier event of a paced window,
/// with its label (`operator`, `stage`, `room`). Set it before the windows open.
#[doc(hidden)]
pub fn set_probe(f: impl Fn(&'static str, &RenderingState, &GraphicsAPI) + 'static) { PROBE.with(|p| *p.borrow_mut() = Some(Rc::new(f))) }

/// Tells the probe, if any, about `state` of the window called `label`.
fn probe(label: &'static str, state: &RenderingState, api: &GraphicsAPI) {
    let f = PROBE.with(|p| p.borrow().clone());
    if let Some(f) = f { f(label, state, api) }
}

/// The swap interval for a window: 1 waits for the monitor's refresh (vsync), 0 does not.
pub fn interval(vsync: bool) -> i32 { i32::from(vsync) }

/// Sets the swap interval of the OpenGL context that is current, through the platform's call as
/// `get_proc_address` finds it. Returns the call used, or `None` when there is none (the driver's default stays).
pub fn set_swap_interval(get_proc_address: &dyn Fn(&CStr) -> *const c_void, interval: i32) -> Option<&'static str> {
    let tries: &[(&'static str, Setter)] = if cfg!(target_os = "windows") {
        &[("wglSwapIntervalEXT", wgl), ("eglSwapInterval", egl)]
    } else if cfg!(target_vendor = "apple") {
        &[("CGLSetParameter", cgl)]
    } else {
        &[("eglSwapInterval", egl), ("glXSwapIntervalMESA", glx_mesa), ("glXSwapIntervalEXT", glx_ext)]
    };
    tries.iter().find(|(_, set)| set(get_proc_address, interval)).map(|(name, _)| *name)
}

/// One platform call: true when it was found and took effect.
type Setter = fn(&dyn Fn(&CStr) -> *const c_void, i32) -> bool;

/// A usable function pointer: `wglGetProcAddress` may return 1, 2, 3 or -1 instead of null for "not found".
pub(crate) fn found(p: *const c_void) -> Option<*const c_void> { (!matches!(p as isize, -1..=3)).then_some(p) }

fn wgl(gpa: &dyn Fn(&CStr) -> *const c_void, interval: i32) -> bool {
    let Some(f) = found(gpa(c"wglSwapIntervalEXT")) else { return false };
    let f: unsafe extern "system" fn(i32) -> i32 = unsafe { std::mem::transmute(f) };
    unsafe { f(interval) != 0 }
}

fn egl(gpa: &dyn Fn(&CStr) -> *const c_void, interval: i32) -> bool {
    let (Some(display), Some(swap)) = (found(gpa(c"eglGetCurrentDisplay")), found(gpa(c"eglSwapInterval"))) else { return false };
    let display: unsafe extern "system" fn() -> *mut c_void = unsafe { std::mem::transmute(display) };
    let swap: unsafe extern "system" fn(*mut c_void, i32) -> u32 = unsafe { std::mem::transmute(swap) };
    let d = unsafe { display() };
    !d.is_null() && unsafe { swap(d, interval) } != 0
}

fn glx_mesa(gpa: &dyn Fn(&CStr) -> *const c_void, interval: i32) -> bool {
    let Some(f) = found(gpa(c"glXSwapIntervalMESA")) else { return false };
    let f: unsafe extern "C" fn(u32) -> i32 = unsafe { std::mem::transmute(f) };
    unsafe { f(interval.max(0) as u32) == 0 }
}

fn glx_ext(gpa: &dyn Fn(&CStr) -> *const c_void, interval: i32) -> bool {
    let (Some(display), Some(drawable), Some(swap)) =
        (found(gpa(c"glXGetCurrentDisplay")), found(gpa(c"glXGetCurrentDrawable")), found(gpa(c"glXSwapIntervalEXT"))) else { return false };
    let display: unsafe extern "C" fn() -> *mut c_void = unsafe { std::mem::transmute(display) };
    let drawable: unsafe extern "C" fn() -> std::ffi::c_ulong = unsafe { std::mem::transmute(drawable) };
    let swap: unsafe extern "C" fn(*mut c_void, std::ffi::c_ulong, i32) = unsafe { std::mem::transmute(swap) };
    let (d, w) = unsafe { (display(), drawable()) };
    if d.is_null() || w == 0 { return false }
    unsafe { swap(d, w, interval) };
    true
}

/// `kCGLCPSwapInterval` in `CGLTypes.h`.
const CGL_SWAP_INTERVAL: i32 = 222;

fn cgl(gpa: &dyn Fn(&CStr) -> *const c_void, interval: i32) -> bool {
    let (Some(current), Some(set)) = (found(gpa(c"CGLGetCurrentContext")), found(gpa(c"CGLSetParameter"))) else { return false };
    let current: unsafe extern "C" fn() -> *mut c_void = unsafe { std::mem::transmute(current) };
    let set: unsafe extern "C" fn(*mut c_void, i32, *const i32) -> i32 = unsafe { std::mem::transmute(set) };
    let ctx = unsafe { current() };
    !ctx.is_null() && unsafe { set(ctx, CGL_SWAP_INTERVAL, &interval) } == 0
}

/// Called from a rendering notifier: at `RenderingSetup` with OpenGL, sets the swap interval for `vsync` (and logs
/// when it cannot); every event goes to the probe.
pub(crate) fn on_render(label: &'static str, vsync: bool, state: &RenderingState, api: &GraphicsAPI) {
    if let (RenderingState::RenderingSetup, GraphicsAPI::NativeOpenGL { get_proc_address }) = (state, api) {
        let i = interval(vsync);
        match set_swap_interval(*get_proc_address, i) {
            Some(call) => eprintln!("{label} screen: swap interval {i} ({call})"),
            None => eprintln!("{label} screen: no swap-interval call in this OpenGL driver; it keeps the driver's default"),
        }
    }
    super::perf::observe(label, state, api);
    probe(label, state, api);
}

/// Gives `window` a rendering notifier that sets its swap interval (`vsync` false: swaps never wait for the
/// monitor) once its OpenGL context exists. Call before the window is shown. False when the renderer has no
/// notifier (the software renderer, tests) or the window already has one.
pub fn attach(window: &slint::Window, label: &'static str, vsync: bool) -> bool {
    let vsync = vsync || vsync_everywhere();
    window.set_rendering_notifier(move |state, api| on_render(label, vsync, &state, api)).is_ok()
}

/// `RP_VSYNC_ALL=1`: every window waits for its monitor's refresh again, as before this module (to compare, or
/// if a driver misbehaves without it).
pub fn vsync_everywhere() -> bool { std::env::var("RP_VSYNC_ALL").as_deref() == Ok("1") }

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static SET: Cell<i32> = const { Cell::new(-5) };
    }
    unsafe extern "system" fn fake_wgl(i: i32) -> i32 { SET.with(|s| s.set(i)); 1 }
    unsafe extern "system" fn fake_egl_display() -> *mut c_void { 8 as *mut c_void }
    unsafe extern "system" fn fake_egl_swap(_d: *mut c_void, i: i32) -> u32 { SET.with(|s| s.set(i)); 1 }
    unsafe extern "C" fn fake_cgl_current() -> *mut c_void { 8 as *mut c_void }
    unsafe extern "C" fn fake_cgl_set(_c: *mut c_void, p: i32, v: *const i32) -> i32 { assert_eq!(p, CGL_SWAP_INTERVAL); SET.with(|s| s.set(unsafe { *v })); 0 }

    /// A `get_proc_address` that knows the fakes for this platform's calls.
    fn gpa(name: &CStr) -> *const c_void {
        match name.to_bytes() {
            b"wglSwapIntervalEXT" => fake_wgl as *const c_void,
            b"eglGetCurrentDisplay" => fake_egl_display as *const c_void,
            b"eglSwapInterval" => fake_egl_swap as *const c_void,
            b"CGLGetCurrentContext" => fake_cgl_current as *const c_void,
            b"CGLSetParameter" => fake_cgl_set as *const c_void,
            _ => std::ptr::null(),
        }
    }

    #[test]
    fn the_operator_does_not_wait_and_the_room_does() {
        assert_eq!((interval(false), interval(true)), (0, 1));
        for want in [0, 1] {
            let used = set_swap_interval(&gpa, want);
            assert!(used.is_some(), "a call was found");
            assert_eq!(SET.with(Cell::get), want);
        }
        if cfg!(windows) { assert_eq!(set_swap_interval(&gpa, 0), Some("wglSwapIntervalEXT")) }
    }

    #[test]
    fn a_driver_without_the_call_is_left_alone() {
        SET.with(|s| s.set(-5));
        assert_eq!(set_swap_interval(&|_| std::ptr::null(), 0), None);
        // wglGetProcAddress's "not found" values are not called.
        assert_eq!(set_swap_interval(&|_| 1 as *const c_void, 0), None);
        assert_eq!(set_swap_interval(&|_| -1isize as *const c_void, 0), None);
        assert_eq!(SET.with(Cell::get), -5);
    }

    #[test]
    fn the_probe_hears_each_window() {
        let heard: Rc<RefCell<Vec<&'static str>>> = Rc::default();
        let h = heard.clone();
        set_probe(move |label, state, _| if matches!(state, RenderingState::AfterRendering) { h.borrow_mut().push(label) });
        let api = GraphicsAPI::NativeOpenGL { get_proc_address: &gpa };
        on_render("operator", false, &RenderingState::AfterRendering, &api);
        on_render("stage", false, &RenderingState::BeforeRendering, &api);
        on_render("stage", false, &RenderingState::AfterRendering, &api);
        assert_eq!(*heard.borrow(), ["operator", "stage"]);
        PROBE.with(|p| p.borrow_mut().take());
    }
}
