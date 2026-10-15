//! Встроенная bge-m3 через fastembed-rs (`docs/features/models/local-models.md`, Т-17).

use std::path::Path;

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

use super::embedder::{EmbedError, Embedder, ModelFingerprint};

pub const BGE_M3_ID: &str = "BAAI/bge-m3";
pub const BGE_M3_DIM: usize = 1024;

/// Длина входа задаётся явно: fastembed по умолчанию молча режет до 512 токенов.
const BGE_M3_MAX_LENGTH: usize = 8192;

/// Меньшие пачки быстрее больших: меньше лишнего дополнения коротких текстов (`docs/knowledge/spike-search.md`).
const BATCH_SIZE: usize = 4;

/// Где считать встроенную модель (`[device]` в `docs/features/settings/settings.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accelerator {
    /// Видеокарта, если она есть, иначе процессор.
    Auto,
    Cpu,
}

/// Где модель считает на самом деле.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Cpu,
    DirectMl,
}

pub struct BuiltinEmbedder {
    model: TextEmbedding,
    fingerprint: ModelFingerprint,
    device: Device,
}

impl BuiltinEmbedder {
    /// Загружает bge-m3 из `models_dir`. Если модели там нет, fastembed скачивает её (Т-17).
    ///
    /// `HF_HOME` в окружении перекрывает `models_dir` в fastembed: воркер убирает его при старте,
    /// а этот конструктор окружение не меняет.
    ///
    /// `Auto` на Windows пробует DirectML и при неудаче берёт процессор; какое устройство
    /// получилось, говорит [`BuiltinEmbedder::device`], воркер пишет это в лог. CoreML на macOS не
    /// проверен, поэтому там `Auto` — процессор.
    pub fn bge_m3(models_dir: &Path, accelerator: Accelerator) -> Result<Self, EmbedError> {
        let (model, device) = match accelerator {
            Accelerator::Cpu => (load(models_dir, false)?, Device::Cpu),
            Accelerator::Auto => match load(models_dir, true) {
                Ok(model) => (model, Device::DirectMl),
                Err(_) => (load(models_dir, false)?, Device::Cpu),
            },
        };
        Ok(Self {
            model,
            fingerprint: ModelFingerprint {
                source: "builtin".to_string(),
                model: BGE_M3_ID.to_string(),
                dim: BGE_M3_DIM,
                doc_prefix: String::new(),
            },
            device,
        })
    }

    pub fn device(&self) -> Device {
        self.device
    }
}

#[cfg(windows)]
fn gpu_providers() -> Vec<fastembed::ExecutionProviderDispatch> {
    vec![
        ort::ep::DirectML::default()
            .with_device_id(0)
            .build()
            .error_on_failure(),
    ]
}

#[cfg(not(windows))]
fn gpu_providers() -> Vec<fastembed::ExecutionProviderDispatch> {
    Vec::new()
}

fn load(models_dir: &Path, gpu: bool) -> Result<TextEmbedding, EmbedError> {
    let providers = if gpu { gpu_providers() } else { Vec::new() };
    if gpu && providers.is_empty() {
        return Err(EmbedError::Load("видеокарта недоступна на этой ОС".into()));
    }
    let mut options = TextInitOptions::new(EmbeddingModel::BGEM3)
        .with_cache_dir(models_dir.to_path_buf())
        .with_max_length(BGE_M3_MAX_LENGTH);
    if gpu {
        options = options.with_execution_providers(providers);
    }
    TextEmbedding::try_new(options).map_err(|error| EmbedError::Load(error.to_string()))
}

impl Embedder for BuiltinEmbedder {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        // fastembed дополняет пачку до самого длинного текста: короткие и длинные считаются порознь.
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|&index| texts[index].len());
        let sorted: Vec<&str> = order.iter().map(|&index| texts[index]).collect();

        let vectors = self
            .model
            .embed(&sorted, Some(BATCH_SIZE))
            .map_err(|error| EmbedError::Embed(error.to_string()))?;
        if vectors.len() != texts.len() {
            return Err(EmbedError::Count {
                expected: texts.len(),
                got: vectors.len(),
            });
        }

        let mut result = vec![Vec::new(); texts.len()];
        for (vector, &index) in vectors.into_iter().zip(&order) {
            result[index] = vector;
        }
        Ok(result)
    }
}
