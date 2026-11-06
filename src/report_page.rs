//! Pages the app writes for the operator to print or keep (FEAT-6b print, later FEAT-7 reports): saved in
//! `<data>/reports/` and opened in the computer's web browser, where Print (and "Save as PDF") are.
use std::{fs, io::Write, path::{Path, PathBuf}, time::{Duration, SystemTime}};

/// Pages older than this are deleted when the next one is written.
const KEEP: Duration = Duration::from_secs(30 * 24 * 3600);

pub fn reports_dir(root: &Path) -> PathBuf { root.join("reports") }

/// What Windows, macOS and Linux all accept in a file name: `name` without `<>:"/\|?*` and control characters.
pub fn safe_name(name: &str, fallback: &str) -> String {
    let clean: String = name.chars().filter(|c| !c.is_control() && !r#"<>:"/\|?*"#.contains(*c)).collect();
    let clean = clean.trim().trim_matches('.').trim();
    if clean.is_empty() { fallback.into() } else { clean.into() }
}

/// Writes `text` to `path` through a `.part` file, so a failed write leaves any older file as it was.
pub fn write_to(path: &Path, text: &str) -> Result<(), String> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let r = (|| {
        let mut f = fs::File::create(&part)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        fs::rename(&part, path)
    })();
    if r.is_err() { let _ = fs::remove_file(&part); }
    r.map_err(|e| crate::print::strings::cant_save(&e.to_string()))
}

/// Deletes `reports/*.html` older than 30 days.
pub fn clean_old(root: &Path) {
    let Ok(entries) = fs::read_dir(reports_dir(root)) else { return };
    let now = SystemTime::now();
    for e in entries.flatten() {
        let p = e.path();
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| now.duration_since(t).ok()).is_some_and(|age| age > KEEP);
        if old && p.extension().is_some_and(|x| x == "html") { let _ = fs::remove_file(p); }
    }
}

/// Writes `<root>/reports/<stem>-<YYYYMMDD-HHMMSS>.html` and opens it. Second value: whether a program was started
/// to open it.
pub fn write_and_open(root: &Path, stem: &str, html: &str) -> Result<(PathBuf, bool), String> {
    clean_old(root);
    let dir = reports_dir(root);
    fs::create_dir_all(&dir).map_err(|e| crate::print::strings::cant_make(&e.to_string()))?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let path = dir.join(format!("{}-{stamp}.html", safe_name(stem, "page")));
    write_to(&path, html).map_err(|e| crate::print::strings::cant_make(&e))?;
    let opened = open(&path);
    Ok((path, opened))
}

/// Opens a file with the program the computer uses for it (the web browser for a page).
#[cfg(not(test))]
pub fn open(path: &Path) -> bool {
    use std::process::{Command, Stdio};
    #[cfg(windows)]
    let mut c = Command::new("explorer");
    #[cfg(target_os = "macos")]
    let mut c = Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut c = Command::new("xdg-open");
    c.arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_ok()
}

/// Tests never start a browser: the page is noted instead.
#[cfg(test)]
pub fn open(path: &Path) -> bool {
    testing::OPENED.lock().unwrap().push(path.to_path_buf());
    true
}

#[cfg(test)]
pub mod testing {
    use std::{path::PathBuf, sync::Mutex};
    pub static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_page_writes_to_reports_and_cleans_old() {
        let root = std::env::temp_dir().join(format!("rp-report-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(reports_dir(&root)).unwrap();
        let old = reports_dir(&root).join("old.html");
        fs::write(&old, "x").unwrap();
        let f = fs::File::options().write(true).open(&old).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(31 * 24 * 3600)).unwrap();
        drop(f);
        let keep = reports_dir(&root).join("notes.txt");
        fs::write(&keep, "x").unwrap();

        let (path, opened) = write_and_open(&root, "Easter: order?", "<p>hi</p>").unwrap();
        assert!(opened);
        assert_eq!(path.parent().unwrap(), reports_dir(&root));
        assert!(path.file_name().unwrap().to_string_lossy().starts_with("Easter order-"), "{path:?}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "<p>hi</p>");
        assert!(!old.exists(), "a month-old page is gone");
        assert!(keep.exists(), "only pages are cleaned");
        assert!(testing::OPENED.lock().unwrap().contains(&path));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn safe_names() {
        assert_eq!(safe_name("Easter: 9/10 AM?", "x"), "Easter 910 AM");
        assert_eq!(safe_name("...", "x"), "x");
    }
}
