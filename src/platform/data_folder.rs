//! Where the app keeps its data: songs, services, settings, logs, backups, pictures and videos, downloaded models,
//! FFmpeg and Bibles. Every part of the app asks here (through `server::data_dir`).
//!
//! Normally that is the person's own folder on this computer (`%LOCALAPPDATA%\RustyPresenter` on Windows,
//! `~/Library/Application Support/RustyPresenter` on macOS, `~/.local/share/RustyPresenter` on Linux).
//!
//! A **portable copy** (Jake, November 18, 2026: "a portable version" of every release) has a file
//! `portable.txt` beside the app: beside `rustypresenter.exe`, beside `RustyPresenter.app`, or beside the AppImage.
//! Then the data is kept in the folder `RustyPresenter Data` beside it, so the whole folder can go on a USB stick
//! and work the same on any computer. When that folder cannot be written to (a locked USB stick, a CD), or the app
//! was opened from inside the zip, or macOS opened a locked copy of it ("app translocation": an app that Finder has
//! not moved since it was downloaded runs from a hidden read-only folder, and the folder it came from cannot be
//! seen), the data goes to the person's own folder for now and [`Found::problem`] says why, in plain words.
//!
//! `RUSTYPRESENTER_DATA` still wins over both (tests and the graphics test use it).
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The file beside the app that makes it a portable copy. What is in it does not matter.
pub const MARKER: &str = "portable.txt";
/// The folder beside the app that a portable copy keeps its data in.
pub const FOLDER: &str = "RustyPresenter Data";

/// Why a portable copy is not keeping its data beside it this time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The folder beside the app cannot be written to (`wanted`).
    ReadOnly { wanted: PathBuf },
    /// The app was opened from inside a zip file that was not unzipped.
    InZip,
    /// macOS opened a locked copy of the app in a hidden place, because it was not moved after downloading.
    Translocated,
}

/// Where the data goes, and what to tell the person about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// The data folder. None only when this computer has no folder for the person's data at all.
    pub dir: Option<PathBuf>,
    /// The data is beside the app (a portable copy that works).
    pub portable: bool,
    pub problem: Option<Problem>,
}

/// The data folder for this run: `RUSTYPRESENTER_DATA`, else the portable folder or the person's own folder.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(d) = env_dir(&|k| std::env::var_os(k)) { return Some(d) }
    current().dir.clone()
}

/// The portable data folder when this run keeps its data beside the app.
pub fn portable_dir() -> Option<PathBuf> {
    if env_dir(&|k| std::env::var_os(k)).is_some() { return None }
    let f = current();
    f.portable.then(|| f.dir.clone()).flatten()
}

/// What to tell the person at start when a portable copy cannot keep its data beside it. None when all is well.
pub fn start_warning() -> Option<String> {
    if env_dir(&|k| std::env::var_os(k)).is_some() { return None }
    let f = current();
    Some(message(f.problem.as_ref()?, f.dir.as_deref().unwrap_or(Path::new(""))))
}

/// This run's answer, worked out once: the folder must not change while the app is open.
pub fn current() -> &'static Found {
    static FOUND: OnceLock<Found> = OnceLock::new();
    FOUND.get_or_init(|| resolve(&super::resources::current_exe(), &|k| std::env::var_os(k), per_user(), &can_write))
}

/// The person's own data folder on this computer.
pub fn per_user() -> Option<PathBuf> { dirs::data_local_dir().map(|p| p.join("RustyPresenter")) }

