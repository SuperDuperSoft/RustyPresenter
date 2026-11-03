//! Runtime binding to the libmpv client API 2.x (client.h, render.h, render_gl.h).
//! libmpv is LGPL and is only loaded with `libloading`, never linked. A missing
//! library is not fatal: callers report [`MISSING`] for video/audio items.
#![allow(non_camel_case_types)]
use crate::platform::resources::{current_exe, native_candidates};
use std::ffi::{CStr, CString, c_char, c_int, c_ulong, c_void};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

pub const MISSING: &str = "The video player is missing. Reinstall RustyPresenter.";

/// Library names tried in each folder, then through the system search path.
#[cfg(windows)]
pub const SYSTEM_NAMES: &[&str] = &["libmpv-2.dll", "mpv-2.dll"];
#[cfg(target_os = "macos")]
pub const SYSTEM_NAMES: &[&str] = &["libmpv.2.dylib", "libmpv.dylib"];
#[cfg(not(any(windows, target_os = "macos")))]
pub const SYSTEM_NAMES: &[&str] = &["libmpv.so.2", "libmpv.so"];

pub type mpv_handle = c_void;
pub type mpv_render_context = c_void;
pub type mpv_format = c_int;

pub const FORMAT_NONE: mpv_format = 0;
pub const FORMAT_STRING: mpv_format = 1;
pub const FORMAT_OSD_STRING: mpv_format = 2;
pub const FORMAT_FLAG: mpv_format = 3;
pub const FORMAT_INT64: mpv_format = 4;
pub const FORMAT_DOUBLE: mpv_format = 5;
pub const FORMAT_NODE: mpv_format = 6;
pub const FORMAT_NODE_ARRAY: mpv_format = 7;
pub const FORMAT_NODE_MAP: mpv_format = 8;
pub const FORMAT_BYTE_ARRAY: mpv_format = 9;

pub const EVENT_NONE: c_int = 0;
pub const EVENT_SHUTDOWN: c_int = 1;
pub const EVENT_LOG_MESSAGE: c_int = 2;
pub const EVENT_GET_PROPERTY_REPLY: c_int = 3;
pub const EVENT_SET_PROPERTY_REPLY: c_int = 4;
pub const EVENT_COMMAND_REPLY: c_int = 5;
pub const EVENT_START_FILE: c_int = 6;
pub const EVENT_END_FILE: c_int = 7;
pub const EVENT_FILE_LOADED: c_int = 8;
pub const EVENT_VIDEO_RECONFIG: c_int = 17;
pub const EVENT_AUDIO_RECONFIG: c_int = 18;
pub const EVENT_SEEK: c_int = 20;
pub const EVENT_PLAYBACK_RESTART: c_int = 21;
pub const EVENT_PROPERTY_CHANGE: c_int = 22;
pub const EVENT_QUEUE_OVERFLOW: c_int = 24;

pub const END_FILE_EOF: c_int = 0;
pub const END_FILE_STOP: c_int = 2;
pub const END_FILE_QUIT: c_int = 3;
pub const END_FILE_ERROR: c_int = 4;
pub const END_FILE_REDIRECT: c_int = 5;

pub const ERROR_SUCCESS: c_int = 0;
pub const ERROR_INVALID_PARAMETER: c_int = -4;
pub const ERROR_OPTION_NOT_FOUND: c_int = -5;

pub const PARAM_INVALID: c_int = 0;
pub const PARAM_API_TYPE: c_int = 1;
pub const PARAM_OPENGL_INIT_PARAMS: c_int = 2;
pub const PARAM_OPENGL_FBO: c_int = 3;
pub const PARAM_FLIP_Y: c_int = 4;
pub const PARAM_ADVANCED_CONTROL: c_int = 10;
pub const PARAM_BLOCK_FOR_TARGET_TIME: c_int = 12;
pub const PARAM_SKIP_RENDERING: c_int = 13;
pub const PARAM_SW_SIZE: c_int = 17;
pub const PARAM_SW_FORMAT: c_int = 18;
pub const PARAM_SW_STRIDE: c_int = 19;
pub const PARAM_SW_POINTER: c_int = 20;
pub const RENDER_API_TYPE_OPENGL: &CStr = c"opengl";
pub const RENDER_API_TYPE_SW: &CStr = c"sw";
pub const UPDATE_FRAME: u64 = 1;

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

