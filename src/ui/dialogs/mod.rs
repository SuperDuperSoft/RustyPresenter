//! The operator's dialogs (Plan 3 Tasks 13–21), which are overlays inside the operator window. So far this holds
//! only the logic that needs no window; the `.slint` files and glue arrive with those tasks.
pub mod add;
pub mod add_media;
pub mod background;
pub mod screens;
pub mod settings;
pub mod song_editor;
#[path = "../../strings/dialogs.rs"]
pub mod strings;

/// A status chip's colour (`.status-chip` in `operator.css`). `Plain` is the web's chip with no extra class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone { Plain, Ok, Warn, Bad }
