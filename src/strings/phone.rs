//! User-facing strings of the Phone remote dialog (Plan 3 Task 19), copied from `web/operator.js:830-874`.
//! Declared from `src/ui/dialogs/phone.rs` with a `#[path]` line, like `strings/dialogs.rs`.

/// The pairing code's countdown, once a second.
pub fn code_works_for(seconds: i64) -> String { format!("Code works for {seconds} more seconds") }
pub const CODE_EXPIRED: &str = "Code expired — click Pair a phone again.";
