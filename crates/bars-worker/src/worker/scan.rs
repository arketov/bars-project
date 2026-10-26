//! Независимая проверка файлов; найденные изменения передаются в очередь обновления (OQ-105).
use super::*;
use bars_core::indexing::indexer::{IndexObserver, IndexingError, ScanSnapshot, scan_index};
use bars_core::storage::{Compatibility, Index, index_path};
use bars_protocol::OperationReport;

pub(super) struct ScanJob {
    id: u64,
    cancel: Arc<AtomicBool>,
    generation: u64,
    key: u64,
    priority: Arc<Mutex<Priority>>,
}
struct Cleanup {
    inner: Arc<Inner>,
    path: PathBuf,
    id: u64,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        let mut scans = lock(&self.inner.scans);
        if scans.get(&self.path).is_some_and(|s| s.id == self.id) {
            scans.remove(&self.path);
        }
    }
}
struct Observer {
    inner: Arc<Inner>,
    id: u64,
    cancel: Arc<AtomicBool>,
    found: Mutex<Vec<String>>,
}

struct ScanResult {
    report: UpdateReport,
    paths: Vec<String>,
}
enum Failure {
    Cancelled,
    ReindexRequired {
        index: Option<String>,
        settings: String,
    },
    Failed(String),
}
fn failed(error: impl ToString) -> Failure {
    Failure::Failed(error.to_string())
}
impl IndexObserver for Observer {
    fn discovered(&self, paths: &[String]) {
        *lock(&self.found) = paths.to_vec();
    }
    fn should_stop(&self) -> bool {
        self.cancel.load(Ordering::SeqCst) || self.inner.stop.load(Ordering::SeqCst)
    }
    fn begin(&self, count: usize) {
        lock(&self.inner.models.operations).edit(self.id, |op| op.docs_total = Some(count));
    }
    fn progress(&self, done: usize, _: usize) {
        lock(&self.inner.models.operations).edit(self.id, |op| op.docs_done = Some(done));
    }
}
impl Worker {
    pub fn scan_receipt(&self, path: &Path) -> Result<(u64, bool, JobKind), WorkerError> {
        self.scan_with_priority(path, Priority::Explicit)
    }

