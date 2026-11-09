//! User-facing strings for running a presentation in PowerPoint (1.2).
use crate::tr;

/// A slide's label in a deck that runs in PowerPoint (the stage shows it).
pub fn slide_of(n: usize, total: usize) -> String { tr!("Slide {n} of {total}", n = n, total = total) }
/// The import question's and Item edit's third answer.
pub fn run_in_powerpoint() -> String { tr!("Run in PowerPoint") }
/// Why "Run in PowerPoint" is greyed.
pub fn needs_powerpoint() -> String { tr!("Run in PowerPoint needs Microsoft PowerPoint on a Windows computer.") }
/// The import question when "Run in PowerPoint" is offered too.
pub fn how(name: &str) -> String {
    tr!("How should “{name}” come in? Editable slides can be changed in the slide editor. Pictures look exactly like PowerPoint. Run in PowerPoint plays every animation, transition and video in PowerPoint itself.", name = name)
}
/// Item edit's question for a deck.
pub fn switch(title: &str) -> String {
    tr!("How should “{title}” show? Editable slides and a deck without its file come in again from the presentation.", title = title)
}
/// Item edit when the deck's own file is not kept: the operator picks it again.
pub fn choose_file_again(title: &str) -> String { tr!("Choose the presentation file for “{title}” again.", title = title) }
/// Item edit after the switch.
pub fn now_runs(title: &str) -> String { tr!("“{title}” runs in PowerPoint now.", title = title) }
pub fn now_pictures(title: &str) -> String { tr!("“{title}” shows as pictures now.", title = title) }

pub fn no_room_screen() -> String {
    tr!("Run in PowerPoint needs the room screen on a monitor. Turn the room screen on in Screens; until then the room shows the slide pictures.")
}
pub fn failed(title: &str, error: &str) -> String {
    tr!("PowerPoint could not run “{title}”: {error} The room shows the slide pictures instead.", title = title, error = error)
}
pub fn ended(title: &str) -> String {
    tr!("PowerPoint ended the slide show of “{title}”. The room shows the slide pictures now; click a slide to start PowerPoint again.", title = title)
}
pub fn no_answer() -> String { tr!("PowerPoint did not answer.") }
