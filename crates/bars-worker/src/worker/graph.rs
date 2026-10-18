use super::*;
use bars_core::graph::{GraphError, LayerState, store};
use bars_core::storage::{Index, Journal, index_path};

pub(super) struct GraphJob {
    pub id: String,
    pub cancel: Arc<AtomicBool>,
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
        let guard = self.request_guard().await;
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
        let id = format!("graph:{}", operations::generation());
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = lock(&self.inner.graph_jobs);
            if let Some(job) = jobs.get(project) {
                return Ok(bars_protocol::GraphAcceptedOutput {
                    accepted: true,
                    job_id: job.id.clone(),
                });
            }
            jobs.insert(
                project.into(),
                GraphJob {
                    id: id.clone(),
                    cancel: cancel.clone(),
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
            let _job = job_gate.lock_owned().await;
            let _gate = gate.read_owned().await;
            let result = tokio::task::spawn_blocking(move || -> Result<(), GraphError> {
                let mut index = Index::open(&index_path(&project), Journal::Wal)
                    .map_err(|e| GraphError::InvalidData(e.to_string()))?;
                store::ensure_schema(index.conn_mut())?;
                store::set_state(index.conn(), LayerState::Building, Some(&job_id), None)?;
                let stopped =
                    || cancel.load(Ordering::Acquire) || inner.stop.load(Ordering::Acquire);
                let result = bars_core::graph::build_index_graph(&project, &mut index, stopped);
                if let Err(error) = &result {
                    let _ = store::set_state(
                        index.conn(),
                        if matches!(error, GraphError::Cancelled) {
                            LayerState::Cancelled
                        } else {
                            LayerState::Failed
                        },
                        None,
                        Some(&error.to_string()),
                    );
                }
                result.map(|_| ())
            })
            .await;
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::warn!("Задание графа не завершено: {e}"),
                Err(e) => log::error!("Задание графа прервано: {e}"),
            }
        });
        lock(&self.inner.tasks).push(handle);
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
        Ok(true)
    }
    pub fn active_graph_job(&self, project: &Path) -> Option<String> {
        lock(&self.inner.graph_jobs)
            .get(project)
            .map(|j| j.id.clone())
    }
}
