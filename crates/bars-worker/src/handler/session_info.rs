use super::*;
use bars_protocol::{
    SessionInfoOutput, SessionInfoParams, SessionModuleInfo, SessionProjectInfo,
    SessionWorktreeInfo,
};

impl WorkerHandler {
    pub(super) async fn session_info(
        &self,
        params: SessionInfoParams,
    ) -> Result<SessionInfoOutput, RpcError> {
        let worker = self.worker.clone();
        let cwd = params.cwd.clone();
        tokio::task::spawn_blocking(move || {
            // Точечная проверка папок от `cwd` вверх: найденный подпроект регистрируется ленивым,
            // индекс не строится — первое обращение сделает первый вызов инструмента (Т-49).
            worker.register_for_cwd(Path::new(&cwd));
        })
        .await
        .map_err(internal)?;
        // Цель вызова: `cwd` сессии; явные `project` и `wt` нужны проверке аудитов с `wt`
        // (у них нет `Target`) и дают `worktree_not_found`/`project_not_found` как у остальных.
        let target = Target {
            cwd: params.cwd,
            project: params.project,
            wt: params.wt,
        };
        let explicit = target.project.is_some() || target.wt().is_some();
        let resolved = if explicit {
            Some(self.resolve_view(&target)?)
        } else {
            self.resolve_view(&target).ok()
        };
        let Some((path, view)) = resolved else {
            return Ok(SessionInfoOutput::default());
        };
        let Some((name, docs_root)) = self.worker.project_metadata(&path) else {
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
        // Сессия в worktree: имя проекта дополняется worktree, модули — владельца.
        let worktree = match view {
            ViewSelector::Worktree(item) => Some(SessionWorktreeInfo {
                name: item.name,
                branch: item.branch,
            }),
            ViewSelector::Main => None,
        };
        Ok(SessionInfoOutput {
            project: Some(SessionProjectInfo {
                name,
                modules,
                worktree,
            }),
        })
    }
}
