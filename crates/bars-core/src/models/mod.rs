//! Модели: embed-модель за trait [`Embedder`], rerank-модель за trait [`Reranker`] и встроенная
//! bge-m3 (`docs/features/models/local-models.md`).

mod builtin;
mod embedder;
mod reranker;

pub use builtin::{Accelerator, BGE_M3_DIM, BGE_M3_ID, BuiltinEmbedder, Device};
pub use embedder::{EmbedError, Embedder, ModelFingerprint};
pub use reranker::{BGE_RERANKER_V2_M3_ID, RerankError, Reranker};
