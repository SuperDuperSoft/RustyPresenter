//! FEAT-1: themes, backgrounds, rich text and overlays (core, Part A).
#[path = "../strings/themes.rs"]
pub mod strings;
pub mod backgrounds;
pub mod richtext;
pub mod saved;
pub mod style;

pub use saved::*;
pub use style::{Theme, TextAlign, clamp_style};
