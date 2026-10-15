//! Модели: embed-модель за trait [`Embedder`], rerank-модель за trait [`Reranker`], встроенные
//! модели каталога и внешние по HTTP (`docs/features/models/local-models.md`,
//! `docs/features/models/external-models.md`).

mod builtin;
mod builtin_rerank;
mod candle;
mod catalog;
mod embedder;
mod remote;
mod reranker;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;

pub use builtin::{
    Accelerator, BuiltinEmbedder, Device, bge_m3_fingerprint, load_builtin_embedder,
    prepare_environment,
};
pub use builtin_rerank::{BuiltinReranker, sigmoid};
pub use candle::CandleEmbedder;
pub use catalog::{
    BGE_M3_DIM, BGE_M3_ID, Backend, BuiltinModel, NOMIC_V2_MOE_ID, QWEN3_0_6B_ID, QWEN3_4B_ID,
    QWEN3_QUERY_PREFIX, builtin_model, builtin_models, is_downloaded,
};
pub use embedder::{EmbedError, Embedder, ModelFingerprint};
pub use remote::{HttpEmbedder, HttpReranker, RemoteTarget, remote_fingerprint};
pub use reranker::{BGE_RERANKER_V2_M3_ID, RerankError, Reranker};
