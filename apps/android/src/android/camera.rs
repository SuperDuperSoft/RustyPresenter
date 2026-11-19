//! The phone's camera through Android's Camera2 (the NDK's camera library): the back or front camera, its
//! pictures read as they come (brightness and colour planes, `frames::Yuv`). Used for the Camera tab and for
//! reading the computer's QR code.
use crate::frames::Yuv;
use ndk::media::image_reader::{AcquireResult, ImageFormat, ImageReader};
use ndk_sys as ffi;
use std::{
    ffi::CStr,
    ptr::null_mut,
    sync::atomic::{AtomicBool, Ordering},
};

/// Set when Android takes the camera away (another app, or an error): the picture loop stops.
static LOST: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn on_disconnected(_: *mut std::ffi::c_void, _: *mut ffi::ACameraDevice) { LOST.store(true, Ordering::Relaxed) }
unsafe extern "C" fn on_error(_: *mut std::ffi::c_void, _: *mut ffi::ACameraDevice, _: std::os::raw::c_int) { LOST.store(true, Ordering::Relaxed) }
unsafe extern "C" fn on_session(_: *mut std::ffi::c_void, _: *mut ffi::ACameraCaptureSession) {}

/// YUV_420_888 in the camera's list of sizes.
const YUV: i32 = 0x23;

pub struct Camera {
    mgr: *mut ffi::ACameraManager,
    device: *mut ffi::ACameraDevice,
    session: *mut ffi::ACameraCaptureSession,
    request: *mut ffi::ACaptureRequest,
    target: *mut ffi::ACameraOutputTarget,
    output: *mut ffi::ACaptureSessionOutput,
    container: *mut ffi::ACaptureSessionOutputContainer,
    reader: ImageReader,
    /// How the camera is mounted, in degrees.
    pub sensor: i32,
    pub front: bool,
}

fn ok(s: ffi::camera_status_t, what: &str) -> Result<(), String> {
    if s == ffi::camera_status_t::ACAMERA_OK { Ok(()) } else { Err(format!("{what}: {}", s.0)) }
}

/// One entry of a camera's description.
unsafe fn entry(meta: *const ffi::ACameraMetadata, tag: ffi::acamera_metadata_tag) -> Option<ffi::ACameraMetadata_const_entry> {
    let mut e: ffi::ACameraMetadata_const_entry = unsafe { std::mem::zeroed() };
    (unsafe { ffi::ACameraMetadata_getConstEntry(meta, tag.0, &mut e) } == ffi::camera_status_t::ACAMERA_OK).then_some(e)
}

