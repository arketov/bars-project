//! Слежение только за проектами с активным MCP или открытым окном (OQ-206).
use super::*;

pub(super) const MCP_LEASE: Duration = Duration::from_secs(3600);
pub(super) const WINDOW_LEASE: Duration = Duration::from_secs(30);

impl Runtime {
    pub(super) fn observed(&self, now: Instant) -> bool {
        self.mcp_until.is_some_and(|until| until > now)
            || self.windows.values().any(|until| *until > now)
    }
}

impl Worker {
    /// Агент работает с проектом: аренда слежения, инструкция и проверка. Для ленивого найденного
    /// подпроекта это первое обращение (Т-49).
    pub fn observe_mcp(&self, project: &Path) -> Result<(), WorkerError> {
        self.observe_mcp_with(project, false)
    }

    /// То же с признаком пассивного вызова (`projects`, `status` без refresh, списки аудитов и
    /// новых тем): ленивый проект такой вызов не продлевает и не будит, у остальных проектов
    /// поведение прежнее.
    pub fn observe_mcp_with(&self, project: &Path, passive: bool) -> Result<(), WorkerError> {
        let project = self.registered(project)?;
        if passive {
            if self.inner.is_lazy(&project) {
                return Ok(());
            }
        } else {
            // Первое обращение до аренды: слежение поднимается только у разбуженного проекта.
            self.inner.wake_lazy(&project);
        }
        let now = Instant::now();
        let first = {
            let mut projects = lock(&self.inner.projects);
            let runtime = projects
                .get_mut(&project)
                .ok_or_else(|| WorkerError::NotRegistered(project.clone()))?;
            let first = !runtime.mcp_until.is_some_and(|until| until > now);
            runtime.mcp_until = Some(now + MCP_LEASE);
            first
        };
        self.inner.ensure_watching(&project);
        if first {
            // Т-8: с проектом работает агент (хук сессии или первый вызов MCP) — кладём
            // `BARS_INSTRUCTION.md`. Окно и CLI этого пути не проходят. Сбой записи вызов не ломает.
            let _ = bars_core::instruction::place(&project, &self.inner.docs_root_of(&project));
            self.inner.activate(&project, Priority::Watch);
        }
        Ok(())
    }

    /// Каждое окно продлевает свою аренду; закрытие/смена проекта снимает только её. Открытие
    /// ленивого подпроекта в окне — первое обращение; первая аренда проекта окном ставит обход
    /// автопоиска (Т-49).
    pub fn observe_window(&self, window: &str, project: Option<&Path>) -> Result<(), WorkerError> {
        let project = project.map(|p| self.registered(p)).transpose()?;
        if let Some(project) = &project {
            self.inner.wake_lazy(project);
        }
        let now = Instant::now();
        let mut first = false;
        {
            let mut projects = lock(&self.inner.projects);
            for (path, runtime) in projects.iter_mut() {
                if Some(path) == project.as_ref() {
                    first = !runtime
                        .windows
                        .get(window)
                        .is_some_and(|until| *until > now);
                    runtime
                        .windows
                        .insert(window.to_owned(), now + WINDOW_LEASE);
                } else {
                    runtime.windows.remove(window);
                }
            }
        }
        self.inner.expire_observers(now);
        if let Some(project) = project {
            self.inner.ensure_watching(&project);
            if first {
                self.inner.activate(&project, Priority::Watch);
                self.inner.schedule_discovery(&project, true);
                // Открытие проекта в окне: worktree владельца и вложенных проектов.
                self.rediscover_worktrees(&project);
            }
        }
        Ok(())
    }
}

impl Inner {
    pub(super) fn expire_observers(&self, now: Instant) {
        let stopped: Vec<_> = {
            let mut projects = lock(&self.projects);
            projects
                .values_mut()
                .filter_map(|runtime| {
                    if runtime.mcp_until.is_some_and(|until| until <= now) {
                        runtime.mcp_until = None;
                    }
                    runtime.windows.retain(|_, until| *until > now);
                    if runtime.observed(now) {
                        return None;
                    }
                    runtime.tail_armed = false;
                    runtime.watcher.take()
                })
                .collect()
        };
        // notify может ждать projects; остановка всегда вне блокировки.
        drop(stopped);
        self.expire_wt_observers(now);
    }

    pub(super) async fn run_activity_timer(self: Arc<Self>) {
        let mut timer = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = timer.tick() => {},
                () = self.stop_wake.notified() => break,
            }
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            self.expire_observers(Instant::now());
            self.updates.retain(&lock(&self.registry).paths());
        }
    }
}
