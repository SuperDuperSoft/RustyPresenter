//! Crash recovery (spec §8.3): the committed frame saved to disk so a restarted app can put it back,
//! and (§9) the supervisor that restarts a crashed app.
pub mod exitcode;
pub mod panic;
pub mod state;
#[path = "../strings/recovery.rs"]
pub mod strings;
pub mod supervisor;
