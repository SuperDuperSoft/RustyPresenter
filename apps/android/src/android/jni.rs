//! The few things only Android's Java side can do, called through JNI: permissions, the phone's name, which way
//! the screen is turned, and Android's own JPEG maker (fast on every phone) for the camera's pictures.
use jni::{JNIEnv, JavaVM, objects::{JByteArray, JObject, JString, JValue}};
use std::sync::OnceLock;

static APP: OnceLock<slint::android::AndroidApp> = OnceLock::new();

pub fn set_app(app: slint::android::AndroidApp) { let _ = APP.set(app); }

pub fn app() -> &'static slint::android::AndroidApp { APP.get().expect("the app is set at start") }

/// Runs `f` with this thread attached to Java (kept attached: worker threads call often). Java's own errors are
/// cleared and come back as None.
fn with<R>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<R>) -> Option<R> {
    let app = app();
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.ok()?;
    let mut env = vm.attach_current_thread_permanently().ok()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let r = env.with_local_frame(16, |env| f(env, &activity));
    if env.exception_check().unwrap_or(false) { let _ = env.exception_clear(); }
    r.ok()
}

pub const CAMERA: &str = "android.permission.CAMERA";
pub const MICROPHONE: &str = "android.permission.RECORD_AUDIO";

pub fn has_permission(name: &str) -> bool {
    with(|env, act| {
        let p = env.new_string(name)?;
        env.call_method(act, "checkSelfPermission", "(Ljava/lang/String;)I", &[JValue::Object(&p)])?.i()
    }) == Some(0)
}

/// Shows Android's own question for permission `name`.
pub fn request_permission(name: &str) {
    with(|env, act| {
        let p = env.new_string(name)?;
        let list = env.new_object_array(1, "java/lang/String", &p)?;
        env.call_method(act, "requestPermissions", "([Ljava/lang/String;I)V", &[JValue::Object(&list), JValue::Int(1)])?;
        Ok(())
    });
}

/// The phone's model ("Pixel 8"), for its name on the computer.
pub fn device_name() -> Option<String> {
    with(|env, _| {
        let v = env.get_static_field("android/os/Build", "MODEL", "Ljava/lang/String;")?.l()?;
        let s: String = env.get_string(&JString::from(v))?.into();
        Ok(s)
    }).filter(|s| !s.trim().is_empty())
}

/// How the screen is turned, in degrees (0, 90, 180, 270), as Android's `Display.getRotation` counts it.
pub fn display_rotation() -> i32 {
    with(|env, act| {
        let wm = env.call_method(act, "getWindowManager", "()Landroid/view/WindowManager;", &[])?.l()?;
        let d = env.call_method(&wm, "getDefaultDisplay", "()Landroid/view/Display;", &[])?.l()?;
        env.call_method(&d, "getRotation", "()I", &[])?.i()
    }).unwrap_or(0) * 90
}

/// An NV21 picture as JPEG (`quality` 0–100), by Android's `YuvImage`.
pub fn jpeg(nv21: &[u8], w: usize, h: usize, quality: i32) -> Option<Vec<u8>> {
    with(|env, _| {
        let data = env.byte_array_from_slice(nv21)?;
        let (w, h) = (w as i32, h as i32);
        let yuv = env.new_object("android/graphics/YuvImage", "([BIII[I)V",
            &[JValue::Object(&data), JValue::Int(17), JValue::Int(w), JValue::Int(h), JValue::Object(&JObject::null())])?;
        let rect = env.new_object("android/graphics/Rect", "(IIII)V", &[JValue::Int(0), JValue::Int(0), JValue::Int(w), JValue::Int(h)])?;
        let out = env.new_object("java/io/ByteArrayOutputStream", "(I)V", &[JValue::Int(256 * 1024)])?;
        env.call_method(&yuv, "compressToJpeg", "(Landroid/graphics/Rect;ILjava/io/OutputStream;)Z",
            &[JValue::Object(&rect), JValue::Int(quality), JValue::Object(&out)])?;
        let bytes = env.call_method(&out, "toByteArray", "()[B", &[])?.l()?;
        env.convert_byte_array(JByteArray::from(bytes))
    })
}
