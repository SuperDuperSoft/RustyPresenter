//! Reads the room's finished OpenGL frame (video, slide, overlays and fades, exactly as the projector gets it)
//! right after Slint draws it, before the buffers swap (`VideoSurface::set_frame_tap`). OpenGL 3 and newer scale
//! it on the graphics card into a picture of the recording's size, upside down so it reads top row first; older
//! OpenGL, or a multisampled window, reads the whole window and scales it on the CPU.
use super::pump::{FrameSink, Raw};
use glow::HasContext;
use slint::PhysicalSize;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct GlCapture {
    sink: Arc<FrameSink>,
    /// The picture of the recording's size, and its framebuffer (OpenGL 3+).
    target: Option<(glow::Framebuffer, glow::Renderbuffer)>,
    /// The fast way failed once (or cannot work here): read the window instead.
    slow: bool,
    last: Option<Instant>,
    /// At most one picture per frame of the file.
    gap: Duration,
    /// A frame skipped for coming too soon asks for one more drawing, so the last picture of a change is never lost.
    redraw_later: Box<dyn Fn(Duration)>,
    owed: bool,
    full: Vec<u8>,
}

impl GlCapture {
    pub fn new(sink: Arc<FrameSink>, fps: u32, redraw_later: impl Fn(Duration) + 'static) -> GlCapture {
        GlCapture { sink, target: None, slow: false, last: None, gap: Duration::from_secs_f64(1.0 / f64::from(fps.max(1))), redraw_later: Box::new(redraw_later), owed: false, full: Vec::new() }
    }

    pub fn sink(&self) -> &Arc<FrameSink> { &self.sink }

    /// Called after each room frame, with the room's GL context current and its frame still in the back buffer.
    pub fn frame(&mut self, gl: &glow::Context, size: PhysicalSize) {
        if self.sink.is_closed() || size.width == 0 || size.height == 0 { return }
        let now = Instant::now();
        if let Some(last) = self.last && now.duration_since(last) < self.gap {
            if !self.owed {
                self.owed = true;
                (self.redraw_later)(self.gap - now.duration_since(last));
            }
            return;
        }
        self.last = Some(now);
        self.owed = false;
        if !self.slow && !self.fast(gl, size) {
            eprintln!("recording: reading the room on the CPU (no OpenGL 3 scaling here)");
            self.slow = true;
        }
        if self.slow { self.read_window(gl, size) }
    }

    /// Scales the window into `target` on the graphics card and reads that. False when it cannot work here.
    fn fast(&mut self, gl: &glow::Context, size: PhysicalSize) -> bool {
        let v = gl.version();
        if v.major < 3 { return false }
        let (w, h) = self.sink.size;
        unsafe {
            let read_was = gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING);
            let draw_was = gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING);
            // A multisampled window cannot be scaled in one blit.
            if gl.get_parameter_i32(glow::SAMPLES) > 0 { return false }
            if self.target.is_none() {
                let (Ok(fb), Ok(rb)) = (gl.create_framebuffer(), gl.create_renderbuffer()) else { return false };
                gl.bind_renderbuffer(glow::RENDERBUFFER, Some(rb));
                gl.renderbuffer_storage(glow::RENDERBUFFER, glow::RGBA8, w as i32, h as i32);
                gl.bind_renderbuffer(glow::RENDERBUFFER, None);
                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(fb));
                gl.framebuffer_renderbuffer(glow::DRAW_FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::RENDERBUFFER, Some(rb));
                let ok = gl.check_framebuffer_status(glow::DRAW_FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE;
                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, framebuffer(draw_was));
                self.target = Some((fb, rb));
                if !ok { return false }
            }
            let Some((fb, _)) = self.target else { return false };
            while gl.get_error() != glow::NO_ERROR {}
            let scissor = gl.is_enabled(glow::SCISSOR_TEST);
            gl.disable(glow::SCISSOR_TEST);
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(fb));
            let mut clear_was = [0.0f32; 4];
            gl.get_parameter_f32_slice(glow::COLOR_CLEAR_VALUE, &mut clear_was);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.clear_color(clear_was[0], clear_was[1], clear_was[2], clear_was[3]);
            // Upside down into the target, so reading it gives the top row first; black bars keep the shape.
            let (x, y, fw, fh) = super::pump::fit_rect(size.width, size.height, w, h);
            let (x0, x1) = (x as i32, (x + fw) as i32);
            let (y_top, y_bottom) = ((h - y) as i32, (h - y - fh) as i32);
            gl.blit_framebuffer(0, 0, size.width as i32, size.height as i32, x0, y_top, x1, y_bottom, glow::COLOR_BUFFER_BIT, glow::LINEAR);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(fb));
            let mut buf = self.sink.buffer();
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, 4);
            gl.read_pixels(0, 0, w as i32, h as i32, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(&mut buf)));
            let ok = gl.get_error() == glow::NO_ERROR;
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, framebuffer(read_was));
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, framebuffer(draw_was));
            if scissor { gl.enable(glow::SCISSOR_TEST) }
            if !ok { return false }
            // The alpha OpenGL leaves is the window's, not ours to keep.
            for a in buf.iter_mut().skip(3).step_by(4) { *a = 255 }
            self.sink.put(buf);
        }
        true
    }

    fn read_window(&mut self, gl: &glow::Context, size: PhysicalSize) {
        let (w, h) = (size.width, size.height);
        self.full.resize(w as usize * h as usize * 4, 0);
        unsafe {
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, 4);
            gl.read_pixels(0, 0, w as i32, h as i32, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(&mut self.full)));
        }
        self.sink.put_raw(&Raw { w, h, bottom_up: true, rgba: &self.full });
    }

    /// Frees the picture; call with the GL context current (after the last frame, or as the context goes).
    pub fn free(&mut self, gl: &glow::Context) {
        if let Some((fb, rb)) = self.target.take() {
            unsafe {
                gl.delete_framebuffer(fb);
                gl.delete_renderbuffer(rb);
            }
        }
    }
}

/// A framebuffer binding as `get_parameter_i32` gave it: 0 is the window's own.
fn framebuffer(id: i32) -> Option<glow::Framebuffer> {
    std::num::NonZeroU32::new(id as u32).map(glow::NativeFramebuffer)
}
