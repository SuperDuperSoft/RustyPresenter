//! User-facing strings of the Services dialog (Plan 3 Task 19), copied from `web/operator.js:776-790`.
//! Declared from `src/ui/dialogs/services.rs` with a `#[path]` line, like `strings/dialogs.rs`.
use crate::{tr, trn};

/// The prompts of New empty service, Copy this service and Rename.
pub fn new_prompt() -> String { tr!("Name for the new service") }
pub fn copy_prompt() -> String { tr!("Name for the copy") }
pub fn rename_prompt() -> String { tr!("Rename service") }

/// The name both prompts start from: "Service " and today's date as the web shows it on church PCs (en-US).
pub fn dated_name(date: &str) -> String { tr!("Service {date}", date = date) }

/// "1 item", "3 items".
pub fn item_count(n: usize) -> String { trn!("{n} item", "{n} items", n) }

/// The trash button's question.
pub fn delete_question(name: &str) -> String { tr!("Delete the service “{name}”? (Songs and media stay in the library.)", name = name) }
