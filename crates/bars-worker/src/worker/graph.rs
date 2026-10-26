use super::*;
use bars_core::graph::{BuildLayer, GraphError, LayerState, store};
use bars_core::storage::{Compatibility, Index, Journal, index_path};

mod inputs;

pub(super) struct GraphJob {
    pub id: String,
    pub cancel: Arc<AtomicBool>,
    pub layer: BuildLayer,
    pending: bool,
    resume: bool,
    target: String,
    wake: Arc<Notify>,
    pub(super) waiting_retry: bool,
    #[cfg(test)]
    pub(super) finish_hook: Option<(Arc<Notify>, Arc<Notify>)>,
}
impl GraphJob {
    pub(super) fn idle(&self) -> bool {
        self.waiting_retry && !self.pending && !self.cancel.load(Ordering::Acquire)
    }
}
struct Cleanup {
    inner: Arc<Inner>,
    project: PathBuf,
    id: String,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        let mut jobs = lock(&self.inner.graph_jobs);
        if jobs.get(&self.project).is_some_and(|j| j.id == self.id) {
            jobs.remove(&self.project);
        }
    }
}
impl Worker {
    pub async fn graph_build(
        &self,
        project: &Path,
    ) -> Result<bars_protocol::GraphAcceptedOutput, GraphError> {
        self.graph_build_layer(project, BuildLayer::All).await
    }
    pub async fn graph_build_layer(
        &self,
        project: &Path,
        layer: BuildLayer,
    ) -> Result<bars_protocol::GraphAcceptedOutput, GraphError> {
        self.graph_start_layer(project, true, true, layer).await
    }
    pub(super) async fn graph_start(
        &self,
        project: &Path,
        explicit: bool,
    ) -> Result<bars_protocol::GraphAcceptedOutput, GraphError> {
        self.graph_start_layer(project, explicit, explicit, BuildLayer::All)
            .await
    }
    pub(super) async fn graph_checked(
        &self,
        project: &Path,
    ) -> Result<bars_protocol::GraphAcceptedOutput, GraphError> {
        self.graph_start_layer(project, false, true, BuildLayer::All)
            .await
    }
    async fn graph_start_layer(
        &self,
        project: &Path,
        explicit: bool,
        resume: bool,
        layer: BuildLayer,
    ) -> Result<bars_protocol::GraphAcceptedOutput, GraphError> {
        let guard = if explicit {
            Some(self.request_guard().await)
        } else {
            None
        };
        if !index_path(project).exists() {
            return Err(GraphError::NotReady("индекс ещё не построен".into()));
        }
        let gate = self
            .gate(project)
            .map_err(|e| GraphError::InvalidParams(e.to_string()))?;
        let job_gate = {
            let projects = lock(&self.inner.projects);
            let runtime = projects
                .get(project)
                .filter(|r| !r.deleting)
                .ok_or_else(|| {
                    GraphError::InvalidParams("проект удаляется или не зарегистрирован".into())
                })?;
            runtime.job_gate.clone()
        };
        // Цель остановки фиксируется до ожидания очереди: новая правка не подавляется.
        let root = project.to_path_buf();
        let target = tokio::task::spawn_blocking(move || {
            let index = Index::open_read_only(&index_path(&root))
                .map_err(|e| GraphError::InvalidData(e.to_string()))?;
            inputs::key(&root, &index)
        })
        .await
        .map_err(|e| GraphError::InvalidData(e.to_string()))??;
        let id = format!("graph:{}", operations::generation());
        let wake = Arc::new(Notify::new());
        {
            let mut jobs = lock(&self.inner.graph_jobs);
            if let Some(job) = jobs.get_mut(project) {
                if explicit && job.layer != layer {
                    return Err(GraphError::InvalidParams("у проекта уже выполняется задание другого слоя; дождитесь его или остановите".into()));
                }
                if !explicit || job.waiting_retry {
                    job.pending = true;
                    job.resume |= resume;
                    job.wake.notify_one();
                }
                return Ok(bars_protocol::GraphAcceptedOutput {
                    accepted: true,
                    job_id: job.id.clone(),
                });
            }
            jobs.insert(
                project.into(),
                GraphJob {
                    id: id.clone(),
                    cancel: Arc::new(AtomicBool::new(false)),
                    layer,
                    pending: false,
                    resume,
                    target,
                    wake: wake.clone(),
                    waiting_retry: false,
                    #[cfg(test)]
                    finish_hook: None,
                },
            );
        }
        let inner = self.inner.clone();
        let project = project.to_path_buf();
        let job_id = id.clone();
        let handle = self.inner.handle.spawn(async move {
            let _request = guard;
            let _cleanup = Cleanup {
                inner: inner.clone(),
                project: project.clone(),
                id: job_id.clone(),
            };
            let mut force = explicit;
            let mut layer = layer;
            let mut build_state = bars_core::graph::GraphBuildState::default();
            loop {
                let job_guard = job_gate.clone().lock_owned().await;
                if inner.stop.load(Ordering::Acquire)
                    || !lock(&inner.projects)
                        .get(&project)
                        .is_some_and(|r| !r.deleting && Arc::ptr_eq(&r.job_gate, &job_gate))
                {
                    return;
                }
                let read_guard = gate.clone().read_owned().await;
                let (cancel, resume, target) = {
                    let mut jobs = lock(&inner.graph_jobs);
                    let Some(job) = jobs.get_mut(&project) else {
                        return;
                    };
                    if !job.cancel.load(Ordering::Acquire) {
                        job.pending = false;
                    }
                    job.waiting_retry = false;
                    (
                        job.cancel.clone(),
                        std::mem::take(&mut job.resume),
                        job.target.clone(),
                    )
                };
                let worker = inner.clone();
                let root = project.clone();
                let name = job_id.clone();
                let validation_gate = gate.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let result = Attempt {
                        inner: &worker,
                        project: &root,
                        id: &name,
                        layer,
                        force,
                        resume,
                        target: &target,
                        cancel: &cancel,
                        build_state: &mut build_state,
                        project_gate: &validation_gate,
                    }
                    .run();
                    (result, build_state)
                })
                .await;
                let result = match result {
                    Ok((result, state)) => {
                        build_state = state;
                        Ok(result)
                    }
                    Err(error) => {
                        build_state = Default::default();
                        Err(error)
                    }
                };
                #[cfg(test)]
                {
                    let hook = lock(&inner.graph_jobs)
                        .get_mut(&project)
                        .and_then(|j| j.finish_hook.take());
                    if let Some((reached, release)) = hook {
                        reached.notify_one();
                        release.notified().await;
                    }
                }
                drop(read_guard);
                drop(job_guard);
                force = false;
                let (retry, stopped, continuing) = match result {
                    Ok(Ok(AttemptResult::Done)) => (None, false, false),
                    Ok(Ok(AttemptResult::Stopped)) => (None, true, false),
                    Ok(Ok(AttemptResult::Continue)) => {
                        (Some(Duration::from_millis(1)), false, true)
                    }
                    Ok(Ok(AttemptResult::Retry(delay))) => (Some(delay), false, false),
                    Ok(Err(e)) => {
                        log::warn!("Задание графа не завершено: {e}");
                        (None, true, false)
                    }
                    Err(e) => {
                        log::error!("Задание графа прервано: {e}");
                        (None, true, false)
                    }
                };
                // Снятие задания и проверка сигнала — одна критическая секция.
                {
                    let mut jobs = lock(&inner.graph_jobs);
                    let Some(job) = jobs.get_mut(&project) else {
                        return;
                    };
                    if job.cancel.load(Ordering::Acquire) && !stopped {
                        // Отмена могла прийти уже после возврата вычисления.
                        job.pending = true;
                    } else {
                        job.cancel = Arc::new(AtomicBool::new(false));
                    }
                    if job.pending {
                        layer = BuildLayer::All;
                        job.layer = layer;
                        continue;
                    }
                    if retry.is_none() {
                        jobs.remove(&project);
                        return;
                    }
                    job.waiting_retry = !continuing;
                }
                let until = tokio::time::Instant::now() + retry.unwrap();
                loop {
                    if inner.stop.load(Ordering::Acquire) {
                        return;
                    }
                    let changed = lock(&inner.graph_jobs)
                        .get(&project)
                        .is_none_or(|j| j.pending || j.cancel.load(Ordering::Acquire));
                    if changed || tokio::time::Instant::now() >= until {
                        break;
                    }
                    tokio::select! {
                        _ = wake.notified() => {},
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {},
                    }
                }
            }
        });
        let mut tasks = lock(&self.inner.tasks);
        tasks.retain(|task| !task.is_finished());
        tasks.push(handle);
        Ok(bars_protocol::GraphAcceptedOutput {
            accepted: true,
            job_id: id,
        })
    }
    pub fn graph_cancel(&self, project: &Path, job_id: &str) -> Result<bool, GraphError> {
        let jobs = lock(&self.inner.graph_jobs);
        let Some(job) = jobs.get(project) else {
            return Ok(false);
        };
        if job.id != job_id {
            return Err(GraphError::InvalidParams(
                "job_id не соответствует активному заданию проекта".into(),
            ));
        }
        job.cancel.store(true, Ordering::Release);
        job.wake.notify_one();
        Ok(true)
    }
    pub fn active_graph_job(&self, project: &Path) -> Option<String> {
        lock(&self.inner.graph_jobs)
            .get(project)
            .map(|j| j.id.clone())
    }
}
struct Attempt<'a> {
    inner: &'a Inner,
    project: &'a Path,
    id: &'a str,
    layer: BuildLayer,
    force: bool,
    resume: bool,
    target: &'a str,
    cancel: &'a AtomicBool,
    build_state: &'a mut bars_core::graph::GraphBuildState,
    project_gate: &'a Arc<RwLock<()>>,
}
enum AttemptResult {
    Done,
    Stopped,
    Continue,
    Retry(Duration),
}
impl Attempt<'_> {
    fn run(self) -> Result<AttemptResult, GraphError> {
        let Self {
            inner,
            project,
            id,
            layer,
            force,
            resume,
            target,
            cancel,
            build_state,
            project_gate,
        } = self;
        let settings = Settings::load(&inner.config.bars_home, project)
            .map_err(|e| GraphError::InvalidData(e.to_string()))?
            .value;
        let fingerprint = inner
            .models
            .fingerprint(&settings)
            .map_err(|e| GraphError::InvalidData(e.to_string()))?;
        let read = Index::open_read_only(&index_path(project))
            .map_err(|e| GraphError::InvalidData(e.to_string()))?;
        if matches!(
            read.compatibility(&fingerprint)
                .map_err(|e| GraphError::InvalidData(e.to_string()))?,
            Compatibility::ModelChanged | Compatibility::SchemaOutdated
        ) {
            return Err(GraphError::NotReady(
                "граф ждёт переиндексации с согласием пользователя".into(),
            ));
        }
        drop(read);
        let mut index = Index::open(&index_path(project), Journal::Wal)
            .map_err(|e| GraphError::InvalidData(e.to_string()))?;
        store::ensure_schema(index.conn_mut())?;
        let key = inputs::key(project, &index)?;
        if cancel.load(Ordering::Acquire) {
            inputs::put(&index, "auto_stopped", target)?;
            store::set_paused(index.conn(), true)?;
            store::set_state(index.conn(), LayerState::Cancelled, None, None)?;
            return Ok(AttemptResult::Stopped);
        }
        let status = store::status(index.conn())?;
        // Старый paused мигрирует в остановку именно текущих входов.
        if status.paused && inputs::get(&index, "auto_stopped")?.is_none() {
            inputs::put(&index, "auto_stopped", &key)?;
        }
        if !resume && inputs::get(&index, "auto_stopped")?.as_deref() == Some(&key) {
            return Ok(AttemptResult::Done);
        }
        store::set_paused(index.conn(), false)?;
        inputs::put(&index, "auto_stopped", "")?;
        if !force
            && status.job_state == bars_core::graph::JobState::Idle
            && status.layers.len() == 3
            && status
                .layers
                .values()
                .all(|l| matches!(l.state, LayerState::Ready | LayerState::Partial))
            && matches!(status.state, LayerState::Ready | LayerState::Partial)
            && inputs::get(&index, "auto_completed")?.as_deref() == Some(&key)
            && inner
                .graph_cache
                .load(
                    project,
                    project_gate,
                    std::time::Instant::now() + Duration::from_secs(5),
                )
                .is_ok()
        {
            return Ok(AttemptResult::Done);
        }
        let mut retry = inputs::retry(&index, &key)?;
        if resume {
            retry = Default::default();
        }
        if let Some(delay) = retry.remaining() {
            return Ok(AttemptResult::Retry(delay));
        }
        {
            let mut jobs = lock(&inner.graph_jobs);
            if let Some(job) = jobs.get_mut(project) {
                job.target = key.clone();
            }
        }
        store::set_state(index.conn(), LayerState::Building, Some(id), None)?;
        let result = bars_core::graph::build_index_graph_step_with_state(
            project,
            &mut index,
            layer,
            build_state,
            || cancel.load(Ordering::Acquire) || inner.stop.load(Ordering::Acquire),
        );
        let stopped = cancel.load(Ordering::Acquire);
        if stopped {
            let stopped_target = if result.is_ok() {
                inputs::key(project, &index)?
            } else {
                key.clone()
            };
            inputs::put(&index, "auto_stopped", &stopped_target)?;
            store::set_paused(index.conn(), true)?;
            store::set_state(index.conn(), LayerState::Cancelled, None, None)?;
        }
        match result {
            Ok(None) => Ok(if stopped {
                AttemptResult::Stopped
            } else {
                AttemptResult::Continue
            }),
            Ok(Some(_)) => {
                let completed = inputs::key(project, &index)?;
                if let Some(job) = lock(&inner.graph_jobs).get_mut(project) {
                    job.target = completed.clone();
                }
                if completed == key {
                    inputs::put(&index, "auto_completed", &completed)?;
                } else if !stopped && let Some(job) = lock(&inner.graph_jobs).get_mut(project) {
                    // Состав ошибочных файлов тоже мог измениться во время расчёта.
                    job.pending = true;
                }
                inputs::put(&index, "auto_retry", "")?;
                // Прогрев опубликованного снимка отдельно от бюджета первого читателя.
                if !stopped
                    && completed == key
                    && let Err(error) = inner.graph_cache.load(
                        project,
                        project_gate,
                        std::time::Instant::now() + Duration::from_secs(5),
                    )
                {
                    log::debug!("Снимок графа не удержан в кэше: {error}");
                }
                Ok(if stopped {
                    AttemptResult::Stopped
                } else {
                    AttemptResult::Done
                })
            }
            Err(GraphError::Cancelled) => Ok(if stopped {
                AttemptResult::Stopped
            } else {
                AttemptResult::Done
            }),
            Err(error) => {
                log::warn!("Задание графа не завершено: {error}");
                if matches!(error, GraphError::StaleRef(_)) && inputs::key(project, &index)? != key
                {
                    if let Some(job) = lock(&inner.graph_jobs).get_mut(project) {
                        job.pending = true;
                    }
                    return Ok(AttemptResult::Done);
                }
                let delay = retry.failed();
                inputs::save_retry(&index, &key, &retry)?;
                Ok(if stopped {
                    AttemptResult::Stopped
                } else {
                    AttemptResult::Retry(delay)
                })
            }
        }
    }
}
