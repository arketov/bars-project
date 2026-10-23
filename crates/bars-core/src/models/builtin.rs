//! Встроенная bge-m3 через fastembed-rs (`docs/features/models/local-models.md`, Т-17).

use std::path::Path;

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

use super::candle::CandleEmbedder;
use super::catalog::{BGE_M3_ID, Backend, builtin_model};
use super::embedder::{EmbedError, Embedder, ModelFingerprint};

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

/// Отпечаток встроенной bge-m3. Известен без загрузки модели: воркер сверяет его с индексом
/// до эмбеддинга (Т-25).
pub fn bge_m3_fingerprint() -> ModelFingerprint {
    builtin_model(BGE_M3_ID)
        .expect("bge-m3 есть в каталоге")
        .fingerprint()
}

/// Отдельные индексные пространства кандидатов; без загрузки весов.
pub fn bge_m3_fp16_fingerprint() -> ModelFingerprint {
    super::embed_artifact::Profile::Fp16.fingerprint()
}
pub fn bge_m3_int8_fingerprint() -> ModelFingerprint {
    super::embed_artifact::Profile::Int8.fingerprint()
}

pub struct BuiltinEmbedder {
    model: TextEmbedding,
    fingerprint: ModelFingerprint,
    device: Device,
    fallback_reason: Option<String>,
    precision: &'static str,
    revision: Option<&'static str>,
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
        let (model, device, fallback_reason) =
            super::execution::load_session(accelerator, |gpu| load(models_dir, gpu))?;
        log::info!("embed: bge-m3 загружен на {device:?}");
        Ok(Self {
            model,
            fingerprint: bge_m3_fingerprint(),
            device,
            fallback_reason,
            precision: "fp32",
            revision: None,
        })
    }

    /// FP16-кандидат: только DirectML, отдельный fingerprint, без неявного CPU fallback.
    pub fn bge_m3_fp16(models_dir: &Path, accelerator: Accelerator) -> Result<Self, EmbedError> {
        if accelerator != Accelerator::Auto || gpu_providers().is_empty() {
            return Err(EmbedError::Load(
                "FP16 embed требует DirectML; для CPU выберите обычную bge-m3 с переиндексацией"
                    .into(),
            ));
        }
        #[cfg(windows)]
        {
            let _probe = ort::session::Session::builder()
                .and_then(|b| Ok(b.with_memory_pattern(false)?))
                .and_then(|b| Ok(b.with_execution_providers(gpu_providers())?))
                .map_err(|e| EmbedError::Load(format!("FP16 embed: DirectML недоступен: {e}")))?;
        }
        Self::quantized(models_dir, super::embed_artifact::Profile::Fp16)
    }

    /// INT8-кандидат на CPU; динамическая quantization требует отдельных текстов.
    pub fn bge_m3_int8(models_dir: &Path) -> Result<Self, EmbedError> {
        Self::quantized(models_dir, super::embed_artifact::Profile::Int8)
    }

    fn quantized(
        models_dir: &Path,
        profile: super::embed_artifact::Profile,
    ) -> Result<Self, EmbedError> {
        use fastembed::{OutputKey, Pooling, QuantizationMode, UserDefinedEmbeddingModel};
        let (path, files) = super::embed_artifact::fetch(models_dir, profile)?;
        let mut custom = UserDefinedEmbeddingModel::new(
            std::fs::read(path).map_err(|e| EmbedError::Load(e.to_string()))?,
            files,
        )
        .with_pooling(Pooling::Cls);
        custom.output_key = Some(OutputKey::ByName("last_hidden_state"));
        let gpu = matches!(profile, super::embed_artifact::Profile::Fp16);
        if !gpu {
            custom = custom.with_quantization(QuantizationMode::Dynamic);
        }
        let mut options = TextInitOptions::new(EmbeddingModel::BGEM3)
            .with_cache_dir(models_dir.into())
            .with_max_length(BGE_M3_MAX_LENGTH);
        if gpu {
            options = options.with_execution_providers(gpu_providers());
        }
        let model = TextEmbedding::try_new_from_user_defined(custom, options.into())
            .map_err(|e| EmbedError::Load(e.to_string()))?;
        Ok(Self {
            model,
            fingerprint: profile.fingerprint(),
            device: if gpu { Device::DirectMl } else { Device::Cpu },
            fallback_reason: None,
            precision: profile.precision(),
            revision: Some(super::embed_artifact::REVISION),
        })
    }

    pub fn device(&self) -> Device {
        self.device
    }
}

#[cfg(windows)]
pub(super) fn gpu_providers() -> Vec<fastembed::ExecutionProviderDispatch> {
    vec![
        ort::ep::DirectML::default()
            .with_device_id(0)
            .build()
            .error_on_failure(),
    ]
}

