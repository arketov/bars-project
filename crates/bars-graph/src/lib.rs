//! GraphRAG без модельного runtime: контракты, источники и ограниченные операции.
//! Контракт: docs/features/graphrag/implementation.md, GR-01…GR-12.

pub mod hints;
pub mod links;
pub mod model;
pub mod physical;
pub mod query;
pub mod query_slice;
pub mod source;
#[cfg(feature = "storage")]
pub mod store;
pub use model::*;
