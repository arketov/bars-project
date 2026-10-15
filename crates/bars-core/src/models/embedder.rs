//! Что индексу нужно от embed-модели: отпечаток и векторы для текстов.

/// Отпечаток модели, которой построен индекс (`docs/models/local-models.md`, Т-25).
///
/// Совпадает у двух моделей — векторы одного индекса сравнимы. Отличается — нужна переиндексация.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFingerprint {
    /// `builtin`, `remote` или `local-openai`.
    pub source: String,
    /// Идентификатор модели: `BAAI/bge-m3` или имя у провайдера.
    pub model: String,
    /// Размерность вектора.
    pub dim: usize,
    /// Префикс, который bars добавляет к тексту документа перед эмбеддингом.
    pub doc_prefix: String,
}

/// Embed-модель. Одна модель считает один вызов за раз (`docs/architecture/worker.md`).
pub trait Embedder {
    fn fingerprint(&self) -> &ModelFingerprint;

    /// Векторы для текстов, в том же порядке. Нормализует их индекс, а не модель.
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError>;

    /// Вектор поискового запроса. Модели с префиксом или инструкцией для запроса (Qwen3, Nomic)
    /// переопределяют метод; у остальных запрос считается как обычный текст.
    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let mut vectors = self.embed(&[text])?;
        if vectors.len() != 1 {
            return Err(EmbedError::Count {
                expected: 1,
                got: vectors.len(),
            });
        }
        Ok(vectors.remove(0))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("модель не загрузилась: {0}")]
    Load(String),
    #[error("эмбеддинг не удался: {0}")]
    Embed(String),
    #[error("модель вернула {got} векторов на {expected} текстов")]
    Count { expected: usize, got: usize },
    #[error("размерность вектора {got}, а у модели {expected}")]
    Dimension { expected: usize, got: usize },
}
