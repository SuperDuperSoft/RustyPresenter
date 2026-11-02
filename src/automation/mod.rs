//! FEAT-2 service automation: timers, message templates, macros, auto-advance and the
//! pre-service countdown. Pure logic here; the App, live engine and UI wire it in (Part B).
//!
//! The strings live at their final path, `src/strings/automation.rs`. Task B1 turns the
//! `#[path]` line below into `pub use crate::strings::automation as strings;`.
#[path = "../strings/automation.rs"]
pub mod strings;

pub mod advance;
pub mod macros;
pub mod messages;
