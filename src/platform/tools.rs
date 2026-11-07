//! Finding LibreOffice, PowerPoint and pdfium (Plan 4 Task 1).
use super::resources;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[path = "../strings/tools.rs"]
pub mod strings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Linux,
}

#[cfg(windows)]
pub const OS: Os = Os::Windows;
#[cfg(target_os = "macos")]
pub const OS: Os = Os::MacOs;
#[cfg(not(any(windows, target_os = "macos")))]
pub const OS: Os = Os::Linux;

/// Reads an environment variable.
pub type VarFn = Box<dyn Fn(&str) -> Option<OsString>>;

/// Everything the search reads from the machine, so the search itself is pure.
pub struct ToolEnv {
    pub os: Os,
    pub var: VarFn,
    pub home: Option<PathBuf>,
    pub path_dirs: Vec<PathBuf>,
    /// The `program` folders named by the Windows registry.
    pub registry: Vec<PathBuf>,
    /// The `/opt/libreoffice*` folders that exist.
    pub opt_dirs: Vec<PathBuf>,
}

impl ToolEnv {
    /// This machine, as it is now.
    pub fn current() -> ToolEnv {
        ToolEnv {
            os: OS,
            var: Box::new(|k| std::env::var_os(k)),
            home: dirs::home_dir(),
            path_dirs: std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default(),
            registry: registry_install_paths(),
            opt_dirs: opt_libreoffice_dirs(),
        }
    }
}

/// Every place LibreOffice may be, best first. `soffice.com` comes before `soffice.exe` because only the
/// `.com` launcher waits for the conversion to finish (check C5).
pub fn soffice_candidates(env: &ToolEnv) -> Vec<PathBuf> {
    let mut c: Vec<PathBuf> = (env.var)("RP_SOFFICE").filter(|v| !v.is_empty()).map(PathBuf::from).into_iter().collect();
    let on_path = |name: &str| env.path_dirs.iter().map(|d| d.join(name)).collect::<Vec<_>>();
    match env.os {
        Os::Windows => {
            let program = Path::new("LibreOffice").join("program").join("soffice.com");
            c.extend(env.registry.iter().map(|d| d.join("soffice.com")));
            for var in ["ProgramFiles", "ProgramFiles(x86)"] {
                c.extend((env.var)(var).map(|d| Path::new(&d).join(&program)));
            }
            let scoop = Path::new("scoop").join("apps").join("libreoffice").join("current").join(&program);
            c.extend((env.var)("USERPROFILE").map(|d| Path::new(&d).join(scoop)));
            c.extend(on_path("soffice.com"));
            c.extend(on_path("soffice.exe"));
        }
        Os::MacOs => {
            let app = Path::new("Applications/LibreOffice.app/Contents/MacOS/soffice");
            c.push(Path::new("/").join(app));
            c.extend(env.home.as_ref().map(|h| h.join(app)));
            c.extend(on_path("soffice"));
        }
        Os::Linux => {
            c.extend(on_path("soffice"));
            c.extend(on_path("libreoffice"));
            c.push(PathBuf::from("/usr/lib/libreoffice/program/soffice"));
            c.push(PathBuf::from("/usr/lib64/libreoffice/program/soffice"));
            c.extend(env.opt_dirs.iter().map(|d| d.join("program/soffice")));
            c.push(PathBuf::from("/snap/bin/libreoffice"));
        }
    }
    c
}

/// A LibreOffice that exists. Snap builds cannot read every folder, so callers need to know (Flatpak is not supported).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Soffice {
    pub path: PathBuf,
    pub snap: bool,
}

impl Soffice {
    pub fn new(path: PathBuf) -> Soffice {
        let snap = path.starts_with("/snap");
        Soffice { path, snap }
    }
}

/// The first LibreOffice that exists. A found path is kept until it disappears. Nothing found is never kept,
/// because a volunteer may install LibreOffice while the app runs.
pub fn find_soffice() -> Option<Soffice> {
    static FOUND: Mutex<Option<Soffice>> = Mutex::new(None);
    if let Some(s) = lock(&FOUND).as_ref().filter(|s| s.path.is_file()) {
        return Some(s.clone());
    }
    find_soffice_in(&FOUND, &ToolEnv::current())
}

pub fn find_soffice_in(cache: &Mutex<Option<Soffice>>, env: &ToolEnv) -> Option<Soffice> {
    let mut cached = lock(cache);
    if let Some(s) = cached.as_ref().filter(|s| s.path.is_file()) {
        return Some(s.clone());
    }
    *cached = soffice_candidates(env).into_iter().find(|p| p.is_file()).map(Soffice::new);
    cached.clone()
}

