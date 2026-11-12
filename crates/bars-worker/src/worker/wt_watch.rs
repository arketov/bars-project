//! Актуальность дельты worktree: слежение за корнем вида W, правки из окна и изменения владельца
//! (`docs/features/projects/worktrees.md`, разделы «Что ставит пересверку затронутых путей» и
//! «Жизненный цикл»; Т-10, Т-41, Т-42, Т-48).
//!
//! Слежение за W включается и выключается по тем же правилам Т-10, что у проекта: пока открыто окно
//! на этом worktree (аренда 30 секунд на окно) или идёт час после последнего MCP-вызова по нему.
//! Событие внутри W при состоянии дельты «нет» отбрасывается. Правка ждёт паузы Т-41 в общей
//! очереди документов (ключ — [`DocTarget::Worktree`]), потом уходит в очередь дельт как сверка
//! путей. Завершение задания владельца ([`Inner::owner_changed`]) ставит сверку тех же путей каждой
//! живой дельте владельца.

use std::collections::BTreeSet;

use bars_core::indexing::nested;

use super::activity::{MCP_LEASE, WINDOW_LEASE};
use super::delta::DeltaKey;
use super::doc_queue::DocTarget;
use super::queue::Priority;
use super::*;

/// Слежение и аренды одного worktree. Запись живёт, пока есть аренда или слежение.
#[derive(Default)]
pub(super) struct WtWatch {
    mcp_until: Option<Instant>,
    windows: HashMap<String, Instant>,
    watcher: Option<ProjectWatcher>,
    /// Слежение сообщило об ошибке: поднимается заново при следующем обращении.
    failed: bool,
}

impl WtWatch {
    #[cfg(test)]
    pub(super) fn has_watcher(&self) -> bool {
        self.watcher.is_some()
    }

    fn observed(&self, now: Instant) -> bool {
        self.mcp_until.is_some_and(|until| until > now)
            || self.windows.values().any(|until| *until > now)
    }
}

/// Слежение всех worktree: ключ — владелец и имя, как у дельты.
pub(super) type WtWatches = HashMap<DeltaKey, WtWatch>;

impl Inner {
    /// Есть ли аренда у worktree: окно открыто на нём или идёт час после MCP-вызова.
    pub(super) fn wt_observed(&self, key: &DeltaKey, now: Instant) -> bool {
        lock(&self.wt_watch)
            .get(key)
            .is_some_and(|watch| watch.observed(now))
    }

    /// Есть ли у worktree состояние дельты (не «нет»).
    fn delta_exists(&self, key: &DeltaKey) -> bool {
        lock(&self.deltas).contains(key)
    }

    /// Поднимает слежение за корнем вида, если у worktree есть дельта и аренда, а слежения нет или
    /// оно сломалось. Дельты нет — события отбрасывались бы, слежение не нужно.
    pub(super) fn ensure_wt_watching(self: &Arc<Self>, key: &DeltaKey) {
        if !self.delta_exists(key) {
            return;
        }
        {
            let watches = lock(&self.wt_watch);
            let needed = watches.get(key).is_some_and(|watch| {
                watch.observed(Instant::now()) && (watch.watcher.is_none() || watch.failed)
            });
            if !needed {
                return;
            }
        }
        let Some(linked) = self
            .linked_of(&key.0)
            .into_iter()
            .find(|item| item.name == key.1)
        else {
            return;
        };
        let docs_root = self.docs_root_of(&key.0);
        // Без папки корня документации в виде следить не за чем; следующая аренда попробует снова.
        if !linked.view_root.join(&docs_root).is_dir() {
            return;
        }
        let weak: Weak<Inner> = Arc::downgrade(self);
        let sink_key = key.clone();
        let sink: Sink = Arc::new(move |event| {
            if let Some(inner) = weak.upgrade() {
                inner.on_wt_event(&sink_key, event);
            }
        });
        let mut started =
            ProjectWatcher::start(&linked.view_root, &docs_root, self.config.debounce, sink).ok();
        if started.is_none() {
            log::warn!(
                "Не удалось запустить слежение за worktree «{}» проекта {}",
                key.1,
                key.0.display()
            );
        }
        let replaced = {
            let mut watches = lock(&self.wt_watch);
            watches
                .get_mut(key)
                .filter(|watch| watch.observed(Instant::now()))
                .and_then(|watch| {
                    watch.failed = started.is_none();
                    std::mem::replace(&mut watch.watcher, started.take())
                })
        };
        // Остановка слежения вне блокировки: его поток может ждать её.
        drop(replaced);
        drop(started);
    }

