//! User-facing strings for presentation imports (Plan 4 Tasks 4 and 5).

pub const NO_SLIDES: &str = "This presentation has no slides.";
pub const MIXED_SIZES: &str = "This presentation mixes slide sizes, which RustyPresenter can't show.";
pub const LO_UNREADABLE: &str = "LibreOffice couldn't read this presentation.";
pub const PDF_UNREADABLE: &str = "This PDF can't be opened. If it has a password, remove it and import again.";
pub const PDFIUM_MISSING: &str = "The slide converter (pdfium) is missing. Reinstall RustyPresenter.";
pub const PDFIUM_STOPPED: &str = "The slide converter stopped responding. Restart RustyPresenter and try again.";
pub const SLIDE_NOT_SAVED: &str = "A slide could not be saved. Check that the disk has free space, then try again.";
pub const LO_TOO_LONG: &str = "LibreOffice took too long and was stopped. Try again; the first import after starting the computer is slower.";
pub const LO_SNAP: &str = "LibreOffice from Snap can't read RustyPresenter's files. Install LibreOffice from your distribution instead (sudo apt install libreoffice-impress).";
pub const LO_NOT_STARTED: &str = "LibreOffice could not be started. Reinstall it and try again.";
