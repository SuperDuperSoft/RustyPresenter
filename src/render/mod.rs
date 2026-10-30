//! Slide rendering shared by every screen: the room output, the live preview and the operator
//! thumbnails all draw text slides with this code, so what you see is what the room sees.
pub mod layout;

pub use layout::{Fonts, Rect, TextLayout, TextSlide, fit_steps, layout};
