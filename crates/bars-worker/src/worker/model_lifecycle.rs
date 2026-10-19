//! Автоматическая выгрузка локальных сессий; blocking drop зарегистрирован в shutdown.
use super::{Inner, Worker};
use std::sync::{Arc, atomic::Ordering};
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct ModelIdlePolicy {
    pub embed: Duration,
    pub rerank: Duration,
    pub check_interval: Duration,
}

impl Default for ModelIdlePolicy {
    fn default() -> Self {
        Self {
            embed: Duration::from_secs(900),
            rerank: Duration::from_secs(300),
            check_interval: Duration::from_secs(10),
        }
    }
}

impl ModelIdlePolicy {
    /// Сроки с учётом `unload_after_minutes` из настроек: `0` — не выгружать, незаданный ключ —
    /// срок политики.
    pub fn with_settings(self, unload: bars_core::settings::ModelUnload) -> Self {
        let pick = |minutes: Option<u32>, fallback: Duration| match minutes {
            Some(0) => Duration::MAX,
            Some(minutes) => Duration::from_secs(u64::from(minutes) * 60),
            None => fallback,
        };
        Self {
            embed: pick(unload.embed_minutes, self.embed),
            rerank: pick(unload.rerank_minutes, self.rerank),
            check_interval: self.check_interval,
        }
    }
}

impl Worker {
    pub(crate) fn refresh_model_lifecycle(&self) {
        self.inner.model_wake.notify_one();
    }
}

impl Inner {
    pub(super) async fn run_model_maintenance(self: Arc<Self>) {
        let policy = self.config.model_idle;
        loop {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep(policy.check_interval.max(Duration::from_millis(1))) => {},
                _ = self.model_wake.notified() => {},
            }
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            let inner = Arc::clone(&self);
            // Await is intentional: shutdown joins this task, including an in-flight drop.
            if tokio::task::spawn_blocking(move || {
                // Битый файл настроек — прежняя политика, rerank считается включённым.
                let (enabled, idle) =
                    match bars_core::settings::UserSettings::load(&inner.config.bars_home) {
                        Ok(settings) => (
                            settings.value.rerank.enabled,
                            policy.with_settings(settings.value.unload),
                        ),
                        Err(_) => (true, policy),
                    };
                inner.models.release_idle(enabled, idle.embed, idle.rerank);
            })
            .await.is_err()
            {
                log::error!("Не удалось очистить неиспользуемые модели");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::{WorkerConfig, models::fake::FakeProvider};
    use bars_core::{
        models::{Embedder, Reranker},
        settings::{Settings, UserSettings},
    };

    async fn wait(mut predicate: impl FnMut() -> bool) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !predicate() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("model maintenance did not complete");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn timer_releases_both_sessions_and_next_query_reloads() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join("settings.toml"),
            "[rerank]\nenabled = true\n",
        )
        .unwrap();
        let mut config = WorkerConfig::new(home.path().into());
        config.model_idle = ModelIdlePolicy {
            embed: Duration::from_millis(40),
            rerank: Duration::from_millis(40),
            check_interval: Duration::from_millis(10),
        };
        let provider = FakeProvider::new("m1");
        let worker = Worker::start(config, provider.clone()).unwrap();
        let mut user = UserSettings::default();
        user.rerank.enabled = true;
        let settings = Settings::resolve(&user, &Default::default());
        let mut embed = worker.inner.models.embedder(&settings).unwrap();
        let mut rerank = worker.inner.models.reranker(&settings);
        let vector = embed.embed_query("q").unwrap();
        let scores = rerank.rerank("q", &["q", "d"]).unwrap();
        wait(|| !embed.slot().is_loaded() && !rerank.slot().is_loaded()).await;
        assert_eq!(embed.embed_query("q").unwrap(), vector);
        assert_eq!(rerank.rerank("q", &["q", "d"]).unwrap(), scores);
        assert_eq!(provider.counters.loads.load(Ordering::SeqCst), 2);
        assert_eq!(provider.counters.rerank_loads.load(Ordering::SeqCst), 2);
        worker.shutdown().await;
    }

    #[test]
    fn settings_minutes_override_the_policy_and_zero_means_never() {
        use bars_core::settings::ModelUnload;
        let policy = ModelIdlePolicy::default();
        let same = policy.with_settings(ModelUnload::default());
        assert_eq!((same.embed, same.rerank), (policy.embed, policy.rerank));
        let set = policy.with_settings(ModelUnload {
            embed_minutes: Some(0),
            rerank_minutes: Some(30),
        });
        assert_eq!(set.embed, Duration::MAX);
        assert_eq!(set.rerank, Duration::from_secs(1800));
        assert_eq!(set.check_interval, policy.check_interval);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unload_after_zero_minutes_keeps_the_model_loaded() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join("settings.toml"),
            "[embed]\nunload_after_minutes = 0\n",
        )
        .unwrap();
        let mut config = WorkerConfig::new(home.path().into());
        config.model_idle = ModelIdlePolicy {
            embed: Duration::from_millis(20),
            rerank: Duration::from_millis(20),
            check_interval: Duration::from_millis(5),
        };
        let worker = Worker::start(config, FakeProvider::new("m1")).unwrap();
        let settings = Settings::resolve(&UserSettings::default(), &Default::default());
        let mut embed = worker.inner.models.embedder(&settings).unwrap();
        embed.prepare().unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(embed.slot().is_loaded(), "0 минут — модель не выгружается");