fn lock(cache: &Mutex<Option<Soffice>>) -> std::sync::MutexGuard<'_, Option<Soffice>> {
    cache.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn install_hint(os: Os) -> String {
    match os {
        Os::Windows => strings::install_hint_windows(),
        Os::MacOs => strings::install_hint_macos(),
        Os::Linux => strings::install_hint_linux(),
    }
}

/// The command in `install_hint`, for a Copy button.
pub fn install_command(os: Os) -> Option<&'static str> {
    Some(match os {
        Os::Windows => "winget install TheDocumentFoundation.LibreOffice",
        Os::MacOs => "brew install --cask libreoffice",
        Os::Linux => "sudo apt install libreoffice-impress",
    })
}

pub fn pdfium_candidates(data_dir: &Path) -> Vec<PathBuf> {
    const NAME: &str = match OS {
        Os::Windows => "pdfium.dll",
        Os::MacOs => "libpdfium.dylib",
        Os::Linux => "libpdfium.so",
    };
    // The pdfium-binaries archive keeps the library in `bin/` (Windows) or `lib/` (macOS, Linux), so an archive
    // unpacked into deps/pdfium is found as it is, as well as a flattened one.
    const ARCHIVE_SUB: &str = match OS {
        Os::Windows => "bin",
        _ => "lib",
    };
    let mut out = resources::native_candidates(&[NAME], "RP_PDFIUM", &resources::current_exe(), data_dir, "pdfium", &|k| std::env::var_os(k));
    let flat = data_dir.join("deps").join("pdfium").join(NAME);
    let nested = data_dir.join("deps").join("pdfium").join(ARCHIVE_SUB).join(NAME);
    let at = out.iter().position(|p| *p == flat).map_or(out.len(), |i| i + 1);
    if !out.contains(&nested) {
        out.insert(at, nested);
    }
    out
}

/// A `file://` address, for LibreOffice's `-env:UserInstallation`. `canonicalize` gives `\\?\C:\…` on Windows,
/// which `Url` refuses and LibreOffice cannot read (check C5).
pub fn file_url(path: &Path, dir: bool) -> Result<String, String> {
    let plain = path.to_str().map(strip_verbatim).map(PathBuf::from);
    let plain = plain.as_deref().unwrap_or(path);
    let url = if dir { url::Url::from_directory_path(plain) } else { url::Url::from_file_path(plain) };
    url.map(String::from).map_err(|()| strings::file_url_failed(&path.display().to_string()))
}

pub(crate) fn strip_verbatim(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
    }
}

fn opt_libreoffice_dirs() -> Vec<PathBuf> {
    if OS != Os::Linux {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir("/opt") else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("libreoffice"))).collect();
    dirs.sort();
    dirs
}

#[cfg(not(windows))]
fn registry_install_paths() -> Vec<PathBuf> {
    Vec::new()
}

/// Is PowerPoint installed? Only Windows has an automation route to it.
#[cfg(not(windows))]
pub fn powerpoint_installed() -> bool {
    false
}

#[cfg(windows)]
pub use win::powerpoint_installed;
#[cfg(windows)]
use win::registry_install_paths;