impl Camera {
    /// Opens the back camera (or the front one) at the size nearest `want` that it can give.
    pub fn open(back: bool, want: (u32, u32)) -> Result<Camera, String> {
        LOST.store(false, Ordering::Relaxed);
        unsafe {
            let mgr = ffi::ACameraManager_create();
            if mgr.is_null() { return Err("no camera manager".into()) }
            let mut list: *mut ffi::ACameraIdList = null_mut();
            if let Err(e) = ok(ffi::ACameraManager_getCameraIdList(mgr, &mut list), "camera list") { ffi::ACameraManager_delete(mgr); return Err(e) }
            // The wanted side first, then any other camera.
            let mut chosen: Option<(std::ffi::CString, i32, bool, Vec<(u32, u32)>)> = None;
            let ids = std::slice::from_raw_parts((*list).cameraIds, (*list).numCameras.max(0) as usize);
            for pass in 0..2 {
                for id in ids {
                    let mut meta: *mut ffi::ACameraMetadata = null_mut();
                    if ffi::ACameraManager_getCameraCharacteristics(mgr, *id, &mut meta) != ffi::camera_status_t::ACAMERA_OK { continue }
                    let facing = entry(meta, ffi::acamera_metadata_tag::ACAMERA_LENS_FACING).map(|e| *e.data.u8_).unwrap_or(1);
                    let front = facing == 0;
                    let sensor = entry(meta, ffi::acamera_metadata_tag::ACAMERA_SENSOR_ORIENTATION).map(|e| *e.data.i32_).unwrap_or(90);
                    let mut sizes = vec![];
                    if let Some(e) = entry(meta, ffi::acamera_metadata_tag::ACAMERA_SCALER_AVAILABLE_STREAM_CONFIGURATIONS) {
                        let v = std::slice::from_raw_parts(e.data.i32_, e.count as usize);
                        for c in v.chunks_exact(4) {
                            if c[0] == YUV && c[3] == 0 && c[1] > 0 && c[2] > 0 { sizes.push((c[1] as u32, c[2] as u32)) }
                        }
                    }
                    ffi::ACameraMetadata_free(meta);
                    if chosen.is_none() && (pass == 1 || front != back) && !sizes.is_empty() {
                        chosen = Some((CStr::from_ptr(*id).to_owned(), sensor, front, sizes));
                    }
                }
                if chosen.is_some() { break }
            }
            ffi::ACameraManager_deleteCameraIdList(list);
            let Some((id, sensor, front, sizes)) = chosen else { ffi::ACameraManager_delete(mgr); return Err("no camera".into()) };
            let (width, height) = crate::frames::choose_size(&sizes, want).unwrap_or((1280, 720));

            let mut cam = Camera {
                mgr, device: null_mut(), session: null_mut(), request: null_mut(), target: null_mut(), output: null_mut(), container: null_mut(),
                reader: ImageReader::new(width as i32, height as i32, ImageFormat::YUV_420_888, 3).map_err(|e| e.to_string())?,
                sensor, front,
            };
            let window = cam.reader.window().map_err(|e| e.to_string())?;
            let win = window.ptr().as_ptr();
            let mut dev_cb = ffi::ACameraDevice_StateCallbacks { context: null_mut(), onDisconnected: Some(on_disconnected), onError: Some(on_error) };
            ok(ffi::ACameraManager_openCamera(mgr, id.as_ptr(), &mut dev_cb, &mut cam.device), "open the camera")?;
            ok(ffi::ACaptureSessionOutputContainer_create(&mut cam.container), "outputs")?;
            ok(ffi::ACaptureSessionOutput_create(win, &mut cam.output), "output")?;
            ok(ffi::ACaptureSessionOutputContainer_add(cam.container, cam.output), "add output")?;
            ok(ffi::ACameraOutputTarget_create(win, &mut cam.target), "target")?;
            ok(ffi::ACameraDevice_createCaptureRequest(cam.device, ffi::ACameraDevice_request_template::TEMPLATE_RECORD, &mut cam.request), "request")?;
            ok(ffi::ACaptureRequest_addTarget(cam.request, cam.target), "add target")?;
            // A steady 30 pictures a second where the camera allows it.
            let fps = [30i32, 30];
            let _ = ffi::ACaptureRequest_setEntry_i32(cam.request, ffi::acamera_metadata_tag::ACAMERA_CONTROL_AE_TARGET_FPS_RANGE.0, 2, fps.as_ptr());
            let session_cb = ffi::ACameraCaptureSession_stateCallbacks { context: null_mut(), onClosed: Some(on_session), onReady: Some(on_session), onActive: Some(on_session) };
            ok(ffi::ACameraDevice_createCaptureSession(cam.device, cam.container, &session_cb, &mut cam.session), "session")?;
            let mut req = cam.request;
            ok(ffi::ACameraCaptureSession_setRepeatingRequest(cam.session, null_mut(), 1, &mut req, null_mut()), "start")?;
            drop(window);
            Ok(cam)
        }
    }

    /// Android took the camera away.
    pub fn lost(&self) -> bool { LOST.load(Ordering::Relaxed) }

    /// The newest picture, given to `f`; None when no new one has come yet.
    pub fn latest<R>(&self, f: impl FnOnce(&Yuv) -> R) -> Option<R> {
        let AcquireResult::Image(img) = self.reader.acquire_latest_image().ok()? else { return None };
        let (w, h) = (img.width().ok()? as usize, img.height().ok()? as usize);
        let y = img.plane_data(0).ok()?;
        let u = img.plane_data(1).ok()?;
        let v = img.plane_data(2).ok()?;
        let frame = Yuv {
            w, h, y, y_row: img.plane_row_stride(0).ok()? as usize, u, v,
            uv_row: img.plane_row_stride(1).ok()? as usize, uv_pixel: img.plane_pixel_stride(1).ok()? as usize,
        };
        Some(f(&frame))
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        unsafe {
            if !self.session.is_null() { ffi::ACameraCaptureSession_stopRepeating(self.session); ffi::ACameraCaptureSession_close(self.session); }
            if !self.request.is_null() { ffi::ACaptureRequest_free(self.request) }
            if !self.target.is_null() { ffi::ACameraOutputTarget_free(self.target) }
            if !self.device.is_null() { ffi::ACameraDevice_close(self.device); }
            if !self.output.is_null() { ffi::ACaptureSessionOutput_free(self.output) }
            if !self.container.is_null() { ffi::ACaptureSessionOutputContainer_free(self.container) }
            ffi::ACameraManager_delete(self.mgr);
        }
    }
}