fn env_dir(var: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    var("RUSTYPRESENTER_DATA").filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Works out the data folder for the app at `exe`. `per_user` is the person's own folder; `writable` says whether
/// a folder can be made and written to (it may make the folder).
pub fn resolve(exe: &Path, var: &dyn Fn(&str) -> Option<OsString>, per_user: Option<PathBuf>, writable: &dyn Fn(&Path) -> bool) -> Found {
    if let Some(d) = env_dir(var) { return Found { dir: Some(d), portable: false, problem: None } }
    let fallback = |problem| Found { dir: per_user.clone(), portable: false, problem: Some(problem) };
    // macOS hides the folder the .app was in, so the file beside it cannot be seen: say so whatever it was.
    if translocated(exe) { return fallback(Problem::Translocated) }
    let Some(home) = app_folder(exe, var) else { return Found { dir: per_user, portable: false, problem: None } };
    if !home.join(MARKER).is_file() { return Found { dir: per_user, portable: false, problem: None } }
    if inside_zip(&home) { return fallback(Problem::InZip) }
    let data = home.join(FOLDER);
    if !writable(&data) { return fallback(Problem::ReadOnly { wanted: data }) }
    Found { dir: Some(data), portable: true, problem: None }
}

/// The folder the person sees the app in: the one holding the AppImage, the `.app`, or the exe.
pub fn app_folder(exe: &Path, var: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    // An AppImage runs from a hidden folder it mounts; APPIMAGE is the file the person double-clicked.
    if let Some(image) = var("APPIMAGE").filter(|v| !v.is_empty()) { return Path::new(&image).parent().map(Path::to_path_buf) }
    let dir = exe.parent()?;
    let bundle = dir.parent().and_then(Path::parent).filter(|app| {
        dir.file_name().is_some_and(|n| n == "MacOS")
            && dir.parent().and_then(Path::file_name).is_some_and(|n| n == "Contents")
            && app.extension().is_some_and(|e| e == "app")
    });
    match bundle {
        Some(app) => app.parent().map(Path::to_path_buf),
        None => Some(dir.to_path_buf()),
    }
}

/// macOS runs an app that was not moved after downloading from `/private/var/folders/.../AppTranslocation/...`, a
/// locked copy of the `.app` alone.
pub fn translocated(exe: &Path) -> bool { exe.components().any(|c| c.as_os_str() == "AppTranslocation") }

/// Opened from inside a zip without unzipping it: Windows unpacks it into a throwaway `Temp1_<name>.zip` folder,
/// Linux file managers open it as `.../archive:...zip/...`.
pub fn inside_zip(dir: &Path) -> bool {
    dir.components().any(|c| c.as_os_str().to_string_lossy().to_lowercase().contains(".zip"))
}

/// The folder can be made and a file written in it (and the file is removed again).
pub fn can_write(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() { return false }
    let probe = dir.join(format!(".write-test-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// What the person is told, with `here` the folder the data goes to instead.
pub fn message(problem: &Problem, here: &Path) -> String {
    match problem {
        Problem::ReadOnly { wanted } => crate::tr!(
            "This copy of RustyPresenter keeps its data beside it, in {wanted}, but that folder cannot be changed (it may be a locked USB stick or a CD). For now your work is kept on this computer, in {here}. To keep it beside the app, copy the whole RustyPresenter folder to a place that can be changed and open it from there.",
            wanted = wanted.display(), here = here.display()),
        Problem::InZip => crate::tr!(
            "RustyPresenter was opened from inside the zip file, so it cannot keep its data beside it. For now your work is kept on this computer, in {here}. Close RustyPresenter, unzip the whole file first (on Windows: right-click it, then Extract All), and open RustyPresenter from the folder that makes.",
            here = here.display()),
        Problem::Translocated => crate::tr!(
            "macOS opened a locked copy of RustyPresenter, because it has not been moved since it was downloaded. For now your work is kept on this computer, in {here}. Close RustyPresenter. In Finder, drag RustyPresenter out of its folder (onto the Desktop, say) and back in again, or into Applications if it came from the .dmg file. Then open it again.",
            here = here.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_var(_: &str) -> Option<OsString> { None }
    fn yes(_: &Path) -> bool { true }
    fn no(_: &Path) -> bool { false }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rp-portable-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn mine() -> Option<PathBuf> { Some(PathBuf::from("/home/me/.local/share/RustyPresenter")) }

    #[test]
    fn without_the_file_the_data_is_in_the_persons_own_folder() {
        let dir = temp("plain");
        let f = resolve(&dir.join("rustypresenter.exe"), &no_var, mine(), &yes);
        assert_eq!(f, Found { dir: mine(), portable: false, problem: None });
        assert!(!dir.join(FOLDER).exists(), "nothing is made beside an installed app");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn with_the_file_the_data_is_beside_the_app() {
        let dir = temp("on");
        std::fs::write(dir.join(MARKER), "portable").unwrap();
        let f = resolve(&dir.join("rustypresenter.exe"), &no_var, mine(), &can_write);
        assert_eq!(f, Found { dir: Some(dir.join(FOLDER)), portable: true, problem: None });
        assert!(dir.join(FOLDER).is_dir(), "the folder is made");
        assert_eq!(std::fs::read_dir(dir.join(FOLDER)).unwrap().count(), 0, "the write test leaves nothing behind");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_folder_with_the_files_name_is_not_the_file() {
        let dir = temp("dir-marker");
        std::fs::create_dir(dir.join(MARKER)).unwrap();
        assert!(!resolve(&dir.join("rp"), &no_var, mine(), &yes).portable);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_folder_that_cannot_be_written_falls_back_and_says_why() {
        let dir = temp("locked");
        std::fs::write(dir.join(MARKER), "").unwrap();
        let f = resolve(&dir.join("rp"), &no_var, mine(), &no);
        assert_eq!(f, Found { dir: mine(), portable: false, problem: Some(Problem::ReadOnly { wanted: dir.join(FOLDER) }) });
        let m = message(f.problem.as_ref().unwrap(), f.dir.as_deref().unwrap());
        assert!(m.contains(&dir.join(FOLDER).display().to_string()) && m.contains("RustyPresenter"), "{m}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_real_folder_that_cannot_be_made_is_not_writable() {
        // A file where the data folder should be: the folder cannot be made, as on a locked stick.
        let dir = temp("blocked");
        std::fs::write(dir.join(MARKER), "").unwrap();
        std::fs::write(dir.join(FOLDER), "in the way").unwrap();
        assert!(!can_write(&dir.join(FOLDER)));
        let f = resolve(&dir.join("rp"), &no_var, mine(), &can_write);
        assert!(matches!(f.problem, Some(Problem::ReadOnly { .. })) && f.dir == mine(), "{f:?}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_environment_wins_over_the_file() {
        let dir = temp("env");
        std::fs::write(dir.join(MARKER), "").unwrap();
        let var = |k: &str| (k == "RUSTYPRESENTER_DATA").then(|| OsString::from("/tests/data"));
        assert_eq!(resolve(&dir.join("rp"), &var, mine(), &yes), Found { dir: Some("/tests/data".into()), portable: false, problem: None });
        let empty = |k: &str| (k == "RUSTYPRESENTER_DATA").then(OsString::new);
        assert!(resolve(&dir.join("rp"), &empty, mine(), &yes).portable, "an empty setting is ignored");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn opened_inside_a_zip_falls_back() {
        let root = temp("zip");
        let dir = root.join("Temp1_RustyPresenter-1.4.0-windows-portable.zip").join("RustyPresenter");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MARKER), "").unwrap();
        let f = resolve(&dir.join("rustypresenter.exe"), &no_var, mine(), &yes);
        assert_eq!(f.problem, Some(Problem::InZip));
        assert_eq!(f.dir, mine());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn the_folder_is_beside_the_mac_app_and_the_appimage() {
        let exe = Path::new("/Users/me/Documents/RustyPresenter/RustyPresenter.app/Contents/MacOS/rustypresenter");
        assert_eq!(app_folder(exe, &no_var).as_deref(), Some(Path::new("/Users/me/Documents/RustyPresenter")));
        let image = |k: &str| (k == "APPIMAGE").then(|| OsString::from("/media/stick/RustyPresenter-1.4.0-linux.AppImage"));
        assert_eq!(app_folder(Path::new("/tmp/.mount_abc/usr/bin/rustypresenter"), &image).as_deref(), Some(Path::new("/media/stick")));
        assert_eq!(app_folder(Path::new("/opt/rp/rustypresenter"), &no_var).as_deref(), Some(Path::new("/opt/rp")));
    }

    #[test]
    fn a_locked_mac_copy_falls_back_and_says_to_move_it() {
        let exe = Path::new("/private/var/folders/xy/abc/T/AppTranslocation/1234-5678/d/RustyPresenter.app/Contents/MacOS/rustypresenter");
        assert!(translocated(exe));
        let f = resolve(exe, &no_var, mine(), &yes);
        assert_eq!(f, Found { dir: mine(), portable: false, problem: Some(Problem::Translocated) });
        assert!(message(&Problem::Translocated, Path::new("/x")).contains("Finder"));
        assert!(!translocated(Path::new("/Applications/RustyPresenter.app/Contents/MacOS/rustypresenter")));
    }

    #[test]
    fn this_test_run_is_not_portable() {
        // The test program has no portable.txt beside it, so the app's own answer is the person's folder.
        assert!(!current().portable);
        assert_eq!(current().problem, None);
    }
}
