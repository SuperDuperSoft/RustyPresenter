//! macOS 14.2 and later: what the Mac plays, heard without a loopback device (BlackHole), through a Core Audio
//! process tap. A tap of everything the Mac plays (`CATapDescription`, stereo, nothing left out) is put in a private
//! "aggregate device" of its own with the chosen output as its clock; reading that device's input gives the tap's
//! sound, float stereo at the output's rate. It is sent to the recorder's FFmpeg like Windows' loopback (silence by
//! the clock for the gaps, Mute, the level meter).
//!
//! Written on Windows, where it can be neither compiled nor tried: it needs a Mac (see PROJECT_PLAN.md, November 17).
//! macOS asks once whether RustyPresenter may record what the Mac plays (`NSAudioCaptureUsageDescription`).
use super::level::Level;
use super::pc_sound::{Feed, Format, silence_needed};
use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::msg_send;
use std::ffi::c_void;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

type AudioObjectID = u32;
type OSStatus = i32;

#[repr(C)]
struct PropertyAddress { selector: u32, scope: u32, element: u32 }

#[repr(C)]
#[derive(Clone, Copy)]
struct StreamDescription {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

#[repr(C)]
struct AudioBuffer { channels: u32, bytes: u32, data: *mut c_void }

/// A list of buffers: `count` of them, the first here and the rest after it.
#[repr(C)]
struct AudioBufferList { count: u32, buffers: [AudioBuffer; 1] }

type IoProc = unsafe extern "C" fn(AudioObjectID, *const c_void, *const AudioBufferList, *const c_void, *mut AudioBufferList, *const c_void, *mut c_void) -> OSStatus;

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyData(object: AudioObjectID, address: *const PropertyAddress, qualifier_size: u32, qualifier: *const c_void, size: *mut u32, data: *mut c_void) -> OSStatus;
    fn AudioHardwareCreateProcessTap(description: *mut AnyObject, tap: *mut AudioObjectID) -> OSStatus;
    fn AudioHardwareDestroyProcessTap(tap: AudioObjectID) -> OSStatus;
    fn AudioHardwareCreateAggregateDevice(description: *const c_void, device: *mut AudioObjectID) -> OSStatus;
    fn AudioHardwareDestroyAggregateDevice(device: AudioObjectID) -> OSStatus;
    fn AudioDeviceCreateIOProcID(device: AudioObjectID, proc_: IoProc, client: *mut c_void, id: *mut *mut c_void) -> OSStatus;
    fn AudioDeviceDestroyIOProcID(device: AudioObjectID, id: *mut c_void) -> OSStatus;
    fn AudioDeviceStart(device: AudioObjectID, id: *mut c_void) -> OSStatus;
    fn AudioDeviceStop(device: AudioObjectID, id: *mut c_void) -> OSStatus;
}

const fn code(s: &[u8; 4]) -> u32 { u32::from_be_bytes(*s) }
const SYSTEM: AudioObjectID = 1;
const GLOBAL: u32 = code(b"glob");
const MAIN: u32 = 0;
const DEFAULT_OUTPUT: u32 = code(b"dOut");
const DEVICE_UID: u32 = code(b"uid ");
const TAP_FORMAT: u32 = code(b"tfmt");
const LINEAR_PCM: u32 = code(b"lpcm");
const IS_FLOAT: u32 = 1;

/// One property of a Core Audio object.
unsafe fn get<T: Copy>(object: AudioObjectID, selector: u32) -> Result<T, String> {
    let address = PropertyAddress { selector, scope: GLOBAL, element: MAIN };
    let mut size = std::mem::size_of::<T>() as u32;
    let mut value = std::mem::MaybeUninit::<T>::uninit();
    // SAFETY: `value` has room for `size` bytes, which Core Audio fills.
    let status = unsafe { AudioObjectGetPropertyData(object, &address, 0, std::ptr::null(), &mut size, value.as_mut_ptr().cast()) };
    if status != 0 { return Err(format!("Core Audio said {status}")) }
    // SAFETY: filled above.
    Ok(unsafe { value.assume_init() })
}

/// This Mac's macOS version ("14.2.1"), empty when it cannot be read.
pub fn version() -> String {
    let mut buf = [0u8; 32];
    let mut len = buf.len();
    // SAFETY: the name is a C string; the buffer and its length are ours.
    let ok = unsafe { libc::sysctlbyname(c"kern.osproductversion".as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) } == 0;
    if !ok { return String::new() }
    String::from_utf8_lossy(&buf[..len]).trim_end_matches('\0').to_string()
}

/// What the callback needs: where the sound goes.
struct Shared { to: Mutex<mpsc::Sender<Vec<u8>>> }