        std::fs::write(
            home.path().join("settings.toml"),
            "[embed]\nunload_after_minutes = 15\n",
        )
        .unwrap();
        worker.refresh_model_lifecycle();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(embed.slot().is_loaded(), "15 минут ещё не прошли");
        worker.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn disabling_wakes_maintenance_and_shutdown_wakes_long_timer() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join("settings.toml"),
            "[rerank]\nenabled = true\n",
        )
        .unwrap();
        let mut config = WorkerConfig::new(home.path().into());
        config.model_idle = ModelIdlePolicy {
            embed: Duration::MAX,
            rerank: Duration::MAX,
            check_interval: Duration::from_secs(600),
        };
        let worker = Worker::start(config, FakeProvider::new("m1")).unwrap();
        let settings = Settings::resolve(&UserSettings::default(), &Default::default());
        let mut embed = worker.inner.models.embedder(&settings).unwrap();
        let mut rerank = worker.inner.models.reranker(&settings);
        embed.prepare().unwrap();
        rerank.rerank("q", &["d"]).unwrap();
        std::fs::write(
            home.path().join("settings.toml"),
            "[rerank]\nenabled = false\n",
        )
        .unwrap();
        worker.refresh_model_lifecycle();
        wait(|| !rerank.slot().is_loaded()).await;
        assert!(embed.slot().is_loaded());
        tokio::time::timeout(Duration::from_secs(1), worker.shutdown())
            .await
            .expect("shutdown waited for idle timer");
        let next = worker.inner.models.embedder(&settings).unwrap();
        assert!(!Arc::ptr_eq(embed.slot(), next.slot()));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_joins_in_flight_blocking_drop() {
        use crate::worker::ModelProvider;
        use bars_core::models::{EmbedError, ModelFingerprint, RerankError};
        use std::sync::atomic::AtomicBool;
        struct SlowDrop {
            entered: Arc<AtomicBool>,
            release: Arc<AtomicBool>,
        }
        impl Reranker for SlowDrop {
            fn rerank(&mut self, _: &str, documents: &[&str]) -> Result<Vec<f32>, RerankError> {
                Ok(vec![0.0; documents.len()])
            }
        }
        impl Drop for SlowDrop {
            fn drop(&mut self) {
                self.entered.store(true, Ordering::SeqCst);
                let deadline = std::time::Instant::now() + Duration::from_secs(2);
                while !self.release.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
        struct Provider {
            base: Arc<FakeProvider>,
            entered: Arc<AtomicBool>,
            release: Arc<AtomicBool>,
        }
        impl ModelProvider for Provider {
            fn fingerprint(&self, settings: &Settings) -> Result<ModelFingerprint, EmbedError> {
                self.base.fingerprint(settings)
            }
            fn load(&self, settings: &Settings) -> Result<Box<dyn Embedder + Send>, EmbedError> {
                self.base.load(settings)
            }
            fn load_reranker(&self, _: &Settings) -> Result<Box<dyn Reranker + Send>, RerankError> {
                Ok(Box::new(SlowDrop {
                    entered: self.entered.clone(),
                    release: self.release.clone(),
                }))
            }
        }
        let home = tempfile::tempdir().unwrap();
        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let mut config = WorkerConfig::new(home.path().into());
        config.model_idle.check_interval = Duration::from_millis(10);
        let worker = Worker::start(
            config,
            Arc::new(Provider {
                base: FakeProvider::new("m1"),
                entered: entered.clone(),
                release: release.clone(),
            }),
        )
        .unwrap();
        worker
            .inner
            .models
            .reranker(&Settings::resolve(&Default::default(), &Default::default()))
            .rerank("q", &["d"])
            .unwrap();
        // Default rerank is disabled, so maintenance starts dropping it immediately.
        wait(|| entered.load(Ordering::SeqCst)).await;
        let stopping = worker.clone();
        let shutdown = tokio::spawn(async move {
            stopping.shutdown().await;
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        let was_waiting = !shutdown.is_finished();
        release.store(true, Ordering::SeqCst);
        tokio::time::timeout(Duration::from_secs(1), shutdown)
            .await
            .unwrap()
            .unwrap();
        assert!(was_waiting, "shutdown detached an in-flight drop");
    }
}
