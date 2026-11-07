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
        // remote.js
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
        // The name a newly paired phone gets in the computer's list of phones.
        ("Android phone", tr!("Android phone")),
    ]
}
