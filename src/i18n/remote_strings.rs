//! The words the phone remote shows (`web/remote.html`, `web/remote.js`), for `GET /i18n.json`. The page looks
//! each one up by its English text with `t('…')`; a test in `server.rs` checks that every one is listed here.
//! Words the server makes itself (errors, "3 slides") are translated where they are made.

use crate::tr;

/// `(English, the current language)` for every word of the phone remote.
pub fn remote_strings() -> Vec<(&'static str, String)> {
    vec![
        // remote.html
        ("RustyPresenter Remote", tr!("RustyPresenter Remote")),
        ("Phone remote", tr!("Phone remote")),
        // {phone} and {pair} are the names of the computer's buttons, shown in bold.
        ("On the presentation computer, click {phone} → {pair}, then type the 6-digit code here.",
            tr!("On the presentation computer, click {phone} → {pair}, then type the 6-digit code here.")),
        ("Phone", tr!("Phone")),
        ("Pair a phone", tr!("Pair a phone")),
        ("Connect", tr!("Connect")),
        ("Connecting…", tr!("Connecting…")),
        ("On screen", tr!("On screen")),
        ("Back", tr!("Back")),
        ("Next", tr!("Next")),
        ("Pause video", tr!("Pause video")),
        ("Play video", tr!("Play video")),
        ("Up next", tr!("Up next")),
        ("Clear All", tr!("Clear All")),
        ("Text", tr!("Text")),
        ("Media", tr!("Media")),
        ("Audio", tr!("Audio")),
        ("Logo", tr!("Logo")),
        ("Service", tr!("Service")),
        ("Tap a slide to put it on the screen.", tr!("Tap a slide to put it on the screen.")),
        ("Cancel", tr!("Cancel")),
        ("Message", tr!("Message")),
        ("Timers", tr!("Timers")),
        ("Macros", tr!("Macros")),
        // N29: a slide on the stage screen only.
        ("Stage only", tr!("Stage only")),
        ("Take it off the stage", tr!("Take it off the stage")),
        ("Stage screen only (the room won't see it)", tr!("Stage screen only (the room won't see it)")),
        // remote.js
        ("Hide message", tr!("Hide message")),
        ("Run “{name}”?", tr!("Run “{name}”?")),
        ("Run", tr!("Run")),
        ("Show", tr!("Show")),
        ("Done", tr!("Done")),
        ("The screen changed — try again.", tr!("The screen changed — try again.")),
        ("The screen changed just now — tap the slide again.", tr!("The screen changed just now — tap the slide again.")),
        ("Not connected", tr!("Not connected")),
        ("Connected", tr!("Connected")),
        ("That QR code has expired. On the computer, click Pair a phone again.",
            tr!("That QR code has expired. On the computer, click Pair a phone again.")),
        ("Could not connect.", tr!("Could not connect.")),
        ("(Logo)", tr!("(Logo)")),
        ("(Music playing)", tr!("(Music playing)")),
        ("(Clear)", tr!("(Clear)")),
        ("End of service", tr!("End of service")),
        // Drawing on the slide (web/draw.js).
        ("Draw on the slide", tr!("Draw on the slide")),
        ("Highlighter", tr!("Highlighter")),
        ("Pen size", tr!("Pen size")),
        ("Undo", tr!("Undo")),
        ("Clear all", tr!("Clear all")),
        // The ink switch: lines that stay, or fade away by themselves like a laser pointer.
        ("Stays", tr!("Stays")),
        ("Fades", tr!("Fades")),
        // N31: the remote's password (Settings → Network), and the stage screen on a phone or tablet (web/stage-view.*).
        ("Password", tr!("Password")),
        ("This remote needs a password too.", tr!("This remote needs a password too.")),
        ("Stage screen", tr!("Stage screen")),
        ("This stage screen needs a password.", tr!("This stage screen needs a password.")),
        ("Open", tr!("Open")),
        // The name a newly paired phone gets in the computer's list of phones.
        ("Android phone", tr!("Android phone")),
    ]
}