#[repr(C)]
pub struct mpv_event {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_event_property {
    pub name: *const c_char,
    pub format: mpv_format,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_event_end_file {
    pub reason: c_int,
    pub error: c_int,
    pub playlist_entry_id: i64,
    pub playlist_insert_id: i64,
    pub playlist_insert_num_entries: c_int,
}

/// `mpv_node`: `format` says which member of `u` is valid.
#[repr(C)]
pub struct mpv_node {
    pub u: mpv_node_value,
    pub format: mpv_format,
}

#[repr(C)]
pub union mpv_node_value {
    pub string: *mut c_char,
    pub flag: c_int,
    pub int64: i64,
    pub double: f64,
    pub list: *mut mpv_node_list,
    pub ba: *mut mpv_byte_array,
}

#[repr(C)]
pub struct mpv_node_list {
    pub num: c_int,
    pub values: *mut mpv_node,
    pub keys: *mut *mut c_char,
}

#[repr(C)]
pub struct mpv_byte_array {
    pub data: *mut c_void,
    pub size: usize,
}

/// `mpv_event.data` of `EVENT_COMMAND_REPLY`. mpv owns the result: it stays valid only
/// until the next `wait_event` and must not be passed to `mpv_free_node_contents`.
#[repr(C)]
pub struct mpv_event_command {
    pub result: mpv_node,
}

/// An `mpv_node` copied out of libmpv, so it stays valid after the original is freed.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    None,
    Str(String),
    Flag(bool),
    Int(i64),
    Double(f64),
    Array(Vec<Node>),
    Map(Vec<(String, Node)>),
    Bytes(Vec<u8>),
}

impl Node {
    /// The value under `key` of a map.
    pub fn get(&self, key: &str) -> Option<&Node> {
        let Node::Map(entries) = self else { return None };
        entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Node::Double(v) => Some(v),
            Node::Int(v) => Some(v as f64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        if let Node::Str(s) = self { Some(s) } else { None }
    }

    pub fn as_bool(&self) -> Option<bool> {
        if let Node::Flag(b) = *self { Some(b) } else { None }
    }

    /// Deep copy of a node.
    ///
    /// # Safety
    /// `n` must be a valid node from libmpv (or laid out the same way).
    pub unsafe fn from_raw(n: *const mpv_node) -> Node {
        unsafe {
            let n = &*n;
            let text = |p: *const c_char| if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().into_owned() };
            let values = |l: &mpv_node_list| (0..l.num.max(0) as usize).map(|i| Node::from_raw(l.values.add(i))).collect::<Vec<_>>();
            match n.format {
                FORMAT_STRING | FORMAT_OSD_STRING => Node::Str(text(n.u.string)),
                FORMAT_FLAG => Node::Flag(n.u.flag != 0),
                FORMAT_INT64 => Node::Int(n.u.int64),
                FORMAT_DOUBLE => Node::Double(n.u.double),
                FORMAT_NODE_ARRAY => n.u.list.as_ref().map_or(Node::None, |l| Node::Array(values(l))),
                FORMAT_NODE_MAP => n.u.list.as_ref().map_or(Node::None, |l| {
                    let keys = (0..l.num.max(0) as usize).map(|i| text(*l.keys.add(i)));
                    Node::Map(keys.zip(values(l)).collect())
                }),
                FORMAT_BYTE_ARRAY => match n.u.ba.as_ref() {
                    Some(ba) if !ba.data.is_null() => Node::Bytes(std::slice::from_raw_parts(ba.data as *const u8, ba.size).to_vec()),
                    _ => Node::None,
                },
                _ => Node::None,
            }
        }
    }
}

pub type UpdateCb = unsafe extern "C" fn(*mut c_void);

/// What a libmpv says about itself. All empty until read, and if reading failed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Versions {
    /// `mpv-version`, such as "mpv 0.41.0".
    pub mpv: String,
    /// `ffmpeg-version`.
    pub ffmpeg: String,
    /// `mpv-configuration`: the options the library was built with.
    pub configuration: String,
}

