//! Ожидание загрузки моделей поиском (ISS-105): скачанная модель, выгруженная из памяти, грузится
//! заново, и поиск ждёт её до предела; не дождался — выдача без недоступного сигнала с пометкой.

use std::sync::atomic::Ordering;

use bars_protocol::{SearchOutput, SearchParams};

use super::*;
use crate::autostart::MemoryAutostart;
use crate::handler::WorkerHandler;
use crate::worker::Worker;

struct Env {
    provider: Arc<FakeProvider>,
    worker: Worker,
    handler: WorkerHandler,
    root: PathBuf,
    _projects: TempDir,
    _home: TempDir,
}

impl Env {
    /// Проект проиндексирован, обе модели выгружены, файлы моделей «на диске».
    async fn unloaded(provider: FakeProvider, rerank: bool, wait: Duration) -> Self {
        let provider = Arc::new(FakeProvider {
            background: true,
            files_on_disk: true,
            ..provider
        });
        let home = TempDir::new().unwrap();
        if rerank {
            // Rerank включается только настройкой пользователя: проект его не перекрывает.
            fs::write(
                home.path().join("settings.toml"),
                "[rerank]\nenabled=true\n",
            )
            .unwrap();
        }
        let mut config = WorkerConfig::new(home.path().to_path_buf()).without_graph_pause();
        config.secrets = Arc::new(bars_core::secrets::MemorySecrets::default());
        config.autostart = Arc::new(MemoryAutostart::new("\"/opt/bars/bars\"", None));
        let worker = Worker::start(config, provider.clone()).unwrap();
        let mut handler = WorkerHandler::new(worker.clone());
        handler.model_wait = wait;
        let projects = TempDir::new().unwrap();
        let root = projects.path().join("proj");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join(".bars")).unwrap();
        fs::write(root.join("docs/a.md"), "# Альфа\n\nТекст про индексацию.\n").unwrap();
        worker.register(&root).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            tokio::time::sleep(Duration::from_millis(60)).await;
            let status = worker.status(&root).unwrap();
            if worker.is_idle() && status.state == ProjectState::Ready && status.pending == 0 {
                break;
            }
            assert!(Instant::now() < deadline, "индекс не готов");
        }
        // Выгрузка по таймеру: как после простоя.
        worker
            .models()
            .release_idle(true, Duration::ZERO, Duration::ZERO);
        Self {
            provider,
            worker,
            handler,
            root,
            _projects: projects,
            _home: home,
        }
    }

    async fn search(&self) -> SearchOutput {
        self.handler
            .search(search(&self.root, "индексацию"))
            .await
            .unwrap()
    }

    /// «Искать по словам сейчас»: `wait_models: false` (OQ-280).
    async fn search_now(&self) -> SearchOutput {
        self.handler
            .search(SearchParams {
                wait_models: Some(false),
                ..search(&self.root, "индексацию")
            })
            .await
            .unwrap()
    }

    fn embed_loads(&self) -> usize {
        self.provider.counters.loads.load(Ordering::SeqCst)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn search_waits_for_an_unloaded_embed_and_answers_in_full() {
    let provider = FakeProvider {
        load_delay: Duration::from_millis(400),
        ..FakeProvider::plain("m1")
    };
    let env = Env::unloaded(provider, false, Duration::from_secs(10)).await;
    let before = env.embed_loads();
    let found = env.search().await;
    assert!(found.semantic_error.is_none(), "{:?}", found.semantic_error);
    assert!(found.rerank_error.is_none());
    assert_eq!(found.results[0].path, "docs/a.md");
    assert_eq!(env.embed_loads(), before + 1, "одна загрузка");
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn search_slower_than_the_wait_answers_by_text_only_with_a_note() {
    let provider = FakeProvider {
        load_delay: Duration::from_millis(2500),
        ..FakeProvider::plain("m1")
    };
    let env = Env::unloaded(provider, false, Duration::from_millis(300)).await;
    let started = Instant::now();
    let found = env.search().await;
    assert!(
        started.elapsed() < Duration::from_millis(2000),
        "ждал дольше предела"
    );
    let note = found.semantic_error.expect("пометка о смысловой части");
    assert!(note.contains("смысловая близость не учтена"), "{note}");
    assert_eq!(found.results[0].path, "docs/a.md");
    // Косинуса нет, порога близости тоже.
    assert_eq!(found.applied.min_score, 0.0);
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_searches_share_one_model_load() {
    let provider = FakeProvider {
        load_delay: Duration::from_millis(400),
        ..FakeProvider::plain("m1")
    };
    let env = Env::unloaded(provider, false, Duration::from_secs(10)).await;
    let before = env.embed_loads();
    let (a, b, c) = tokio::join!(env.search(), env.search(), env.search());
    for found in [a, b, c] {
        assert!(found.semantic_error.is_none());
        assert_eq!(found.results[0].path, "docs/a.md");
    }
    assert_eq!(env.embed_loads(), before + 1, "модель загружена один раз");
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn search_waits_for_a_loading_rerank_and_reranks() {
    let provider = FakeProvider {
        rerank_load_delay: Duration::from_millis(400),
        ..FakeProvider::plain("m1")
    };
    let env = Env::unloaded(provider, true, Duration::from_secs(10)).await;
    let found = env.search().await;
    assert!(found.rerank_error.is_none(), "{:?}", found.rerank_error);
    assert!(found.semantic_error.is_none());
    assert_eq!(found.applied.score_kind, bars_protocol::ScoreKind::Rerank);
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rerank_slower_than_the_wait_is_skipped_after_the_honest_wait() {
    let provider = FakeProvider {
        rerank_load_delay: Duration::from_millis(2500),
        ..FakeProvider::plain("m1")
    };
    let env = Env::unloaded(provider, true, Duration::from_millis(400)).await;
    let started = Instant::now();
    let found = env.search().await;
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_millis(300),
        "не дождался предела: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_millis(2000),
        "ждал дольше предела: {elapsed:?}"
    );
    assert!(found.rerank_error.is_some());
    assert!(found.semantic_error.is_none());
    assert_eq!(found.applied.score_kind, bars_protocol::ScoreKind::Cosine);
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wait_models_false_answers_at_once_by_text_with_the_same_note() {
    // Предел 10 с и загрузка 2,5 с: без `wait_models: false` поиск ждал бы модель.
    let provider = FakeProvider {
        load_delay: Duration::from_millis(2500),
        ..FakeProvider::plain("m1")
    };
    let env = Env::unloaded(provider, false, Duration::from_secs(10)).await;
    let started = Instant::now();
    let found = env.search_now().await;
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "ждал модель: {:?}",
        started.elapsed()
    );
    let note = found.semantic_error.expect("пометка о смысловой части");
    assert!(note.contains("смысловая близость не учтена"), "{note}");
    assert_eq!(found.results[0].path, "docs/a.md");
    assert_eq!(found.applied.min_score, 0.0);
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wait_models_false_marks_both_unloaded_models_as_skipped_without_waiting() {
    let provider = FakeProvider {
        rerank_load_delay: Duration::from_millis(2500),
        ..FakeProvider::plain("m1")
    };
    let env = Env::unloaded(provider, true, Duration::from_secs(10)).await;
    let started = Instant::now();
    let found = env.search_now().await;
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "ждал модель: {:?}",
        started.elapsed()
    );
    assert!(found.semantic_error.is_some());
    assert!(found.rerank_error.is_some());
    assert_eq!(found.applied.score_kind, bars_protocol::ScoreKind::None);
    assert_eq!(found.results[0].path, "docs/a.md");
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wait_models_false_does_not_degrade_a_search_with_loaded_models() {
    let env = Env::unloaded(FakeProvider::plain("m1"), false, Duration::from_secs(10)).await;
    // Обычный поиск ждёт загрузку и оставляет модель в памяти: «сейчас» отвечает полностью.
    let waited = env.search().await;
    assert!(waited.semantic_error.is_none(), "{:?}", waited.semantic_error);
    let found = env.search_now().await;
    assert!(found.semantic_error.is_none(), "{:?}", found.semantic_error);
    assert_eq!(found.applied.score_kind, bars_protocol::ScoreKind::Cosine);
    assert_eq!(found.results[0].path, "docs/a.md");
    env.worker.shutdown().await;
}
