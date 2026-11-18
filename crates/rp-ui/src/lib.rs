//! The screens Slint makes from `ui/app.slint`. Kept in their own crate: the generated code is large, and building
//! it apart keeps the app's own build smaller (less memory, and editing Rust code does not rebuild it).
slint::include_modules!();
// Dock panels: the globals panels show, written to every window (see `build.rs`, `panes`).
include!(concat!(env!("OUT_DIR"), "/panes.rs"));
