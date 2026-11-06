//! Opens a file with the computer's own program for it (the printable page in the browser).
use std::path::Path;

/// Starts the program and returns at once; a thread waits for it so it leaves nothing behind. Tests open nothing.
pub fn open_file(path: &Path) -> Result<(), String> {
    if cfg!(test) { return Ok(()) }
    let program = if cfg!(windows) { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let mut child = std::process::Command::new(program).arg(path)
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .spawn().map_err(|e| e.to_string())?;
    std::thread::spawn(move || { let _ = child.wait(); });
    Ok(())
}
