//! User-facing strings of the stage screen and its layouts.
//! Declared from `src/stage/mod.rs` with a `#[path]` line, like `strings/dialogs.rs`.

/// What the stage is called everywhere ("Stage layout", never "theme").
pub const STAGE_LAYOUT: &str = "Stage layout";

/// The built-in stage layouts.
pub const CLASSIC: &str = "Classic";
pub const BAND: &str = "Band";
pub const SPEAKER: &str = "Speaker";

/// Slides that have no words of their own.
pub const SLIDE_PICTURE: &str = "(slide picture)";
pub const VIDEO: &str = "(video)";
pub const PICTURE: &str = "(picture)";
pub const AUDIO: &str = "(audio)";
pub const LOGO: &str = "(logo)";

pub const NOTHING_LIVE: &str = "Nothing live";
pub const END_OF_SERVICE: &str = "End of service";

/// Small captions above a box's words.
pub const NEXT: &str = "NEXT";
pub const NEXT_SECTION: &str = "NEXT SECTION";
pub fn next_item(title: &str) -> String { format!("NEXT · {title}") }

/// How many slides of the live item are still to come.
pub fn slides_left(n: usize) -> String {
    match n {
        0 => "Last slide".into(),
        1 => "1 slide left".into(),
        n => format!("{n} slides left"),
    }
}

/// Timer boxes.
pub const TIMER_REMOVED: &str = "Timer removed";
pub fn paused(name: &str) -> String { format!("{name} · paused") }
