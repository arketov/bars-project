//! Модели в памяти воркера: ленивая загрузка, мьютекс на модель, пачки (`docs/architecture/worker.md`,
//! «Модели в памяти воркера»).
//!
//! Отпечаток модели известен по настройкам и без загрузки: проверка Т-25 не загружает модель.
//! Сама модель грузится при первом эмбеддинге и остаётся в памяти, пока настройки не сменят её.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bars_core::models::{
    Accelerator, BGE_RERANKER_V2_M3_ID, BuiltinReranker, EmbedError, Embedder, HttpEmbedder,
    HttpReranker, ModelFingerprint, RemoteTarget, RerankError, Reranker, builtin_model,
    load_builtin_embedder, prepare_environment, remote_fingerprint,
};
use bars_core::secrets::{KeyRef, OsSecrets, SecretStore};
use bars_core::settings::{EmbedSettings, ModelSource, RerankSettings, Settings};

use super::lock;

/// Сколько текстов встроенная модель считает за один захват мьютекса. Между пачками мьютекс
/// свободен, и эмбеддинг поискового запроса ждёт не дольше одной пачки.
pub const BATCH_SIZE: usize = 4;

/// Откуда воркер берёт модели по настройкам.
pub trait ModelProvider: Send + Sync {
    /// Отпечаток embed-модели из настроек, без загрузки. У внешней модели размерность до первого
    /// ответа провайдера неизвестна и равна нулю.
    fn fingerprint(&self, settings: &Settings) -> Result<ModelFingerprint, EmbedError>;

    /// Загружает embed-модель; вызывается при первом эмбеддинге.
    fn load(&self, settings: &Settings) -> Result<Box<dyn Embedder + Send>, EmbedError>;

    /// Сколько текстов уходит модели за один захват мьютекса.
    fn batch_size(&self, _settings: &Settings) -> usize {
        BATCH_SIZE
    }

    /// Загружает rerank-модель; вызывается при первом реранке.
    fn load_reranker(&self, _settings: &Settings) -> Result<Box<dyn Reranker + Send>, RerankError> {
        Err(RerankError::Load(
            "rerank-модель в этой сборке не подключена".into(),
        ))
    }
}

/// Модели по настройкам: встроенные из `~/.bars/models` и внешние по HTTP (Т-17). Ключи внешних
/// провайдеров берутся из хранилища ОС.
pub struct ConfiguredProvider {
    models_dir: PathBuf,
    secrets: Arc<dyn SecretStore>,
}

/// Прежнее имя: когда-то провайдер знал только встроенную bge-m3.
pub type BuiltinProvider = ConfiguredProvider;

impl ConfiguredProvider {
    /// Папка моделей и хранилище ключей ОС. Выставляет окружение процесса для встроенных
    /// моделей, поэтому вызывается при старте воркера, до рабочих потоков.
    pub fn new(models_dir: PathBuf) -> Self {
        Self::with_secrets(models_dir, Arc::new(OsSecrets::default()))
    }

    pub fn with_secrets(models_dir: PathBuf, secrets: Arc<dyn SecretStore>) -> Self {
        prepare_environment(&models_dir);
        Self {
            models_dir,
            secrets,
        }
    }

    fn target(
        &self,
        source: ModelSource,
        model: &str,
        base_url: Option<&str>,
        key_name: Option<&str>,
        key_env: Option<&str>,
    ) -> Result<RemoteTarget, String> {
        let base_url = base_url
            .ok_or_else(|| format!("для источника `{}` не задан `base_url`", source.as_str()))?;
        Ok(RemoteTarget {
            source: source.as_str(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            key: KeyRef {
                name: key_name.map(str::to_string),
                env: key_env.map(str::to_string),
                store: Arc::clone(&self.secrets),
            },
        })
    }

    fn embed_target(&self, embed: &EmbedSettings) -> Result<RemoteTarget, EmbedError> {
        self.target(
            embed.source,
            &embed.model,
            embed.base_url.as_deref(),
            embed.api_key_name.as_deref(),
            embed.api_key_env.as_deref(),
        )
        .map_err(EmbedError::Load)
    }
}

impl ModelProvider for ConfiguredProvider {
    fn fingerprint(&self, settings: &Settings) -> Result<ModelFingerprint, EmbedError> {
        let embed = &settings.embed;
        match embed.source {
            ModelSource::Builtin => builtin_model(&embed.model)
                .map(|model| model.fingerprint())
                .ok_or_else(|| {
                    EmbedError::Load(format!(
                        "модели `{}` нет во встроенном каталоге",
                        embed.model
                    ))
                }),
            source => Ok(remote_fingerprint(source.as_str(), &embed.model)),
        }
    }