/// Core Audio's callback, on its own thread, each few milliseconds: the tap's sound as stereo float bytes.
unsafe extern "C" fn heard(_: AudioObjectID, _: *const c_void, input: *const AudioBufferList, _: *const c_void, _: *mut AudioBufferList, _: *const c_void, client: *mut c_void) -> OSStatus {
    if input.is_null() || client.is_null() { return 0 }
    // SAFETY: `client` is the `Shared` given to `AudioDeviceCreateIOProcID`, alive until the callback is removed;
    // the list holds `count` buffers one after the other, each `bytes` long, valid during this call.
    let (shared, buffers) = unsafe {
        let list = &*input;
        let first = std::ptr::addr_of!(list.buffers).cast::<AudioBuffer>();
        let all: Vec<(u32, &[f32])> = (0..list.count as usize).map(|i| &*first.add(i))
            .filter(|b| !b.data.is_null())
            .map(|b| (b.channels, std::slice::from_raw_parts(b.data.cast::<f32>(), b.bytes as usize / 4))).collect();
        (&*(client as *const Shared), all)
    };
    let bytes = super::pc_sound::to_stereo(&buffers);
    if let Ok(to) = shared.to.try_lock() { let _ = to.send(bytes); }
    0
}

/// The tap and its device, running. Dropping it stops and removes them.
struct Tap {
    tap: AudioObjectID,
    device: AudioObjectID,
    proc_id: *mut c_void,
    shared: *mut Shared,
    _description: Retained<AnyObject>,
    rate: u32,
}

// SAFETY: the ids are plain numbers; the description and `shared` are only touched again when it is dropped, from
// whichever thread holds it.
unsafe impl Send for Tap {}

impl Tap {
    /// Taps everything the Mac plays, timed by output `uid` (empty: the default output).
    unsafe fn open(uid: &str, to: mpsc::Sender<Vec<u8>>) -> Result<Tap, String> {
        // SAFETY: plain Core Audio and Objective-C calls with values made here.
        unsafe {
            let class = AnyClass::get(c"CATapDescription").ok_or("this macOS has no process taps")?;
            let nobody: CFArray<CFType> = CFArray::from_CFTypes(&[]);
            let nobody = &*(nobody.as_concrete_TypeRef() as *const AnyObject);
            let made: Allocated<AnyObject> = msg_send![class, alloc];
            let description: Retained<AnyObject> = msg_send![made, initStereoGlobalTapButExcludeProcesses: nobody];
            let _: () = msg_send![&*description, setPrivate: Bool::YES];
            // 0: what is tapped is still heard on the speakers.
            let _: () = msg_send![&*description, setMuteBehavior: 0isize];
            let id: Retained<AnyObject> = msg_send![&*description, UUID];
            let id: Retained<AnyObject> = msg_send![&*id, UUIDString];
            let tap_uid = CFString::wrap_under_get_rule(Retained::as_ptr(&id) as CFStringRef).to_string();
            let mut tap: AudioObjectID = 0;
            let status = AudioHardwareCreateProcessTap(Retained::as_ptr(&description) as *mut AnyObject, &mut tap);
            if status != 0 { return Err(format!("the tap could not be made (Core Audio said {status}; is recording this Mac's sound allowed in System Settings → Privacy?)")) }
            let output = if uid.is_empty() { default_output_uid()? } else { uid.to_string() };
            let key = |k: &str| CFString::new(k).as_CFType();
            let sub = CFDictionary::from_CFType_pairs(&[(key("uid"), CFString::new(&output).as_CFType())]);
            let tapped = CFDictionary::from_CFType_pairs(&[(key("uid"), CFString::new(&tap_uid).as_CFType()), (key("drift"), CFBoolean::true_value().as_CFType())]);
            let description_dict = CFDictionary::from_CFType_pairs(&[
                (key("name"), CFString::new("RustyPresenter: this Mac's sound").as_CFType()),
                (key("uid"), CFString::new(&format!("RustyPresenter-tap-{}", uuid::Uuid::new_v4())).as_CFType()),
                (key("master"), CFString::new(&output).as_CFType()),
                (key("private"), CFBoolean::true_value().as_CFType()),
                (key("stacked"), CFBoolean::false_value().as_CFType()),
                (key("tapautostart"), CFBoolean::true_value().as_CFType()),
                (key("subdevices"), CFArray::from_CFTypes(&[sub]).as_CFType()),
                (key("taps"), CFArray::from_CFTypes(&[tapped]).as_CFType()),
            ]);
            let mut device: AudioObjectID = 0;
            let status = AudioHardwareCreateAggregateDevice(description_dict.as_concrete_TypeRef() as *const c_void, &mut device);
            if status != 0 { AudioHardwareDestroyProcessTap(tap); return Err(format!("the tap's device could not be made (Core Audio said {status})")) }
            let undo = |msg: String| { AudioHardwareDestroyAggregateDevice(device); AudioHardwareDestroyProcessTap(tap); Err(msg) };
            let format: StreamDescription = match get(tap, TAP_FORMAT) { Ok(f) => f, Err(e) => return undo(e) };
            if format.format_id != LINEAR_PCM || format.format_flags & IS_FLOAT == 0 || format.bits_per_channel != 32 {
                return undo(format!("the tap's sound is not 32-bit float ({} bits)", format.bits_per_channel));
            }
            let shared = Box::into_raw(Box::new(Shared { to: Mutex::new(to) }));
            let mut proc_id: *mut c_void = std::ptr::null_mut();
            let status = AudioDeviceCreateIOProcID(device, heard, shared.cast(), &mut proc_id);
            if status != 0 { drop(Box::from_raw(shared)); return undo(format!("the tap could not be read (Core Audio said {status})")) }
            let status = AudioDeviceStart(device, proc_id);
            if status != 0 {
                AudioDeviceDestroyIOProcID(device, proc_id);
                drop(Box::from_raw(shared));
                return undo(format!("the tap did not start (Core Audio said {status})"));
            }
            Ok(Tap { tap, device, proc_id, shared, _description: description, rate: format.sample_rate.round() as u32 })
        }
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        // SAFETY: undoes `open` in reverse; the callback is gone before `shared` is freed.
        unsafe {
            AudioDeviceStop(self.device, self.proc_id);
            AudioDeviceDestroyIOProcID(self.device, self.proc_id);
            drop(Box::from_raw(self.shared));
            AudioHardwareDestroyAggregateDevice(self.device);
            AudioHardwareDestroyProcessTap(self.tap);
        }
    }
}