    /// Вызывается в потоке `notify`: останавливать слежение отсюда нельзя.
    fn on_wt_event(self: &Arc<Self>, key: &DeltaKey, event: WatchEvent) {
        if self.stop.load(Ordering::SeqCst) || !self.wt_observed(key, Instant::now()) {
            return;
        }
        // Состояние «нет»: события отбрасываются, дельту создаёт только обращение.
        if !self.delta_exists(key) {
            return;
        }
        match event {
            WatchEvent::Tree => {}
            WatchEvent::Files(files) => self.touch_worktree_files(key, files, DocSource::Watch),
            WatchEvent::Failed(message) => {
                log::warn!(
                    "Слежение за worktree «{}» проекта {} сообщило об ошибке: {message}",
                    key.1,
                    key.0.display()
                );
                // События могли потеряться: сверка всех документов вида. Пометка «сломано» —
                // после постановки: `request_delta` перезапустил бы слежение из его же потока.
                self.request_delta(&key.0, &key.1, Priority::Watch, None);
                if let Some(watch) = lock(&self.wt_watch).get_mut(key) {
                    watch.failed = true;
                }
            }
            WatchEvent::Rescan => {
                self.request_delta(&key.0, &key.1, Priority::Watch, None);
            }
        }
    }

    /// Файлы вида изменились (слежение — `Watch`, окно — `Edit`). Существующие `.md` ждут паузы
    /// Т-41 в очереди документов с ключом `(владелец, имя, путь)`; пропавшие документы пауза не
    /// держит; изменение папки неизвестного содержимого — сверка всего вида. Дельты нет — ничего.
    pub(super) fn touch_worktree_files(
        self: &Arc<Self>,
        key: &DeltaKey,
        files: Vec<PathBuf>,
        source: DocSource,
    ) {
        if !self.delta_exists(key) {
            return;
        }
        let Some(linked) = self
            .linked_of(&key.0)
            .into_iter()
            .find(|item| item.name == key.1)
        else {
            return;
        };
        let excluded = nested::excluded(&key.0);
        let pause = self.edit_pause(&key.0);
        let target = DocTarget::worktree(&key.0, &key.1);
        let now = Instant::now();
        let mut whole = false;
        let mut touched = false;
        {
            let mut docs = lock(&self.docs);
            for file in files {
                let Some(relative) = doc_key(&linked.view_root, &file) else {
                    continue;
                };
                // Область вложенного проекта принадлежит ему, как у слежения за проектом.
                if nested::is_excluded(&relative, &excluded) {
                    continue;
                }
                if file.is_file() {
                    docs.touch(&target, &relative, source, pause, now);
                    touched = true;
                } else if Path::new(&relative)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
                {
                    // Удалённый документ: паузы не нужно, сверка уберёт его из дельты.
                    docs.touch(&target, &relative, source, Duration::ZERO, now);
                    touched = true;
                } else {
                    whole = true;
                }
            }
        }
        if whole {
            self.request_delta(&key.0, &key.1, source.priority(), None);
        }
        if touched {
            self.submit_worktree_docs(&key.0, &key.1);
            self.doc_wake.notify_one();
        }
    }

    /// Готовые документы worktree уходят из очереди документов в очередь дельт одной пачкой.
    pub(super) fn submit_worktree_docs(self: &Arc<Self>, owner: &Path, name: &str) {
        let target = DocTarget::worktree(owner, name);
        let Some(ready) = lock(&self.docs).take_ready(&target) else {
            return;
        };
        // Дельту могли удалить, пока документ ждал паузы: тогда сверять нечего.
        if !self.delta_exists(&(owner.to_path_buf(), name.to_string())) {
            return;
        }
        self.request_delta(owner, name, ready.source.priority(), Some(ready.paths));
    }

    /// Завершилось задание индексации владельца: `paths` — документы, которые оно записало в
    /// индекс или удалило из него (`None` — все, после полной переиндексации). Каждой живой
    /// дельте владельца ставится сверка этих путей: `Watch`, если дельтой пользуются сейчас (окно
    /// открыто или аренда MCP), иначе `Periodic`.
    pub(super) fn owner_changed(self: &Arc<Self>, owner: &Path, paths: Option<BTreeSet<String>>) {
        if paths.as_ref().is_some_and(BTreeSet::is_empty) {
            return;
        }
        let keys: Vec<DeltaKey> = lock(&self.deltas)
            .keys()
            .filter(|(of, _)| of == owner)
            .cloned()
            .collect();
        let now = Instant::now();
        for key in keys {
            let priority = if self.wt_observed(&key, now) {
                Priority::Watch
            } else {
                Priority::Periodic
            };
            self.request_delta(owner, &key.1, priority, paths.clone());
        }
    }