    fn load(&self, settings: &Settings) -> Result<Box<dyn Embedder + Send>, EmbedError> {
        let embed = &settings.embed;
        match embed.source {
            ModelSource::Builtin => {
                load_builtin_embedder(&embed.model, &self.models_dir, settings.accelerator)
            }
            _ => Ok(Box::new(HttpEmbedder::new(
                self.embed_target(embed)?,
                embed.batch_size,
            ))),
        }
    }

    fn batch_size(&self, settings: &Settings) -> usize {
        match settings.embed.source {
            ModelSource::Builtin => BATCH_SIZE,
            _ => settings.embed.batch_size,
        }
    }

    fn load_reranker(&self, settings: &Settings) -> Result<Box<dyn Reranker + Send>, RerankError> {
        let rerank = &settings.rerank;
        match rerank.source {
            ModelSource::Builtin => {
                if rerank.model != BGE_RERANKER_V2_M3_ID {
                    return Err(RerankError::Load(format!(
                        "встроенный rerank один: {BGE_RERANKER_V2_M3_ID}, а не `{}`",
                        rerank.model
                    )));
                }
                Ok(Box::new(BuiltinReranker::bge_reranker_v2_m3(
                    &self.models_dir,
                    settings.accelerator,
                )?))
            }
            source => {
                let target = self
                    .target(
                        source,
                        &rerank.model,
                        rerank.base_url.as_deref(),
                        rerank.api_key_name.as_deref(),
                        rerank.api_key_env.as_deref(),
                    )
                    .map_err(RerankError::Load)?;
                Ok(Box::new(HttpReranker::new(target)))
            }
        }
    }
}

type Loader = Box<dyn Fn() -> Result<Box<dyn Embedder + Send>, EmbedError> + Send + Sync>;

/// Одна модель за мьютексом.
pub struct ModelSlot {
    fingerprint: ModelFingerprint,
    batch_size: usize,
    loader: Loader,
    model: Mutex<Option<Box<dyn Embedder + Send>>>,
}

impl ModelSlot {
    /// Отпечаток по настройкам: у внешней модели размерность в нём может быть неизвестна.
    pub fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    /// Лучший известный отпечаток: у загруженной внешней модели размерность уже из ответа.
    fn known_fingerprint(&self) -> ModelFingerprint {
        lock(&self.model).as_ref().map_or_else(
            || self.fingerprint.clone(),
            |model| model.fingerprint().clone(),
        )
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
    /// Обновляется после каждого обращения к модели: размерность внешней модели узнаётся из ответа.
    fingerprint: ModelFingerprint,
}

impl SharedEmbedder {
    fn new(slot: Arc<ModelSlot>) -> Self {
        let fingerprint = slot.known_fingerprint();
        Self { slot, fingerprint }
    }

    pub fn slot(&self) -> &Arc<ModelSlot> {
        &self.slot
    }

    fn refresh(&mut self) {
        self.fingerprint = self.slot.known_fingerprint();
    }
}

impl Embedder for SharedEmbedder {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    fn prepare(&mut self) -> Result<(), EmbedError> {
        self.slot.with(|model| model.prepare())?;
        self.refresh();
        Ok(())
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        // Короткие тексты считаются вместе: пачка дополняется до самого длинного текста.
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|&index| texts[index].len());
        let sorted: Vec<&str> = order.iter().map(|&index| texts[index]).collect();

        let mut vectors = Vec::with_capacity(texts.len());
        for batch in sorted.chunks(self.slot.batch_size) {
            let part = self.slot.with(|model| model.embed(batch))?;
            if part.len() != batch.len() {
                return Err(EmbedError::Count {
                    expected: batch.len(),
                    got: part.len(),
                });
            }
            vectors.extend(part);
        }
        self.refresh();

        let mut result = vec![Vec::new(); texts.len()];
        for (vector, &index) in vectors.into_iter().zip(&order) {
            result[index] = vector;
        }
        Ok(result)
    }

    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let vector = self.slot.with(|model| model.embed_query(text))?;
        self.refresh();
        Ok(vector)
    }
}

type RerankLoader = Box<dyn Fn() -> Result<Box<dyn Reranker + Send>, RerankError> + Send + Sync>;

/// Одна rerank-модель за мьютексом.
pub struct RerankSlot {
    loader: RerankLoader,
    model: Mutex<Option<Box<dyn Reranker + Send>>>,
}