/// The loaded libmpv: raw entry points plus small helpers. Shared across threads
/// (the client API is thread-safe).
pub struct Mpv {
    pub client_api_version: unsafe extern "C" fn() -> c_ulong,
    pub create: unsafe extern "C" fn() -> *mut mpv_handle,
    pub initialize: unsafe extern "C" fn(*mut mpv_handle) -> c_int,
    pub terminate_destroy: unsafe extern "C" fn(*mut mpv_handle),
    pub set_option_string: unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int,
    pub command: unsafe extern "C" fn(*mut mpv_handle, *mut *const c_char) -> c_int,
    pub command_async: unsafe extern "C" fn(*mut mpv_handle, u64, *mut *const c_char) -> c_int,
    pub command_ret: unsafe extern "C" fn(*mut mpv_handle, *mut *const c_char, *mut mpv_node) -> c_int,
    pub free_node_contents: unsafe extern "C" fn(*mut mpv_node),
    pub request_log_messages_raw: unsafe extern "C" fn(*mut mpv_handle, *const c_char) -> c_int,
    pub set_property_string: unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int,
    pub get_property: unsafe extern "C" fn(*mut mpv_handle, *const c_char, mpv_format, *mut c_void) -> c_int,
    pub get_property_string: unsafe extern "C" fn(*mut mpv_handle, *const c_char) -> *mut c_char,
    pub observe_property: unsafe extern "C" fn(*mut mpv_handle, u64, *const c_char, mpv_format) -> c_int,
    pub wait_event: unsafe extern "C" fn(*mut mpv_handle, f64) -> *mut mpv_event,
    /// Wakes a thread blocked in `wait_event` (used to stop event threads).
    pub wakeup: unsafe extern "C" fn(*mut mpv_handle),
    pub free: unsafe extern "C" fn(*mut c_void),
    pub error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    pub render_context_create:
        unsafe extern "C" fn(*mut *mut mpv_render_context, *mut mpv_handle, *mut mpv_render_param) -> c_int,
    pub render_context_set_update_callback: unsafe extern "C" fn(*mut mpv_render_context, Option<UpdateCb>, *mut c_void),
    pub render_context_update: unsafe extern "C" fn(*mut mpv_render_context) -> u64,
    pub render_context_render: unsafe extern "C" fn(*mut mpv_render_context, *mut mpv_render_param) -> c_int,
    pub render_context_report_swap: unsafe extern "C" fn(*mut mpv_render_context),
    pub render_context_free: unsafe extern "C" fn(*mut mpv_render_context),
    path: PathBuf,
    /// `mpv-version` and `ffmpeg-version`, read once by [`Mpv::versions`].
    versions: OnceLock<Versions>,
    _lib: libloading::Library,
}

