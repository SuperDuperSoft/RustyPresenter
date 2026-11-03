//! User-facing strings for importing decks and media (Plan 4).

pub const ENCRYPTED: &str = "This presentation has a password. Open it in PowerPoint, remove the password (File → Info → Protect), save, and import again.";
pub const NOT_PRESENTATION: &str = "This doesn't look like a PowerPoint file.";

// The slide converters (Plan 4 Tasks 4 and 5).
pub const LO_TIMEOUT: &str = "LibreOffice took too long and was stopped. Try again; the first import after starting the computer is slower.";
pub const LO_CANNOT_READ: &str = "LibreOffice couldn't read this presentation.";
pub const LO_SNAP: &str = "LibreOffice from Snap can't read RustyPresenter's files. Install LibreOffice from your distribution instead (sudo apt install libreoffice-impress).";
/// The raw OS error goes to the log, never to the operator.
pub const LO_NOT_STARTED: &str = "LibreOffice could not be started. Reinstall it and try again.";
pub const NOT_PDF: &str = "This doesn't look like a PDF file.";

pub const PDF_NO_SLIDES: &str = "This presentation has no slides.";
pub const PDF_MIXED_SIZES: &str = "This presentation mixes slide sizes, which RustyPresenter can't show.";
pub const PDFIUM_MISSING: &str = "The slide converter (pdfium) is missing. Reinstall RustyPresenter.";
pub const PDFIUM_STUCK: &str = "The slide converter stopped responding. Restart RustyPresenter and try again.";

pub fn pdf_page_failed(page: usize) -> String {
    format!("Slide {page} could not be turned into a picture.")
}
pub const PDF_CANNOT_OPEN: &str = "This PDF can't be opened. If it has a password, remove it and import again.";
pub const SLIDE_NOT_SAVED: &str = "A slide could not be saved. Check that the disk has free space, then try again.";

// PowerPoint (Plan 4 Task 6b). Task 6c falls back to LibreOffice after any of these.
pub const PPT_TIMEOUT: &str = "PowerPoint took too long and was stopped.";
pub const PPT_CANNOT_OPEN: &str = "PowerPoint couldn't open this presentation.";
/// The raw OS error goes to the log, never to the operator.
pub const PPT_NOT_STARTED: &str = "PowerPoint could not be started. Check that it opens by itself, then try again.";
