//! Очередь документов: запись на документ, пауза после правки, порядок выдачи
//! (`docs/features/indexing/document-queue.md`, Т-41, Т-42).
//!
//! Структура ничего не знает про потоки, часы и диск: воркер кладёт её за мьютекс и сам
//! передаёт текущий момент. Выполняет индексацию по-прежнему задание очереди проектов
//! ([`super::queue::JobQueue`]); эта очередь решает, когда и какие документы ему отдать.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::queue::Priority;

/// Откуда пришло изменение. Порядок вариантов — порядок приоритета (Т-42).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DocSource {
    /// Накопленное расхождение, найденное проверкой.
    Periodic,
    /// Событие слежения за файлами.
    Watch,
    /// Правка в окне bars.
    Edit,
    /// Явная команда пользователя.
    Explicit,
}

impl DocSource {
    pub fn priority(self) -> Priority {
        match self {
            Self::Periodic => Priority::Periodic,
            Self::Watch => Priority::Watch,
            Self::Edit => Priority::Edit,
            Self::Explicit => Priority::Explicit,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocState {
    /// Ждёт, пока пройдёт пауза после правки.
    Waiting,
    /// Готов, ждёт своей очереди.
    Queued,
    /// Идёт эмбеддинг.
    Indexing,
    /// Не проиндексирован после повторов.
    Error,
}

#[derive(Debug, Clone)]
struct DocEntry {
    state: DocState,
    source: DocSource,
    /// У `Waiting` — когда закончится пауза.
    due_at: Option<Instant>,
    /// Порядок появления записи.
    seq: u64,
    /// Документ изменили, пока он индексировался: после этого он снова ждёт паузы до этого момента.
    redo_due: Option<Instant>,
    error: Option<String>,
}

/// Запись очереди, как её видит окно.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocView {
    pub path: String,
    pub state: DocState,
    pub source: DocSource,
    /// Сколько осталось ждать паузы; только у `Waiting`.
    pub due_in: Option<Duration>,
    pub error: Option<String>,
}

/// Сводка очереди проекта.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocCounts {
    pub waiting: usize,
    pub queued: usize,
    pub indexing: usize,
    pub error: usize,
}

impl DocCounts {
    pub fn total(&self) -> usize {
        self.waiting + self.queued + self.indexing + self.error
    }
}

/// Документы, которые стали готовыми после паузы.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ready {
    pub project: PathBuf,
    /// Пути от корня проекта.
    pub paths: BTreeSet<String>,
    pub source: DocSource,
}

#[derive(Debug, Default)]
pub struct DocQueue {
    entries: BTreeMap<(PathBuf, String), DocEntry>,
    next_seq: u64,
}

