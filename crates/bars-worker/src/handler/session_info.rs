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
        let modules = self
            .modules(&path, &docs_root)
            .await?
            .into_iter()
            .map(|module| SessionModuleInfo {
                name: module.name,
                documents: module.documents,
                title: module.title,
            })
            .collect();
        Ok(SessionInfoOutput {
            project: Some(SessionProjectInfo { name, modules }),
        })
    }
}
