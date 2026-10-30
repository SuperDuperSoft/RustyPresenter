//! Minimal hand binding for the libmpv functions this check needs, loaded at runtime with
//! libloading (libmpv is never linked at build time). Constants mirror client.h / render.h /
//! render_gl.h from mpv client API 2.x.
#![allow(non_camel_case_types, dead_code)]
use std::ffi::{CStr, CString, c_char, c_int, c_void};

pub type mpv_handle = c_void;
pub type mpv_render_context = c_void;

#[repr(C)]
pub struct mpv_render_param {
    pub kind: c_int,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_opengl_init_params {
    pub get_proc_address: Option<unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *mut c_void>,
    pub get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
pub struct mpv_opengl_fbo {
    pub fbo: c_int,
    pub w: c_int,
    pub h: c_int,
    pub internal_format: c_int,
}

pub const PARAM_INVALID: c_int = 0;
pub const PARAM_API_TYPE: c_int = 1;
pub const PARAM_OPENGL_INIT_PARAMS: c_int = 2;
pub const PARAM_OPENGL_FBO: c_int = 3;
pub const PARAM_FLIP_Y: c_int = 4;
pub const PARAM_BLOCK_FOR_TARGET_TIME: c_int = 12;
pub const PARAM_SW_SIZE: c_int = 17;
pub const PARAM_SW_FORMAT: c_int = 18;
pub const PARAM_SW_STRIDE: c_int = 19;
pub const PARAM_SW_POINTER: c_int = 20;
pub const UPDATE_FRAME: u64 = 1;

type UpdateCb = unsafe extern "C" fn(*mut c_void);

pub struct Lib {
    _lib: libloading::Library,
    pub client_api_version: unsafe extern "C" fn() -> std::ffi::c_ulong,
    pub create: unsafe extern "C" fn() -> *mut mpv_handle,
    pub initialize: unsafe extern "C" fn(*mut mpv_handle) -> c_int,
    pub set_option_string: unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int,
    pub command: unsafe extern "C" fn(*mut mpv_handle, *mut *const c_char) -> c_int,
    pub get_property_string: unsafe extern "C" fn(*mut mpv_handle, *const c_char) -> *mut c_char,
    pub free: unsafe extern "C" fn(*mut c_void),
    pub error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    pub render_context_create:
        unsafe extern "C" fn(*mut *mut mpv_render_context, *mut mpv_handle, *mut mpv_render_param) -> c_int,
    pub render_context_set_update_callback: unsafe extern "C" fn(*mut mpv_render_context, Option<UpdateCb>, *mut c_void),
    pub render_context_update: unsafe extern "C" fn(*mut mpv_render_context) -> u64,
    pub render_context_render: unsafe extern "C" fn(*mut mpv_render_context, *mut mpv_render_param) -> c_int,
    pub render_context_report_swap: unsafe extern "C" fn(*mut mpv_render_context),
    pub render_context_free: unsafe extern "C" fn(*mut mpv_render_context),
    pub wait_event: unsafe extern "C" fn(*mut mpv_handle, f64) -> *mut mpv_event,
    pub terminate_destroy: unsafe extern "C" fn(*mut mpv_handle),
}

#[repr(C)]
pub struct mpv_event {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}
pub const EVENT_SHUTDOWN: c_int = 1;
pub const EVENT_END_FILE: c_int = 7;

impl Lib {
    pub fn load(path: &str) -> Result<Lib, libloading::Error> {
        unsafe {
            let lib = libloading::Library::new(path)?;
            macro_rules! sym {
                ($name:literal) => {
                    *lib.get(concat!($name, "\0").as_bytes())?
                };
            }
            Ok(Lib {
                client_api_version: sym!("mpv_client_api_version"),
                create: sym!("mpv_create"),
                initialize: sym!("mpv_initialize"),
                set_option_string: sym!("mpv_set_option_string"),
                command: sym!("mpv_command"),
                get_property_string: sym!("mpv_get_property_string"),
                free: sym!("mpv_free"),
                error_string: sym!("mpv_error_string"),
                render_context_create: sym!("mpv_render_context_create"),
                render_context_set_update_callback: sym!("mpv_render_context_set_update_callback"),
                render_context_update: sym!("mpv_render_context_update"),
                render_context_render: sym!("mpv_render_context_render"),
                render_context_report_swap: sym!("mpv_render_context_report_swap"),
                render_context_free: sym!("mpv_render_context_free"),
                wait_event: sym!("mpv_wait_event"),
                terminate_destroy: sym!("mpv_terminate_destroy"),
                _lib: lib,
            })
        }
    }

    pub fn err(&self, code: c_int) -> String {
        unsafe { CStr::from_ptr((self.error_string)(code)).to_string_lossy().into_owned() }
    }

    pub fn set_option(&self, h: *mut mpv_handle, k: &str, v: &str) {
        let (k2, v2) = (CString::new(k).unwrap(), CString::new(v).unwrap());
        let r = unsafe { (self.set_option_string)(h, k2.as_ptr(), v2.as_ptr()) };
        if r < 0 {
            eprintln!("mpv option {k}={v} failed: {}", self.err(r));
        }
    }

    pub fn command(&self, h: *mut mpv_handle, args: &[&str]) -> c_int {
        let owned: Vec<CString> = args.iter().map(|a| CString::new(*a).unwrap()).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        unsafe { (self.command)(h, ptrs.as_mut_ptr()) }
    }

    pub fn get(&self, h: *mut mpv_handle, name: &str) -> String {
        let n = CString::new(name).unwrap();
        unsafe {
            let p = (self.get_property_string)(h, n.as_ptr());
            if p.is_null() {
                return "(unavailable)".into();
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.free)(p as *mut c_void);
            s
        }
    }
}
