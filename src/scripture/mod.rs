//! FEAT-5 Bible extras core: keyword search, two translations and slide breaks.
//! Pure and unit-tested; Part B wires it into the app, slides and render.

pub mod cache;
pub mod input;
pub mod sources;
pub mod types;
// FEAT-5 B1: switch to crate::strings::scripture once src/strings/mod.rs exists
#[path = "../strings/scripture.rs"]
pub mod strings;
#[cfg(test)]
pub(crate) mod test_support;

pub use types::*;
