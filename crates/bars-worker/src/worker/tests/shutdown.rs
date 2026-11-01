//! Управляемый embed подтверждает границу выхода без зависимости от скорости модели.
use super::*;
use bars_core::models::{EmbedError, Embedder, ModelFingerprint};
use std::sync::{Condvar, atomic::AtomicBool};

#[derive(Default)]
struct EmbedGate {
    armed: AtomicBool,
    entered: tokio::sync::Notify,
    released: Mutex<bool>,
    wake: Condvar,
}

impl EmbedGate {
    fn release(&self) {
        *lock(&self.released) = true;
        self.wake.notify_all();
    }

    fn wait(&self) {
        if !self.armed.swap(false, Ordering::SeqCst) {
            return;
        }
        self.entered.notify_one();
        let (released, _) = self
            .wake
            .wait_timeout_while(lock(&self.released), Duration::from_secs(20), |v| !*v)
            .unwrap();
        assert!(*released, "тест должен отпустить embed");
    }
}

struct ControlledProvider {
    provider: Arc<FakeProvider>,
    gate: Arc<EmbedGate>,
}

struct ControlledEmbedder {
    inner: Box<dyn Embedder + Send>,
    gate: Arc<EmbedGate>,
}

impl Embedder for ControlledEmbedder {
    fn fingerprint(&self) -> &ModelFingerprint {
        self.inner.fingerprint()
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.gate.wait();
        self.inner.embed(texts)
    }
}

impl ModelProvider for ControlledProvider {
    fn fingerprint(&self, settings: &Settings) -> Result<ModelFingerprint, EmbedError> {
        self.provider.fingerprint(settings)
    }

    fn load(&self, settings: &Settings) -> Result<Box<dyn Embedder + Send>, EmbedError> {
        Ok(Box::new(ControlledEmbedder {
            inner: self.provider.load(settings)?,
            gate: Arc::clone(&self.gate),
        }))
    }

