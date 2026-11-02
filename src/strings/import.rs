//! User-facing strings for the slide converters (Plan 4 Tasks 4 and 5).

pub const LO_TIMEOUT: &str = "LibreOffice took too long and was stopped. Try again; the first import after starting the computer is slower.";
pub const LO_CANNOT_READ: &str = "LibreOffice couldn't read this presentation.";
pub const LO_SNAP: &str = "LibreOffice from Snap can't read RustyPresenter's files. Install LibreOffice from your distribution instead (sudo apt install libreoffice-impress).";
pub const NOT_PDF: &str = "This doesn't look like a PDF file.";

pub fn lo_could_not_start(reason: &str) -> String {
    format!("LibreOffice could not start: {reason}")
}

pub const PDF_NO_SLIDES: &str = "This presentation has no slides.";
pub const PDF_MIXED_SIZES: &str = "This presentation mixes slide sizes, which RustyPresenter can't show.";
pub const PDFIUM_MISSING: &str = "The slide converter (pdfium) is missing. Reinstall RustyPresenter.";
pub const PDFIUM_STUCK: &str = "The slide converter stopped responding. Restart RustyPresenter and try again.";

pub fn pdf_page_failed(page: usize) -> String {
    format!("Slide {page} could not be turned into a picture.")
}
pub const PDF_CANNOT_OPEN: &str = "RustyPresenter couldn't open this PDF.";
