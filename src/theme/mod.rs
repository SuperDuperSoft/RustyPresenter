//! FEAT-1: themes, backgrounds, rich text and overlays (core, Part A).
#[path = "../strings/themes.rs"]
pub mod strings;
pub mod actions;
#[cfg(test)]
mod app_tests;
#[cfg(test)]
mod live_tests;
pub mod assets;
pub mod backgrounds;
pub mod builtins;
pub mod layouts;
pub mod merge;
pub mod migrate;
pub mod overlay;
pub mod paint;
pub mod resolve;
pub mod rich_edit;
pub mod richtext;
pub mod saved;
pub mod style;

pub use resolve::{church_default, draws_text, find, resolve, service_of};
pub use saved::*;
pub use layouts::{Layout, LayoutKind};
pub use style::{Theme, TextAlign, clamp_style};