impl RerankSlot {
    pub fn is_loaded(&self) -> bool {
        lock(&self.model).is_some()
    }
}

/// Rerank-модель для поиска: грузится при первом реранке.
#[derive(Clone)]
pub struct SharedReranker {
    slot: Arc<RerankSlot>,
}

impl SharedReranker {
    pub fn slot(&self) -> &Arc<RerankSlot> {
        &self.slot
    }
}

impl Reranker for SharedReranker {
    fn rerank(&mut self, query: &str, documents: &[&str]) -> Result<Vec<f32>, RerankError> {
        let mut guard = lock(&self.slot.model);
        if guard.is_none() {
            *guard = Some((self.slot.loader)()?);
        }
        let model = guard.as_mut().expect("модель загружена строкой выше");
        model.rerank(query, documents)
    }
}

/// Какая embed-модель сейчас в памяти.
struct Current {
    fingerprint: ModelFingerprint,
    embed: EmbedSettings,
    accelerator: Accelerator,
    slot: Arc<ModelSlot>,
}

/// Какая rerank-модель сейчас в памяти.
struct CurrentRerank {
    rerank: RerankSettings,
    accelerator: Accelerator,
    slot: Arc<RerankSlot>,
}

/// Менеджер моделей воркера.
pub struct Models {
    provider: Arc<dyn ModelProvider>,
    current: Mutex<Option<Current>>,
    current_rerank: Mutex<Option<CurrentRerank>>,
}

