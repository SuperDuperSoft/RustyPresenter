//! User-facing strings for finding the tools imports need (Plan 4 Task 1).

pub const INSTALL_HINT_WINDOWS: &str = "Install LibreOffice (free) from libreoffice.org, or run: winget install TheDocumentFoundation.LibreOffice";
pub const INSTALL_HINT_MACOS: &str = "Install LibreOffice (free) from libreoffice.org, or run: brew install --cask libreoffice";
pub const INSTALL_HINT_LINUX: &str = "Install LibreOffice Impress, for example: sudo apt install libreoffice-impress";

pub fn file_url_failed(path: &str) -> String {
    format!("Cannot make a file address from {path}")
}
