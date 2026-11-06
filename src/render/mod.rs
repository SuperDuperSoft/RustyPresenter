//! Slide rendering shared by every screen: the room output, the live preview and the operator
//! thumbnails all draw text slides with this code, so what you see is what the room sees.
pub mod cache;
pub mod color;
pub mod layout;
pub mod parallel;
pub mod raster;

pub use cache::{Generation, RenderKey, Renderer, Wanted};
pub use color::parse_color;
pub use layout::{Fonts, Rect, TextLayout, TextSlide, fit_steps, layout, passage_px};
pub use raster::{Premul, Rendered, render_text};
