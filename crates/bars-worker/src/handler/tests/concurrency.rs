//! Регрессия ISS-16: настоящий IPC и обработчик окна на одном индексирующем воркере.
use super::*;
use bars_core::models::{EmbedError, Embedder, ModelFingerprint, RerankError, Reranker};
use bars_core::settings::Settings;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, mpsc};

struct Provider {
    inner: Arc<FakeProvider>,
    block: Arc<AtomicBool>,
    entered: mpsc::Sender<()>,
    resume: Arc<Mutex<mpsc::Receiver<()>>>,
}
struct Model {
    inner: Box<dyn Embedder + Send>,
    block: Arc<AtomicBool>,
    entered: mpsc::Sender<()>,
    resume: Arc<Mutex<mpsc::Receiver<()>>>,
}
impl ModelProvider for Provider {
    fn fingerprint(&self, s: &Settings) -> Result<ModelFingerprint, EmbedError> {
        self.inner.fingerprint(s)
    }
    fn load(&self, s: &Settings) -> Result<Box<dyn Embedder + Send>, EmbedError> {
        Ok(Box::new(Model {
            inner: self.inner.load(s)?,
            block: self.block.clone(),
            entered: self.entered.clone(),
            resume: self.resume.clone(),
        }))
    }
    fn load_reranker(&self, s: &Settings) -> Result<Box<dyn Reranker + Send>, RerankError> {
        self.inner.load_reranker(s)
    }
}
impl Embedder for Model {
    fn fingerprint(&self) -> &ModelFingerprint {
        self.inner.fingerprint()
    }
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if self.block.swap(false, Ordering::SeqCst) {
            self.entered.send(()).unwrap();
            self.resume
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }
        // Достаточно документов, чтобы поиск успел завершиться до конца полной индексации.
        std::thread::sleep(Duration::from_millis(20));
        self.inner.embed(texts)
    }
    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, EmbedError> {
        self.inner.embed_query(text)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_rebuild_keeps_status_window_controls_and_full_ipc_search_responsive() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let block = Arc::new(AtomicBool::new(false));
    let provider = Arc::new(Provider {
        inner: FakeProvider::new("m1"),
        block: block.clone(),
        entered: entered_tx,
        resume: Arc::new(Mutex::new(resume_rx)),
    });
    let home = TempDir::new().unwrap();
    fs::write(
        home.path().join("settings.toml"),
        "[rerank]\nenabled = true\n",
    )
    .unwrap();
    let project = TempDir::new().unwrap();
    let root = project.path();
    fs::create_dir(root.join("docs")).unwrap();
    for i in 0..40 {
        fs::write(
            root.join(format!("docs/{i}.md")),
            format!("# Документ {i}\n\nИндексация и поиск раздела {i}.\n"),
        )
        .unwrap();
    }
    let worker =
        crate::worker::Worker::start(WorkerConfig::new(home.path().into()), provider).unwrap();
    let handler = crate::handler::WorkerHandler::new(worker.clone());
    let endpoint = Endpoint::named("iss16-concurrency");
    let server = tokio::spawn(crate::ipc::run_server(
        crate::ipc::bind(&endpoint).unwrap(),
        Arc::new(handler.clone()),
    ));
    worker.register(root).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !worker.is_idle() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(worker.status(root).unwrap().last_error.is_none());
    let mut client = Client::connect(&endpoint, ClientKind::Mcp).await.unwrap();
    block.store(true, Ordering::SeqCst);
    worker.reindex(root, true, false).unwrap();
    tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(5)).unwrap())
        .await
        .unwrap();
    fs::write(
        root.join("docs/new.md"),
        "# Новый\n\nДокумент появился после начала полной индексации.\n",
    )
    .unwrap();
    // Пачка ещё удерживает модель: статус/ход и приём проверки от неё не зависят.
    let status = tokio::time::timeout(
        Duration::from_millis(500),
        client.call(&StatusParams {
            target: target(root),
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(status.state, ProjectState::Indexing);
    for call in [
        Call::Activity(ActivityParams {}),
        Call::OperationEvents(Default::default()),
        Call::Reindex(ReindexParams {
            target: target(root),
            full: false,
            accept_model_change: false,
        }),
    ] {
        tokio::time::timeout(Duration::from_millis(500), handler.dispatch(call))
            .await
            .unwrap()
            .unwrap();
    }
    let scan_deadline = Instant::now() + Duration::from_millis(500);
    let epoch = worker.operation_events(&Default::default()).epoch;
    loop {
        let events = worker.operation_events(&bars_protocol::OperationEventsParams {
            epoch: Some(epoch.clone()),
            after: Some(0),
        });
        if let Some(scan) = events.completed.iter().find(|op| op.kind == "scan") {
            assert_eq!(scan.state, "success");
            let report = scan.report.as_ref().unwrap();
            assert_eq!(report.added, 1);
            assert!(!report.index_applied);
            break;
        }
        assert!(
            Instant::now() < scan_deadline,
            "проверка ждёт занятую модель"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        worker.status(root).unwrap().queued,
        "изменение обнаружено, обновление ждёт текущую индексацию"
    );
    resume_tx.send(()).unwrap();
    for window in [false, true] {
        let result: bars_protocol::SearchOutput =
            tokio::time::timeout(Duration::from_millis(500), async {
                if window {
                    serde_json::from_value(
                        handler
                            .dispatch(Call::Search(search(root, "индексация")))
                            .await
                            .unwrap(),
                    )
                    .unwrap()
                } else {
                    client.call(&search(root, "индексация")).await.unwrap()
                }
            })
            .await
            .unwrap();
        assert!(!result.results.is_empty());
        assert!(
            result.indexing,
            "поиск должен закончиться до конца индексации"
        );
        assert_eq!(result.applied.score_kind, bars_protocol::ScoreKind::Rerank);
        assert!(result.rerank_error.is_none());
    }
    while !worker.is_idle() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let index =
        bars_core::storage::Index::open_read_only(&bars_core::storage::index_path(root)).unwrap();
    assert!(
        index
            .documents()
            .unwrap()
            .iter()
            .any(|d| d.path == "docs/new.md"),
        "обновление после подмены не должно терять найденный документ"
    );
    drop(index);
    worker.shutdown().await;
    server.abort();
}
