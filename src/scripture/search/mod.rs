//! Keyword search over a local translation (spec §3.1, §5.5): query parsing, the shared
//! tokenizer and result snippets, the in-memory index with ranking, and the search service.

pub mod index;
pub mod query;
pub mod service;

pub use index::*;
pub use query::*;
pub use service::*;
