//! Qwen3 Q4_K_M, ADR-0002. Контекст принадлежит модели, backend — процессу.
use super::{Accelerator, BuiltinModel, EmbedError, Embedder, ExecutionInfo, ModelFingerprint};
use llama_cpp_2::{
    context::{
        LlamaContext,
        params::{LlamaContextParams, LlamaPoolingType},
    },
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{LlamaModel, params::LlamaModelParams},
};
use std::{num::NonZeroU32, path::Path, sync::OnceLock};

const MAX_TOKENS: usize = 8192;
static BACKEND: OnceLock<Result<LlamaBackend, String>> = OnceLock::new();
fn backend() -> Result<&'static LlamaBackend, EmbedError> {
    BACKEND
        .get_or_init(|| LlamaBackend::init().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(load_error)
}
fn load_error(e: impl std::fmt::Display) -> EmbedError {
    EmbedError::Load(e.to_string())
}
fn embed_error(e: impl std::fmt::Display) -> EmbedError {
    EmbedError::Embed(e.to_string())
}
struct ContextState<'a> {
    ctx: Option<LlamaContext<'a>>,
    capacity: usize,
}
self_cell::self_cell! {
    struct Session {
        owner:LlamaModel,
        #[covariant]
        dependent:ContextState,
    }
}
pub struct GgufEmbedder {
    session: Option<Session>,
    models_dir: std::path::PathBuf,
    info: &'static BuiltinModel,
    fingerprint: ModelFingerprint,
    gpu: bool,
    fallback: Option<String>,
}
pub(super) fn fingerprint(info: &BuiltinModel) -> ModelFingerprint {
    let a = super::gguf_artifact::artifact(info.id).expect("GGUF каталога имеет артефакт");
    let recipe = format!(
        "{}:{}:{}:{}:{}:{}:last:eos:q4_k_m",
        a.repo, a.revision, a.file, a.sha256, info.max_length, info.query_prefix
    );
    let recipe = blake3::hash(recipe.as_bytes()).to_hex();
    ModelFingerprint {
        source: "builtin".into(),
        model: format!("{}@gguf-q4_k_m-{}", info.id, &recipe[..16]),
        dim: info.dim,
        doc_prefix: info.doc_prefix.into(),
    }
}
fn capacity(tokens: usize) -> usize {
    tokens.max(256).next_power_of_two().min(MAX_TOKENS)
}
fn bounded<T: Copy>(tokens: &mut Vec<T>, max: usize) {
    if tokens.len() > max {
        // Last pooling должен видеть EOS даже после усечения длинного входа.
        let last = *tokens.last().unwrap();
        tokens.truncate(max);
        tokens[max - 1] = last;
    }
}
impl GgufEmbedder {
    pub fn load(
        info: &'static BuiltinModel,
        dir: &Path,
        accelerator: Accelerator,
    ) -> Result<Self, EmbedError> {
        let path = super::gguf_artifact::ensure(dir, info.id)?;
        let backend = backend()?;
        let build = |gpu: bool| -> Result<Session, EmbedError> {
            if gpu && !backend.supports_gpu_offload() {
                return Err(load_error("Vulkan/Metal-устройство недоступно"));
            }
            let model = LlamaModel::load_from_file(
                backend,
                &path,
                &LlamaModelParams::default().with_n_gpu_layers(if gpu { 1000 } else { 0 }),
            )
            .map_err(load_error)?;
            if model.n_embd_out() as usize != info.dim {
                return Err(EmbedError::Dimension {
                    expected: info.dim,
                    got: model.n_embd_out() as usize,
                });
            }
            let session = Session::new(model, |_| ContextState {
                ctx: None,
                capacity: 0,
            });
            Ok(session)
        };
        let (session, gpu, fallback) = match accelerator {
            Accelerator::Cpu => (build(false)?, false, None),
            Accelerator::Auto => match build(true) {
                Ok(s) => (s, true, None),
                Err(e) => (build(false)?, false, Some(e.to_string())),
            },
        };
        log::info!(
            "GGUF: {} · {} · Q4_K_M",
            info.id,
            if gpu { "GPU" } else { "CPU" }
        );
        Ok(Self {
            session: Some(session),
            models_dir: dir.into(),
            info,
            fingerprint: fingerprint(info),
            gpu,
            fallback,
        })
    }
    fn one(&mut self, text: &str) -> Result<Vec<f32>, EmbedError> {
        if self.session.is_none() {
            let mut cpu = Self::load(self.info, &self.models_dir, Accelerator::Cpu)?;
            cpu.fallback = self.fallback.clone();
            *self = cpu;
        }
        let result = self.one_on_device(text);
        if !self.gpu || result.is_ok() {
            return result;
        }
        let reason = result.unwrap_err().to_string();
        // Не держать GPU-контекст, пока создаётся CPU fallback с теми же весами.
        self.session.take();
        self.gpu = false;
        self.fallback = Some(reason.clone());
        let mut cpu = Self::load(self.info, &self.models_dir, Accelerator::Cpu)?;
        cpu.fallback = Some(reason);
        *self = cpu;
        self.one_on_device(text)
    }
    fn one_on_device(&mut self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let backend = backend()?;
        let dim = self.info.dim;
        self.session
            .as_mut()
            .ok_or_else(|| load_error("GGUF-сессия недоступна после ошибки загрузки"))?
            .with_dependent_mut(|model, state| {
                let mut tokens = model.vocab().tokenize(text.as_bytes(), true, true);
                bounded(&mut tokens, MAX_TOKENS);
                if tokens.is_empty() {
                    return Err(embed_error("GGUF: tokenizer вернул пустой вход"));
                }
                let needed = capacity(tokens.len());
                if needed > state.capacity {
                    // Освободить старый KV-cache до новой allocation; одновременно только один контекст.
                    state.ctx.take();
                    state.capacity = 0;
                    let threads =
                        std::thread::available_parallelism().map_or(1, |n| n.get().min(8)) as i32;
                    let params = LlamaContextParams::default()
                        .with_n_ctx(NonZeroU32::new(needed as u32))
                        .with_n_batch(needed as u32)
                        .with_n_ubatch(512.min(needed) as u32)
                        .with_n_threads(threads)
                        .with_n_threads_batch(threads)
                        .with_embeddings(true)
                        .with_pooling_type(LlamaPoolingType::Last);
                    state.ctx = Some(model.new_context(backend, params).map_err(embed_error)?);
                    state.capacity = needed;
                }
                let ctx = state.ctx.as_mut().unwrap();
                ctx.clear_kv_cache();
                let mut batch = LlamaBatch::new(tokens.len(), 1);
                batch.add_sequence(&tokens, 0, false).map_err(embed_error)?;
                ctx.decode(&mut batch).map_err(embed_error)?;
                let vector = ctx.embeddings_seq_ith(0).map_err(embed_error)?;
                normalize(vector, dim)
            })
    }
}
fn normalize(vector: &[f32], dim: usize) -> Result<Vec<f32>, EmbedError> {
    if vector.len() != dim {
        return Err(EmbedError::Dimension {
            expected: dim,
            got: vector.len(),
        });
    }
    let norm = vector
        .iter()
        .map(|v| (*v as f64).powi(2))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm <= 0.0 {
        return Err(embed_error("GGUF: некорректный вектор"));
    }
    Ok(vector.iter().map(|v| (*v as f64 / norm) as f32).collect())
}
impl Embedder for GgufEmbedder {
    fn execution(&self) -> ExecutionInfo {
        ExecutionInfo {
            backend: "llama.cpp",
            actual: self.session.as_ref().map(|_| {
                if self.gpu {
                    if cfg!(target_os = "macos") {
                        "metal"
                    } else {
                        "vulkan"
                    }
                } else {
                    "cpu"
                }
            }),
            fallback_reason: self.fallback.clone(),
            precision: Some("q4_k_m"),
            revision: Some(
                super::gguf_artifact::artifact(self.info.id)
                    .unwrap()
                    .revision,
            ),
        }
    }
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        texts.iter().map(|t| self.one(t)).collect()
    }
    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, EmbedError> {
        self.one(&format!("{}{text}", self.info.query_prefix))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn truncation_preserves_last_token_and_context_bounds() {
        let mut t = vec![1, 2, 3, 4, 99];
        bounded(&mut t, 3);
        assert_eq!(t, [1, 2, 99]);
        assert_eq!(capacity(1), 256);
        assert_eq!(capacity(513), 1024);
        assert_eq!(capacity(MAX_TOKENS), MAX_TOKENS);
        assert!(normalize(&[f32::NAN, 1.0], 2).is_err());
        assert!(normalize(&[0.0, 0.0], 2).is_err());
        assert!(normalize(&[1.0], 2).is_err());
        assert_eq!(normalize(&[3.0, 4.0], 2).unwrap(), [0.6, 0.8]);
    }
    #[test]
    fn gguf_fingerprint_requires_consent_for_previous_candle_vectors() {
        let info = super::super::builtin_model(super::super::QWEN3_0_6B_ID).unwrap();
        let current = fingerprint(info);
        let mut old = current.clone();
        old.model = info.id.into();
        assert!(!current.matches(&old));
        assert!(current.matches(&fingerprint(info)));
        fn send<T: Send>() {}
        send::<GgufEmbedder>();
    }
}
