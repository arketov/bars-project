//! Nomic Embed v2 MoE через candle (`docs/features/models/local-models.md`, ADR-0002).
//!
//! Считаются только на процессоре: видеокарта (DirectML) есть лишь у ONNX-моделей. Веса в `f32` —
//! так модель проверена в fastembed.

use std::path::Path;

use candle_core::{DType, Device};
use fastembed::NomicV2MoeTextEmbedding;

use super::catalog::{Backend, BuiltinModel};
use super::embedder::{EmbedError, Embedder, ModelFingerprint};

/// Меньшие пачки быстрее: меньше лишнего дополнения коротких текстов.
const BATCH_SIZE: usize = 4;

pub struct CandleEmbedder {
    inner: NomicV2MoeTextEmbedding,
    info: &'static BuiltinModel,
    fingerprint: ModelFingerprint,
}

impl CandleEmbedder {
    /// Загружает модель из `models_dir`; если её там нет, fastembed скачивает её.
    ///
    /// candle-загрузчики fastembed берут папку кэша из переменной `FASTEMBED_CACHE_DIR`, а не из
    /// параметра: её выставляет [`super::prepare_environment`] при создании провайдера воркера.
    pub fn load(info: &'static BuiltinModel, _models_dir: &Path) -> Result<Self, EmbedError> {
        let device = Device::Cpu;
        let load_error = |error: candle_core::Error| EmbedError::Load(error.to_string());
        let inner = match info.backend {
            Backend::NomicV2Moe => {
                NomicV2MoeTextEmbedding::from_hf(info.id, &device, DType::F32, info.max_length)
                    .map_err(load_error)?
            }
            Backend::Onnx | Backend::Gguf => {
                return Err(EmbedError::Load(format!(
                    "{} не считается через candle",
                    info.id
                )));
            }
        };
        Ok(Self {
            inner,
            info,
            fingerprint: info.fingerprint(),
        })
    }

    fn run(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let result = self.inner.embed(texts);
        result.map_err(|error| EmbedError::Embed(error.to_string()))
    }

    fn embed_with_prefix(&self, texts: &[&str], prefix: &str) -> Result<Vec<Vec<f32>>, EmbedError> {
        // fastembed дополняет пачку до самого длинного текста: короткие и длинные считаются порознь.
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|&index| texts[index].len());

        let mut result = vec![Vec::new(); texts.len()];
        for batch in order.chunks(BATCH_SIZE) {
            let prefixed: Vec<String> = batch
                .iter()
                .map(|&index| format!("{prefix}{}", texts[index]))
                .collect();
            let vectors = self.run(&prefixed)?;
            if vectors.len() != batch.len() {
                return Err(EmbedError::Count {
                    expected: batch.len(),
                    got: vectors.len(),
                });
            }
            for (vector, &index) in vectors.into_iter().zip(batch) {
                result[index] = vector;
            }
        }
        Ok(result)
    }
}

impl Embedder for CandleEmbedder {
    fn execution(&self) -> super::ExecutionInfo {
        super::ExecutionInfo {
            backend: "candle",
            actual: Some("cpu"),
            fallback_reason: None,
            precision: Some("fp32"),
            revision: None,
        }
    }
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.embed_with_prefix(texts, self.info.doc_prefix)
    }

    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let mut vectors = self.embed_with_prefix(&[text], self.info.query_prefix)?;
        Ok(vectors.remove(0))
    }
}
