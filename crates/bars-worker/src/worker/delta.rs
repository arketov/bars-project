//! Дельта индекса worktree в воркере: состояния в памяти, очередь сверок `(владелец, имя)`,
//! ленивое первое построение и часовая уборка (`docs/features/projects/worktrees.md`, разделы
//! «Построение и обновление дельты», «Очередь и конкурентность», «Жизненный цикл»; Т-48).
//!
//! Ядро сверки — `bars_core::indexing::worktree_delta`; здесь только то, что принадлежит воркеру:
//! замок индекса владельца на чтение, модель, пауза и остановка, состояния и файлы. Дельта
//! создаётся только по обращению ([`Worker::ensure_worktree_delta`]); обнаружение worktree её
//! не создаёт.

use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;

use bars_core::indexing::indexer::{IndexObserver, IndexSettings};
use bars_core::indexing::nested;
use bars_core::indexing::worktree_delta::{
    self as delta, DeltaCheck, DeltaError, IndexOwner, OwnerSnapshot, OwnerView, SyncRequest,
    SyncScope,
};
use bars_core::storage::{Index, index_path};
use bars_protocol::{DeltaState, WorktreeDeltaStatus};
use tokio::sync::OwnedRwLockReadGuard;

use super::queue::Priority;
use super::*;

/// Ключ дельты: владелец и имя worktree.
pub(super) type DeltaKey = (PathBuf, String);

/// Через сколько опрашивается очередь, если сигнал потерялся (занятый владелец сигнала не даёт).
const POLL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    /// Первое построение: сколько документов вида просмотрено из скольких.
    Building {
        done: usize,
        total: usize,
    },
    Ready,
    Error(String),
}

#[derive(Debug, Clone)]
struct Entry {
    phase: Phase,
    /// Документов в дельте (отличающихся от владельца).
    documents: usize,
}

#[derive(Debug)]
struct Queued {
    seq: u64,
    priority: Priority,
    /// `None` — все документы вида.
    paths: Option<BTreeSet<String>>,
}

/// Состояния и очередь дельт всех владельцев.
#[derive(Default)]
pub(super) struct Deltas {
    entries: HashMap<DeltaKey, Entry>,
    queue: Vec<(DeltaKey, Queued)>,
    running: Option<(DeltaKey, Arc<AtomicBool>)>,
    next_seq: u64,
}

impl Deltas {
    /// Есть ли у worktree состояние дельты (не «нет»).
    pub(super) fn contains(&self, key: &DeltaKey) -> bool {
        self.entries.contains_key(key)
    }

    /// Ключи дельт с известным состоянием.
    pub(super) fn keys(&self) -> impl Iterator<Item = &DeltaKey> {
        self.entries.keys()
    }

    /// Ничего не ждёт и не выполняется.
    pub(super) fn idle(&self) -> bool {
        self.queue.is_empty() && self.running.is_none()
    }
}

/// Итог задания дельты.
enum Outcome {
    Done {
        documents: usize,
    },
    /// Worktree исчез, дельта удалена или несовместима: состояние «нет».
    Dropped,
    /// Сверять не с чем или нельзя (владелец без индекса, в `reindex_required`, не готов).
    Skipped,
    Cancelled,
    Failed(String),
}

fn same_name(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        left.to_lowercase() == right.to_lowercase()
    } else {
        left == right
    }
}

/// Индекс владельца для сверки под замком проекта на чтение: замок держится на время одного
/// вызова (снимок или пачка векторов), не на всю сверку.
struct GateOwner {
    gate: Arc<RwLock<()>>,
    handle: Handle,
    owner: PathBuf,
}

impl GateOwner {
    fn open(&self) -> Result<(Index, OwnedRwLockReadGuard<()>), DeltaError> {
        let guard = self.handle.block_on(Arc::clone(&self.gate).read_owned());
        let path = index_path(&self.owner);
        if !path.is_file() {
            return Err(DeltaError::OwnerNotReady("у владельца нет индекса".into()));
        }
        Ok((Index::open_read_only(&path)?, guard))
    }
}