    pub(super) fn scan_with_priority(
        &self,
        path: &Path,
        priority: Priority,
    ) -> Result<(u64, bool, JobKind), WorkerError> {
        let path = self.registered(path)?;
        let settings = Settings::load(self.bars_home(), &path)
            .map_err(|e| io::Error::other(e.to_string()))?
            .value;
        let key = operations::settings_key(&settings);
        let mut tasks = lock(&self.inner.tasks);
        let projects = lock(&self.inner.projects);
        let runtime = projects
            .get(&path)
            .filter(|r| !r.deleting)
            .ok_or_else(|| WorkerError::NotRegistered(path.clone()))?;
        match runtime.status.state {
            ProjectState::Unavailable => return Err(WorkerError::Unavailable(path)),
            ProjectState::ReindexRequired => return Err(WorkerError::ConsentRequired(path)),
            _ => {}
        }
        if self.inner.stop.load(Ordering::SeqCst) {
            return Err(io::Error::other("воркер останавливается").into());
        }
        let mut scans = lock(&self.inner.scans);
        if let Some(scan) = scans.get(&path) {
            if scan.generation == runtime.generation
                && scan.key == key
                && !scan.cancel.load(Ordering::SeqCst)
            {
                let mut current = lock(&scan.priority);
                *current = (*current).max(priority);
                lock(&self.inner.models.operations).edit(scan.id, |op| {
                    op.source = match *current {
                        Priority::Explicit => "explicit",
                        Priority::Edit => "edit",
                        Priority::Watch => "watch",
                        Priority::Periodic => "periodic",
                    }
                    .into();
                });
                return Ok((scan.id, true, JobKind::Check));
            }
            scan.cancel.store(true, Ordering::SeqCst);
        }
        let generation = runtime.generation;
        let gate = runtime.gate.clone();
        let id = lock(&self.inner.models.operations).begin(
            "scan",
            match priority {
                Priority::Explicit => "explicit",
                Priority::Edit => "edit",
                Priority::Watch => "watch",
                Priority::Periodic => "periodic",
            },
            Some(path.display().to_string()),
            generation,
            None,
            None,
            false,
            key,
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let scan_priority = Arc::new(Mutex::new(priority));
        scans.insert(
            path.clone(),
            ScanJob {
                id,
                cancel: cancel.clone(),
                generation,
                key,
                priority: scan_priority.clone(),
            },
        );
        let inner = self.inner.clone();
        let task = self.inner.handle.spawn(async move {
            let _cleanup = Cleanup {
                inner: inner.clone(),
                path: path.clone(),
                id,
            };
            let _pending = operations::PendingResult(inner.models.operations.clone(), id);
            let read = gate.read_owned().await;
            let worker = inner.clone();
            let root = path.clone();
            let stopped = cancel.clone();
            let result = tokio::task::spawn_blocking(move || -> Result<ScanResult, Failure> {
                let index_gate = read;
                let observer = Observer {
                    inner: worker.clone(),
                    id,
                    cancel,
                    found: Mutex::new(Vec::new()),
                };
                if observer.should_stop() {
                    return Err(Failure::Cancelled);
                }
                lock(&worker.models.operations).edit(id, |op| op.state = "running".into());
                let index = if index_path(&root).exists() {
                    Some(Index::open_read_only(&index_path(&root)).map_err(failed)?)
                } else {
                    None
                };
                let fingerprint = worker.models.fingerprint(&settings).map_err(failed)?;
                if let Some(index) = &index {
                    match index.compatibility(&fingerprint).map_err(failed)? {
                        Compatibility::ModelChanged | Compatibility::SchemaOutdated => {
                            return Err(Failure::ReindexRequired {
                                index: index
                                    .stored_fingerprint()
                                    .map_err(failed)?
                                    .map(|fp| job::describe(&fp)),
                                settings: job::describe(&fingerprint),
                            });
                        }
                        _ => {}
                    }
                }
                let snapshot = ScanSnapshot::read(index.as_ref()).map_err(failed)?;
                drop(index);
                // Обход диска не удерживает читателя перед ожидающей подменой индекса.
                drop(index_gate);
                let report = scan_index(&root, &snapshot, &settings.index_settings(), &observer)
                    .map_err(|e| {
                        if matches!(e, IndexingError::Cancelled) {
                            Failure::Cancelled
                        } else {
                            failed(e)
                        }
                    })?;
                let paths = lock(&observer.found).clone();
                Ok(ScanResult { report, paths })
            })
            .await;
            let mut check_graph = false;
            {
                let mut projects = lock(&inner.projects);
                let priority = *lock(&scan_priority);
                let current = projects
                    .get_mut(&path)
                    .filter(|r| !r.deleting && r.generation == generation);
                let valid = current.is_some()
                    && !stopped.load(Ordering::SeqCst)
                    && !inner.stop.load(Ordering::SeqCst)
                    && Settings::load(&inner.config.bars_home, &path)
                        .is_ok_and(|s| operations::settings_key(&s.value) == key);
                if !valid {
                    lock(&inner.models.operations).finish(
                    id,
                    "cancelled",
                    None,
                    Some(
                        "Проверка устарела или проект остановлен; повторите её при необходимости."
                            .into(),
                    ),
                );
                    return;
                }
                let result = result.map_err(failed).and_then(|v| v);
                match result {
                    Ok(ScanResult { report, paths }) => {
                        // Т-8 сохраняется и при проверке без задания обновления индекса.
                        let _ = bars_core::instruction::sync(&path, &inner.docs_root_of(&path));
                        if let Some(runtime) = current {
                            runtime.status.last_error = None;
                        }
                        check_graph = true;
                        let update = report.full_check
                            || report.added + report.changed + report.removed > 0
                            || !report.problems.is_empty();
                        let outcome = OperationReport {
                            added: report.added,
                            changed: report.changed,
                            removed: report.removed,
                            unchanged: report.unchanged,
                            problems: report.problems.len(),
                            problem_paths: report
                                .problems
                                .iter()
                                .take(20)
                                .map(|p| p.path.chars().take(512).collect())
                                .collect(),
                            index_applied: false,
                        };
                        if update {
                            if projects.get(&path).is_some_and(|runtime| {
                                !matches!(
                                    runtime.status.state,
                                    ProjectState::ReindexRequired | ProjectState::Unavailable
                                )
                            }) {
                                let mut docs = lock(&inner.docs);
                                for document in paths {
                                    docs.found(
                                        &path,
                                        &document,
                                        match priority {
                                            Priority::Explicit => DocSource::Explicit,
                                            Priority::Edit => DocSource::Edit,
                                            Priority::Watch => DocSource::Watch,
                                            Priority::Periodic => DocSource::Periodic,
                                        },
                                    );
                                }
                            }
                            inner.enqueue_for(&path, Job::check(priority), &projects);
                        }
                        lock(&inner.models.operations).finish(
                            id,
                            if report.problems.is_empty() {
                                "success"
                            } else {
                                "partial"
                            },
                            Some(outcome),
                            None,
                        );
                    }
                    Err(failure) => {
                        let (state, error) = match failure {
                            Failure::Cancelled => ("cancelled", None),
                            Failure::ReindexRequired { index, settings } => {
                                if let Some(runtime) = current {
                                    runtime.status.state = ProjectState::ReindexRequired;
                                    runtime.status.model_index = index;
                                    runtime.status.model_settings = Some(settings);
                                }
                                (
                                    "error",
                                    Some("Нужна переиндексация с согласием пользователя.".into()),
                                )
                            }
                            Failure::Failed(message) => {
                                if let Some(runtime) = current {
                                    runtime.status.last_error = Some(message.clone());
                                }
                                ("error", Some(message))
                            }
                        };
                        lock(&inner.models.operations).finish(id, state, None, error);
                    }
                }
            }
            if check_graph {
                let priority = *lock(&scan_priority);
                let worker = Worker {
                    inner: inner.clone(),
                };
                let result = if priority == Priority::Explicit {
                    worker.graph_checked(&path).await
                } else {
                    worker.graph_start(&path, false).await
                };
                if let Err(error) = result {
                    log::warn!("Граф после проверки не поставлен на обновление: {error}");
                }
            }
        });
        tasks.push(task);
        Ok((id, false, JobKind::Check))
    }
}

impl Inner {
    pub(super) fn cancel_scan(&self, path: &Path) -> bool {
        lock(&self.scans)
            .get(path)
            .is_some_and(|s| !s.cancel.swap(true, Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::super::models::fake::FakeProvider;
    use super::*;
    use bars_protocol::OperationEventsParams;

    async fn setup() -> (
        tempfile::TempDir,
        tempfile::TempDir,
        Worker,
        Arc<FakeProvider>,
    ) {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        fs::create_dir(project.path().join("docs")).unwrap();
        fs::create_dir(project.path().join("other")).unwrap();
        fs::write(project.path().join("docs/a.md"), "# Документ\n").unwrap();
        let provider = FakeProvider::new("m1");
        let worker =
            Worker::start(WorkerConfig::new(home.path().into()), provider.clone()).unwrap();
        worker.register(project.path()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !worker.is_idle() {
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        (home, project, worker, provider)
    }
    async fn finish(worker: &Worker, id: u64) -> bars_protocol::Operation {
        let epoch = worker.operation_events(&Default::default()).epoch;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let events = worker.operation_events(&OperationEventsParams {
                epoch: Some(epoch.clone()),
                after: Some(0),
            });
            if let Some(op) = events.completed.into_iter().find(|op| op.id == id) {
                return op;
            }
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn scan_merges_cancels_and_ignores_the_update_pause() {
        let (_home, root, worker, provider) = setup().await;
        worker.pause();
        let gate = worker.gate(root.path()).unwrap().write_owned().await;
        let (id, merged, _) = worker.scan_receipt(root.path()).unwrap();
        assert!(!merged);
        assert_eq!(
            worker.status(root.path()).unwrap().state,
            ProjectState::Indexing
        );
        assert_eq!(worker.projects()[0].state, ProjectState::Indexing);
        assert_eq!(
            worker.scan_receipt(root.path()).unwrap(),
            (id, true, JobKind::Check)
        );
        assert!(worker.cancel(root.path()).unwrap());
        drop(gate);
        assert_eq!(finish(&worker, id).await.state, "cancelled");
        let count = provider.counters.texts.load(Ordering::SeqCst);
        let (next, _, _) = worker.scan_receipt(root.path()).unwrap();
        let result = finish(&worker, next).await;
        assert_eq!(result.state, "success");
        assert_eq!(result.report.unwrap().unchanged, 1);
        assert!(worker.queued().is_empty());
        assert_eq!(provider.counters.texts.load(Ordering::SeqCst), count);
        worker.shutdown().await;
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn changed_settings_discard_scan_and_do_not_queue_a_stale_update() {
        let (_home, root, worker, _) = setup().await;
        let gate = worker.gate(root.path()).unwrap().write_owned().await;
        let (id, _, _) = worker.scan_receipt(root.path()).unwrap();
        fs::write(project_config_path(root.path()), "docs_root = \"other\"\n").unwrap();
        drop(gate);
        assert_eq!(finish(&worker, id).await.state, "cancelled");
        assert!(worker.queued().is_empty());
        assert!(worker.doc_queue(root.path(), 200).unwrap().0.is_empty());
        worker.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn explicit_check_promotes_an_inflight_background_scan() {
        let (_home, root, worker, _) = setup().await;
        worker.pause();
        fs::write(root.path().join("docs/new.md"), "# Новое\n").unwrap();
        let gate = worker.gate(root.path()).unwrap().write_owned().await;
        let (id, _, _) = worker
            .scan_with_priority(root.path(), Priority::Periodic)
            .unwrap();
        let (same, merged, _) = worker.scan_receipt(root.path()).unwrap();
        assert!(merged);
        assert_eq!(same, id);
        worker
            .scan_with_priority(root.path(), Priority::Watch)
            .unwrap();
        drop(gate);
        let result = finish(&worker, id).await;
        assert_eq!(result.source, "explicit");
        assert_eq!(result.report.unwrap().added, 1);
        assert_eq!(worker.queued()[0].priority, Priority::Explicit);
        worker.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn scan_exposes_explicit_documents_while_indexing_is_paused() {
        let (_home, root, worker, provider) = setup().await;
        worker.pause();
        let project = worker.projects()[0].path.clone();
        let deaf = ProjectWatcher::start(
            &project,
            "docs",
            Duration::from_millis(100),
            Arc::new(|_| {}),
        )
        .unwrap();
        drop(
            lock(&worker.inner.projects)
                .get_mut(&project)
                .unwrap()
                .watcher
                .replace(deaf),
        );
        let before = provider.counters.embed_calls.load(Ordering::SeqCst);
        fs::write(root.path().join("docs/b.md"), "# Найден проверкой\n").unwrap();
        let (id, _, _) = worker.scan_receipt(&project).unwrap();
        assert_eq!(finish(&worker, id).await.state, "success");
        let (items, counts) = worker.doc_queue(&project, 200).unwrap();
        assert_eq!((counts.queued, counts.indexing), (1, 0));
        assert_eq!(
            (items[0].path.as_str(), items[0].source),
            ("docs/b.md", DocSource::Explicit)
        );
        assert_eq!(provider.counters.embed_calls.load(Ordering::SeqCst), before);
        worker.resume();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !worker.is_idle() {
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(worker.doc_queue(&project, 200).unwrap().0.is_empty());
        worker.shutdown().await;
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn model_mismatch_requires_consent_without_loading_a_model() {
        let (_home, root, worker, provider) = setup().await;
        provider.set_model("m2");
        let count = provider.counters.loads.load(Ordering::SeqCst);
        let (id, _, _) = worker.scan_receipt(root.path()).unwrap();
        assert_eq!(finish(&worker, id).await.state, "error");
        assert_eq!(
            worker.status(root.path()).unwrap().state,
            ProjectState::ReindexRequired
        );
        assert!(worker.queued().is_empty());
        assert_eq!(provider.counters.loads.load(Ordering::SeqCst), count);
        worker.shutdown().await;
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn deleting_a_project_cancels_scan_without_recreating_the_index() {
        let (_home, root, worker, _) = setup().await;
        let gate = worker.gate(root.path()).unwrap().write_owned().await;
        let (id, _, _) = worker.scan_receipt(root.path()).unwrap();
        let removing = worker.clone();
        let path = root.path().display().to_string();
        let deletion = tokio::spawn(async move { removing.delete_project(&path).await });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !lock(&worker.inner.scans)
            .values()
            .all(|job| job.cancel.load(Ordering::SeqCst))
        {
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        drop(gate);
        deletion.await.unwrap().unwrap();
        assert_eq!(finish(&worker, id).await.state, "cancelled");
        assert!(!index_path(root.path()).exists());
        worker.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn re_registration_does_not_merge_with_or_publish_an_old_scan() {
        let (_home, root, worker, _) = setup().await;
        worker.pause();
        let gate = worker.gate(root.path()).unwrap().write_owned().await;
        let (old, _, _) = worker.scan_receipt(root.path()).unwrap();
        worker
            .unregister(&root.path().display().to_string())
            .unwrap();
        worker.register(root.path()).unwrap();
        let (new, merged, _) = worker.scan_receipt(root.path()).unwrap();
        assert!(!merged);
        assert_ne!(new, old);
        let fresh = finish(&worker, new).await;
        assert_eq!(fresh.state, "success");
        drop(gate);
        let stale = finish(&worker, old).await;
        assert_eq!(stale.state, "cancelled");
        assert_ne!(fresh.generation, stale.generation);
        worker.shutdown().await;
    }
}
