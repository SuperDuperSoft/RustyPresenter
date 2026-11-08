//! User-facing strings of the slide editor (N26, N27).
//! Declared from `src/design/mod.rs` with a `#[path]` line, like `strings/stage.rs`.
use crate::tr;

pub fn too_many_slides() -> String { tr!("A slide set can have at most 200 slides.") }
pub fn too_many_objects() -> String { tr!("A slide can have at most 60 objects.") }
/// The words a new text box starts with.
pub fn sample_text() -> String { tr!("Your words here") }
/// The dialog's title.
pub fn editor_title() -> String { tr!("Slide editor") }
/// A new slide set's name.
pub fn new_title() -> String { tr!("New slides") }
/// A slide's name in the list: "Slide 3".
pub fn slide_name(n: usize) -> String { tr!("Slide {n}", n = n) }
/// A build step's name under the slide's: "Slide 3 · 2 of 4".
pub fn build_name(n: usize, step: u16, of: u16) -> String { tr!("Slide {n} · {step} of {of}", n = n, step = step, of = of) }
pub fn saved() -> String { tr!("Slides saved") }
pub fn discard_question() -> String { tr!("Throw away the changes to these slides?") }
pub fn discard() -> String { tr!("Throw away") }
pub fn keep_editing() -> String { tr!("Keep editing") }
pub fn style_copied() -> String { tr!("Style copied. Choose other boxes (or none, for every text box) and press Ctrl+Alt+V.") }
pub fn nothing_to_copy() -> String { tr!("Choose a box to copy its style.") }
pub fn nothing_copied() -> String { tr!("Copy a style first (Ctrl+Alt+C).") }
pub fn style_pasted() -> String { tr!("Style pasted") }
pub fn picture_unreadable() -> String { tr!("That picture could not be read.") }
/// A video object, drawn where it sits until it plays.
pub fn video_label() -> String { tr!("Video") }
/// A web page object's label before its address.
pub fn web_label() -> String { tr!("Web page") }
