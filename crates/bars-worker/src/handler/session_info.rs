use super::*;
use bars_protocol::{SessionInfoOutput, SessionInfoParams, SessionModuleInfo, SessionProjectInfo};

impl WorkerHandler {
    pub(super) async fn session_info(
        &self,
        params: SessionInfoParams,
    ) -> Result<SessionInfoOutput, RpcError> {
        let worker = self.worker.clone();
        let metadata = tokio::task::spawn_blocking(move || {
            let path = worker.project_for_cwd(Path::new(&params.cwd))?;
            let (name, docs_root) = worker.project_metadata(&path)?;
            Some((path, name, docs_root))
        })
        .await
        .map_err(internal)?;
        let Some((path, name, docs_root)) = metadata else {
            return Ok(SessionInfoOutput::default());
        };
        let gate = self
            .worker
            .gate(&path)
            .map_err(worker_error)?
            .read_owned()
            .await;
        let modules = tokio::task::spawn_blocking(move || -> Result<_, RpcError> {
            let _gate = gate;
            if !index_path(&path).exists() {
                return Ok(Vec::new());
            }
            let index = Index::open_read_only(&index_path(&path)).map_err(internal)?;
            let docs_root = docs_root.replace('\\', "/");
            let docs_root = docs_root.trim_end_matches('/');
            let prefix = if docs_root == "." || docs_root.is_empty() {
                String::new()
            } else {
                format!("{docs_root}/")
            };
            // Один проход только по метаданным; вложенный Index не задаёт назначение модуля.
            let mut statement = index
                .conn()
                .prepare(
                    "SELECT module, count(*), MAX(CASE WHEN lower(path) = lower(?1 || \
                 CASE WHEN module = '.' THEN '' ELSE module || '/' END || 'index.md') \
                 THEN title END) FROM documents GROUP BY module ORDER BY module",
                )
                .map_err(internal)?;
            let rows = statement
                .query_map([prefix], |row| {
                    Ok(SessionModuleInfo {
                        name: row.get(0)?,
                        documents: row.get(1)?,
                        title: row.get(2)?,
                    })
                })
                .map_err(internal)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(internal)
        })
        .await
        .map_err(internal)??;
        Ok(SessionInfoOutput {
            project: Some(SessionProjectInfo { name, modules }),
        })
    }
}
