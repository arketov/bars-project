//! Хранилище: индекс проекта в SQLite (`docs/storage/schema.md`, Т-5).

mod index;
pub mod stem;

pub(crate) use index::decode_vector;
pub use index::{
    Compatibility, DocumentRecord, Index, Journal, Layout, SCHEMA_VERSION, StorageError,
    StoredDocument, index_path,
};
