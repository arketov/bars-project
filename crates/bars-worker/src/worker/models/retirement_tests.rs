use super::*;
use std::sync::mpsc;

struct SlowDrop {
    started: mpsc::Sender<std::thread::ThreadId>,
    proceed: mpsc::Receiver<()>,
}

impl Drop for SlowDrop {
    fn drop(&mut self) {
        self.started.send(std::thread::current().id()).unwrap();
        self.proceed.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}

struct SlowEmbed(SlowDrop, ModelFingerprint);
impl Embedder for SlowEmbed {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.1
    }
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let _ = &self.0;
        Ok(vec![vec![1.0]; texts.len()])
    }
}

struct SlowRerank(SlowDrop);
impl Reranker for SlowRerank {
    fn rerank(&mut self, _: &str, documents: &[&str]) -> Result<Vec<f32>, RerankError> {
        let _ = &self.0;
        Ok(vec![1.0; documents.len()])
    }
}

fn probe() -> (
    SlowDrop,
    mpsc::Receiver<std::thread::ThreadId>,
    mpsc::Sender<()>,
) {
    let (started, observed) = mpsc::channel();
    let (proceed, wait) = mpsc::channel();
    (
        SlowDrop {
            started,
            proceed: wait,
        },
        observed,
        proceed,
    )
}

#[tokio::test]
async fn replacement_and_last_reader_drop_leave_the_async_thread_free() {
    for reranker in [false, true] {
        for retain_reader in [false, true] {
            let models = Models::new(fake::FakeProvider::new("m1"));
            let mut settings = Settings::resolve(&Default::default(), &Default::default());
            let (slow, started, proceed) = probe();
            let mut embed = None;
            let mut rerank = None;
            if reranker {
                let reader = models.reranker(&settings);
                lock(&reader.slot.model).model = Some(Box::new(SlowRerank(slow)));
                if retain_reader {
                    rerank = Some(reader);
                }
            } else {
                let reader = models.embedder(&settings).unwrap();
                lock(&reader.slot.model).model = Some(Box::new(SlowEmbed(
                    slow,
                    bars_core::models::bge_m3_fingerprint(),
                )));
                if retain_reader {
                    embed = Some(reader);
                }
            }
            settings.embed.accelerator = Accelerator::Cpu;
            settings.rerank.accelerator = Accelerator::Cpu;
            if reranker {
                let _ = models.reranker(&settings);
            } else {
                let _ = models.embedder(&settings).unwrap();
            }
            if retain_reader {
                assert!(
                    started.try_recv().is_err(),
                    "старый запрос ещё владеет сессией"
                );
                if let Some(reader) = &mut embed {
                    assert!(reader.embed_query("q").is_ok());
                }
                if let Some(reader) = &mut rerank {
                    assert!(reader.rerank("q", &["d"]).is_ok());
                }
            }
            drop(embed);
            drop(rerank);
            let destructor_thread = started.recv_timeout(Duration::from_secs(2)).unwrap();
            assert_ne!(destructor_thread, std::thread::current().id());
            // Destructor всё ещё ждёт; короткая async-задача и замки доступны.
            tokio::spawn(async {}).await.unwrap();
            assert!(models.current.try_lock().is_ok());
            assert!(models.current_rerank.try_lock().is_ok());
            assert!(
                models.loads.gate.try_lock().is_err(),
                "constructor не накладывается на drop"
            );
            proceed.send(()).unwrap();
            tokio::task::spawn_blocking(move || models.clear())
                .await
                .unwrap();
        }
    }
}

#[test]
fn clear_waits_for_already_submitted_destruction() {
    let models = Arc::new(Models::new(fake::FakeProvider::new("m1")));
    let settings = Settings::resolve(&Default::default(), &Default::default());
    let (slow, started, proceed) = probe();
    let reader = models.embedder(&settings).unwrap();
    lock(&reader.slot.model).model = Some(Box::new(SlowEmbed(
        slow,
        bars_core::models::bge_m3_fingerprint(),
    )));
    drop(reader);
    let (finished, observed) = mpsc::channel();
    let owner = Arc::clone(&models);
    let cleanup = std::thread::spawn(move || {
        owner.clear();
        finished.send(()).unwrap();
    });
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(observed.try_recv().is_err());
    proceed.send(()).unwrap();
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    cleanup.join().unwrap();
}

#[test]
fn replacement_constructor_waits_until_old_session_is_freed() {
    let provider = fake::FakeProvider::new("m1");
    let models = Arc::new(Models::new(provider.clone()));
    let mut settings = Settings::resolve(&Default::default(), &Default::default());
    let (slow, started, proceed) = probe();
    let reader = models.embedder(&settings).unwrap();
    lock(&reader.slot.model).model = Some(Box::new(SlowEmbed(
        slow,
        bars_core::models::bge_m3_fingerprint(),
    )));
    drop(reader);
    settings.embed.accelerator = Accelerator::Cpu;
    let mut replacement = models.embedder(&settings).unwrap();
    let (attempting, attempt) = mpsc::channel();
    let (finished, done) = mpsc::channel();
    let constructor = std::thread::spawn(move || {
        attempting.send(()).unwrap();
        replacement.prepare().unwrap();
        finished.send(()).unwrap();
    });
    attempt.recv_timeout(Duration::from_secs(2)).unwrap();
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        done.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert_eq!(provider.counters.load_attempts.load(Ordering::SeqCst), 0);
    proceed.send(()).unwrap();
    done.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(provider.counters.loads.load(Ordering::SeqCst), 1);
    constructor.join().unwrap();
    models.clear();
}

#[tokio::test]
async fn worker_shutdown_waits_for_retirement_without_blocking_async_tasks() {
    use crate::worker::{Worker, WorkerConfig};
    let home = tempfile::tempdir().unwrap();
    let worker = Worker::start(
        WorkerConfig::new(home.path().into()),
        fake::FakeProvider::new("m1"),
    )
    .unwrap();
    let mut settings = Settings::resolve(&Default::default(), &Default::default());
    let (slow, started, proceed) = probe();
    let reader = worker.models().embedder(&settings).unwrap();
    lock(&reader.slot.model).model = Some(Box::new(SlowEmbed(
        slow,
        bars_core::models::bge_m3_fingerprint(),
    )));
    drop(reader);
    settings.embed.accelerator = Accelerator::Cpu;
    let _ = worker.models().embedder(&settings).unwrap();
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    let owner = worker.clone();
    let shutdown = tokio::spawn(async move { owner.shutdown().await });
    tokio::task::yield_now().await;
    assert!(!shutdown.is_finished());
    tokio::spawn(async {}).await.unwrap();
    proceed.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), shutdown)
        .await
        .unwrap()
        .unwrap();
}
