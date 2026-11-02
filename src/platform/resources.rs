//! Where the package put our files, for each kind of install (Plan 4 Task 1).
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The folder holding bundled files for this install.
pub fn resource_dir() -> PathBuf {
    resource_dir_from(&current_exe(), &|k| std::env::var_os(k))
}

/// `RUSTYPRESENTER_RESOURCES` → macOS bundle → AppImage → `/usr/bin` install → the exe's folder.
pub fn resource_dir_from(exe: &Path, var: &dyn Fn(&str) -> Option<OsString>) -> PathBuf {
    if let Some(dir) = non_empty(var, "RUSTYPRESENTER_RESOURCES") {
        return PathBuf::from(dir);
    }
    if let Some(contents) = bundle_contents(exe) {
        return contents.join("Resources");
    }
    if let Some(appdir) = non_empty(var, "APPDIR") {
        return Path::new(&appdir).join("usr/lib/rustypresenter");
    }
    let exe_dir = exe.parent().unwrap_or(Path::new(""));
    if exe_dir == Path::new("/usr/bin") {
        return PathBuf::from("/usr/lib/rustypresenter");
    }
    exe_dir.to_path_buf()
}

/// `<name>.app/Contents/Frameworks`, for an exe inside a macOS bundle.
pub fn frameworks_dir_from(exe: &Path) -> Option<PathBuf> {
    bundle_contents(exe).map(|c| c.join("Frameworks"))
}

/// Where to look for a native library, in order: the env path, the resource folder, the Frameworks folder,
/// the exe's folder, `<data>/deps/<deps_sub>/`, then the bare names for the system search.
/// A folder that is reached twice (the resource folder is often the exe's folder) is listed once.
pub fn native_candidates(names: &[&str], env_key: &str, exe: &Path, data_dir: &Path, deps_sub: &str, var: &dyn Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let mut dirs = vec![resource_dir_from(exe, var)];
    dirs.extend(frameworks_dir_from(exe));
    dirs.extend(exe.parent().map(Path::to_path_buf));
    dirs.push(data_dir.join("deps").join(deps_sub));
    dirs.push(PathBuf::new());

    let mut out: Vec<PathBuf> = non_empty(var, env_key).map(PathBuf::from).into_iter().collect();
    for dir in &dirs {
        for name in names {
            let path = dir.join(name);
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

pub(crate) fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_default()
}

fn non_empty(var: &dyn Fn(&str) -> Option<OsString>, key: &str) -> Option<OsString> {
    var(key).filter(|v| !v.is_empty())
}

/// `<name>.app/Contents`, when the exe sits at `<name>.app/Contents/MacOS/<exe>`.
fn bundle_contents(exe: &Path) -> Option<&Path> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let shaped = macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && app.extension()? == "app";
    shaped.then_some(contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_var(_: &str) -> Option<OsString> {
        None
    }

    fn one_var(key: &'static str, value: &'static str) -> impl Fn(&str) -> Option<OsString> {
        move |k| (k == key).then(|| OsString::from(value))
    }

    #[test]
    fn env_override_wins() {
        let var = one_var("RUSTYPRESENTER_RESOURCES", "/custom/res");
        assert_eq!(resource_dir_from(Path::new("/usr/bin/rustypresenter"), &var), Path::new("/custom/res"));
    }

    #[test]
    fn empty_env_override_is_ignored() {
        let var = one_var("RUSTYPRESENTER_RESOURCES", "");
        assert_eq!(resource_dir_from(Path::new("/opt/rp/rustypresenter"), &var), Path::new("/opt/rp"));
    }

    #[test]
    fn mac_bundle_uses_contents_resources() {
        let exe = Path::new("/Applications/RustyPresenter.app/Contents/MacOS/rustypresenter");
        assert_eq!(resource_dir_from(exe, &no_var), Path::new("/Applications/RustyPresenter.app/Contents/Resources"));
    }

    #[test]
    fn folder_merely_named_macos_is_not_a_bundle() {
        let exe = Path::new("/home/jake/Contents/MacOS/rustypresenter");
        assert_eq!(resource_dir_from(exe, &no_var), Path::new("/home/jake/Contents/MacOS"));
    }

    #[test]
    fn appimage_uses_appdir_usr_lib() {
        let var = one_var("APPDIR", "/tmp/.mount_rp");
        let exe = Path::new("/tmp/.mount_rp/usr/bin/rustypresenter");
        assert_eq!(resource_dir_from(exe, &var), Path::new("/tmp/.mount_rp/usr/lib/rustypresenter"));
    }

    #[test]
    fn deb_install_uses_usr_lib_name() {
        assert_eq!(resource_dir_from(Path::new("/usr/bin/rustypresenter"), &no_var), Path::new("/usr/lib/rustypresenter"));
    }

    #[test]
    fn nsis_and_dev_use_exe_dir() {
        let exe = Path::new("C:/Program Files/RustyPresenter/rustypresenter.exe");
        assert_eq!(resource_dir_from(exe, &no_var), Path::new("C:/Program Files/RustyPresenter"));
    }

    #[test]
    fn frameworks_only_inside_a_bundle() {
        let exe = Path::new("/Applications/RustyPresenter.app/Contents/MacOS/rustypresenter");
        assert_eq!(frameworks_dir_from(exe).as_deref(), Some(Path::new("/Applications/RustyPresenter.app/Contents/Frameworks")));
        assert_eq!(frameworks_dir_from(Path::new("/usr/bin/rustypresenter")), None);
    }

    #[test]
    fn native_candidates_order() {
        let exe = Path::new("/Applications/RP.app/Contents/MacOS/rp");
        let var = one_var("RP_LIB", "/env/lib.so");
        let got = native_candidates(&["lib.so"], "RP_LIB", exe, Path::new("/data"), "sub", &var);
        let want: Vec<PathBuf> = [
            "/env/lib.so",
            "/Applications/RP.app/Contents/Resources/lib.so",
            "/Applications/RP.app/Contents/Frameworks/lib.so",
            "/Applications/RP.app/Contents/MacOS/lib.so",
            "/data/deps/sub/lib.so",
            "lib.so",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn native_candidates_try_each_folder_once() {
        // Dev runs and NSIS installs: the resource folder is the exe folder.
        let got = native_candidates(&["a.dll", "b.dll"], "RP_LIB", Path::new("/app/rp.exe"), Path::new("/data"), "sub", &no_var);
        let want: Vec<PathBuf> = ["/app/a.dll", "/app/b.dll", "/data/deps/sub/a.dll", "/data/deps/sub/b.dll", "a.dll", "b.dll"].iter().map(PathBuf::from).collect();
        assert_eq!(got, want);
    }
}
