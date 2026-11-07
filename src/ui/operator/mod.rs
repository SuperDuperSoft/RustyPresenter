//! The native operator window (Plan 3), rebuilt 1:1 from `web/operator.html`, `operator.css` and `operator.js`.
//! Pure logic that needs no window lives in `view`; the window and its update follower in `window`.
pub mod add_item;
pub mod bible;
pub mod checks;
pub mod file_input;
#[cfg(test)]
mod fit;
pub mod files;
#[path = "../../strings/operator.rs"]
pub mod strings;
pub mod keys;
pub mod live;
pub mod media_bin;
pub mod menu;
pub mod overlay_strip;
pub mod overlays;
pub mod service;
pub mod service_theme;
pub mod slides;
#[cfg(test)]
mod song_flows;
pub mod text_size;
pub mod view;
pub mod window;
