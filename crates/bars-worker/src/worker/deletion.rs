//! Полное удаление по подтверждению окна (`features/projects/deletion.md`).

use super::*;

impl Worker {
    pub(crate) async fn request_guard(&self) -> tokio::sync::OwnedRwLockReadGuard<()> {
        Arc::clone(&self.inner.requests).read_owned().await
    }

    /// Начатая очистка завершается даже при отключении вызывающего окна.
    pub async fn delete_project(&self, selector: &str) -> Result<PathBuf, WorkerError> {
        let worker = self.clone();
        let selector = selector.to_string();
        tokio::spawn(async move {
            let _requests = Arc::clone(&worker.inner.requests).write_owned().await;
            worker.delete_project_inner(&selector).await
        })
        .await
        .map_err(|error| io::Error::other(error.to_string()))?
    }

    async fn delete_project_inner(&self, selector: &str) -> Result<PathBuf, WorkerError> {
        let path = self.resolve(selector)?;
        // Runtime остаётся до конца: таймер, события и очередь видят запрет новых заданий.
        let (job_gate, gate, watcher) = {
            let mut projects = lock(&self.inner.projects);
            let runtime = projects
                .get_mut(&path)
                .ok_or_else(|| WorkerError::NotRegistered(path.clone()))?;
            runtime.deleting = true;
            let watcher = runtime.watcher.take();
            (
                Arc::clone(&runtime.job_gate),
                Arc::clone(&runtime.gate),
                watcher,
            )
        };
        self.inner.cancel_queued(&path);
        self.inner.cancel_scan(&path);
        if let Some(cell) = lock(&self.inner.current)
            .as_ref()
            .filter(|cell| cell.project == path)
        {
            cell.cancel();
        }
        // notify может ждать projects; остановка вне мьютексов и вне async executor.
        let _ = tokio::task::spawn_blocking(move || drop(watcher)).await;
        let _job = job_gate.lock_owned().await;
        let _readers = gate.write_owned().await;
        self.inner.graph_cache.remove(&path);
        let project = path.clone();
        let home = self.bars_home().to_path_buf();
        let result = tokio::task::spawn_blocking(move || remove_working_files(&project, &home))
            .await
            .map_err(|error| io::Error::other(error.to_string()))
            .and_then(|result| result)
            .map_err(WorkerError::from)
            .and_then(|()| {
                lock(&self.inner.registry)
                    .remove(&path)
                    .map_err(WorkerError::from)
            });
        match result {
            Ok(_) => {
                lock(&self.inner.projects).remove(&path);
                self.inner.cancel_queued(&path);
                Ok(path)
            }
            Err(error) => {
                if let Some(runtime) = lock(&self.inner.projects).get_mut(&path) {
                    runtime.deleting = false;
                    runtime.status.state = if path.is_dir() {
                        ProjectState::Ready
                    } else {
                        ProjectState::Unavailable
                    };
                    runtime.status.last_error = Some(format!("удаление не завершено: {error}"));
                }
                Err(error)
            }
        }
    }
}

fn redirected(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT, включая junction.
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn remove_working_files(project: &Path, home: &Path) -> io::Result<()> {
    let directory = project.join(".bars");
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.is_dir() || redirected(&metadata) {
        return Err(io::Error::other(
            "папка .bars перенаправлена или не является папкой; удаление отменено",
        ));
    }
    let target = fs::canonicalize(&directory)?;
    if fs::canonicalize(home).is_ok_and(|home| home == target) {
        return Err(io::Error::other(
            "общую папку .bars пользователя удалять нельзя",
        ));
    }
    if target.parent() != Some(fs::canonicalize(project)?.as_path()) {
        return Err(io::Error::other("папка .bars находится вне проекта"));
    }
    // Только принадлежащие bars файлы: пользовательская документация даже внутри .bars цела.
    let mut files = vec![
        directory.join("config.toml"),
        directory.join("config.toml.bars-tmp"),
    ];
    // Рабочая БД «Новых тем» и её sidecar-файлы (NT-09).
    for sidecar in ["", "-wal", "-shm", "-journal"] {
        files.push(directory.join(format!("new-topics.sqlite3{sidecar}")));
        // Результаты аудитов знаний: производные от текста, пересоздаются запуском.
        files.push(directory.join(format!("audits.sqlite3{sidecar}")));
    }
    for suffix in ["", ".new", ".old"] {
        for sidecar in ["", "-wal", "-shm", "-journal"] {
            files.push(directory.join(format!("index.sqlite{suffix}{sidecar}")));
        }
    }
    // Проверяем все пути до первой записи; под известным именем может лежать папка пользователя.
    for file in &files {
        match fs::symlink_metadata(file) {
            Ok(metadata) if !metadata.is_file() || redirected(&metadata) => {
                return Err(io::Error::other(format!(
                    "небезопасный рабочий файл: {}",
                    file.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    for file in files {
        match fs::remove_file(file) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    if fs::read_dir(&directory)?.next().is_none() {
        fs::remove_dir(&directory)?;
    }
    Ok(())
}
