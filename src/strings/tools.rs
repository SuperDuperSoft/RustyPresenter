//! User-facing strings for finding the tools imports need (Plan 4 Task 1).
use crate::tr;

pub fn install_hint_windows() -> String { tr!("Install LibreOffice (free) from libreoffice.org, or run: winget install TheDocumentFoundation.LibreOffice") }
pub fn install_hint_macos() -> String { tr!("Install LibreOffice (free) from libreoffice.org, or run: brew install --cask libreoffice") }
pub fn install_hint_linux() -> String { tr!("Install LibreOffice Impress, for example: sudo apt install libreoffice-impress") }

pub fn file_url_failed(path: &str) -> String {
    tr!("Cannot make a file address from {path}", path = path)
}
