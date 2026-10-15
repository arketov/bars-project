//! Модели в памяти воркера: ленивая загрузка, мьютекс на модель, пачки по 4 (`docs/architecture/worker.md`,
//! «Модели в памяти воркера»).
//!
//! Отпечаток модели известен по настройкам и без загрузки: проверка Т-25 не загружает модель.
//! Сама модель грузится при первом эмбеддинге и остаётся в памяти, пока настройки не сменят её.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bars_core::models::{
    Accelerator, BGE_M3_ID, BuiltinEmbedder, EmbedError, Embedder, ModelFingerprint,
    bge_m3_fingerprint,
};
use bars_core::settings::{ModelSource, Settings};

use super::lock;

/// Сколько текстов модель считает за один захват мьютекса. Между пачками мьютекс свободен, и
/// эмбеддинг поискового запроса ждёт не дольше одной пачки.
pub const BATCH_SIZE: usize = 4;

/// Откуда воркер берёт embed-модель по настройкам.
pub trait ModelProvider: Send + Sync {
    /// Отпечаток модели из настроек, без загрузки.
    fn fingerprint(&self, settings: &Settings) -> Result<ModelFingerprint, EmbedError>;

    /// Загружает модель; вызывается при первом эмбеддинге.
    fn load(&self, settings: &Settings) -> Result<Box<dyn Embedder + Send>, EmbedError>;
}

/// Встроенные модели из `~/.bars/models`. Внешние провайдеры и остальные встроенные модели —
/// срез 6.
pub struct BuiltinProvider {
    models_dir: PathBuf,
}

impl BuiltinProvider {
    pub fn new(models_dir: PathBuf) -> Self {
        Self { models_dir }
    }

    fn check(settings: &Settings) -> Result<(), EmbedError> {
        let embed = &settings.embed;
        if embed.source == ModelSource::Builtin && embed.model == BGE_M3_ID {
            Ok(())
        } else {
            Err(EmbedError::Load(format!(
                "модель `{}` ({}) пока не поддерживается: доступна только встроенная {BGE_M3_ID}",
                embed.model,
                embed.source.as_str()
            )))
        }
    }
}

impl ModelProvider for BuiltinProvider {
    fn fingerprint(&self, settings: &Settings) -> Result<ModelFingerprint, EmbedError> {
        Self::check(settings)?;
        Ok(bge_m3_fingerprint())
    }

    fn load(&self, settings: &Settings) -> Result<Box<dyn Embedder + Send>, EmbedError> {
        Self::check(settings)?;
        Ok(Box::new(BuiltinEmbedder::bge_m3(
            &self.models_dir,
            settings.accelerator,
        )?))
    }
}

type Loader = Box<dyn Fn() -> Result<Box<dyn Embedder + Send>, EmbedError> + Send + Sync>;

/// Одна модель за мьютексом.
pub struct ModelSlot {
    fingerprint: ModelFingerprint,
    loader: Loader,
    model: Mutex<Option<Box<dyn Embedder + Send>>>,
}

impl ModelSlot {
    pub fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    pub fn is_loaded(&self) -> bool {
        lock(&self.model).is_some()
    }

    /// Выполняет `f` с исключительным доступом к модели, загрузив её, если надо.
    fn with<R>(
        &self,
        f: impl FnOnce(&mut (dyn Embedder + Send)) -> Result<R, EmbedError>,
    ) -> Result<R, EmbedError> {
        let mut guard = lock(&self.model);
        if guard.is_none() {
            *guard = Some((self.loader)()?);
        }
        let model = guard.as_mut().expect("модель загружена строкой выше");
        f(model.as_mut())
    }
}

/// Embed-модель для индексации и поиска: мьютекс берётся на пачку, а не на весь вызов.
#[derive(Clone)]
pub struct SharedEmbedder {
    slot: Arc<ModelSlot>,
}

impl SharedEmbedder {
    pub fn slot(&self) -> &Arc<ModelSlot> {
        &self.slot
    }
}

