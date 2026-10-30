//! Monitors and the app's windows (operator, room output, stage display).
//! Each window is a Tauri WebView2 window with its own data folder, so each runs in
//! its own browser process: a crash in one page cannot take down the room output.
use crate::app::{App, Monitor, Screens};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak, atomic::{AtomicU32, Ordering}},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

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

/// The Tauri windowing layer: one WebView2 window per role, each with its own browser process.
pub struct TauriScreens {
    handle: AppHandle,
    app: Weak<App>,
    port: u16,
    data_root: PathBuf,
    windows: Mutex<HashMap<String, Window>>,
}

impl TauriScreens {
    pub fn new(handle: AppHandle, app: &Arc<App>) -> Arc<Self> {
        Arc::new(Self { handle, app: Arc::downgrade(app), port: app.port, data_root: app.store.root.clone(), windows: Mutex::new(HashMap::new()) })
    }

    /// Reopen outputs that were closed unexpectedly, and reload ones whose page crashed.
    pub async fn watchdog(self: Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let Some(app) = self.app.upgrade() else { return };
            let handle = &self.handle;
            let mut dead = vec![];
            let mut reloaded = vec![];
            {
                let mut w = self.windows.lock().unwrap();
                for (role, win) in w.iter_mut() {
                    if role == "operator" { continue; }
                    if !win.alive(handle) { dead.push((role.clone(), win.monitor.clone(), win.opened.elapsed())); continue; }
                    // The window is open but its page is not connected: the renderer crashed or hung.
                    let conns = if role == "room" { &app.room_conns } else { &app.stage_conns };
                    if conns.load(Ordering::Relaxed) > 0 || win.opened.elapsed() < Duration::from_secs(15) { win.quiet_since = None; continue; }
                    let since = *win.quiet_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= Duration::from_secs(6) && win.reload(handle) { reloaded.push(role.clone()); }
                }
                for (r, _, _) in &dead { w.remove(r); }
            }
            for role in reloaded {
                eprintln!("{role} page stopped responding; reloaded it");
                if role == "room" { app.room_recovered(); }
            }
            for (role, mon, lived) in dead {
                if lived < Duration::from_secs(10) {
                    // Closed right away: don't fight the user or loop on a window that cannot open.
                    eprintln!("{role} window closed right after opening; not reopening automatically");
                    continue;
                }
                eprintln!("{role} window closed unexpectedly; reopening");
                let present = mon.as_ref().is_none_or(|id| monitors().iter().any(|m| &m.id == id));
                if present && self.open(&role, mon.as_deref()).is_ok() && role == "room" { app.room_recovered(); }
            }
        }
    }
}

impl Screens for TauriScreens {
    fn open(&self, role: &str, monitor: Option<&str>) -> Result<(), String> {
        let (path, title) = match role {
            "room" => ("/output", "RustyPresenter — Room screen"),
            "stage" => ("/stage", "RustyPresenter — Stage display"),
            "operator" => ("/", "RustyPresenter"),
            _ => return Err("unknown window".into()),
        };
        let handle = &self.handle;
        // A second launch just brings the operator window forward.
        if role == "operator" {
            let label = self.windows.lock().unwrap().get(role).map(|w| w.label.clone());
            if let Some(w) = label.and_then(|l| handle.get_webview_window(&l)) {
                let _ = w.unminimize();
                let _ = w.set_focus();
                return Ok(());
            }
        }
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        let mons = monitors();
        let mon = match monitor {
            Some(id) => Some(mons.iter().find(|m| m.id == id).ok_or("That screen is not connected right now.")?),
            None => None,
        };
        if let Some(old) = self.windows.lock().unwrap().remove(role) { old.close(handle); }
        let profile = self.data_root.join("webview").join(role);
        let w = open_app_window(handle, role, title, &url, &profile, mon)?;
        self.windows.lock().unwrap().insert(role.to_string(), w);
        Ok(())
    }

    fn close(&self, role: &str) {
        if let Some(w) = self.windows.lock().unwrap().remove(role) { w.close(&self.handle); }
    }

    fn open_roles(&self) -> Vec<String> { self.windows.lock().unwrap().keys().cloned().collect() }

    fn monitors(&self) -> Vec<Monitor> { monitors() }
}
