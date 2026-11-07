//! FEAT-4 song sections: kinds and colours, where each song slide came from, the jump rule, and what an edit to
//! the live item means for the room. Pure: no UI, no locks.
#[path = "../strings/song.rs"]
pub mod strings;

pub mod arrangement;
mod jump;
mod kind;
mod map;
mod occurrences;
mod reconcile;
pub(crate) mod refs;

pub use jump::{JumpError, JumpTarget, jump_for_key, jump_target, jump_toast};
pub use kind::{SectionKind, color_for_label};
pub(crate) use kind::translated_head;
#[cfg(test)]
pub(crate) fn kind_tests_es() -> &'static str { kind::tests::ES }
pub use map::{Mapped, map_position, same_on_screen};
pub use occurrences::{Occurrence, OccurrenceView, key_hint, live_section_line, numbered_verses, occurrence_views, occurrences};
pub use reconcile::{ChangeReason, Decision, LiveChange, decide, refresh_slide, words};
pub use refs::{SectionRef, resolve_order, song_refs};
