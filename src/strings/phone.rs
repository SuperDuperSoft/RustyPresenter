//! User-facing strings of the Phone remote dialog (Plan 3 Task 19), copied from `web/operator.js:830-874`.
//! Declared from `src/ui/dialogs/phone.rs` with a `#[path]` line, like `strings/dialogs.rs`.
use crate::{tr, trn};

/// The pairing code's countdown, once a second.
pub fn code_works_for(seconds: i64) -> String {
    trn!("Code works for {n} more seconds", "Code works for {n} more seconds", seconds.max(0))
}
pub fn code_expired() -> String { tr!("Code expired — click Pair a phone or tablet again.") }
