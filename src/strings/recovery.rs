//! User-facing strings of crash recovery (Plan 4 Tasks 10–11), quoted from the plan.
//! Declared from `src/recovery/mod.rs` with a `#[path]` line until a shared `src/strings/mod.rs` exists.

/// The supervisor's last word after `Guard` gives up (Review Focus 11).
pub fn gave_up(log: &std::path::Path) -> String {
    format!("RustyPresenter stopped after crashing 3 times in a minute. The log is at {}.", log.display())
}
