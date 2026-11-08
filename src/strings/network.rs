//! User-facing strings of the network extras (N31). Declared from `src/network/mod.rs` with a `#[path]` line.
use crate::tr;

pub fn password_short() -> String { tr!("Use at least 4 characters for a password.") }
pub fn password_missing() -> String { tr!("Type a password, or turn the password off.") }
pub fn port_range() -> String { tr!("Use a port from 1024 to 65535.") }
pub fn port_taken(port: u16) -> String { tr!("Port {port} is the app's own. Pick another one.", port = port) }
pub fn tcp_failed(port: u16, error: &str) -> String { tr!("The TCP control could not use port {port}: {error}", port = port, error = error) }
pub fn wrong_password() -> String { tr!("That password isn't right.") }
pub fn too_fast() -> String { tr!("Too fast — wait a second and try again.") }
pub fn stage_view_off() -> String { tr!("The stage view is turned off on the presentation computer.") }
pub fn saved() -> String { tr!("Network settings saved.") }
