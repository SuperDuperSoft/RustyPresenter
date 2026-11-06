//! The native operator window (Plan 3), rebuilt 1:1 from `web/operator.html`, `operator.css` and `operator.js`.
//! Pure logic that needs no window lives in `view`; the window and its update follower in `window`.
pub mod add_item;
pub mod bible;
pub mod file_input;
#[cfg(test)]
mod fit;
pub mod files;
#[path = "../../strings/operator.rs"]
pub mod strings;
pub mod keys;
pub mod live;
pub mod overlays;
pub mod service;
pub mod slides;
pub mod view;
pub mod window;
