//! Native screens: the `.slint` files under `ui/` (compiled by `build.rs` from `ui/app.slint`) and their Rust glue.
slint::include_modules!();

pub mod stage;
pub mod room;