#[cfg(windows)]
mod win {
    use std::ffi::OsString;
    use std::iter::once;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegGetValueW, RegOpenKeyExW};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(once(0)).collect()
    }

    /// The default value of a registry key, as text.
    fn default_value(root: HKEY, key: &str) -> Option<OsString> {
        let key = wide(key);
        let mut bytes: u32 = 0;
        // SAFETY: `key` is NUL-terminated and outlives the call; a null data pointer asks for the size only.
        let r = unsafe { RegGetValueW(root, key.as_ptr(), null(), RRF_RT_REG_SZ, null_mut(), null_mut(), &mut bytes) };
        if (r != ERROR_SUCCESS && r != ERROR_MORE_DATA) || bytes < 2 {
            return None;
        }
        let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
        // SAFETY: `buf` holds `bytes` bytes, and `bytes` tells the call so.
        let r = unsafe { RegGetValueW(root, key.as_ptr(), null(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr().cast(), &mut bytes) };
        if r != ERROR_SUCCESS {
            return None;
        }
        let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        (len > 0).then(|| OsString::from_wide(&buf[..len]))
    }

    /// LibreOffice's `program` folders, from `SOFTWARE\LibreOffice\UNO\InstallPath`.
    pub fn registry_install_paths() -> Vec<PathBuf> {
        const KEY: &str = r"SOFTWARE\LibreOffice\UNO\InstallPath";
        const KEY_32: &str = r"SOFTWARE\WOW6432Node\LibreOffice\UNO\InstallPath";
        [(HKEY_LOCAL_MACHINE, KEY), (HKEY_LOCAL_MACHINE, KEY_32), (HKEY_CURRENT_USER, KEY)].into_iter().filter_map(|(root, key)| default_value(root, key)).map(PathBuf::from).collect()
    }

    pub fn powerpoint_installed() -> bool {
        let key = wide(r"PowerPoint.Application\CLSID");
        let mut opened: HKEY = null_mut();
        // SAFETY: `key` is NUL-terminated and `opened` is a valid out-pointer.
        let r = unsafe { RegOpenKeyExW(HKEY_CLASSES_ROOT, key.as_ptr(), 0, KEY_READ, &mut opened) };
        if r != ERROR_SUCCESS {
            return false;
        }
        // SAFETY: the call above opened this key.
        unsafe { RegCloseKey(opened) };
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(os: Os, vars: &[(&'static str, &'static str)]) -> ToolEnv {
        let vars = vars.to_vec();
        ToolEnv {
            os,
            var: Box::new(move |k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))),
            home: Some(PathBuf::from("/home/jake")),
            path_dirs: vec![PathBuf::from("/p1"), PathBuf::from("/p2")],
            registry: vec![],
            opt_dirs: vec![],
        }
    }

    fn pos(c: &[PathBuf], p: &Path) -> usize {
        c.iter().position(|x| x == p).unwrap_or_else(|| panic!("{} not in {c:?}", p.display()))
    }

    #[test]
    fn windows_candidates_prefer_registry_then_program_files_soffice_com() {
        let mut e = env(Os::Windows, &[("ProgramFiles", "C:/PF"), ("ProgramFiles(x86)", "C:/PF86"), ("USERPROFILE", "C:/U/jake")]);
        e.registry = vec![PathBuf::from("D:/LO/program")];
        let c = soffice_candidates(&e);
        let want: Vec<PathBuf> = [
            "D:/LO/program/soffice.com",
            "C:/PF/LibreOffice/program/soffice.com",
            "C:/PF86/LibreOffice/program/soffice.com",
            "C:/U/jake/scoop/apps/libreoffice/current/LibreOffice/program/soffice.com",
            "/p1/soffice.com",
            "/p2/soffice.com",
            "/p1/soffice.exe",
            "/p2/soffice.exe",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(c, want);
    }

    #[test]
    fn windows_skips_folders_whose_variable_is_unset() {
        let c = soffice_candidates(&env(Os::Windows, &[]));
        assert!(c.iter().all(|p| p.starts_with("/p1") || p.starts_with("/p2")), "{c:?}");
    }

    #[test]
    fn mac_candidates_include_user_applications() {
        let c = soffice_candidates(&env(Os::MacOs, &[]));
        let sys = pos(&c, Path::new("/Applications/LibreOffice.app/Contents/MacOS/soffice"));
        let user = pos(&c, Path::new("/home/jake/Applications/LibreOffice.app/Contents/MacOS/soffice"));
        let path = pos(&c, Path::new("/p1/soffice"));
        assert!(sys < user && user < path, "{c:?}");
    }

    #[test]
    fn linux_candidates_cover_usr_lib_opt_and_snap() {
        let mut e = env(Os::Linux, &[]);
        e.opt_dirs = vec![PathBuf::from("/opt/libreoffice26.8")];
        let c = soffice_candidates(&e);
        let want: Vec<PathBuf> = [
            "/p1/soffice",
            "/p2/soffice",
            "/p1/libreoffice",
            "/p2/libreoffice",
            "/usr/lib/libreoffice/program/soffice",
            "/usr/lib64/libreoffice/program/soffice",
            "/opt/libreoffice26.8/program/soffice",
            "/snap/bin/libreoffice",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(c, want);
    }

    #[test]
    fn rp_soffice_override_first() {
        for os in [Os::Windows, Os::MacOs, Os::Linux] {
            let c = soffice_candidates(&env(os, &[("RP_SOFFICE", "/mine/soffice")]));
            assert_eq!(c[0], Path::new("/mine/soffice"), "{os:?}");
        }
    }

    #[test]
    fn snap_path_is_flagged() {
        assert!(Soffice::new(PathBuf::from("/snap/bin/libreoffice")).snap);
        assert!(!Soffice::new(PathBuf::from("/usr/bin/soffice")).snap);
        assert_eq!(Soffice::new(PathBuf::from("/usr/bin/soffice")).path, Path::new("/usr/bin/soffice"));
    }

    #[cfg(windows)]
    #[test]
    fn file_url_escapes_spaces_and_accents() {
        assert_eq!(file_url(Path::new(r"C:\Users\Jake Smith\é"), true).unwrap(), "file:///C:/Users/Jake%20Smith/%C3%A9/");
    }

    #[cfg(windows)]
    #[test]
    fn file_url_strips_verbatim_prefix() {
        assert_eq!(file_url(Path::new(r"\\?\C:\Users\a.pptx"), false).unwrap(), "file:///C:/Users/a.pptx");
    }

    #[cfg(not(windows))]
    #[test]
    fn file_url_escapes_spaces_and_accents() {
        assert_eq!(file_url(Path::new("/home/Jake Smith/é"), true).unwrap(), "file:///home/Jake%20Smith/%C3%A9/");
        assert_eq!(file_url(Path::new("/home/a.pptx"), false).unwrap(), "file:///home/a.pptx");
    }

    #[test]
    fn verbatim_prefixes_are_removed_for_any_os() {
        assert_eq!(strip_verbatim(r"\\?\C:\a b\c"), r"C:\a b\c");
        assert_eq!(strip_verbatim(r"\\?\UNC\server\share\c"), r"\\server\share\c");
        assert_eq!(strip_verbatim("/plain/path"), "/plain/path");
    }

    #[test]
    fn file_url_refuses_a_relative_path() {
        assert!(file_url(Path::new("relative/a.pptx"), false).unwrap_err().contains("relative/a.pptx"));
    }

    #[test]
    fn install_hint_per_os() {
        assert!(install_hint(Os::Windows).contains("winget install TheDocumentFoundation.LibreOffice"));
        assert!(install_hint(Os::MacOs).contains("brew install --cask libreoffice"));
        assert!(install_hint(Os::Linux).contains("sudo apt install libreoffice-impress"));
    }

    #[test]
    fn install_command_is_inside_the_hint() {
        for os in [Os::Windows, Os::MacOs, Os::Linux] {
            let cmd = install_command(os).expect("every OS has a command");
            assert!(install_hint(os).ends_with(cmd), "{os:?}");
        }
    }

    // The tests below search as Windows, whatever they run on: the Linux and macOS lists hold fixed system paths,
    // and a machine with LibreOffice installed would answer from those.
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rp-tools-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn negative_soffice_result_is_not_cached() {
        let d = scratch("neg");
        let mut e = env(Os::Windows, &[]);
        e.path_dirs = vec![d.clone()];
        let cache = Mutex::new(None);
        assert_eq!(find_soffice_in(&cache, &e), None);
        std::fs::write(d.join("soffice.com"), "").unwrap();
        assert_eq!(find_soffice_in(&cache, &e).map(|s| s.path), Some(d.join("soffice.com")));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn found_soffice_is_cached_until_it_disappears() {
        let (a, b) = (scratch("a"), scratch("b"));
        std::fs::write(a.join("soffice.com"), "").unwrap();
        let mut e = env(Os::Windows, &[]);
        e.path_dirs = vec![a.clone()];
        let cache = Mutex::new(None);
        assert_eq!(find_soffice_in(&cache, &e).map(|s| s.path), Some(a.join("soffice.com")));
        // The search would find nothing now, so a result can only be the cached one.
        e.path_dirs = vec![];
        assert_eq!(find_soffice_in(&cache, &e).map(|s| s.path), Some(a.join("soffice.com")));
        std::fs::remove_file(a.join("soffice.com")).unwrap();
        std::fs::write(b.join("soffice.com"), "").unwrap();
        e.path_dirs = vec![b.clone()];
        assert_eq!(find_soffice_in(&cache, &e).map(|s| s.path), Some(b.join("soffice.com")));
        std::fs::remove_dir_all(&a).unwrap();
        std::fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_folder_named_soffice_is_not_found() {
        let d = scratch("dir");
        std::fs::create_dir(d.join("soffice.com")).unwrap();
        let mut e = env(Os::Windows, &[]);
        e.path_dirs = vec![d.clone()];
        assert_eq!(find_soffice_in(&Mutex::new(None), &e), None);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn pdfium_candidates_look_in_deps() {
        let c = pdfium_candidates(Path::new("/data"));
        assert!(c.iter().any(|p| p.starts_with("/data/deps/pdfium")), "{c:?}");
        let sub = if OS == Os::Windows { "bin" } else { "lib" };
        let flat = c.iter().position(|p| p.parent() == Some(Path::new("/data/deps/pdfium")));
        let nested = c.iter().position(|p| p.parent() == Some(&Path::new("/data/deps/pdfium").join(sub)));
        assert!(matches!((flat, nested), (Some(f), Some(n)) if n == f + 1), "the archive's {sub}/ folder is looked in too: {c:?}");
        assert!(c.last().is_some_and(|p| p.components().count() == 1), "{c:?}");
    }
}
