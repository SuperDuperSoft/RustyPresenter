//! Keyword search over a local translation (spec §3.1, §5.5): query parsing, the shared
//! tokenizer and result snippets, and the in-memory index with ranking.

pub mod index;
pub mod query;

pub use index::*;
pub use query::*;