/// The default output's id as Core Audio knows it.
unsafe fn default_output_uid() -> Result<String, String> {
    // SAFETY: plain property reads; the id string comes back owned ("copy" rule).
    unsafe {
        let device: AudioObjectID = get(SYSTEM, DEFAULT_OUTPUT)?;
        let uid: CFStringRef = get(device, DEVICE_UID)?;
        if uid.is_null() { return Err("the default output has no id".into()) }
        Ok(CFString::wrap_under_create_rule(uid).to_string())
    }
}

/// Starts hearing what the Mac plays, timed by output `device` (mpv's name, `coreaudio/<id>`; empty: the default).
/// Gives the feed for FFmpeg, its level, and what stops it.
pub fn start(device: &str, mute: Arc<AtomicBool>) -> Result<(Feed, Arc<Level>, Box<dyn FnMut() + Send>), String> {
    let port = crate::platform::ports::tcp().map_err(|e| e.to_string())?;
    let (tx, rx) = mpsc::channel();
    // SAFETY: see `Tap::open`.
    let tap = unsafe { Tap::open(device.strip_prefix("coreaudio/").unwrap_or(""), tx)? };
    let format = Format { samples: "f32le", rate: tap.rate, channels: 2 };
    let stop = Arc::new(AtomicBool::new(false));
    let level: Arc<Level> = Arc::default();
    let (s, l) = (stop.clone(), level.clone());
    let thread = std::thread::Builder::new().name("pc-sound-mac".into()).spawn(move || run(rx, format, port, &s, &mute, &l)).map_err(|e| e.to_string())?;
    let mut parts = Some((tap, thread));
    let stopper = move || {
        stop.store(true, Ordering::Relaxed);
        if let Some((tap, thread)) = parts.take() { let _ = thread.join(); drop(tap) }
    };
    Ok((Feed { format, port }, level, Box::new(stopper)))
}

const LOOK_EVERY: Duration = Duration::from_millis(10);
const CONNECT_FOR: Duration = Duration::from_secs(30);

/// Connects to FFmpeg, then sends what the tap hears (and silence for the gaps) until stopped or FFmpeg goes.
fn run(rx: mpsc::Receiver<Vec<u8>>, format: Format, port: u16, stop: &AtomicBool, mute: &AtomicBool, level: &Level) {
    let began = Instant::now();
    let mut out = loop {
        if stop.load(Ordering::Relaxed) { return }
        match std::net::TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => break s,
            Err(_) if began.elapsed() < CONNECT_FOR => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => { eprintln!("pc sound: FFmpeg did not listen: {e}"); return }
        }
    };
    let _ = out.set_nodelay(true);
    let frame = format.frame_bytes();
    // What came before FFmpeg listened is old: dropped.
    while rx.try_recv().is_ok() {}
    let t0 = Instant::now();
    let mut written: u64 = 0;
    let mut buf: Vec<u8> = vec![];
    while !stop.load(Ordering::Relaxed) {
        buf.clear();
        while let Ok(b) = rx.try_recv() { buf.extend_from_slice(&b) }
        buf.truncate(buf.len() / frame * frame);
        written += (buf.len() / frame) as u64;
        let gap = silence_needed(t0.elapsed(), written, format.rate);
        buf.resize(buf.len() + gap as usize * frame, 0);
        written += gap;
        let muted = mute.load(Ordering::Relaxed);
        if muted { buf.fill(0) }
        level.hear(if muted { 0 } else { super::pc_sound::peak(&format, &buf) });
        if !buf.is_empty() && out.write_all(&buf).is_err() { break }
        std::thread::sleep(LOOK_EVERY);
    }
}
