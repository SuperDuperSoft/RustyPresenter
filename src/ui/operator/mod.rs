//! The native operator window (Plan 3), rebuilt 1:1 from `web/operator.html`, `operator.css` and `operator.js`.
//! Pure logic that needs no window lives in `view`; the window and its update follower in `window`.
#[path = "../../strings/operator.rs"]
pub mod strings;
pub mod keys;
pub mod view;
pub mod window;