impl Mpv {
    /// Finds and loads libmpv: see `platform::resources::native_candidates` for where it looks.
    /// The first library that loads decides; a too-old one is an error, not skipped.
    pub fn load() -> Result<Arc<Mpv>, String> {
        let data = crate::server::data_dir().unwrap_or_default();
        let mut first_err = None;
        for p in Mpv::candidates(&data) {
            // A full path is only worth trying when the file exists; bare names use the system search.
            if p.components().count() > 1 && !p.is_file() {
                continue;
            }
            match Mpv::load_from(&p) {
                Ok(m) => return Ok(m),
                Err(e) if e.contains("too old") => return Err(e),
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        if let Some(e) = first_err {
            eprintln!("libmpv not loaded: {e}");
        }
        Err(MISSING.into())
    }

    /// Where `load` looks, in order.
    pub fn candidates(data_dir: &Path) -> Vec<PathBuf> {
        native_candidates(SYSTEM_NAMES, "RP_LIBMPV", &current_exe(), data_dir, "mpv", &|k| std::env::var_os(k))
    }

    pub fn load_from(path: &Path) -> Result<Arc<Mpv>, String> {
        let fail = |e: &dyn std::fmt::Display| format!("Cannot load the video player from {}: {e}", path.display());
        let lib = open(path).map_err(|e| fail(&e))?;
        let mpv = unsafe {
            macro_rules! sym {
                ($name:literal) => {
                    *lib.get(concat!($name, "\0").as_bytes()).map_err(|e| fail(&e))?
                };
            }
            Mpv {
                client_api_version: sym!("mpv_client_api_version"),
                create: sym!("mpv_create"),
                initialize: sym!("mpv_initialize"),
                terminate_destroy: sym!("mpv_terminate_destroy"),
                set_option_string: sym!("mpv_set_option_string"),
                command: sym!("mpv_command"),
                command_async: sym!("mpv_command_async"),
                command_ret: sym!("mpv_command_ret"),
                free_node_contents: sym!("mpv_free_node_contents"),
                request_log_messages_raw: sym!("mpv_request_log_messages"),
                set_property_string: sym!("mpv_set_property_string"),
                get_property: sym!("mpv_get_property"),
                get_property_string: sym!("mpv_get_property_string"),
                observe_property: sym!("mpv_observe_property"),
                wait_event: sym!("mpv_wait_event"),
                wakeup: sym!("mpv_wakeup"),
                free: sym!("mpv_free"),
                error_string: sym!("mpv_error_string"),
                render_context_create: sym!("mpv_render_context_create"),
                render_context_set_update_callback: sym!("mpv_render_context_set_update_callback"),
                render_context_update: sym!("mpv_render_context_update"),
                render_context_render: sym!("mpv_render_context_render"),
                render_context_report_swap: sym!("mpv_render_context_report_swap"),
                render_context_free: sym!("mpv_render_context_free"),
                path: path.to_path_buf(),
                versions: OnceLock::new(),
                _lib: lib,
            }
        };
        let (major, minor) = mpv.api_version();
        if major != 2 {
            return Err(format!("This libmpv is too old (client API {major}.{minor}); RustyPresenter needs 2.x."));
        }
        Ok(Arc::new(mpv))
    }

    /// Client API version as (major, minor).
    pub fn api_version(&self) -> (u32, u32) {
        let v = unsafe { (self.client_api_version)() } as u64;
        ((v >> 16) as u32, (v & 0xffff) as u32)
    }

    /// The file this library was loaded from (or the bare system name).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The `mpv-version` and `ffmpeg-version` properties (e.g. "mpv 0.39.0", "n7.1"), read once on a throwaway
    /// idle handle that never plays; empty when the handle cannot start.
    pub fn versions(&self) -> Versions {
        self.versions
            .get_or_init(|| unsafe {
                let h = (self.create)();
                if h.is_null() {
                    return Default::default();
                }
                for (k, v) in [("vo", "null"), ("ao", "null"), ("idle", "yes"), ("terminal", "no"), ("config", "no")] {
                    let _ = self.set_option(h, k, v);
                }
                let read = |name| self.get_string(h, name).unwrap_or_default();
                let v = if (self.initialize)(h) >= 0 {
                    Versions { mpv: read("mpv-version"), ffmpeg: read("ffmpeg-version"), configuration: read("mpv-configuration") }
                } else {
                    Default::default()
                };
                (self.terminate_destroy)(h);
                v
            })
            .clone()
    }

    pub fn err(&self, code: c_int) -> String {
        unsafe { CStr::from_ptr((self.error_string)(code)).to_string_lossy().into_owned() }
    }

    // Helpers below: `h` must be a live handle from this library's `create`.

    pub unsafe fn set_option(&self, h: *mut mpv_handle, k: &str, v: &str) -> Result<(), String> {
        let (Ok(k2), Ok(v2)) = (CString::new(k), CString::new(v)) else { return Err(format!("bad option {k}")) };
        self.check(unsafe { (self.set_option_string)(h, k2.as_ptr(), v2.as_ptr()) }).map_err(|e| format!("mpv option {k}={v}: {e}"))
    }

    pub unsafe fn set_property(&self, h: *mut mpv_handle, k: &str, v: &str) -> Result<(), String> {
        let (Ok(k2), Ok(v2)) = (CString::new(k), CString::new(v)) else { return Err(format!("bad property {k}")) };
        self.check(unsafe { (self.set_property_string)(h, k2.as_ptr(), v2.as_ptr()) }).map_err(|e| format!("mpv property {k}={v}: {e}"))
    }

    /// Runs a command and waits for it (e.g. `["stop"]`).
    pub unsafe fn command(&self, h: *mut mpv_handle, args: &[&str]) -> Result<(), String> {
        let r = with_argv(args, |argv| unsafe { (self.command)(h, argv) });
        self.check(r).map_err(|e| format!("mpv {}: {e}", args.first().unwrap_or(&"")))
    }

    /// Queues a command; its result arrives as `EVENT_COMMAND_REPLY` carrying `reply`.
    pub unsafe fn command_async(&self, h: *mut mpv_handle, reply: u64, args: &[&str]) -> Result<(), String> {
        let r = with_argv(args, |argv| unsafe { (self.command_async)(h, reply, argv) });
        self.check(r).map_err(|e| format!("mpv {}: {e}", args.first().unwrap_or(&"")))
    }

    /// Runs a command, waits for it, and returns a copy of its result (e.g. `["screenshot-raw", "video"]`).
    pub unsafe fn command_node(&self, h: *mut mpv_handle, args: &[&str]) -> Result<Node, String> {
        let mut raw = mpv_node { u: mpv_node_value { int64: 0 }, format: FORMAT_NONE };
        let r = with_argv(args, |argv| unsafe { (self.command_ret)(h, argv, &mut raw) });
        self.check(r).map_err(|e| format!("mpv {}: {e}", args.first().unwrap_or(&"")))?;
        unsafe {
            let copy = Node::from_raw(&raw);
            (self.free_node_contents)(&mut raw);
            Ok(copy)
        }
    }

    /// A copy of a property as a node (`track-list`, for one).
    pub unsafe fn get_node(&self, h: *mut mpv_handle, name: &str) -> Option<Node> {
        let mut raw = mpv_node { u: mpv_node_value { int64: 0 }, format: FORMAT_NONE };
        if !unsafe { self.get_raw(h, name, FORMAT_NODE, &mut raw as *mut mpv_node as *mut c_void) } {
            return None;
        }
        unsafe {
            let copy = Node::from_raw(&raw);
            (self.free_node_contents)(&mut raw);
            Some(copy)
        }
    }

    /// Asks for `EVENT_LOG_MESSAGE` events at `level` and above (`"error"`, `"no"` to stop).
    pub unsafe fn request_log_messages(&self, h: *mut mpv_handle, level: &str) -> Result<(), String> {
        let Ok(l) = CString::new(level) else { return Err(format!("bad log level {level}")) };
        self.check(unsafe { (self.request_log_messages_raw)(h, l.as_ptr()) }).map_err(|e| format!("mpv log level {level}: {e}"))
    }

    pub unsafe fn observe(&self, h: *mut mpv_handle, reply: u64, name: &str, format: mpv_format) -> Result<(), String> {
        let Ok(n) = CString::new(name) else { return Err(format!("bad property {name}")) };
        self.check(unsafe { (self.observe_property)(h, reply, n.as_ptr(), format) }).map_err(|e| format!("mpv observe {name}: {e}"))
    }

    pub unsafe fn get_double(&self, h: *mut mpv_handle, name: &str) -> Option<f64> {
        let mut v = 0f64;
        unsafe { self.get_raw(h, name, FORMAT_DOUBLE, &mut v as *mut f64 as *mut c_void) }.then_some(v)
    }

    pub unsafe fn get_flag(&self, h: *mut mpv_handle, name: &str) -> Option<bool> {
        let mut v: c_int = 0;
        unsafe { self.get_raw(h, name, FORMAT_FLAG, &mut v as *mut c_int as *mut c_void) }.then_some(v != 0)
    }

    pub unsafe fn get_string(&self, h: *mut mpv_handle, name: &str) -> Option<String> {
        let n = CString::new(name).ok()?;
        unsafe {
            let p = (self.get_property_string)(h, n.as_ptr());
            if p.is_null() {
                return None;
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.free)(p as *mut c_void);
            Some(s)
        }
    }

    unsafe fn get_raw(&self, h: *mut mpv_handle, name: &str, format: mpv_format, out: *mut c_void) -> bool {
        let Ok(n) = CString::new(name) else { return false };
        unsafe { (self.get_property)(h, n.as_ptr(), format, out) >= 0 }
    }

    fn check(&self, r: c_int) -> Result<(), String> {
        if r < 0 { Err(self.err(r)) } else { Ok(()) }
    }
}

/// Calls `f` with a NULL-terminated argv; an argument containing NUL is an invalid parameter.
fn with_argv(args: &[&str], f: impl FnOnce(*mut *const c_char) -> c_int) -> c_int {
    let Ok(owned) = args.iter().map(|a| CString::new(*a)).collect::<Result<Vec<_>, _>>() else { return ERROR_INVALID_PARAMETER };
    let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).chain([std::ptr::null()]).collect();
    f(ptrs.as_mut_ptr())
}

/// On Windows a full path loads with its own folder first in the DLL search, so libraries
/// shipped next to libmpv are found.
#[cfg(windows)]
fn open(path: &Path) -> Result<libloading::Library, libloading::Error> {
    use libloading::os::windows::{LOAD_WITH_ALTERED_SEARCH_PATH, Library};
    let flags = if path.is_absolute() { LOAD_WITH_ALTERED_SEARCH_PATH } else { 0 };
    unsafe { Library::load_with_flags(path, flags).map(Into::into) }
}

#[cfg(not(windows))]
fn open(path: &Path) -> Result<libloading::Library, libloading::Error> {
    unsafe { libloading::Library::new(path) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_from_nested_map_and_bytes() {
        let string = |p: &CStr| mpv_node { u: mpv_node_value { string: p.as_ptr() as *mut _ }, format: FORMAT_STRING };
        let int = |v| mpv_node { u: mpv_node_value { int64: v }, format: FORMAT_INT64 };
        let mut array = [string(c"video"), mpv_node { u: mpv_node_value { flag: 1 }, format: FORMAT_FLAG }, mpv_node { u: mpv_node_value { double: 1.5 }, format: FORMAT_DOUBLE }, mpv_node { u: mpv_node_value { int64: 0 }, format: FORMAT_NONE }];
        let mut array_list = mpv_node_list { num: array.len() as c_int, values: array.as_mut_ptr(), keys: std::ptr::null_mut() };
        let mut bytes = [1u8, 2, 3];
        let mut ba = mpv_byte_array { data: bytes.as_mut_ptr() as *mut c_void, size: 3 };
        let mut keys = [c"tracks".as_ptr() as *mut c_char, c"data".as_ptr() as *mut c_char, c"w".as_ptr() as *mut c_char];
        let mut values = [
            mpv_node { u: mpv_node_value { list: &mut array_list }, format: FORMAT_NODE_ARRAY },
            mpv_node { u: mpv_node_value { ba: &mut ba }, format: FORMAT_BYTE_ARRAY },
            int(7),
        ];
        let mut map = mpv_node_list { num: values.len() as c_int, values: values.as_mut_ptr(), keys: keys.as_mut_ptr() };
        let root = mpv_node { u: mpv_node_value { list: &mut map }, format: FORMAT_NODE_MAP };
        let n = unsafe { Node::from_raw(&root) };
        assert_eq!(n.get("tracks"), Some(&Node::Array(vec![Node::Str("video".into()), Node::Flag(true), Node::Double(1.5), Node::None])));
        assert_eq!(n.get("data"), Some(&Node::Bytes(vec![1, 2, 3])));
        assert_eq!(n.get("w").and_then(Node::as_f64), Some(7.0));
        assert_eq!(n.get("missing"), None);
        assert_eq!(Node::Str("x".into()).as_str(), Some("x"));
        assert_eq!(Node::Int(3).get("w"), None);
    }
    use std::path::{Path, PathBuf};

    fn candidates_with(env: Option<&'static str>) -> Vec<PathBuf> {
        let var = move |k: &str| env.filter(|_| k == "RP_LIBMPV").map(std::ffi::OsString::from);
        native_candidates(SYSTEM_NAMES, "RP_LIBMPV", Path::new("/app/rp"), Path::new("/data"), "mpv", &var)
    }

    #[test]
    fn candidates_start_with_env_override() {
        assert_eq!(candidates_with(Some("D:/custom/libmpv-2.dll"))[0], Path::new("D:/custom/libmpv-2.dll"));
    }

    #[test]
    fn candidates_include_exe_dir_and_data_dir() {
        let c = candidates_with(None);
        let n = SYSTEM_NAMES[0];
        let exe = c.iter().position(|p| *p == Path::new("/app").join(n)).expect("exe dir");
        let data = c.iter().position(|p| *p == Path::new("/data/deps/mpv").join(n)).expect("data dir");
        let sys = c.iter().position(|p| *p == Path::new(n)).expect("system name");
        assert!(exe < data && data < sys, "{c:?}");
        assert_eq!(Mpv::candidates(Path::new("/data")).iter().filter(|p| p.starts_with("/data/deps/mpv")).count(), SYSTEM_NAMES.len());
    }

    #[test]
    fn load_from_missing_file_is_a_readable_error() {
        let p = std::env::temp_dir().join("rp-no-such-dir").join("libmpv-2.dll");
        let e = Mpv::load_from(&p).err().expect("missing file must fail");
        assert!(e.contains(&p.display().to_string()), "{e}");
    }

    #[test]
    fn loads_real_libmpv_when_available() {
        if std::env::var("RP_VIDEO_TESTS").as_deref() != Ok("1") {
            return;
        }
        let mpv = Mpv::load().expect("libmpv (set RP_LIBMPV)");
        assert_eq!(mpv.api_version().0, 2);
        unsafe {
            let h = (mpv.create)();
            assert!(!h.is_null());
            mpv.set_option(h, "vo", "null").unwrap();
            assert_eq!((mpv.initialize)(h), 0);
            assert_eq!(mpv.get_flag(h, "idle-active"), Some(true));
            assert!(mpv.get_double(h, "volume").is_some());
            assert!(mpv.command(h, &["no-such-command"]).is_err());
            mpv.observe(h, 1, "pause", FORMAT_FLAG).unwrap();
            mpv.set_property(h, "pause", "yes").unwrap();
            assert_eq!(mpv.get_string(h, "pause").as_deref(), Some("yes"));
            (mpv.terminate_destroy)(h);
        }
    }

    #[test]
    fn mpv_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<Mpv>();
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn struct_layouts_match_client_h() {
        assert_eq!(std::mem::size_of::<mpv_event>(), 24);
        assert_eq!(std::mem::size_of::<mpv_event_end_file>(), 32);
        assert_eq!(std::mem::size_of::<mpv_event_property>(), 24);
        assert_eq!(std::mem::size_of::<mpv_node>(), 16);
        assert_eq!(std::mem::size_of::<mpv_node_list>(), 24);
        assert_eq!(std::mem::size_of::<mpv_byte_array>(), 16);
        assert_eq!(std::mem::size_of::<mpv_event_command>(), 16);
    }
}
