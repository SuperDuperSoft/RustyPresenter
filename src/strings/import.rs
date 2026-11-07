//! User-facing strings for importing decks and media (Plan 4).
use crate::tr;

pub fn encrypted() -> String { tr!("This presentation has a password. Open it in PowerPoint, remove the password (File → Info → Protect), save, and import again.") }
pub fn not_presentation() -> String { tr!("This doesn't look like a PowerPoint file.") }

// The slide converters (Plan 4 Tasks 4 and 5).
pub fn lo_timeout() -> String { tr!("LibreOffice took too long and was stopped. Try again; the first import after starting the computer is slower.") }
pub fn lo_cannot_read() -> String { tr!("LibreOffice couldn't read this presentation.") }
pub fn lo_snap() -> String { tr!("LibreOffice from Snap can't read RustyPresenter's files. Install LibreOffice from your distribution instead (sudo apt install libreoffice-impress).") }
/// The raw OS error goes to the log, never to the operator.
pub fn lo_not_started() -> String { tr!("LibreOffice could not be started. Reinstall it and try again.") }
// PowerPoint on Windows (Plan 4 Task 6b). PowerPoint's own error text goes to the log, never to the operator.
pub fn ppt_timeout() -> String { tr!("PowerPoint took too long and was stopped. Try again.") }
pub fn ppt_cannot_read() -> String { tr!("PowerPoint couldn't open this presentation.") }
pub fn ppt_not_started() -> String { tr!("PowerPoint could not be started.") }
pub fn not_pdf() -> String { tr!("This doesn't look like a PDF file.") }

// Media probes (Plan 4 Task 7). The raw mpv error goes to the log, never to the operator.
pub fn cant_play() -> String { tr!("This file can't be played.") }
pub fn too_slow() -> String { tr!("This file took too long to open.") }
pub fn bad_picture() -> String { tr!("This picture can't be opened. Save it as PNG or JPEG and try again.") }

pub fn pdf_no_slides() -> String { tr!("This presentation has no slides.") }
pub fn pdf_mixed_sizes() -> String { tr!("This presentation mixes slide sizes, which RustyPresenter can't show.") }
pub fn pdfium_missing() -> String { tr!("The slide converter (pdfium) is missing. Reinstall RustyPresenter.") }
pub fn pdfium_stuck() -> String { tr!("The slide converter stopped responding. Restart RustyPresenter and try again.") }

pub fn pdf_page_failed(page: usize) -> String {
    tr!("Slide {page} could not be turned into a picture.", page = page)
}
pub fn pdf_cannot_open() -> String { tr!("This PDF can't be opened. If it has a password, remove it and import again.") }
pub fn slide_not_saved() -> String { tr!("A slide could not be saved. Check that the disk has free space, then try again.") }

// Adding media (Plan 4 Task 8).
pub fn waiting_for_file() -> String { tr!("Waiting for another file…") }
pub fn adding(name: &str) -> String {
    tr!("Adding “{name}”", name = name)
}

// Importing a presentation (Plan 4 Task 6c).
pub fn importing(name: &str) -> String {
    tr!("Importing “{name}”", name = name)
}
pub fn keynote_package() -> String { tr!("Save the Keynote file as a single file and import again.") }
pub fn waiting_for_libreoffice() -> String { tr!("Waiting for LibreOffice…") }
pub fn waiting_for_powerpoint() -> String { tr!("Waiting for PowerPoint…") }
/// `App::job_note` keeps this one beside any other note.
pub fn fallback_note() -> String { tr!("PowerPoint couldn't open it; used LibreOffice.") }
pub fn libreoffice_note() -> String { tr!("Imported with LibreOffice — check the slides.") }
pub fn slide_count_differs(expected: usize, imported: usize) -> String {
    tr!("{expected} slides were expected but {imported} were imported.", expected = expected, imported = imported)
}
/// Both notes in one: imported with LibreOffice, and a different number of slides.
pub fn libreoffice_note_slide_count(expected: usize, imported: usize) -> String {
    tr!("Imported with LibreOffice — check the slides. {expected} slides were expected but {imported} were imported.",
        expected = expected, imported = imported)
}
pub fn install_libreoffice(hint: &str) -> String {
    tr!("To import presentations, install LibreOffice. {hint}", hint = hint)
}
