//! Встроенный rerank `BAAI/bge-reranker-v2-m3` через fastembed-rs (`docs/features/models/local-models.md`).

use std::path::Path;

use fastembed::{RerankInitOptions, RerankerModel, TextRerank, UserDefinedRerankingModel};

use super::builtin::{Accelerator, Device, gpu_providers};
use super::catalog::model_files_report;
use super::reranker::{BGE_RERANKER_V2_M3_ID, RerankError, Reranker};

/// Наибольший логит ниже этого значения даёт оценку меньше 0,03%: у рабочей модели на запросе
/// с подходящим чанком он заметно выше.
const SUSPICIOUS_MAX_LOGIT: f32 = -8.0;

/// Вход «запрос + чанк» ограничен 512 токенами: на прототипе 30 пар по 1024 токена занимали
/// 15–29 с (`docs/knowledge/spike-search.md`).
const MAX_LENGTH: usize = 512;

/// Пар за один проход модели.
const CPU_BATCH_SIZE: usize = 4;
const GPU_BATCH_SIZE: usize = 16;

pub struct BuiltinReranker {
    model: TextRerank,
    device: Device,
    fallback_reason: Option<String>,
    batch_size: usize,
}

impl BuiltinReranker {
    /// Загружает bge-reranker-v2-m3 из `models_dir`; если её там нет, fastembed скачивает её.
    /// `Auto` на Windows пробует DirectML и при неудаче берёт процессор.
    pub fn bge_reranker_v2_m3(
        models_dir: &Path,
        accelerator: Accelerator,
    ) -> Result<Self, RerankError> {
        let (model, device, fallback_reason) =
            super::execution::load_session(accelerator, |gpu| load(models_dir, gpu))?;
        log::info!(
            "rerank: загружен на {device:?}; precision={}; batch={}; файлы модели: {}",
            if device == Device::DirectMl {
                "fp16"
            } else {
                "fp32"
            },
            if device == Device::DirectMl {
                GPU_BATCH_SIZE
            } else {
                CPU_BATCH_SIZE
            },
            if device == Device::DirectMl {
                "pinned onnx-community FP16 (BLAKE3 verified)".into()
            } else {
                files_report(models_dir)
            }
        );
        Ok(Self {
            model,
            device,
            fallback_reason,
            batch_size: if device == Device::DirectMl {
                GPU_BATCH_SIZE
            } else {
                CPU_BATCH_SIZE
            },
        })
    }

    pub fn device(&self) -> Device {
        self.device
    }
}

fn files_report(models_dir: &Path) -> String {
    model_files_report(models_dir, BGE_RERANKER_V2_M3_ID)
        .unwrap_or_else(|| "не найдены в кэше".to_string())
}

/// Строка для лога по сырым логитам: пределы и пометка, если модель, похоже, выдаёт мусор.
fn logits_report(device: Device, logits: &[f32]) -> String {
    let min = logits.iter().copied().fold(f32::INFINITY, f32::min);
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let flag = if max < SUSPICIOUS_MAX_LOGIT || !max.is_finite() {
        "; подозрительно: все оценки около нуля"
    } else {
        ""
    };
    format!(
        "rerank: {device:?}, пар {}, логит от {min:.3} до {max:.3}{flag}",
        logits.len()
    )
}

fn load(models_dir: &Path, gpu: bool) -> Result<TextRerank, RerankError> {
    let providers = if gpu { gpu_providers() } else { Vec::new() };
    if gpu && providers.is_empty() {
        return Err(RerankError::Load("видеокарта недоступна на этой ОС".into()));
    }
    // Проверяем регистрацию EP до скачивания GPU-весов на машине без рабочей GPU.
    #[cfg(windows)]
    if gpu {
        let _probe = ort::session::Session::builder()
            .map_err(|e| RerankError::Load(e.to_string()))?
            .with_memory_pattern(false)
            .map_err(|e| RerankError::Load(e.to_string()))?
            .with_execution_providers(gpu_providers())
            .map_err(|e| RerankError::Load(e.to_string()))?;
    }
    let mut options = RerankInitOptions::new(RerankerModel::BGERerankerV2M3)
        .with_cache_dir(models_dir.to_path_buf())
        .with_max_length(MAX_LENGTH);
    if gpu {
        options = options.with_execution_providers(providers);
        let (path, tokenizer) = super::rerank_artifact::fp16(models_dir)?;
        return TextRerank::try_new_from_user_defined(
            UserDefinedRerankingModel::new(path, tokenizer),
            options.into(),
        )
        .map_err(|error| RerankError::Load(error.to_string()));
    }
    TextRerank::try_new(options).map_err(|error| RerankError::Load(error.to_string()))
}

