//! RustyPresenter's remote for Android phones and tablets: the browser remote's jobs in an app, so a volunteer opens
//! it and it is there (no address to type each Sunday, no browser certificate step for the camera and microphone).
//! It talks to the computer exactly as the browser pages do (`web/` in the desktop app), so the browser keeps
//! working beside it.
//!
//! Everything that does not need a phone is here and tested on any computer; `android` is the phone itself:
//! the screens (Slint), the microphone, the camera, permissions.
pub mod address;
pub mod discovery;
pub mod fit;
pub mod frames;
pub mod http;
pub mod lang;
pub mod protocol;
pub mod saved;
pub mod ws;

#[cfg(any(target_os = "android", feature = "preview"))]
pub mod screens;

#[cfg(target_os = "android")]
mod android;
