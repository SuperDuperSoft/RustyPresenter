//! The native operator window (Plan 3), rebuilt 1:1 from `web/operator.html`, `operator.css` and `operator.js`.
//! Pure logic that needs no window lives in `view`; the Slint glue arrives with the window itself.
pub mod file_input;
pub mod files;
#[path = "../../strings/operator.rs"]
pub mod strings;
pub mod view;
