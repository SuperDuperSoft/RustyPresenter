//! User-facing strings of the stage screen and its layouts.
//! Declared from `src/stage/mod.rs` with a `#[path]` line, like `strings/dialogs.rs`.

/// What the stage is called everywhere ("Stage layout", never "theme").
pub const STAGE_LAYOUT: &str = "Stage layout";

/// The built-in stage layouts.
pub const CLASSIC: &str = "Classic";
pub const LYRICS_ONLY: &str = "Lyrics only";
pub const BAND: &str = "Band";
pub const SPEAKER: &str = "Speaker";
pub const CLOCK_COUNTDOWN: &str = "Clock and countdown";

/// Box kinds for the editor's Add menu, in `ElementKind` order (`presets::all_kinds`).
pub const KIND_NAMES: [&str; 12] = [
    "Current slide", "Next slide", "Section", "Next section", "Item title", "Slides left",
    "Notes", "Clock", "Timer", "Video time left", "Message", "Text",
];
pub const KIND_HELP: [&str; 12] = [
    "the words on screen now", "the words coming next", "Verse, Chorus, Bridge…", "the next part of the song",
    "the live item's name", "“3 slides left”", "the slide's stage notes", "the time of day", "a countdown or count-up",
    "time left on the video or audio", "messages sent from the back", "words you type once",
];

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

/// Saving the church's stage layouts.
pub const LAYOUT_GONE: &str = "That stage layout is gone.";
pub const BUILTIN_LOCKED: &str = "Built-in stage layouts can't be changed. Make a copy first.";
pub const NAME_EMPTY: &str = "Give the stage layout a name.";
pub fn name_taken(name: &str) -> String { format!("There is already a stage layout called “{name}”.") }
pub const TOO_MANY_BOXES: &str = "A stage layout can have at most 24 boxes.";

/// Asking before a layout is deleted (`refs::delete_question`).
pub fn delete_layout(name: &str) -> String { format!("Delete “{name}”?") }
pub fn used_by(parts: &str) -> String { format!("It is used by {parts}.") }
pub fn service_items(n: usize) -> String { if n == 1 { "1 service item".into() } else { format!("{n} service items") } }
pub const THE_STAGE_DEFAULT: &str = "the stage default";
pub fn macros(n: usize) -> String { if n == 1 { "1 macro".into() } else { format!("{n} macros") } }
pub const AND: &str = "and";
pub const ITEMS_AND_DEFAULT_USE_CLASSIC: &str = "Items and the default will use Classic";
pub const ITEM_USES_DEFAULT: &str = "That item will use the stage default";
pub const ITEMS_USE_DEFAULT: &str = "Those items will use the stage default";
pub const DEFAULT_USES_CLASSIC: &str = "The default will use Classic";
pub const MACRO_WONT_RUN: &str = "the macro won't run until it is edited";
pub const MACROS_WONT_RUN: &str = "the macros won't run until they are edited";
