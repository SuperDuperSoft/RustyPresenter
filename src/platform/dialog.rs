//! A plain error box for when the app cannot start, before or without any window of its own.

/// Shows `msg` in the system's error dialog, and prints it for anyone reading the log.
pub fn error(msg: &str) {
    eprintln!("{msg}");
    rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title("RustyPresenter").set_description(msg).show();
}
