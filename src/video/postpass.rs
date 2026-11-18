//! N20: a screen's colour fix and corner pin, applied to the whole finished frame (slides, pictures and video alike)
//! after Slint has drawn it and before it is shown. OpenGL only: the frame is copied to a texture and drawn back
//! through one small shader (`ScreenFix::colour` and `ScreenFix::screen_to_picture` say what it does). One copy and
//! one full-screen draw per frame; nothing at all while the fix is the default.
use super::surface::{restore_gl, save_gl};
use crate::screen_fix::ScreenFix;
use glow::HasContext;
use slint::{GraphicsAPI, PhysicalSize, RenderingState};
use std::sync::{Arc, Mutex};

const VERTEX: &str = "
out vec2 v_pos;
void main() {
    // One triangle over the whole screen, from the vertex number alone.
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    v_pos = p;
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}";

const FRAGMENT: &str = "
in vec2 v_pos;
out vec4 o_color;
uniform sampler2D u_frame;
uniform mat3 u_warp;
uniform vec3 u_gain;
uniform float u_gamma;
uniform float u_black;
void main() {
    // The screen point with y down, as `ScreenFix` counts it.
    vec3 s = u_warp * vec3(v_pos.x, 1.0 - v_pos.y, 1.0);
    vec2 uv = s.xy / s.z;
    if (s.z <= 0.0 || uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) { o_color = vec4(0.0, 0.0, 0.0, 1.0); return; }
    vec3 c = texture(u_frame, vec2(uv.x, 1.0 - uv.y)).rgb;
    c = pow(clamp(c * u_gain, 0.0, 1.0), vec3(1.0 / u_gamma));
    o_color = vec4(u_black + (1.0 - u_black) * c, 1.0);
}";

/// The fix a window follows, shared with whoever changes it.
pub type SharedFix = Arc<Mutex<ScreenFix>>;

struct Gl { program: glow::Program, vao: glow::VertexArray, tex: glow::Texture, size: (i32, i32) }

/// The pass's GL objects, made on first use in the window's context.
#[derive(Default)]
pub struct PostPass { gl: Option<Gl>, failed: bool }

fn header(gl: &glow::Context) -> &'static str {
    if gl.version().is_embedded { "#version 300 es\nprecision highp float;\n" } else { "#version 150\n" }
}

unsafe fn compile(gl: &glow::Context) -> Result<(glow::Program, glow::VertexArray), String> {
    unsafe {
        let program = gl.create_program()?;
        for (kind, body) in [(glow::VERTEX_SHADER, VERTEX), (glow::FRAGMENT_SHADER, FRAGMENT)] {
            let sh = gl.create_shader(kind)?;
            gl.shader_source(sh, &format!("{}{body}", header(gl)));
            gl.compile_shader(sh);
            if !gl.get_shader_compile_status(sh) { return Err(gl.get_shader_info_log(sh)) }
            gl.attach_shader(program, sh);
            gl.delete_shader(sh);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) { return Err(gl.get_program_info_log(program)) }
        Ok((program, gl.create_vertex_array()?))
    }
}

impl PostPass {
    /// Runs the fix over the frame just drawn into the bound framebuffer. Call with the window's GL context current
    /// (the rendering notifier's `AfterRendering`). A pass that cannot be made is turned off and said once.
    ///
    /// # Safety
    /// `gl` must be the current context.
    pub unsafe fn run(&mut self, gl: &glow::Context, size: PhysicalSize, fix: &ScreenFix) {
        if !fix.needs_pass() || self.failed || size.width == 0 || size.height == 0 { return }
        let Some(warp) = fix.screen_to_picture() else { return };
        unsafe {
            if self.gl.is_none() {
                match compile(gl).and_then(|(program, vao)| Ok(Gl { program, vao, tex: gl.create_texture()?, size: (0, 0) })) {
                    Ok(g) => self.gl = Some(g),
                    Err(e) => { eprintln!("The screen colour and corner fix cannot be drawn here: {e}"); self.failed = true; return }
                }
            }
            let Some(g) = self.gl.as_mut() else { return };
            let saved = save_gl(gl);
            let (w, h) = (size.width as i32, size.height as i32);
            let fbo = std::num::NonZeroU32::new(saved.draw_fbo as u32).map(glow::NativeFramebuffer);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, fbo);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(g.tex));
            if g.size != (w, h) {
                gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, w, h, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(None));
                for (p, v) in [(glow::TEXTURE_MIN_FILTER, glow::LINEAR), (glow::TEXTURE_MAG_FILTER, glow::LINEAR), (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE), (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE)] {
                    gl.tex_parameter_i32(glow::TEXTURE_2D, p, v as i32);
                }
                g.size = (w, h);
            }
            gl.copy_tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, 0, 0, w, h);
            for c in [glow::SCISSOR_TEST, glow::BLEND, glow::CULL_FACE, glow::DEPTH_TEST, glow::STENCIL_TEST] { gl.disable(c) }
            gl.viewport(0, 0, w, h);
            gl.use_program(Some(g.program));
            gl.bind_vertex_array(Some(g.vao));
            let at = |n: &str| gl.get_uniform_location(g.program, n);
            gl.uniform_1_i32(at("u_frame").as_ref(), 0);
            // Row major to GL's column major.
            let m = warp;
            gl.uniform_matrix_3_f32_slice(at("u_warp").as_ref(), false, &[m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]]);
            gl.uniform_3_f32(at("u_gain").as_ref(), fix.red, fix.green, fix.blue);
            gl.uniform_1_f32(at("u_gamma").as_ref(), fix.gamma);
            gl.uniform_1_f32(at("u_black").as_ref(), fix.black);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            restore_gl(gl, &saved);
        }
    }

    /// Frees the GL objects (the context is going away). Call with it current.
    ///
    /// # Safety
    /// `gl` must be the context the pass was made in, current.
    pub unsafe fn free(&mut self, gl: &glow::Context) {
        if let Some(g) = self.gl.take() {
            unsafe {
                gl.delete_program(g.program);
                gl.delete_vertex_array(g.vao);
                gl.delete_texture(g.tex);
            }
        }
        self.failed = false;
    }
}

/// A window with no video (the stage): its rendering notifier keeps its frame pacing (`ui::pacing`) and runs the
/// fix. False when the window has no notifier (software drawing): then there is no fix.
pub fn attach(window: &slint::Window, label: &'static str, vsync: bool, fix: SharedFix) -> bool {
    let vsync = vsync || crate::ui::pacing::vsync_everywhere();
    let mut gl: Option<glow::Context> = None;
    let mut pass = PostPass::default();
    window.set_rendering_notifier(move |state, api| {
        crate::ui::pacing::on_render(label, vsync, &state, api);
        match state {
            RenderingState::RenderingSetup => if let GraphicsAPI::NativeOpenGL { get_proc_address } = api {
                gl = Some(unsafe { glow::Context::from_loader_function_cstr(|n| get_proc_address(n)) });
            },
            RenderingState::AfterRendering => if let Some(gl) = gl.as_ref() {
                let f = *fix.lock().unwrap();
                let mut vp = [0i32; 4];
                unsafe { gl.get_parameter_i32_slice(glow::VIEWPORT, &mut vp) };
                unsafe { pass.run(gl, PhysicalSize::new(vp[2].max(0) as u32, vp[3].max(0) as u32), &f) }
            },
            RenderingState::RenderingTeardown => if let Some(gl) = gl.take() { unsafe { pass.free(&gl) } },
            _ => {}
        }
    }).is_ok()
}