impl OwnerView for GateOwner {
    fn snapshot(&self) -> Result<OwnerSnapshot, DeltaError> {
        let (index, _guard) = self.open()?;
        Ok(OwnerSnapshot::read(&index)?)
    }

    fn vectors(&self, embed_hashes: &[&str]) -> Result<HashMap<String, Vec<f32>>, DeltaError> {
        let (index, _guard) = self.open()?;
        IndexOwner(&index).vectors(embed_hashes)
    }
}

/// Ход, пауза и остановка сверки.
struct Observer {
    inner: Arc<Inner>,
    key: DeltaKey,
    cancel: Arc<AtomicBool>,
}

impl Observer {
    fn building(&self, change: impl FnOnce(&mut usize, &mut usize)) -> bool {
        let mut deltas = lock(&self.inner.deltas);
        match deltas.entries.get_mut(&self.key) {
            Some(Entry {
                phase: Phase::Building { done, total },
                ..
            }) => {
                change(done, total);
                true
            }
            _ => false,
        }
    }
}

impl IndexObserver for Observer {
    fn cancel_requested(&self) -> bool {
        self.cancel.load(Ordering::SeqCst) || self.inner.stop.load(Ordering::SeqCst)
    }

    fn should_stop(&self) -> bool {
        loop {
            if self.cancel_requested() {
                return true;
            }
            if !self.inner.pause.is_paused() {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn begin(&self, documents: usize) {
        if self.building(|_, total| *total = documents) {
            self.inner
                .updates
                .changed(&self.key.0, updates::STATUS, false);
        }
    }

    fn progress(&self, documents_done: usize, _: usize) {
        let publish = self.building(|done, _| *done = documents_done);
        if publish && documents_done % 25 == 0 {
            self.inner
                .updates
                .changed(&self.key.0, updates::STATUS, false);
        }
    }
}

impl Inner {
    /// Ставит сверку дельты `(владелец, имя)`: `paths = None` — все документы вида. Ленивый
    /// владелец сначала активируется. Владелец в `reindex_required` или `unavailable`, пропавший
    /// worktree и останавливающийся воркер заданий не получают. `true` — задание поставлено.
    pub(super) fn request_delta(
        self: &Arc<Self>,
        owner: &Path,
        name: &str,
        priority: Priority,
        paths: Option<BTreeSet<String>>,
    ) -> bool {
        if self.stop.load(Ordering::SeqCst) {
            return false;
        }
        let Some(linked) = self
            .linked_of(owner)
            .into_iter()
            .find(|item| same_name(&item.name, name))
        else {
            return false;
        };
        // Первое обращение к worktree ленивого владельца: сначала индекс владельца.
        if self.is_lazy(owner) {
            self.activate(owner, Priority::Explicit);
        }
        let usable = lock(&self.projects).get(owner).is_some_and(|runtime| {
            !runtime.deleting
                && !matches!(
                    runtime.status.state,
                    ProjectState::Unavailable | ProjectState::ReindexRequired | ProjectState::Lazy
                )
        });
        if !usable {
            return false;
        }
        let key: DeltaKey = (owner.to_path_buf(), linked.name);
        {
            let mut deltas = lock(&self.deltas);
            match deltas.queue.iter_mut().find(|(queued, _)| *queued == key) {
                Some((_, job)) => {
                    job.priority = job.priority.max(priority);
                    job.paths = match (job.paths.take(), paths) {
                        (Some(mut mine), Some(theirs)) => {
                            mine.extend(theirs);
                            Some(mine)
                        }
                        _ => None,
                    };
                }
                None => {
                    let seq = deltas.next_seq;
                    deltas.next_seq += 1;
                    deltas.queue.push((
                        key.clone(),
                        Queued {
                            seq,
                            priority,
                            paths,
                        },
                    ));
                }
            }
            // Готовая дельта остаётся готовой, пока её пересверяют; без неё — «строится».
            let entry = deltas.entries.entry(key.clone()).or_insert(Entry {
                phase: Phase::Building { done: 0, total: 0 },
                documents: 0,
            });
            if matches!(entry.phase, Phase::Error(_)) {
                entry.phase = Phase::Building { done: 0, total: 0 };
            }
        }
        self.updates.changed(owner, updates::STATUS, false);
        self.delta_wake.notify_one();
        // Дельта появилась: слежение за W поднимается, если у worktree уже есть аренда.
        self.ensure_wt_watching(&key);
        true
    }

    /// Задание, которое можно взять сейчас: индексация владельцев тише и главнее; одна сверка
    /// за раз. `None` — ждать.
    fn take_delta_job(&self) -> Option<(DeltaKey, Queued, Arc<AtomicBool>)> {
        let main_busy =
            |inner: &Self| !lock(&inner.queue).is_empty() || inner.busy.load(Ordering::SeqCst);
        if main_busy(self) {
            return None;
        }
        let mut deltas = lock(&self.deltas);
        if deltas.running.is_some() {
            return None;
        }
        let index = deltas
            .queue
            .iter()
            .enumerate()
            .max_by(|(_, (_, left)), (_, (_, right))| {
                left.priority
                    .cmp(&right.priority)
                    .then(right.seq.cmp(&left.seq))
            })
            .map(|(index, _)| index)?;
        let (key, job) = deltas.queue.remove(index);
        let cancel = Arc::new(AtomicBool::new(false));
        deltas.running = Some((key.clone(), Arc::clone(&cancel)));
        self.delta_busy.store(true, Ordering::SeqCst);
        drop(deltas);
        // Очередь проектов могла ожить между проверкой и флагом: уступаем ей. Замок очереди
        // берётся вне замка дельт: `is_idle` держит их в обратном порядке.
        if main_busy(self) {
            let mut deltas = lock(&self.deltas);
            deltas.running = None;
            deltas.queue.push((key, job));
            self.delta_busy.store(false, Ordering::SeqCst);
            return None;
        }
        Some((key, job, cancel))
    }

    pub(super) async fn run_delta_queue(self: Arc<Self>) {
        // Проверка старта: уборка дельт и восстановление «готова» из файлов.
        let inner = Arc::clone(&self);
        let _ = tokio::task::spawn_blocking(move || inner.check_deltas()).await;
        loop {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            let next = if self.pause.is_paused() {
                None
            } else {
                self.take_delta_job()
            };
            match next {
                Some((key, job, cancel)) => self.run_delta_job(key, job, cancel).await,
                None => {
                    tokio::select! {
                        () = self.delta_wake.notified() => {},
                        () = tokio::time::sleep(POLL) => {},
                    }
                }
            }
        }
    }

    async fn run_delta_job(self: &Arc<Self>, key: DeltaKey, job: Queued, cancel: Arc<AtomicBool>) {
        let inner = Arc::clone(self);
        let worker_key = key.clone();
        let result =
            tokio::task::spawn_blocking(move || inner.delta_job(&worker_key, &job, &cancel)).await;
        let outcome = result
            .unwrap_or_else(|error| Outcome::Failed(format!("внутренняя ошибка воркера: {error}")));
        let mut dropped = false;
        {
            let mut deltas = lock(&self.deltas);
            deltas.running = None;
            let requeued = deltas.queue.iter().any(|(queued, _)| *queued == key);
            match outcome {
                Outcome::Done { documents } => {
                    deltas.entries.insert(
                        key.clone(),
                        Entry {
                            phase: Phase::Ready,
                            documents,
                        },
                    );
                }
                Outcome::Failed(message) => {
                    log::warn!(
                        "Дельта worktree «{}» проекта {} не построена: {message}",
                        key.1,
                        key.0.display()
                    );
                    deltas.entries.insert(
                        key.clone(),
                        Entry {
                            phase: Phase::Error(message),
                            documents: 0,
                        },
                    );
                }
                Outcome::Dropped | Outcome::Skipped => {
                    deltas.entries.remove(&key);
                    dropped = true;
                }
                Outcome::Cancelled => {
                    // Остановка оставляет готовую дельту готовой; недостроенная возвращается к «нет».
                    if deltas
                        .entries
                        .get(&key)
                        .is_some_and(|entry| matches!(entry.phase, Phase::Building { .. }))
                        && !requeued
                    {
                        deltas.entries.remove(&key);
                    }
                }
            }
        }
        self.delta_busy.store(false, Ordering::SeqCst);
        if dropped {
            // Состояние «нет»: события W отбрасываются, слежение не нужно.
            self.drop_wt_watcher(&key);
            lock(&self.docs).remove_worktree(&key.0, &key.1);
        }
        self.updates.changed(&key.0, updates::STATUS, false);
        // Очередь проектов ждала конца сверки.
        self.wake.notify_one();
        self.delta_wake.notify_one();
    }

    /// Блокирующее тело задания: сверка с владельцем. Метаданные, не совпавшие с владельцем,
    /// удаляют дельту (она производная) и ведут к состоянию «нет».
    fn delta_job(
        self: &Arc<Self>,
        key: &DeltaKey,
        job: &Queued,
        cancel: &Arc<AtomicBool>,
    ) -> Outcome {
        let (owner, name) = key;
        let Some(linked) = self
            .linked_of(owner)
            .into_iter()
            .find(|item| item.name == *name)
        else {
            return Outcome::Dropped;
        };
        let gate = {
            let projects = lock(&self.projects);
            let Some(runtime) = projects.get(owner).filter(|runtime| {
                !runtime.deleting
                    && !matches!(
                        runtime.status.state,
                        ProjectState::Unavailable
                            | ProjectState::ReindexRequired
                            | ProjectState::Lazy
                    )
            }) else {
                return Outcome::Skipped;
            };
            Arc::clone(&runtime.gate)
        };
        let settings = match Settings::load(&self.config.bars_home, owner) {
            Ok(loaded) => loaded.value,
            Err(error) => return Outcome::Failed(error.to_string()),
        };
        let fingerprint = match self.models.fingerprint(&settings) {
            Ok(fingerprint) => fingerprint,
            Err(error) => return Outcome::Failed(error.to_string()),
        };
        let index_settings = settings.index_settings();
        let view = GateOwner {
            gate,
            handle: self.handle.clone(),
            owner: owner.clone(),
        };
        let snapshot = match view.snapshot() {
            Ok(snapshot) => snapshot,
            Err(DeltaError::OwnerNotReady(_)) => return Outcome::Skipped,
            Err(error) => return Outcome::Failed(error.to_string()),
        };
        // Владелец ещё не переиндексирован новой моделью: сверять не с чем.
        if !snapshot
            .fingerprint
            .as_ref()
            .is_some_and(|stored| stored.matches(&fingerprint))
        {
            return Outcome::Skipped;
        }
        if let Some(reason) = delta_mismatch(owner, name, &snapshot, &index_settings) {
            log::info!(
                "Дельта worktree «{name}» проекта {} удалена: {reason}",
                owner.display()
            );
            if let Err(error) = remove_delta(owner, name) {
                return Outcome::Failed(format!("дельта не удалена: {error}"));
            }
        }
        let mut embedder = match self.models.embedder(&settings) {
            Ok(embedder) => embedder,
            Err(error) => return Outcome::Failed(error.to_string()),
        };
        let mut index = match delta::open_delta(owner, name) {
            Ok(index) => index,
            Err(error) => return Outcome::Failed(error.to_string()),
        };
        let ready = delta::is_ready(&index).unwrap_or(false);
        let paths: Option<HashSet<String>> = job
            .paths
            .as_ref()
            .filter(|_| ready)
            .map(|paths| paths.iter().cloned().collect());
        let excluded = nested::excluded(owner);
        let observer = Observer {
            inner: Arc::clone(self),
            key: key.clone(),
            cancel: Arc::clone(cancel),
        };
        let result = delta::sync(
            &mut index,
            &view,
            &mut embedder,
            &SyncRequest {
                view_root: &linked.view_root,
                settings: &index_settings,
                excluded: &excluded,
                scope: paths.as_ref().map_or(SyncScope::All, SyncScope::Paths),
                name,
            },
            &observer,
        );
        let closed = index.close();
        match result {
            Ok(report) => {
                for problem in &report.problems {
                    // reason может содержать путь и ответ провайдера: пишем метаданные.
                    log::warn!(
                        "Проблема сверки дельты: проект {}, worktree {name}, файл {}, этап {}",
                        owner.display(),
                        problem.path,
                        problem.stage
                    );
                }
                log::info!(
                    "Дельта worktree «{name}» проекта {}: в виде {}, в дельте {}, записано {}, \
                     векторов посчитано {}, скопировано {}",
                    owner.display(),
                    report.view_documents,
                    report.delta_documents,
                    report.written,
                    report.chunks_embedded,
                    report.chunks_copied
                );
                match closed {
                    Ok(()) => Outcome::Done {
                        documents: report.delta_documents,
                    },
                    Err(error) => Outcome::Failed(error.to_string()),
                }
            }
            Err(DeltaError::Cancelled) => Outcome::Cancelled,
            Err(DeltaError::OwnerNotReady(_)) => Outcome::Skipped,
            Err(DeltaError::Incompatible(reason)) => {
                log::info!(
                    "Дельта worktree «{name}» проекта {} удалена: {reason}",
                    owner.display()
                );
                match remove_delta(owner, name) {
                    Ok(()) => Outcome::Dropped,
                    Err(error) => Outcome::Failed(format!("дельта не удалена: {error}")),
                }
            }
            Err(error) => Outcome::Failed(error.to_string()),
        }
    }

    /// Проверка старта и часовая (`worktrees.md`, «Жизненный цикл»): дельты пропавших и
    /// оторванных worktree и дельты с другой моделью, нарезкой, `docs_root` или более новым
    /// форматом удаляются; у живых «готова» восстанавливается из файла, а дельта, отставшая от
    /// владельца или недостроенная, ставится на сверку с приоритетом `Periodic`. Владелец в
    /// `reindex_required`, `unavailable` или ленивый заданий не получает. Без изменений ничего
    /// не ставится (НТР-3). Блокирующая: зовётся из `spawn_blocking`.
    pub(super) fn check_deltas(self: &Arc<Self>) {
        let owners = lock(&self.registry).paths();
        for owner in owners {
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            if self.is_lazy(&owner) {
                continue;
            }
            let state = lock(&self.projects)
                .get(&owner)
                .filter(|runtime| !runtime.deleting)
                .map(|runtime| (runtime.status.state, Arc::clone(&runtime.gate)));
            let Some((state, gate)) = state else {
                continue;
            };
            if !matches!(state, ProjectState::Ready | ProjectState::Indexing) {
                continue;
            }
            let Ok(read) = fs::read_dir(delta::deltas_dir(&owner)) else {
                continue;
            };
            let names: Vec<String> = read
                .flatten()
                .filter(|entry| entry.path().is_dir())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect();
            if names.is_empty() {
                continue;
            }
            let live = self.linked_of(&owner);
            // Снимок владельца и настройки нужны только если есть что сверять.
            let mut context: Option<(OwnerSnapshot, IndexSettings)> = None;
            for name in names {
                let Some(item) = live.iter().find(|item| same_name(&item.name, &name)) else {
                    self.drop_delta(&owner, &name, "worktree пропал или оторван");
                    continue;
                };
                if context.is_none() {
                    let Ok(settings) =
                        Settings::load(&self.config.bars_home, &owner).map(|loaded| loaded.value)
                    else {
                        break;
                    };
                    let view = GateOwner {
                        gate: Arc::clone(&gate),
                        handle: self.handle.clone(),
                        owner: owner.clone(),
                    };
                    let Ok(snapshot) = view.snapshot() else {
                        break;
                    };
                    let Ok(fingerprint) = self.models.fingerprint(&settings) else {
                        break;
                    };
                    // Владелец ждёт переиндексации новой моделью: его дельты проверит следующий проход.
                    if !snapshot
                        .fingerprint
                        .as_ref()
                        .is_some_and(|stored| stored.matches(&fingerprint))
                    {
                        break;
                    }
                    context = Some((snapshot, settings.index_settings()));
                }
                let (snapshot, settings) = context.as_ref().expect("контекст создан");
                if let Some(reason) = delta_mismatch(&owner, &item.name, snapshot, settings) {
                    self.drop_delta(&owner, &item.name, &reason);
                    continue;
                }
                self.restore_delta(&owner, &item.name, snapshot);
            }
        }
    }

    /// Совместимая дельта живого worktree: «готова» из файла и сверка, если отстала.
    fn restore_delta(self: &Arc<Self>, owner: &Path, name: &str, snapshot: &OwnerSnapshot) {
        let Ok(index) = Index::open_read_only(&delta::delta_path(owner, name)) else {
            return;
        };
        let ready = delta::is_ready(&index).unwrap_or(false);
        let documents = index.document_count().unwrap_or(0);
        let (digest, _) = delta::stored_sync(&index).unwrap_or((None, None));
        drop(index);
        let key: DeltaKey = (owner.to_path_buf(), name.to_string());
        if ready {
            let mut deltas = lock(&self.deltas);
            deltas.entries.entry(key).or_insert(Entry {
                phase: Phase::Ready,
                documents,
            });
        }
        let stale = digest.as_deref() != Some(snapshot.digest.as_str());
        if !ready || stale {
            self.request_delta(owner, name, Priority::Periodic, None);
        }
    }

    /// Удаляет дельту worktree: файлы, состояние и задания. Идущую сверку не трогает — следующая
    /// проверка вернётся к ней.
    fn drop_delta(&self, owner: &Path, name: &str, reason: &str) {
        let key: DeltaKey = (owner.to_path_buf(), name.to_string());
        {
            let mut deltas = lock(&self.deltas);
            if deltas
                .running
                .as_ref()
                .is_some_and(|(running, _)| *running == key)
            {
                return;
            }
            deltas.queue.retain(|(queued, _)| *queued != key);
            deltas.entries.remove(&key);
        }
        self.drop_wt_watcher(&key);
        lock(&self.docs).remove_worktree(owner, name);
        log::info!(
            "Дельта worktree «{name}» проекта {} удалена: {reason}",
            owner.display()
        );
        if let Err(error) = remove_delta(owner, name) {
            log::warn!(
                "Дельта worktree «{name}» проекта {} не удалена: {error}",
                owner.display()
            );
        }
        self.updates.changed(owner, updates::STATUS, false);
    }

    /// Владелец снят, остановлен или удаляется: сверки его дельт отменяются, состояния забываются,
    /// слежение за его worktree и их записи в очереди документов снимаются.
    pub(super) fn cancel_deltas(&self, owner: &Path) {
        let mut deltas = lock(&self.deltas);
        deltas.queue.retain(|((queued, _), _)| queued != owner);
        deltas.entries.retain(|(entry, _), _| entry != owner);
        if let Some(((running, _), cancel)) = deltas.running.as_ref()
            && running == owner
        {
            cancel.store(true, Ordering::SeqCst);
        }
        drop(deltas);
        // Слежение за W и записи очереди документов тех же worktree.
        self.cancel_wt_watching(owner);
    }

    /// Идёт ли сверка дельты этого владельца.
    pub(super) fn delta_running_for(&self, owner: &Path) -> bool {
        lock(&self.deltas)
            .running
            .as_ref()
            .is_some_and(|((running, _), _)| running == owner)
    }
}

/// Не подходит ли дельта на диске индексу владельца; `None` — нет файла или подходит.
fn delta_mismatch(
    owner: &Path,
    name: &str,
    snapshot: &OwnerSnapshot,
    settings: &IndexSettings,
) -> Option<String> {
    let path = delta::delta_path(owner, name);
    if !path.is_file() {
        return None;
    }
    let index = match Index::open_read_only(&path) {
        Ok(index) => index,
        Err(error) => return Some(format!("файл дельты не открывается: {error}")),
    };
    match delta::check_delta(&index, snapshot, settings) {
        Ok(DeltaCheck::Mismatch(reason)) => Some(reason),
        Ok(_) => None,
        Err(error) => Some(format!("метаданные дельты не читаются: {error}")),
    }
}

/// Удаляет папку дельты worktree и пустую `.bars/worktrees`.
fn remove_delta(owner: &Path, name: &str) -> io::Result<()> {
    let directory = delta::delta_dir(owner, name);
    match fs::remove_dir_all(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    // Пустую папку всех дельт не оставляем; занятая или непустая остаётся.
    let _ = fs::remove_dir(delta::deltas_dir(owner));
    Ok(())
}

impl Worker {
    /// Первое обращение к worktree: сессия в нём, поиск с `wt`, открытие в окне, проверка вида.
    /// Ставит сверку всех документов вида в очередь с приоритетом `Explicit`; дельта строится
    /// лениво и только по такому обращению. `false` — worktree не найден, владелец не готов
    /// (`reindex_required`, `unavailable`) или воркер останавливается.
    pub fn ensure_worktree_delta(&self, project: &Path, name: &str) -> bool {
        // Дельта строится или готова — повторное обращение (каждый поиск) задания не ставит:
        // актуальность держат слежение и проверки. Ошибка повторяется при обращении.
        let known = lock(&self.inner.deltas)
            .entries
            .get(&(project.to_path_buf(), name.to_string()))
            .is_some_and(|entry| !matches!(entry.phase, Phase::Error(_)));
        if known {
            return true;
        }
        self.inner
            .request_delta(project, name, Priority::Explicit, None)
    }

    /// Явная сверка вида (`bars-cli reindex --wt`, `bars_status refresh=true wt=…`): ставит в
    /// очередь сверку всех документов вида, даже если дельта готова; без дельты это её первое
    /// построение. `false` — worktree не найден или владелец не готов, как у
    /// [`Worker::ensure_worktree_delta`].
    pub fn resync_worktree(&self, project: &Path, name: &str) -> bool {
        self.inner
            .request_delta(project, name, Priority::Explicit, None)
    }

    /// Пересверка перечисленных путей дельты (пути от корня проекта через `/`); без готовой
    /// дельты ставится полная сверка. Шов для слежения за W и изменений владельца (шаг 6.3.4).
    pub fn resync_worktree_paths(
        &self,
        project: &Path,
        name: &str,
        priority: Priority,
        paths: BTreeSet<String>,
    ) -> bool {
        self.inner
            .request_delta(project, name, priority, Some(paths))
    }

    /// Состояние дельты worktree для `status`: «нет», «строится», «готова» или «ошибка».
    pub fn worktree_delta_status(&self, project: &Path, name: &str) -> WorktreeDeltaStatus {
        let deltas = lock(&self.inner.deltas);
        let key: DeltaKey = (project.to_path_buf(), name.to_string());
        let pending = deltas
            .queue
            .iter()
            .find(|(queued, _)| *queued == key)
            .map_or(0, |(_, job)| job.paths.as_ref().map_or(0, BTreeSet::len));
        let none = WorktreeDeltaStatus {
            state: DeltaState::None,
            documents: 0,
            done: None,
            total: None,
            pending: 0,
            error: None,
        };
        let Some(entry) = deltas.entries.get(&key) else {
            return none;
        };
        let documents = u32::try_from(entry.documents).unwrap_or(u32::MAX);
        let pending = u32::try_from(pending).unwrap_or(u32::MAX);
        match &entry.phase {
            Phase::Building { done, total } => WorktreeDeltaStatus {
                state: DeltaState::Building,
                documents,
                done: Some(u32::try_from(*done).unwrap_or(u32::MAX)),
                total: Some(u32::try_from(*total).unwrap_or(u32::MAX)),
                pending,
                error: None,
            },
            Phase::Ready => WorktreeDeltaStatus {
                state: DeltaState::Ready,
                documents,
                pending,
                ..none
            },
            Phase::Error(message) => WorktreeDeltaStatus {
                state: DeltaState::Error,
                documents,
                pending,
                error: Some(message.clone()),
                ..none
            },
        }
    }
}
