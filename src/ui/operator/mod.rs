//! The native operator window (Plan 3), rebuilt 1:1 from `web/operator.html`, `operator.css` and `operator.js`.
//! Pure logic that needs no window lives in `view`; the window and its update follower in `window`.
pub mod add_item;
pub mod automation;
pub mod bible;
pub mod bible_extras;
pub mod checks;
pub mod file_input;
#[cfg(test)]
mod fit;
pub mod files;
pub mod help;
#[path = "../../strings/operator.rs"]
pub mod strings;
pub mod keys;
pub mod list_filter;
pub mod live;
pub mod media_bin;
pub mod video_inputs;
pub mod menu;
pub mod outputs;
pub mod overlay_strip;
pub mod overlays;
pub mod service;
pub mod service_theme;
pub mod slides;
pub mod stage_only;
#[cfg(test)]
mod song_flows;
pub mod text_size;
pub mod transfer;
pub mod view;
pub mod window;
