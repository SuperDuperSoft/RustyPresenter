//! Crash recovery: the committed frame kept on disk, the supervisor that restarts a crashed app,
//! and the panic log.
pub mod exitcode;
pub mod headless;
pub mod panic;
pub mod restart;
pub mod restore;
pub mod safe_mode;
pub mod state;
pub mod supervisor;

use std::path::{Path, PathBuf};

/// `<data>/logs/native-YYYYMMDD.log` for `date`.
pub fn log_path(root: &Path, date: chrono::NaiveDate) -> PathBuf {
    root.join("logs").join(format!("native-{}.log", date.format("%Y%m%d")))
}

/// Today's log in the data folder. The folder comes from the environment only: the supervisor
/// reads no saved data.
pub fn todays_log() -> PathBuf {
    let root = crate::server::data_dir().unwrap_or_else(|| PathBuf::from("."));
    log_path(&root, chrono::Local::now().date_naive())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_file_is_per_day_in_the_logs_folder() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        assert_eq!(log_path(Path::new("data"), d), Path::new("data").join("logs").join("native-20261101.log"));
    }
}
