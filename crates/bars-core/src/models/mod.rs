//! Модели: embed-модель за trait [`Embedder`], rerank-модель за trait [`Reranker`], встроенные
//! модели каталога и внешние по HTTP (`docs/features/models/local-models.md`,
//! `docs/features/models/external-models.md`).

mod artifact;
mod builtin;
mod builtin_rerank;
mod candle;
mod catalog;
mod embed_artifact;
mod embedder;
mod execution;
mod failure;
mod gguf;
mod gguf_artifact;
pub use failure::{ModelErrorKind, ModelFailure, is_out_of_memory, library_kind};
pub use gguf::GgufEmbedder;
/// Скачивает и проверяет GGUF без создания модели и её KV-cache.
pub fn download_gguf_artifact(dir: &std::path::Path, id: &str) -> Result<(), ModelFailure> {
    gguf_artifact::ensure(dir, id)
        .map(|_| ())
        .map_err(|e| e.failure())
}
pub use execution::ExecutionInfo;
mod logprob_rerank;
mod remote;
pub use logprob_rerank::LogprobReranker;
mod rerank_artifact;
mod reranker;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;

pub use builtin::{
    Accelerator, BuiltinEmbedder, Device, bge_m3_fingerprint, bge_m3_fp16_fingerprint,
    bge_m3_int8_fingerprint, load_builtin_embedder, prepare_environment,
};
pub use builtin_rerank::{BuiltinReranker, sigmoid};
pub use candle::CandleEmbedder;
pub use catalog::{
    BGE_M3_DIM, BGE_M3_FP16_ID, BGE_M3_ID, Backend, BuiltinModel, NOMIC_V2_MOE_ID, QWEN3_0_6B_ID,
    QWEN3_4B_ID, QWEN3_QUERY_PREFIX, builtin_model, builtin_models, download_onnx_artifacts,
    download_percent, is_downloaded,
};
pub use embedder::{EmbedError, Embedder, ModelFingerprint};
pub use remote::{HttpEmbedder, HttpReranker, RemoteTarget, remote_fingerprint};
pub use rerank_artifact::{
    reranker_download_percent, reranker_files_present, reranker_is_downloaded,
};
pub use reranker::{BGE_RERANKER_V2_M3_ID, RerankError, Reranker};
