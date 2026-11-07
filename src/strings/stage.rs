//! User-facing strings of the stage screen and its layouts.
//! Declared from `src/stage/mod.rs` with a `#[path]` line, like `strings/dialogs.rs`.
use crate::{tr, trn};

/// What the stage is called everywhere ("Stage layout", never "theme").
pub fn stage_layout() -> String { tr!("Stage layout") }

/// The built-in stage layouts.
pub fn classic() -> String { tr!("Classic") }
pub fn lyrics_only() -> String { tr!("Lyrics only") }
pub fn band() -> String { tr!("Band") }
pub fn speaker() -> String { tr!("Speaker") }
pub fn clock_countdown() -> String { tr!("Clock and countdown") }

/// Box kinds for the editor's Add menu, in `ElementKind` order (`presets::all_kinds`).
pub fn kind_names() -> [String; 12] {
    [
        tr!("Current slide"), tr!("Next slide"), tr!("Section"), tr!("Next section"), tr!("Item title"), tr!("Slides left"),
        tr!("Notes"), tr!("Clock"), tr!("Timer"), tr!("Video time left"), tr!("Message"), tr!("Text"),
    ]
}
pub fn kind_help() -> [String; 12] {
    [
        tr!("the words on screen now"), tr!("the words coming next"), tr!("Verse, Chorus, Bridge…"), tr!("the next part of the song"),
        tr!("the live item's name"), tr!("“3 slides left”"), tr!("the slide's stage notes"), tr!("the time of day"), tr!("a countdown or count-up"),
        tr!("time left on the video or audio"), tr!("messages sent from the back"), tr!("words you type once"),
    ]
}

/// Slides that have no words of their own.
pub fn slide_picture() -> String { tr!("(slide picture)") }
pub fn video() -> String { tr!("(video)") }
pub fn picture() -> String { tr!("(picture)") }
pub fn audio() -> String { tr!("(audio)") }
pub fn logo() -> String { tr!("(logo)") }

pub fn nothing_live() -> String { tr!("Nothing live") }
pub fn end_of_service() -> String { tr!("End of service") }

/// Small captions above a box's words.
pub fn next() -> String { tr!("NEXT") }
pub fn next_section() -> String { tr!("NEXT SECTION") }
pub fn next_item(title: &str) -> String { tr!("NEXT · {title}", title = title) }

/// How many slides of the live item are still to come.
pub fn slides_left(n: usize) -> String {
    match n {
        0 => tr!("Last slide"),
        n => trn!("{n} slide left", "{n} slides left", n),
    }
}

/// Timer boxes.
pub fn timer_removed() -> String { tr!("Timer removed") }
pub fn paused(name: &str) -> String { tr!("{timer} · paused", timer = name) }

/// Saving the church's stage layouts.
pub fn layout_gone() -> String { tr!("That stage layout is gone.") }
pub fn builtin_locked() -> String { tr!("Built-in stage layouts can't be changed. Make a copy first.") }
pub fn name_empty() -> String { tr!("Give the stage layout a name.") }
pub fn name_taken(name: &str) -> String { tr!("There is already a stage layout called “{name}”.", name = name) }
pub fn too_many_boxes() -> String { tr!("A stage layout can have at most 24 boxes.") }

/// Asking before a layout is deleted (`refs::delete_question`).
pub fn delete_layout(name: &str) -> String { tr!("Delete “{name}”?", name = name) }
pub fn used_by(parts: &str) -> String { tr!("It is used by {things}.", things = parts) }
pub fn service_items(n: usize) -> String { trn!("{n} service item", "{n} service items", n) }
pub fn the_stage_default() -> String { tr!("the stage default") }
pub fn macros(n: usize) -> String { trn!("{n} macro", "{n} macros", n) }
pub fn and() -> String { tr!("and") }
pub fn items_and_default_use_classic() -> String { tr!("Items and the default will use Classic") }
pub fn item_uses_default() -> String { tr!("That item will use the stage default") }
pub fn items_use_default() -> String { tr!("Those items will use the stage default") }
pub fn default_uses_classic() -> String { tr!("The default will use Classic") }
pub fn macro_wont_run() -> String { tr!("the macro won't run until it is edited") }
pub fn macros_wont_run() -> String { tr!("the macros won't run until they are edited") }