#[cfg(not(windows))]
pub(super) fn gpu_providers() -> Vec<fastembed::ExecutionProviderDispatch> {
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
    fn execution(&self) -> super::ExecutionInfo {
        super::ExecutionInfo {
            backend: "onnx",
            actual: Some(match self.device {
                Device::Cpu => "cpu",
                Device::DirectMl => "directml",
            }),
            fallback_reason: self.fallback_reason.clone(),
            precision: Some(self.precision),
            revision: self.revision,
        }
    }
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        // fastembed дополняет пачку до самого длинного текста: короткие и длинные считаются порознь.
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|&index| texts[index].len());
        let sorted: Vec<&str> = order.iter().map(|&index| texts[index]).collect();

        let vectors = if self.precision == "int8" {
            let mut vectors = Vec::with_capacity(texts.len());
            for text in sorted {
                vectors.extend(
                    self.model
                        .embed([text], None)
                        .map_err(|e| EmbedError::Embed(e.to_string()))?,
                );
            }
            vectors
        } else {
            self.model
                .embed(&sorted, Some(BATCH_SIZE))
                .map_err(|e| EmbedError::Embed(e.to_string()))?
        };
        if vectors.len() != texts.len() {
            return Err(EmbedError::Count {
                expected: texts.len(),
                got: vectors.len(),
            });
        }
        if self.precision != "fp32" {
            check_quantized_vectors(&vectors, self.fingerprint.dim)?;
        }

        let mut result = vec![Vec::new(); texts.len()];
        for (vector, &index) in vectors.into_iter().zip(&order) {
            result[index] = vector;
        }
        Ok(result)
    }
}

fn check_quantized_vectors(vectors: &[Vec<f32>], dim: usize) -> Result<(), EmbedError> {
    if vectors.iter().any(|v| {
        let norm = v.iter().map(|x| x * x).sum::<f32>();
        v.len() != dim || v.iter().any(|x| !x.is_finite()) || !norm.is_finite() || norm <= 0.
    }) {
        return Err(EmbedError::Embed(
            "quantized embed вернул некорректный вектор".into(),
        ));
    }
    Ok(())
}

/// Выставляет окружение процесса так, чтобы все встроенные модели лежали в `models_dir`:
/// `FASTEMBED_CACHE_DIR` для candle-моделей и без `HF_HOME`, который в fastembed перекрывает
/// папку кэша (`docs/features/models/local-models.md`, «Где лежат скачанные модели»).
///
/// Вызывается один раз при создании провайдера воркера, до запуска рабочих потоков.
pub fn prepare_environment(models_dir: &Path) {
    // SAFETY: воркер вызывает функцию при старте, пока другие потоки не читают окружение; на
    // Windows изменение окружения процесса потокобезопасно.
    unsafe {
        std::env::set_var("FASTEMBED_CACHE_DIR", models_dir);
        std::env::remove_var("HF_HOME");
    }
}

/// Загружает встроенную embed-модель каталога.
pub fn load_builtin_embedder(
    id: &str,
    models_dir: &Path,
    accelerator: Accelerator,
) -> Result<Box<dyn Embedder + Send>, EmbedError> {
    let info = builtin_model(id)
        .ok_or_else(|| EmbedError::Load(format!("модели `{id}` нет во встроенном каталоге")))?;
    match info.backend {
        Backend::Onnx if info.id == super::catalog::BGE_M3_FP16_ID => Ok(Box::new(
            BuiltinEmbedder::bge_m3_fp16(models_dir, accelerator)?,
        )),
        Backend::Onnx => Ok(Box::new(BuiltinEmbedder::bge_m3(models_dir, accelerator)?)),
        Backend::Gguf => Ok(Box::new(super::GgufEmbedder::load(
            info,
            models_dir,
            accelerator,
        )?)),
        Backend::NomicV2Moe => Ok(Box::new(CandleEmbedder::load(info, models_dir)?)),
    }
}

#[cfg(test)]
mod quantized_tests {
    use super::*;
    #[test]
    fn invalid_dense_output_is_rejected_before_index_or_search() {
        check_quantized_vectors(&[vec![1., 0.]], 2).unwrap();
        for vector in [
            vec![0., 0.],
            vec![f32::NAN, 0.],
            vec![f32::INFINITY, 0.],
            vec![f32::MAX, 0.],
            vec![1.],
        ] {
            assert!(check_quantized_vectors(&[vector], 2).is_err());
        }
    }
    #[test]
    fn fp16_cpu_is_rejected_before_network_or_model_loading() {
        let dir = tempfile::tempdir().unwrap();
        let error = BuiltinEmbedder::bge_m3_fp16(dir.path(), Accelerator::Cpu)
            .err()
            .unwrap();
        assert!(error.to_string().contains("DirectML"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        #[cfg(windows)]
        {
            let error =
                load_builtin_embedder(super::super::BGE_M3_FP16_ID, dir.path(), Accelerator::Cpu)
                    .err()
                    .unwrap();
            assert!(error.to_string().contains("DirectML"));
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        }
    }
}
