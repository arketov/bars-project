//! Что поиску нужно от rerank-модели (`docs/features/search/hybrid-search.md`, «Реранк кандидатов»).

/// Встроенная rerank-модель по умолчанию (Т-17, OQ-22).
pub const BGE_RERANKER_V2_M3_ID: &str = "BAAI/bge-reranker-v2-m3";

/// Rerank-модель: оценивает, насколько каждый документ отвечает запросу.
pub trait Reranker {
    /// Оценки документов в том же порядке, каждая от 0 до 1 (`docs/features/settings/settings.md`): модель,
    /// которая отдаёт сырой логит, пропускает его через сигмоиду сама.
    fn rerank(&mut self, query: &str, documents: &[&str]) -> Result<Vec<f32>, RerankError>;
}

#[derive(Debug, thiserror::Error)]
pub enum RerankError {
    #[error("rerank-модель не загрузилась: {0}")]
    Load(String),
    #[error("реранк не удался: {0}")]
    Rerank(String),
    #[error("модель вернула {got} оценок на {expected} документов")]
    Count { expected: usize, got: usize },
}