impl DocQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Изменение документа: правка, событие слежения, найденное проверкой расхождение. Одна запись
    /// на документ (Т-42): повторное изменение сдвигает паузу, источник берётся старший.
    /// Пауза `0` — запись сразу готова.
    pub fn touch(
        &mut self,
        project: &Path,
        path: &str,
        source: DocSource,
        pause: Duration,
        now: Instant,
    ) {
        let due = now + pause;
        let ready = pause.is_zero();
        let key = (project.to_path_buf(), path.to_string());
        match self.entries.get_mut(&key) {
            Some(entry) => {
                entry.source = entry.source.max(source);
                match entry.state {
                    DocState::Indexing => entry.redo_due = Some(due),
                    _ => {
                        entry.error = None;
                        if ready {
                            entry.state = DocState::Queued;
                            entry.due_at = None;
                        } else {
                            entry.state = DocState::Waiting;
                            entry.due_at = Some(due);
                        }
                    }
                }
            }
            None => {
                let seq = self.next_seq;
                self.next_seq += 1;
                self.entries.insert(
                    key,
                    DocEntry {
                        state: if ready {
                            DocState::Queued
                        } else {
                            DocState::Waiting
                        },
                        source,
                        due_at: (!ready).then_some(due),
                        seq,
                        redo_due: None,
                        error: None,
                    },
                );
            }
        }
    }

    /// Когда истечёт ближайшая пауза.
    pub fn next_due(&self) -> Option<Instant> {
        self.entries
            .values()
            .filter(|entry| entry.state == DocState::Waiting)
            .filter_map(|entry| entry.due_at)
            .min()
    }

    /// Переводит документы с истёкшей паузой в готовые; возвращает их по проектам.
    pub fn promote_due(&mut self, now: Instant) -> Vec<Ready> {
        let mut ready: BTreeMap<PathBuf, Ready> = BTreeMap::new();
        for ((project, path), entry) in &mut self.entries {
            if entry.state == DocState::Waiting && entry.due_at.is_some_and(|due| due <= now) {
                entry.state = DocState::Queued;
                entry.due_at = None;
                let item = ready.entry(project.clone()).or_insert_with(|| Ready {
                    project: project.clone(),
                    paths: BTreeSet::new(),
                    source: entry.source,
                });
                item.paths.insert(path.clone());
                item.source = item.source.max(entry.source);
            }
        }
        ready.into_values().collect()
    }

    /// Готовые документы проекта (`Queued`), если они есть.
    pub fn ready_of(&self, project: &Path) -> Option<Ready> {
        let mut ready: Option<Ready> = None;
        for ((owner, path), entry) in &self.entries {
            if owner != project || entry.state != DocState::Queued {
                continue;
            }
            let item = ready.get_or_insert_with(|| Ready {
                project: project.to_path_buf(),
                paths: BTreeSet::new(),
                source: entry.source,
            });
            item.paths.insert(path.clone());
            item.source = item.source.max(entry.source);
        }
        ready
    }

    /// Документ ждёт паузы: проверка проекта его пропускает.
    pub fn is_waiting(&self, project: &Path, path: &str) -> bool {
        self.entries
            .get(&(project.to_path_buf(), path.to_string()))
            .is_some_and(|entry| entry.state == DocState::Waiting)
    }

    /// Задание берёт документы в работу. Готовые (`Queued`) берутся все; ждущие — только по явной
    /// команде пользователя и только названные в ней (`explicit`; `None` — все). Возвращает
    /// взятые пути.
    pub fn begin(
        &mut self,
        project: &Path,
        explicit: Option<Option<&BTreeSet<String>>>,
    ) -> Vec<String> {
        let mut taken = Vec::new();
        for ((owner, path), entry) in &mut self.entries {
            if owner != project {
                continue;
            }
            let take = match entry.state {
                DocState::Queued => true,
                DocState::Waiting => match explicit {
                    None => false,
                    Some(None) => true,
                    Some(Some(listed)) => listed.contains(path),
                },
                DocState::Indexing | DocState::Error => false,
            };
            if take {
                entry.state = DocState::Indexing;
                entry.due_at = None;
                entry.redo_due = None;
                taken.push(path.clone());
            }
        }
        taken
    }

    /// Задание закончилось. `failed` — документы, которые не удалось проиндексировать, с
    /// причиной; `keep` — вернуть остальные в очередь (задание прервано сбоем), иначе они
    /// успешно разобраны и записи снимаются. Документ, изменённый во время работы, снова ждёт паузы.
    pub fn finish(
        &mut self,
        project: &Path,
        taken: &[String],
        failed: &BTreeMap<String, String>,
        keep: bool,
    ) {
        for path in taken {
            let key = (project.to_path_buf(), path.clone());
            let Some(entry) = self.entries.get_mut(&key) else {
                continue;
            };
            if entry.state != DocState::Indexing {
                continue;
            }
            if let Some(due) = entry.redo_due.take() {
                entry.state = DocState::Waiting;
                entry.due_at = Some(due);
                entry.error = None;
            } else if let Some(reason) = failed.get(path) {
                entry.state = DocState::Error;
                entry.error = Some(reason.clone());
            } else if keep {
                entry.state = DocState::Queued;
            } else {
                self.entries.remove(&key);
            }
        }
    }

    /// «Повторить»: документ с ошибкой снова готов. `false` — такой записи нет.
    pub fn retry(&mut self, project: &Path, path: &str) -> bool {
        match self
            .entries
            .get_mut(&(project.to_path_buf(), path.to_string()))
        {
            Some(entry) if entry.state == DocState::Error => {
                entry.state = DocState::Queued;
                entry.error = None;
                true
            }
            _ => false,
        }
    }

    /// Снимает все записи проекта (остановка, удаление, смена модели).
    pub fn remove_project(&mut self, project: &Path) -> usize {
        let before = self.entries.len();
        self.entries.retain(|(owner, _), _| owner != project);
        before - self.entries.len()
    }

    pub fn counts(&self, project: &Path) -> DocCounts {
        let mut counts = DocCounts::default();
        for ((owner, _), entry) in &self.entries {
            if owner != project {
                continue;
            }
            match entry.state {
                DocState::Waiting => counts.waiting += 1,
                DocState::Queued => counts.queued += 1,
                DocState::Indexing => counts.indexing += 1,
                DocState::Error => counts.error += 1,
            }
        }
        counts
    }

    /// Записи проекта в порядке выполнения: идущие, готовые, ждущие паузы, с ошибкой. Внутри
    /// группы — по источнику (старший раньше), затем по порядку появления.
    pub fn snapshot(&self, project: &Path, now: Instant, limit: usize) -> Vec<DocView> {
        let mut items: Vec<(&String, &DocEntry)> = self
            .entries
            .iter()
            .filter(|((owner, _), _)| owner == project)
            .map(|((_, path), entry)| (path, entry))
            .collect();
        let rank = |state: DocState| match state {
            DocState::Indexing => 0,
            DocState::Queued => 1,
            DocState::Waiting => 2,
            DocState::Error => 3,
        };
        items.sort_by(|(_, a), (_, b)| {
            rank(a.state)
                .cmp(&rank(b.state))
                .then(b.source.cmp(&a.source))
                .then(a.due_at.cmp(&b.due_at))
                .then(a.seq.cmp(&b.seq))
        });
        items
            .into_iter()
            .take(limit)
            .map(|(path, entry)| DocView {
                path: path.clone(),
                state: entry.state,
                source: entry.source,
                due_in: entry
                    .due_at
                    .filter(|_| entry.state == DocState::Waiting)
                    .map(|due| due.saturating_duration_since(now)),
                error: entry.error.clone(),
            })
            .collect()
    }

    /// Есть документ, который ещё ждёт паузы или индексации (записи с ошибкой не в счёт).
    pub fn has_pending(&self) -> bool {
        self.entries
            .values()
            .any(|entry| entry.state != DocState::Error)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAUSE: Duration = Duration::from_secs(30);

    fn project() -> PathBuf {
        PathBuf::from("p")
    }

    fn at(base: Instant, seconds: u64) -> Instant {
        base + Duration::from_secs(seconds)
    }

    #[test]
    fn repeated_edits_keep_one_record_and_restart_the_pause() {
        let base = Instant::now();
        let mut queue = DocQueue::new();
        for second in [0, 2, 4, 6, 8] {
            queue.touch(&project(), "a.md", DocSource::Edit, PAUSE, at(base, second));
        }
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.next_due(), Some(at(base, 38)));
        assert!(queue.promote_due(at(base, 37)).is_empty());
        let ready = queue.promote_due(at(base, 38));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].paths.len(), 1);
        assert_eq!(queue.counts(&project()).queued, 1);
    }

    #[test]
    fn zero_pause_makes_the_record_ready_at_once() {
        let mut queue = DocQueue::new();
        queue.touch(
            &project(),
            "a.md",
            DocSource::Periodic,
            Duration::ZERO,
            Instant::now(),
        );
        assert_eq!(queue.counts(&project()).queued, 1);
        assert_eq!(queue.next_due(), None);
    }

    #[test]
    fn the_senior_source_wins() {
        let now = Instant::now();
        let mut queue = DocQueue::new();
        queue.touch(&project(), "a.md", DocSource::Watch, PAUSE, now);
        queue.touch(&project(), "a.md", DocSource::Edit, PAUSE, now);
        queue.touch(&project(), "a.md", DocSource::Watch, PAUSE, now);
        let view = &queue.snapshot(&project(), now, 10)[0];
        assert_eq!(view.source, DocSource::Edit);
    }

    #[test]
    fn an_edit_during_indexing_makes_the_document_wait_again_once() {
        let base = Instant::now();
        let mut queue = DocQueue::new();
        queue.touch(&project(), "a.md", DocSource::Edit, Duration::ZERO, base);
        let taken = queue.begin(&project(), None);
        assert_eq!(taken, vec!["a.md".to_string()]);
        queue.touch(&project(), "a.md", DocSource::Edit, PAUSE, at(base, 1));
        assert_eq!(queue.counts(&project()).indexing, 1);
        queue.finish(&project(), &taken, &BTreeMap::new(), false);
        assert_eq!(queue.len(), 1);
        assert!(queue.is_waiting(&project(), "a.md"));
        assert_eq!(queue.next_due(), Some(at(base, 31)));
    }

    #[test]
    fn success_removes_the_record_and_failure_keeps_it_with_the_reason() {
        let now = Instant::now();
        let mut queue = DocQueue::new();
        for path in ["ok.md", "bad.md"] {
            queue.touch(&project(), path, DocSource::Watch, Duration::ZERO, now);
        }
        let taken = queue.begin(&project(), None);
        let failed = BTreeMap::from([("bad.md".to_string(), "сбой модели".to_string())]);
        queue.finish(&project(), &taken, &failed, false);
        assert_eq!(queue.len(), 1);
        let view = &queue.snapshot(&project(), now, 10)[0];
        assert_eq!(
            (view.path.as_str(), view.state),
            ("bad.md", DocState::Error)
        );
        assert_eq!(view.error.as_deref(), Some("сбой модели"));

        // Новая правка и «Повторить» выводят документ из ошибки.
        assert!(queue.retry(&project(), "bad.md"));
        assert!(!queue.retry(&project(), "bad.md"));
        queue.touch(&project(), "bad.md", DocSource::Edit, PAUSE, now);
        assert!(queue.is_waiting(&project(), "bad.md"));
    }

    #[test]
    fn an_interrupted_job_returns_the_rest_to_the_queue() {
        let now = Instant::now();
        let mut queue = DocQueue::new();
        for path in ["a.md", "b.md", "c.md"] {
            queue.touch(&project(), path, DocSource::Watch, Duration::ZERO, now);
        }
        let taken = queue.begin(&project(), None);
        let failed = BTreeMap::from([("a.md".to_string(), "сбой".to_string())]);
        queue.finish(&project(), &taken, &failed, true);
        let counts = queue.counts(&project());
        assert_eq!((counts.error, counts.queued, counts.indexing), (1, 2, 0));
    }

    #[test]
    fn waiting_documents_are_taken_only_by_an_explicit_command() {
        let now = Instant::now();
        let mut queue = DocQueue::new();
        queue.touch(&project(), "a.md", DocSource::Edit, PAUSE, now);
        queue.touch(&project(), "b.md", DocSource::Edit, PAUSE, now);
        queue.touch(&project(), "r.md", DocSource::Watch, Duration::ZERO, now);
        assert_eq!(queue.begin(&project(), None), vec!["r.md".to_string()]);

        let listed = BTreeSet::from(["a.md".to_string()]);
        assert_eq!(
            queue.begin(&project(), Some(Some(&listed))),
            vec!["a.md".to_string()]
        );
        assert!(queue.is_waiting(&project(), "b.md"));
        assert_eq!(
            queue.begin(&project(), Some(None)),
            vec!["b.md".to_string()]
        );
    }

    #[test]
    fn order_is_indexing_then_ready_by_source_then_waiting_then_errors() {
        let now = Instant::now();
        let mut queue = DocQueue::new();
        queue.touch(
            &project(),
            "watch.md",
            DocSource::Watch,
            Duration::ZERO,
            now,
        );
        queue.touch(&project(), "edit.md", DocSource::Edit, Duration::ZERO, now);
        queue.touch(
            &project(),
            "explicit.md",
            DocSource::Explicit,
            Duration::ZERO,
            now,
        );
        queue.touch(&project(), "waiting.md", DocSource::Edit, PAUSE, now);
        let order: Vec<String> = queue
            .snapshot(&project(), now, 10)
            .into_iter()
            .map(|view| view.path)
            .collect();
        assert_eq!(order, ["explicit.md", "edit.md", "watch.md", "waiting.md"]);
        let waiting = queue.snapshot(&project(), at(now, 10), 10);
        assert_eq!(waiting[3].due_in, Some(Duration::from_secs(20)));
        assert_eq!(queue.snapshot(&project(), now, 2).len(), 2);
    }

    #[test]
    fn removing_a_project_leaves_the_others() {
        let now = Instant::now();
        let mut queue = DocQueue::new();
        queue.touch(&project(), "a.md", DocSource::Watch, PAUSE, now);
        queue.touch(Path::new("other"), "a.md", DocSource::Watch, PAUSE, now);
        assert_eq!(queue.remove_project(&project()), 1);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.counts(Path::new("other")).total(), 1);
    }
}
