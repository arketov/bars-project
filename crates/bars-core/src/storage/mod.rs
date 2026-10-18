//! Хранилище: индекс проекта в SQLite (`docs/architecture/schema.md`, Т-5).

mod index;
pub mod stem;

#[cfg(test)]
pub(crate) use index::decode_vector;
pub use index::{
    Compatibility, DocumentRecord, Index, Journal, Layout, SCHEMA_VERSION, StorageError,
    StoredDocument, index_path,
};