impl Embedder for SharedEmbedder {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.slot.fingerprint
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        // Короткие тексты считаются вместе: пачка дополняется до самого длинного текста.
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|&index| texts[index].len());
        let sorted: Vec<&str> = order.iter().map(|&index| texts[index]).collect();

        let mut vectors = Vec::with_capacity(texts.len());
        for batch in sorted.chunks(BATCH_SIZE) {
            let part = self.slot.with(|model| model.embed(batch))?;
            if part.len() != batch.len() {
                return Err(EmbedError::Count {
                    expected: batch.len(),
                    got: part.len(),
                });
            }
            vectors.extend(part);
        }

        let mut result = vec![Vec::new(); texts.len()];
        for (vector, &index) in vectors.into_iter().zip(&order) {
            result[index] = vector;
        }
        Ok(result)
    }

    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, EmbedError> {
        self.slot.with(|model| model.embed_query(text))
    }
}

/// Какая модель сейчас в памяти.
struct Current {
    fingerprint: ModelFingerprint,
    accelerator: Accelerator,
    slot: Arc<ModelSlot>,
}

/// Менеджер моделей воркера.
pub struct Models {
    provider: Arc<dyn ModelProvider>,
    current: Mutex<Option<Current>>,
}

impl Models {
    pub fn new(provider: Arc<dyn ModelProvider>) -> Self {
        Self {
            provider,
            current: Mutex::new(None),
        }
    }

    /// Отпечаток модели из настроек; модель не загружается.
    pub fn fingerprint(&self, settings: &Settings) -> Result<ModelFingerprint, EmbedError> {
        self.provider.fingerprint(settings)
    }

