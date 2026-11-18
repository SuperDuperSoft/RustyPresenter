//! A message box for when there is no operator window to show an error in.

/// The console gets it too, so a run from a terminal keeps a copy after the box is closed.
pub fn error(msg: &str) {
    eprintln!("{msg}");
    rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title("RustyPresenter").set_description(msg).show();
}
