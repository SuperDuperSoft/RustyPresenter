//! Keeping scripture plans between rebuilds: the plan key, the plan cache, the estimate
//! fallback used before a measured plan is ready, and resolving an item's verse text.
pub mod cache;

pub use cache::*;