impl Models {
    pub fn new(provider: Arc<dyn ModelProvider>) -> Self {
        Self {
            provider,
            current: Mutex::new(None),
            current_rerank: Mutex::new(None),
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
            && existing.embed == settings.embed
            && existing.accelerator == settings.accelerator
        {
            return Ok(SharedEmbedder::new(Arc::clone(&existing.slot)));
        }
        let provider = Arc::clone(&self.provider);
        let owned = settings.clone();
        let slot = Arc::new(ModelSlot {
            fingerprint: fingerprint.clone(),
            batch_size: self.provider.batch_size(settings).max(1),
            loader: Box::new(move || provider.load(&owned)),
            model: Mutex::new(None),
        });
        *current = Some(Current {
            fingerprint,
            embed: settings.embed.clone(),
            accelerator: settings.accelerator,
            slot: Arc::clone(&slot),
        });
        Ok(SharedEmbedder::new(slot))
    }

    /// Скачивает встроенную модель, загрузив её и сразу выпустив из рук: файлы остаются в кэше
    /// (кнопка «Скачать» в настройках). Блокирующий вызов.
    pub fn download(&self, settings: &Settings, reranker: bool) -> Result<(), String> {
        if reranker {
            self.provider
                .load_reranker(settings)
                .map(drop)
                .map_err(|error| error.to_string())
        } else {
            self.provider
                .load(settings)
                .map(drop)
                .map_err(|error| error.to_string())
        }
    }

    /// Rerank-модель по настройкам; загружается при первом реранке. Модель из прежних настроек
    /// выгружается, как только её отпустят все, кто с ней работает.
    pub fn reranker(&self, settings: &Settings) -> SharedReranker {
        let mut current = lock(&self.current_rerank);
        if let Some(existing) = current.as_ref()
            && existing.rerank == settings.rerank
            && existing.accelerator == settings.accelerator
        {
            return SharedReranker {
                slot: Arc::clone(&existing.slot),
            };
        }
        let provider = Arc::clone(&self.provider);
        let owned = settings.clone();
        let slot = Arc::new(RerankSlot {
            loader: Box::new(move || provider.load_reranker(&owned)),
            model: Mutex::new(None),
        });
        *current = Some(CurrentRerank {
            rerank: settings.rerank.clone(),
            accelerator: settings.accelerator,
            slot: Arc::clone(&slot),
        });
        SharedReranker { slot }
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
        pub rerank_loads: AtomicUsize,
    }

    pub struct FakeProvider {
        pub counters: Arc<Counters>,
        /// Имя «модели»: меняется в тестах, чтобы отпечаток сменился.
        pub model: Mutex<String>,
        pub fail_load: bool,
    }

    impl FakeProvider {
        pub fn new(model: &str) -> Arc<Self> {
            Arc::new(Self::plain(model))
        }

        pub fn plain(model: &str) -> Self {
            Self {
                counters: Arc::new(Counters::default()),
                model: Mutex::new(model.to_string()),
                fail_load: false,
            }
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

    /// Оценивает документ по числу слов запроса в нём.
    struct FakeReranker;

    impl Reranker for FakeReranker {
        fn rerank(&mut self, query: &str, documents: &[&str]) -> Result<Vec<f32>, RerankError> {
            let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
            Ok(documents
                .iter()
                .map(|document| {
                    let text = document.to_lowercase();
                    let hits = words
                        .iter()
                        .filter(|word| text.contains(word.as_str()))
                        .count();
                    0.1 + 0.8 * hits as f32 / words.len().max(1) as f32
                })
                .collect())
        }
    }

    impl ModelProvider for FakeProvider {
        fn load_reranker(
            &self,
            _settings: &Settings,
        ) -> Result<Box<dyn Reranker + Send>, RerankError> {
            if self.fail_load {
                return Err(RerankError::Load("нет файлов модели".into()));
            }
            self.counters.rerank_loads.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(FakeReranker))
        }

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

    use bars_core::models::bge_m3_fingerprint;

    fn provider() -> ConfiguredProvider {
        ConfiguredProvider::with_secrets(
            PathBuf::from("models-not-used"),
            Arc::new(bars_core::secrets::MemorySecrets::default()),
        )
    }

    #[test]
    fn provider_knows_every_builtin_model_and_its_fingerprint_without_loading() {
        let provider = provider();
        assert_eq!(
            provider.fingerprint(&settings()).unwrap(),
            bge_m3_fingerprint()
        );
        for model in bars_core::models::builtin_models() {
            let mut chosen = settings();
            chosen.embed.model = model.id.to_string();
            assert_eq!(provider.fingerprint(&chosen).unwrap(), model.fingerprint());
        }

        let mut unknown = settings();
        unknown.embed.model = "qwen3".into();
        assert!(matches!(
            provider.fingerprint(&unknown),
            Err(EmbedError::Load(_))
        ));
    }

    #[test]
    fn external_models_get_a_fingerprint_with_unknown_dimension() {
        let provider = provider();
        let mut remote = settings();
        remote.embed.source = ModelSource::Remote;
        remote.embed.model = "text-embedding-3-small".into();
        let fingerprint = provider.fingerprint(&remote).unwrap();
        assert_eq!(fingerprint.source, "remote");
        assert_eq!(fingerprint.dim, 0);
        // Источник входит в отпечаток: тот же адрес как локальный — другая модель для Т-25.
        remote.embed.source = ModelSource::LocalOpenai;
        assert_eq!(
            provider.fingerprint(&remote).unwrap().source,
            "local-openai"
        );
    }

    #[test]
    fn external_embed_without_base_url_is_a_load_error() {
        let mut remote = settings();
        remote.embed.source = ModelSource::LocalOpenai;
        remote.embed.model = "m".into();
        let error = provider().load(&remote).err().expect("ошибка загрузки");
        assert!(error.to_string().contains("base_url"), "{error}");
    }

    #[test]
    fn batch_size_is_four_for_builtin_and_from_settings_for_external() {
        let provider = provider();
        assert_eq!(provider.batch_size(&settings()), BATCH_SIZE);
        let mut remote = settings();
        remote.embed.source = ModelSource::Remote;
        remote.embed.batch_size = 17;
        assert_eq!(provider.batch_size(&remote), 17);
    }

    #[test]
    fn rerank_model_is_loaded_lazily_and_shared_until_settings_change() {
        let provider = FakeProvider::new("m1");
        let models = Models::new(provider.clone());
        let mut reranker = models.reranker(&settings());
        assert!(!reranker.slot().is_loaded());
        assert_eq!(provider.counters.rerank_loads.load(Ordering::SeqCst), 0);

        let scores = reranker
            .rerank("борщ", &["рецепт борща", "очередь"])
            .unwrap();
        assert!(scores[0] > scores[1]);
        reranker.rerank("борщ", &["x"]).unwrap();
        assert_eq!(provider.counters.rerank_loads.load(Ordering::SeqCst), 1);

        let again = models.reranker(&settings());
        assert!(Arc::ptr_eq(reranker.slot(), again.slot()));
        let mut changed = settings();
        changed.rerank.model = "другой".into();
        let other = models.reranker(&changed);
        assert!(!Arc::ptr_eq(reranker.slot(), other.slot()));
    }

    #[test]
    fn rerank_that_cannot_load_is_an_error_from_the_call() {
        let provider = Arc::new(FakeProvider {
            fail_load: true,
            ..FakeProvider::plain("m1")
        });
        let models = Models::new(provider);
        let mut reranker = models.reranker(&settings());
        assert!(matches!(
            reranker.rerank("q", &["d"]),
            Err(RerankError::Load(_))
        ));
    }
}
