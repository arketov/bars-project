//! Встроенный rerank `BAAI/bge-reranker-v2-m3` через fastembed-rs (`docs/features/models/local-models.md`).

use std::path::Path;

use fastembed::{RerankInitOptions, RerankerModel, TextRerank};

use super::builtin::{Accelerator, gpu_providers};
use super::reranker::{RerankError, Reranker};

/// Вход «запрос + чанк» ограничен 512 токенами: на прототипе 30 пар по 1024 токена занимали
/// 15–29 с (`docs/knowledge/spike-search.md`).
const MAX_LENGTH: usize = 512;

/// Пар за один проход модели.
const BATCH_SIZE: usize = 4;

pub struct BuiltinReranker {
    model: TextRerank,
}

impl BuiltinReranker {
    /// Загружает bge-reranker-v2-m3 из `models_dir`; если её там нет, fastembed скачивает её.
    /// `Auto` на Windows пробует DirectML и при неудаче берёт процессор.
    pub fn bge_reranker_v2_m3(
        models_dir: &Path,
        accelerator: Accelerator,
    ) -> Result<Self, RerankError> {
        let model = match accelerator {
            Accelerator::Cpu => load(models_dir, false)?,
            Accelerator::Auto => load(models_dir, true).or_else(|_| load(models_dir, false))?,
        };
        Ok(Self { model })
    }
}

fn load(models_dir: &Path, gpu: bool) -> Result<TextRerank, RerankError> {
    let providers = if gpu { gpu_providers() } else { Vec::new() };
    if gpu && providers.is_empty() {
        return Err(RerankError::Load("видеокарта недоступна на этой ОС".into()));
    }
    let mut options = RerankInitOptions::new(RerankerModel::BGERerankerV2M3)
        .with_cache_dir(models_dir.to_path_buf())
        .with_max_length(MAX_LENGTH);
    if gpu {
        options = options.with_execution_providers(providers);
    }
    TextRerank::try_new(options).map_err(|error| RerankError::Load(error.to_string()))
}

/// Логит модели → оценка от 0 до 1 (`docs/features/settings/settings.md`).
pub fn sigmoid(logit: f32) -> f32 {
    1.0 / (1.0 + (-logit).exp())
}

impl Reranker for BuiltinReranker {
    fn rerank(&mut self, query: &str, documents: &[&str]) -> Result<Vec<f32>, RerankError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        // fastembed возвращает результаты по убыванию оценки; `index` — место документа во входе.
        let results = self
            .model
            .rerank(query, documents, false, Some(BATCH_SIZE))
            .map_err(|error| RerankError::Rerank(error.to_string()))?;
        if results.len() != documents.len() {
            return Err(RerankError::Count {
                expected: documents.len(),
                got: results.len(),
            });
        }
        let mut scores = vec![0.0; documents.len()];
        for result in results {
            scores[result.index] = sigmoid(result.score);
        }
        Ok(scores)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigmoid_maps_logits_into_the_unit_range() {
        assert_eq!(sigmoid(0.0), 0.5);
        assert!(sigmoid(10.0) > 0.999);
        assert!(sigmoid(-10.0) < 0.001);
        assert!(sigmoid(-1000.0) >= 0.0);
        assert!(sigmoid(1000.0) <= 1.0);
    }
}
