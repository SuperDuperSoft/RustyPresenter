//! Monitors and the app's windows (operator, room output, stage display).
//! Each window is a Tauri WebView2 window with its own data folder, so each runs in
//! its own browser process: a crash in one page cannot take down the room output.
use serde::Serialize;
use std::{path::Path, sync::atomic::{AtomicU32, Ordering}, time::Instant};
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Monitor {
    /// Windows device name, e.g. `\\.\DISPLAY2`. Used as the role binding.
    pub id: String,
    pub label: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub primary: bool,
}

#[cfg(windows)]
pub fn monitors() -> Vec<Monitor> {
    use windows_sys::Win32::{Foundation::{LPARAM, RECT}, Graphics::Gdi::*};
    unsafe extern "system" fn cb(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> i32 {
        let list = unsafe { &mut *(data as *mut Vec<Monitor>) };
        let mut info: MONITORINFOEXW = unsafe { std::mem::zeroed() };
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if unsafe { GetMonitorInfoW(h, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO) } != 0 {
            let r = info.monitorInfo.rcMonitor;
            let len = info.szDevice.iter().position(|c| *c == 0).unwrap_or(info.szDevice.len());
            let id = String::from_utf16_lossy(&info.szDevice[..len]);
            let primary = info.monitorInfo.dwFlags & 1 != 0;
            let n = list.len() + 1;
            list.push(Monitor { label: format!("Screen {n}{}", if primary { " (this laptop / main)" } else { "" }), id, x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top, primary });
        }
        1
    }
    let mut list: Vec<Monitor> = vec![];
    // Tauri makes the process per-monitor DPI aware, so these are physical pixels.
    unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(cb), &mut list as *mut _ as LPARAM); }
    // Number screens left-to-right, like Windows' Display settings usually do.
    list.sort_by_key(|m| (m.x, m.y));
    for (i, m) in list.iter_mut().enumerate() {
        m.label = format!("Screen {}{} — {}×{}", i + 1, if m.primary { " (main)" } else { "" }, m.width, m.height);
    }
    list
}

#[cfg(not(windows))]
pub fn monitors() -> Vec<Monitor> {
    vec![Monitor { id: "main".into(), label: "Screen 1 (main) — 1920×1080".into(), x: 0, y: 0, width: 1920, height: 1080, primary: true }]
}

/// Tauri's default WebView2 flags, plus autoplay for video and no pinch/swipe navigation.
const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required --disable-pinch --overscroll-history-navigation=0";

pub struct Window {
    /// Tauri window label. Each opening gets a new one, so a closing window never clashes with its replacement.
    pub label: String,
    pub url: String,
    pub monitor: Option<String>,
    pub opened: Instant,
    /// When the page was first seen without a live connection (crashed or hung renderer).
    pub quiet_since: Option<Instant>,
}

impl Window {
    pub fn alive(&self, app: &AppHandle) -> bool { app.get_webview_window(&self.label).is_some() }
    pub fn close(self, app: &AppHandle) { if let Some(w) = app.get_webview_window(&self.label) { let _ = w.destroy(); } }
    /// Load the page again, which also restarts a crashed renderer.
    pub fn reload(&mut self, app: &AppHandle) -> bool {
        let (Some(w), Ok(url)) = (app.get_webview_window(&self.label), self.url.parse()) else { return false };
        self.opened = Instant::now();
        self.quiet_since = None;
        w.navigate(url).is_ok()
    }
}

static SEQ: AtomicU32 = AtomicU32::new(0);

/// Open `url` as a window. `profile` is the window's own WebView2 data folder. `on` = full screen on that monitor.
pub fn open_app_window(app: &AppHandle, role: &str, title: &str, url: &str, profile: &Path, on: Option<&Monitor>) -> Result<Window, String> {
    let label = format!("{role}-{}", SEQ.fetch_add(1, Ordering::Relaxed));
    let parsed = url.parse().map_err(|e| format!("Bad window address {url}: {e}"))?;
    let mut b = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parsed))
        .title(title)
        .data_directory(profile.to_path_buf())
        .additional_browser_args(BROWSER_ARGS)
        .visible(false);
    b = if on.is_some() { b.decorations(false).focused(false) } else { b.inner_size(1500.0, 900.0).center() };
    let w = b.build().map_err(|e| format!("Could not open the {role} window: {e}"))?;
    if let Some(m) = on {
        // Aim at the middle of the screen so mixed display scaling still lands on the right monitor.
        let _ = w.set_position(PhysicalPosition::new(m.x + m.width / 4, m.y + m.height / 4));
        let _ = w.set_fullscreen(true);
    }
    let _ = w.show();
    Ok(Window { label, url: url.into(), monitor: on.map(|m| m.id.clone()), opened: Instant::now(), quiet_since: None })
}

/// Show an error when the app has no window (and, in release builds, no console) to show it in.
pub fn error_box(msg: &str) {
    eprintln!("{msg}");
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        unsafe { MessageBoxW(std::ptr::null_mut(), wide(msg).as_ptr(), wide("RustyPresenter").as_ptr(), MB_OK | MB_ICONERROR); }
    }
}
