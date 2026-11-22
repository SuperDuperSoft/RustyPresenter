//! User-facing strings of Screen Configuration.
//! Declared from `src/ui/dialogs/screens.rs` with a `#[path]` line, like `strings/dialogs.rs`.
use crate::tr;

/// The two lists.
pub fn audience() -> String { tr!("Room") }
pub fn stage() -> String { tr!("Stage") }
pub fn audience_screen() -> String { tr!("Audience screen") }
pub fn stage_screen() -> String { tr!("Stage screen") }

/// A row's status chip.
pub fn showing() -> String { tr!("Showing") }
pub fn off() -> String { tr!("Off") }
pub fn no_output() -> String { tr!("No output") }
pub fn unplugged() -> String { tr!("Unplugged") }

/// The Output choices.
pub fn output_none() -> String { tr!("None") }
pub fn output_window() -> String { tr!("In a window (for testing)") }
pub fn output_gone() -> String { tr!("Not connected right now") }

/// The notes under the settings.
pub fn on_main() -> String { tr!("This is the screen you are working on. Turning this on covers it.") }
pub fn mirrors(first: &str) -> String { tr!("Shows the same picture as {screen}. Sound only plays from {screen}.", screen = first) }

pub fn remove() -> String { tr!("Remove") }
pub fn remove_question(name: &str) -> String { tr!("Remove “{name}”?", name = name) }

/// N20: a screen set up before its projector is there.
pub fn output_placeholder() -> String { tr!("Placeholder (connect the projector later)") }
pub fn placeholder() -> String { tr!("Placeholder") }
pub fn placeholder_note() -> String { tr!("Nothing shows on a placeholder. Set it up now, then pick the projector here once it is plugged in.") }
pub fn stage_mirrors() -> String { tr!("Shows the audience screen's picture instead of a stage layout. It has no sound.") }

/// The Picture tab's choices.
pub fn test_off() -> String { tr!("Off") }
pub fn test_grid() -> String { tr!("Grid") }
pub fn test_bars() -> String { tr!("Colour bars") }
pub fn test_white() -> String { tr!("White") }
pub fn test_black() -> String { tr!("Black") }
pub fn corner_names() -> [String; 4] { [tr!("Top left"), tr!("Top right"), tr!("Bottom right"), tr!("Bottom left")] }
/// Where the picked corner is, e.g. "Moved right 1.5 %, down 0.0 %".
pub fn corner_at(x: f32, y: f32) -> String {
    let side = if x < 0.0 { tr!("left") } else { tr!("right") };
    let way = if y < 0.0 { tr!("up") } else { tr!("down") };
    tr!("Moved {side} {x} %, {way} {y} %", side = side, x = format!("{:.1}", x.abs() * 100.0), way = way, y = format!("{:.1}", y.abs() * 100.0))
}
pub fn delay_ms(ms: u64) -> String { tr!("about {ms} ms", ms = ms) }
pub fn fix_needs_opengl() -> String { tr!("Colour and corner changes show when the screens draw with OpenGL (Settings → General → Screens and graphics → Graphics: Automatic or OpenGL). Test patterns and the delay work either way.") }

/// The layout map: what a connected monitor shows when none of our screens is on it.
pub fn main_monitor() -> String { tr!("Main") }

/// "Slides are drawn at": the first choice, and what the picked one means for the screen shown.
pub fn draw_own() -> String { tr!("The screen's own size") }
pub fn drawn_at(width: u32, height: u32) -> String {
    tr!("This screen's slides are drawn at {width} × {height}. A smaller size is stretched to fill the screen: a little softer, and much easier for a slow computer or a 4K screen. It applies to every audience screen. Video keeps its own sharpness.", width = width.to_string(), height = height.to_string())
}
