//! User-facing strings of the Services dialog (Plan 3 Task 19), copied from `web/operator.js:776-790`.
//! Declared from `src/ui/dialogs/services.rs` with a `#[path]` line, like `strings/dialogs.rs`.

/// The prompts of New empty service, Copy this service and Rename.
pub const NEW_PROMPT: &str = "Name for the new service";
pub const COPY_PROMPT: &str = "Name for the copy";
pub const RENAME_PROMPT: &str = "Rename service";

/// The name both prompts start from: "Service " and today's date as the web shows it on church PCs (en-US).
pub fn dated_name(date: &str) -> String { format!("Service {date}") }

/// "1 item", "3 items".
pub fn item_count(n: usize) -> String { format!("{n} item{}", if n == 1 { "" } else { "s" }) }

/// The trash button's question.
pub fn delete_question(name: &str) -> String { format!("Delete the service “{name}”? (Songs and media stay in the library.)") }