// Ошибки графа/драйвера не маскируем повтором. Уменьшаем только пачку при нехватке памяти.
fn smaller_batch(device: Device, current: usize, message: &str) -> Option<usize> {
    if device != Device::DirectMl || !super::is_out_of_memory(message) {
        return None;
    }
    match current {
        5.. => Some(4),
        2..=4 => Some(1),
        _ => None,
    }
}

/// Логит модели → оценка от 0 до 1 (`docs/features/settings/settings.md`).
pub fn sigmoid(logit: f32) -> f32 {
    1.0 / (1.0 + (-logit).exp())
}

impl Reranker for BuiltinReranker {
    fn execution(&self) -> super::ExecutionInfo {
        super::ExecutionInfo {
            backend: "onnx",
            actual: Some(match self.device {
                Device::Cpu => "cpu",
                Device::DirectMl => "directml",
            }),
            fallback_reason: self.fallback_reason.clone(),
            precision: Some(if self.device == Device::DirectMl {
                "fp16"
            } else {
                "fp32"
            }),
            revision: (self.device == Device::DirectMl).then_some(super::rerank_artifact::REVISION),
        }
    }
    fn rerank(&mut self, query: &str, documents: &[&str]) -> Result<Vec<f32>, RerankError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        // fastembed возвращает результаты по убыванию оценки; `index` — место документа во входе.
        let results = loop {
            match self
                .model
                .rerank(query, documents, false, Some(self.batch_size))
            {
                Ok(results) => break results,
                Err(error) => {
                    let message = error.to_string();
                    let Some(batch) = smaller_batch(self.device, self.batch_size, &message) else {
                        return Err(RerankError::Rerank(message));
                    };
                    log::warn!(
                        "rerank: не хватило памяти GPU; batch {} -> {batch}",
                        self.batch_size
                    );
                    self.batch_size = batch;
                }
            }
        };
        if results.len() != documents.len() {
            return Err(RerankError::Count {
                expected: documents.len(),
                got: results.len(),
            });
        }
        let logits: Vec<f32> = results.iter().map(|result| result.score).collect();
        if logits
            .iter()
            .copied()
            .reduce(f32::max)
            .is_some_and(|max| max < -8.0)
        {
            log::warn!("{}", logits_report(self.device, &logits));
        } else {
            log::debug!("{}", logits_report(self.device, &logits));
        }
        let mut scores = vec![0.0; documents.len()];
        let mut seen = vec![false; documents.len()];
        for result in results {
            if result.index >= scores.len() || seen[result.index] || !result.score.is_finite() {
                return Err(RerankError::Rerank(
                    "модель вернула неверные индексы или нечисловой логит".into(),
                ));
            }
            seen[result.index] = true;
            scores[result.index] = sigmoid(result.score);
        }
        Ok(scores)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_backoff_is_bounded_and_gpu_only() {
        assert_eq!(
            smaller_batch(Device::DirectMl, 16, "E_OUTOFMEMORY"),
            Some(4)
        );
        assert_eq!(
            smaller_batch(Device::DirectMl, 16, "HRESULT: 0x8007000E"),
            Some(4)
        );
        assert_eq!(
            smaller_batch(Device::DirectMl, 16, "HRESULT: 0x887A0005"),
            None
        );
        assert_eq!(
            smaller_batch(Device::DirectMl, 4, "Failed to allocate buffer"),
            Some(1)
        );
        assert_eq!(smaller_batch(Device::DirectMl, 1, "out of memory"), None);
        assert_eq!(smaller_batch(Device::Cpu, 4, "out of memory"), None);
        assert_eq!(
            smaller_batch(Device::DirectMl, 16, "unsupported operator"),
            None
        );
    }

    #[test]
    fn sigmoid_maps_logits_into_the_unit_range() {
        assert_eq!(sigmoid(0.0), 0.5);
        assert!(sigmoid(10.0) > 0.999);
        assert!(sigmoid(-10.0) < 0.001);
        assert!(sigmoid(-1000.0) >= 0.0);
        assert!(sigmoid(1000.0) <= 1.0);
    }

    #[test]
    fn logits_report_flags_a_model_that_scores_everything_near_zero() {
        let healthy = logits_report(Device::Cpu, &[-3.0, 2.5, 0.1]);
        assert!(healthy.contains("пар 3") && !healthy.contains("подозрительно"));
        let broken = logits_report(Device::DirectMl, &[-12.0, -9.5]);
        assert!(broken.contains("DirectMl") && broken.contains("подозрительно"));
        assert!(logits_report(Device::Cpu, &[f32::NAN]).contains("подозрительно"));
    }
}