    /// Embed-модель по настройкам. Модель из прежних настроек выгружается, как только её
    /// отпустят все, кто с ней работает (смена модели в настройках, `worker.md`).
    pub fn embedder(&self, settings: &Settings) -> Result<SharedEmbedder, EmbedError> {
        let fingerprint = self.provider.fingerprint(settings)?;
        let mut current = lock(&self.current);
        if let Some(existing) = current.as_ref()
            && existing.fingerprint == fingerprint
            && existing.accelerator == settings.accelerator
        {
            return Ok(SharedEmbedder {
                slot: Arc::clone(&existing.slot),
            });
        }
        let provider = Arc::clone(&self.provider);
        let owned = settings.clone();
        let slot = Arc::new(ModelSlot {
            fingerprint: fingerprint.clone(),
            loader: Box::new(move || provider.load(&owned)),
            model: Mutex::new(None),
        });
        *current = Some(Current {
            fingerprint,
            accelerator: settings.accelerator,
            slot: Arc::clone(&slot),
        });
        Ok(SharedEmbedder { slot })
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod fake {
    //! Подделка модели для тестов воркера: считает вызовы и не требует файлов модели.

    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    pub struct Counters {
        pub loads: AtomicUsize,
        pub embed_calls: AtomicUsize,
        pub texts: AtomicUsize,
        pub max_batch: AtomicUsize,
    }

    pub struct FakeProvider {
        pub counters: Arc<Counters>,
        /// Имя «модели»: меняется в тестах, чтобы отпечаток сменился.
        pub model: Mutex<String>,
        pub fail_load: bool,
    }

    impl FakeProvider {
        pub fn new(model: &str) -> Arc<Self> {
            Arc::new(Self {
                counters: Arc::new(Counters::default()),
                model: Mutex::new(model.to_string()),
                fail_load: false,
            })
        }

        pub fn set_model(&self, model: &str) {
            *lock(&self.model) = model.to_string();
        }
    }

    fn fake_fingerprint(model: &str) -> ModelFingerprint {
        ModelFingerprint {
            source: "builtin".into(),
            model: model.into(),
            dim: 3,
            doc_prefix: String::new(),
        }
    }

    struct FakeEmbedder {
        fingerprint: ModelFingerprint,
        counters: Arc<Counters>,
    }

    impl Embedder for FakeEmbedder {
        fn fingerprint(&self) -> &ModelFingerprint {
            &self.fingerprint
        }

        fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
            self.counters.embed_calls.fetch_add(1, Ordering::SeqCst);
            self.counters.texts.fetch_add(texts.len(), Ordering::SeqCst);
            self.counters
                .max_batch
                .fetch_max(texts.len(), Ordering::SeqCst);
            Ok(texts
                .iter()
                .map(|text| {
                    let len = text.chars().count() as f32;
                    vec![len, 1.0, (len * 0.37).sin()]
                })
                .collect())
        }
    }

    impl ModelProvider for FakeProvider {
        fn fingerprint(&self, _settings: &Settings) -> Result<ModelFingerprint, EmbedError> {
            Ok(fake_fingerprint(&lock(&self.model)))
        }

        fn load(&self, _settings: &Settings) -> Result<Box<dyn Embedder + Send>, EmbedError> {
            if self.fail_load {
                return Err(EmbedError::Load("нет файлов модели".into()));
            }
            self.counters.loads.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(FakeEmbedder {
                fingerprint: fake_fingerprint(&lock(&self.model)),
                counters: Arc::clone(&self.counters),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeProvider;
    use super::*;
    use std::sync::atomic::Ordering;

    fn settings() -> Settings {
        Settings::resolve(&Default::default(), &Default::default())
    }

    #[test]
    fn model_is_not_loaded_until_the_first_embedding() {
        let provider = FakeProvider::new("m1");
        let models = Models::new(provider.clone());
        let mut embedder = models.embedder(&settings()).unwrap();
        assert_eq!(embedder.fingerprint().model, "m1");
        assert_eq!(provider.counters.loads.load(Ordering::SeqCst), 0);
        assert!(!embedder.slot().is_loaded());

        embedder.embed(&["a"]).unwrap();
        embedder.embed(&["b"]).unwrap();
        assert_eq!(provider.counters.loads.load(Ordering::SeqCst), 1);
        assert!(embedder.slot().is_loaded());
    }

    #[test]
    fn embedding_goes_in_batches_of_four_and_keeps_order() {
        let provider = FakeProvider::new("m1");
        let models = Models::new(provider.clone());
        let mut embedder = models.embedder(&settings()).unwrap();

        let texts = [
            "ccc", "a", "bbbbbb", "dd", "eeeee", "ffffffff", "g", "hhhh", "iiiiiii",
        ];
        let vectors = embedder.embed(&texts).unwrap();

        assert_eq!(vectors.len(), texts.len());
        for (text, vector) in texts.iter().zip(&vectors) {
            assert_eq!(vector[0], text.chars().count() as f32, "порядок сохранён");
        }
        assert_eq!(
            provider.counters.max_batch.load(Ordering::SeqCst),
            BATCH_SIZE
        );
        assert_eq!(provider.counters.embed_calls.load(Ordering::SeqCst), 3);
        assert_eq!(provider.counters.texts.load(Ordering::SeqCst), 9);
    }

    #[test]
    fn query_gets_the_model_between_batches() {
        let provider = FakeProvider::new("m1");
        let models = Models::new(provider.clone());
        let mut embedder = models.embedder(&settings()).unwrap();
        let mut other = embedder.clone();

        // Мьютекс не держится между вызовами: запрос проходит сразу после пачки.
        embedder.embed(&["a", "b", "c", "d", "e"]).unwrap();
        let query = other.embed_query("запрос").unwrap();
        assert_eq!(query[0], 6.0);
        assert_eq!(provider.counters.loads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn same_settings_share_one_slot_and_a_new_model_replaces_it() {
        let provider = FakeProvider::new("m1");
        let models = Models::new(provider.clone());
        let first = models.embedder(&settings()).unwrap();
        let again = models.embedder(&settings()).unwrap();
        assert!(Arc::ptr_eq(first.slot(), again.slot()));

        provider.set_model("m2");
        let second = models.embedder(&settings()).unwrap();
        assert!(!Arc::ptr_eq(first.slot(), second.slot()));
        assert_eq!(second.fingerprint().model, "m2");

        // Старая модель живёт, пока её держит первый; у менеджера её больше нет.
        drop(again);
        assert_eq!(Arc::strong_count(first.slot()), 1);
    }

    #[test]
    fn builtin_provider_accepts_only_bge_m3_for_now() {
        let provider = BuiltinProvider::new(PathBuf::from("models"));
        assert_eq!(
            provider.fingerprint(&settings()).unwrap(),
            bge_m3_fingerprint()
        );

        let mut other = settings();
        other.embed.model = "qwen3".into();
        assert!(matches!(
            provider.fingerprint(&other),
            Err(EmbedError::Load(_))
        ));
    }
}
