//! Что индексу нужно от embed-модели: отпечаток и векторы для текстов.

/// Отпечаток модели, которой построен индекс (`docs/features/models/local-models.md`, Т-25).
///
/// Совпадает у двух моделей — векторы одного индекса сравнимы. Отличается — нужна переиндексация.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFingerprint {
    /// `builtin`, `remote`, `local-openai` или `ollama`.
    pub source: String,
    /// Идентификатор модели: `BAAI/bge-m3` или имя у провайдера.
    pub model: String,
    /// Размерность вектора.
    pub dim: usize,
    /// Префикс, который bars добавляет к тексту документа перед эмбеддингом.
    pub doc_prefix: String,
}

impl ModelFingerprint {
    /// Тот же ли это отпечаток. Размерность `0` значит «ещё не известна» (внешняя модель до первого
    /// ответа провайдера) и сравнивается как любая: модель, источник и префикс решают сами.
    pub fn matches(&self, other: &Self) -> bool {
        self.source == other.source
            && self.model == other.model
            && self.doc_prefix == other.doc_prefix
            && (self.dim == other.dim || self.dim == 0 || other.dim == 0)
    }
}

/// Embed-модель. Одна модель считает один вызов за раз (`docs/architecture/worker.md`).
pub trait Embedder {
    fn execution(&self) -> super::ExecutionInfo {
        super::ExecutionInfo::default()
    }
    fn fingerprint(&self) -> &ModelFingerprint;

    /// Узнаёт то, что отпечаток до первого обращения не знает: размерность внешней модели. Для
    /// встроенных моделей ничего не делает. После вызова `fingerprint().dim` известна.
    fn prepare(&mut self) -> Result<(), EmbedError> {
        Ok(())
    }

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
    /// Файлы модели не получены по сети (huggingface.co).
    #[error("модель не загрузилась: {0}")]
    Network(String),
    /// Нет нужных файлов, файл пуст, не сошёлся размер или хеш.
    #[error("модель не загрузилась: {0}")]
    Files(String),
    #[error("provider rejected document: {0}")]
    Rejected(String),
    #[error("эмбеддинг не удался: {0}")]
    Embed(String),
    #[error("модель вернула {got} векторов на {expected} текстов")]
    Count { expected: usize, got: usize },
    #[error("размерность вектора {got}, а у модели {expected}")]
    Dimension { expected: usize, got: usize },
}

impl EmbedError {
    /// Вид ошибки встроенной модели: типы `Network` и `Files` задаёт проверка bars, остальное
    /// разбирается по тексту библиотеки.
    pub fn failure(&self) -> super::ModelFailure {
        use super::{ModelErrorKind, ModelFailure};
        match self {
            Self::Network(_) => ModelFailure::new(ModelErrorKind::Network, self.to_string()),
            Self::Files(_) => ModelFailure::new(ModelErrorKind::FilesMissing, self.to_string()),
            other => ModelFailure::from_library(other.to_string()),
        }
    }
}