    fn download(
        &self,
        _settings: &Settings,
        _reranker: bool,
    ) -> Result<(), bars_core::models::ModelFailure> {
        self.gate.wait();
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_job_created_after_shutdown_does_not_open_an_index_or_load_a_model() {
    let env = Env::new();
    let worker = env.start();
    worker.shutdown().await;
    let project = env.project("late", &[("a.md", DOC)]);
    let job = queue::Job::check(Priority::Explicit);
    let cell = JobCell::new(&project, &job);
    assert!(!cell.cancel_requested());
    let observer = JobObserver::new(
        Arc::clone(&worker.inner.pause),
        cell,
        Arc::clone(&worker.inner.stop),
    );
    let result = job::run(
        &JobEnv {
            bars_home: env.home.path(),
            models: &worker.inner.models,
            gate: Arc::new(tokio::sync::RwLock::new(())),
            handle: worker.inner.handle.clone(),
            observer: &observer,
        },
        &project,
        &job,
    );
    assert!(matches!(result.outcome, Outcome::Cancelled));
    assert!(!project.join(".bars").exists());
    assert_eq!(
        env.provider.counters.load_attempts.load(Ordering::SeqCst),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_does_not_wait_for_an_unfinished_download() {
    let env = Env::new();
    let gate = Arc::new(EmbedGate::default());
    gate.armed.store(true, Ordering::SeqCst);
    let worker = Worker::start(
        env.config(),
        Arc::new(ControlledProvider {
            provider: Arc::clone(&env.provider),
            gate: Arc::clone(&gate),
        }),
    )
    .unwrap();
    assert!(worker.start_download("BAAI/bge-m3", false));
    tokio::time::timeout(Duration::from_secs(20), gate.entered.notified())
        .await
        .expect("скачивание началось");
    // Скачивание остаётся удержанным до окончания shutdown.
    tokio::time::timeout(Duration::from_secs(5), worker.shutdown())
        .await
        .expect("выход не ждёт скачивания");
    assert!(worker.downloading().contains("BAAI/bge-m3"));
    gate.release();
    wait_until(
        "собственная задача скачивания закончилась",
        || worker.downloading().is_empty(),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_commits_only_the_current_document_and_startup_finishes_the_rest() {
    interrupted_job(false, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_discards_the_rebuild_and_preserves_consent_on_startup() {
    interrupted_job(true, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_wakes_a_running_job_paused_between_documents() {
    interrupted_job(false, true).await;
}

async fn interrupted_job(rebuild: bool, paused: bool) {
    let env = Env::new();
    let project = env.project("alpha", &[("old.md", DOC)]);
    let queued = env.project("queued", &[("waiting.md", DOC)]);
    let gate = Arc::new(EmbedGate::default());
    let worker = Worker::start(
        env.config(),
        Arc::new(ControlledProvider {
            provider: Arc::clone(&env.provider),
            gate: Arc::clone(&gate),
        }),
    )
    .unwrap();
    worker.register(&project).unwrap();
    idle(&worker).await;
    let old_index = fs::read(index_path(&project)).unwrap();
    for n in 0..3 {
        fs::write(
            project.join(format!("docs/new-{n}.md")),
            format!("# Новый документ {n}\n\nСодержимое для индексации {n}.\n"),
        )
        .unwrap();
    }
    if rebuild {
        env.provider.set_model("m2");
        worker.reindex(&project, false, false).unwrap();
        idle(&worker).await;
        assert_eq!(state(&worker, &project), ProjectState::ReindexRequired);
    }
    let before = env.provider.counters.embed_calls.load(Ordering::SeqCst);
    let previous_error = Some("Ошибка предыдущего задания".to_string());
    lock(&worker.inner.projects)
        .get_mut(&project)
        .unwrap()
        .status
        .last_error
        .clone_from(&previous_error);
    gate.armed.store(true, Ordering::SeqCst);
    worker.reindex(&project, rebuild, rebuild).unwrap();
    tokio::time::timeout(Duration::from_secs(20), gate.entered.notified())
        .await
        .expect("индексация должна войти в управляемый embed");
    worker.register(&queued).unwrap();
    wait_until("второй проект ждёт", || {
        worker.queued().iter().any(|job| job.project == queued)
    })
    .await;

    if paused {
        worker.pause();
        gate.release();
        wait_until("идущее задание на паузе", || {
            lock(&worker.inner.current)
                .as_ref()
                .is_some_and(|cell| cell.paused())
        })
        .await;
    }
    let shutdown = {
        let worker = worker.clone();
        tokio::spawn(async move { worker.shutdown().await })
    };
    wait_until("воркер принял выход", || {
        worker.is_stopping()
    })
    .await;
    if !paused {
        assert!(!shutdown.is_finished(), "текущий embed должен дописаться");
        gate.release();
    }
    tokio::time::timeout(Duration::from_secs(20), shutdown)
        .await
        .expect("выход не должен ждать остальные документы")
        .unwrap();
    assert_eq!(
        env.provider.counters.embed_calls.load(Ordering::SeqCst),
        before + 1
    );
    assert!(worker.progress().is_none());
    assert_eq!(worker.status(&project).unwrap().last_error, previous_error);
    assert_eq!(documents_or_zero(&queued), 0);
    let mut temporary = index_path(&project).into_os_string();
    temporary.push(".new");
    assert!(!PathBuf::from(temporary).exists());
    if rebuild {
        assert_eq!(state(&worker, &project), ProjectState::ReindexRequired);
        assert_eq!(fs::read(index_path(&project)).unwrap(), old_index);
        assert_eq!(documents(&project), 1);
    } else {
        assert_eq!(state(&worker, &project), ProjectState::Ready);
        assert_eq!(
            documents(&project),
            2,
            "текущий документ записан, следующие — нет"
        );
    }
    drop(worker);

    let restarted = env.start();
    idle(&restarted).await;
    if rebuild {
        assert_eq!(state(&restarted, &project), ProjectState::ReindexRequired);
        assert_eq!(documents(&project), 1);
        restarted.reindex(&project, true, true).unwrap();
        idle(&restarted).await;
    }
    assert_eq!(documents(&project), 4);
    restarted.shutdown().await;
}