    /// Останавливает слежение за W, аренды остаются: вызывается, когда дельты больше нет.
    pub(super) fn drop_wt_watcher(&self, key: &DeltaKey) {
        let watcher = lock(&self.wt_watch)
            .get_mut(key)
            .and_then(|watch| watch.watcher.take());
        if let Some(watcher) = watcher {
            std::thread::spawn(move || drop(watcher));
        }
    }

    /// Отмена проекта: слежение, аренды и записи очереди документов всех его worktree снимаются.
    pub(super) fn cancel_wt_watching(&self, owner: &Path) {
        let removed: Vec<WtWatch> = {
            let mut watches = lock(&self.wt_watch);
            let keys: Vec<DeltaKey> = watches
                .keys()
                .filter(|(of, _)| of == owner)
                .cloned()
                .collect();
            keys.iter().filter_map(|key| watches.remove(key)).collect()
        };
        lock(&self.docs).remove_worktrees(owner);
        // notify может ждать блокировки: остановка вне неё и вне потока вызова.
        std::thread::spawn(move || drop(removed));
    }

    /// Аренды истекли: слежение за W без аренды останавливается (Т-10).
    pub(super) fn expire_wt_observers(&self, now: Instant) {
        let stopped: Vec<ProjectWatcher> = {
            let mut watches = lock(&self.wt_watch);
            let mut stopped = Vec::new();
            watches.retain(|_, watch| {
                if watch.mcp_until.is_some_and(|until| until <= now) {
                    watch.mcp_until = None;
                }
                watch.windows.retain(|_, until| *until > now);
                if watch.observed(now) {
                    return true;
                }
                stopped.extend(watch.watcher.take());
                false
            });
            stopped
        };
        drop(stopped);
    }
}

impl Worker {
    /// Канонический ключ дельты по владельцу и имени worktree; `None` — такого worktree нет.
    fn worktree_key(&self, project: &Path, name: &str) -> Option<DeltaKey> {
        let project = self.registered(project).ok()?;
        self.inner
            .linked_of(&project)
            .into_iter()
            .find(|item| item.name == name || item.name.eq_ignore_ascii_case(name))
            .map(|item| (project, item.name))
    }

    /// Агент работает с worktree: аренда слежения на час (Т-10). Дельты может ещё не быть: слежение
    /// поднимется, когда она появится.
    pub fn observe_worktree_mcp(&self, project: &Path, name: &str) {
        let Some(key) = self.worktree_key(project, name) else {
            return;
        };
        {
            let mut watches = lock(&self.inner.wt_watch);
            watches.entry(key.clone()).or_default().mcp_until = Some(Instant::now() + MCP_LEASE);
        }
        self.inner.ensure_wt_watching(&key);
    }

    /// Окно показывает worktree: каждое окно продлевает свою аренду на 30 секунд, смена или
    /// закрытие снимает только её. `None` — окно ушло с worktree.
    pub fn observe_worktree_window(&self, window: &str, target: Option<(&Path, &str)>) {
        let key = target.and_then(|(project, name)| self.worktree_key(project, name));
        let now = Instant::now();
        {
            let mut watches = lock(&self.inner.wt_watch);
            for (watched, watch) in watches.iter_mut() {
                if Some(watched) != key.as_ref() {
                    watch.windows.remove(window);
                }
            }
            if let Some(key) = &key {
                watches
                    .entry(key.clone())
                    .or_default()
                    .windows
                    .insert(window.to_owned(), now + WINDOW_LEASE);
            }
        }
        self.inner.expire_wt_observers(now);
        if let Some(key) = key {
            self.inner.ensure_wt_watching(&key);
        }
    }

    /// Правка документов в worktree из окна: пути ждут паузы Т-41 и уходят в дельту сверкой путей
    /// (`Edit`). У worktree без дельты правка ничего не ставит: дельту создаёт обращение.
    pub fn edit_worktree_files(&self, project: &Path, name: &str, files: Vec<PathBuf>) {
        let Some(key) = self.worktree_key(project, name) else {
            return;
        };
        self.inner
            .touch_worktree_files(&key, files, DocSource::Edit);
    }
}
