//! Очередь индексации: по одной записи на проект, слияние повторных заданий, приоритеты
//! (`docs/architecture/worker.md`, «Очередь индексации»).
//!
//! Структура ничего не знает про потоки и диск: воркер кладёт её за мьютекс.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Откуда пришло задание. Порядок вариантов — порядок приоритета: больший выполняется раньше.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Старт воркера и часовая проверка.
    Periodic,
    /// События слежения за файлами.
    Watch,
    /// Правка документа в окне bars (Т-42).
    Edit,
    /// Явная переиндексация, хук начала сессии, регистрация.
    Explicit,
}

/// Что проверять в проекте.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Весь проект.
    Full,
    /// Только перечисленные файлы (пути на диске).
    Files(BTreeSet<PathBuf>),
}

/// Полная переиндексация во временном файле с подменой (`docs/indexing/updates.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rebuild {
    /// Человек согласился на переиндексацию после смены модели (Т-25).
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub operation_id: u64,
    pub priority: Priority,
    pub scope: Scope,
    /// Если задано, вместо проверки актуальности строится новый индекс.
    pub rebuild: Option<Rebuild>,
}

impl Job {
    pub fn check(priority: Priority) -> Self {
        Self {
            operation_id: 0,
            priority,
            scope: Scope::Full,
            rebuild: None,
        }
    }

    pub fn files(priority: Priority, files: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            operation_id: 0,
            priority,
            scope: Scope::Files(files.into_iter().collect()),
            rebuild: None,
        }
    }

    pub fn rebuild(accepted: bool) -> Self {
        Self {
            operation_id: 0,
            priority: Priority::Explicit,
            scope: Scope::Full,
            rebuild: Some(Rebuild { accepted }),
        }
    }

    /// Сколько документов названо в задании; полная проверка их не перечисляет.
    pub fn listed_files(&self) -> usize {
        match &self.scope {
            Scope::Full => 0,
            Scope::Files(files) => files.len(),
        }
    }

    /// Сливает повторное задание в это: больший приоритет, полная проверка поглощает список
    /// файлов, переиндексация поглощает проверку, согласие хоть одного задания сохраняется.
    fn merge(&mut self, other: Job) {
        self.priority = self.priority.max(other.priority);
        self.scope = match (std::mem::replace(&mut self.scope, Scope::Full), other.scope) {
            (Scope::Files(mut mine), Scope::Files(theirs)) => {
                mine.extend(theirs);
                Scope::Files(mine)
            }
            _ => Scope::Full,
        };
        self.rebuild = match (self.rebuild, other.rebuild) {
            (Some(mine), Some(theirs)) => Some(Rebuild {
                accepted: mine.accepted || theirs.accepted,
            }),
            (mine, theirs) => mine.or(theirs),
        };
    }
}

#[derive(Debug)]
struct Entry {
    project: PathBuf,
    /// Порядок постановки: при равном приоритете раньше выполняется тот, кто раньше встал.
    seq: u64,
    job: Job,
}

#[derive(Debug, Default)]
pub struct JobQueue {
    entries: Vec<Entry>,
    next_seq: u64,
}

impl JobQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ставит задание. Если проект уже ждёт, задания сливаются и место в очереди сохраняется.
    pub fn push(&mut self, project: &Path, job: Job) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.project == project)
        {
            entry.job.merge(job);
            return;
        }
        self.entries.push(Entry {
            project: project.to_path_buf(),
            seq: self.next_seq,
            job,
        });
        self.next_seq += 1;
    }

    /// Забирает задание с наибольшим приоритетом; при равных — самое давнее.
    pub fn pop(&mut self) -> Option<(PathBuf, Job)> {
        let position = self
            .entries
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.job.priority.cmp(&b.job.priority).then(b.seq.cmp(&a.seq)))
            .map(|(position, _)| position)?;
        let entry = self.entries.remove(position);
        Some((entry.project, entry.job))
    }

    /// Снимает проект с очереди; `true`, если он там был.
    pub fn remove(&mut self, project: &Path) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.project != project);
        self.entries.len() != before
    }

    /// Все задания в порядке выполнения.
    pub fn snapshot(&self) -> Vec<(PathBuf, Job)> {
        let mut entries: Vec<&Entry> = self.entries.iter().collect();
        entries.sort_by(|a, b| b.job.priority.cmp(&a.job.priority).then(a.seq.cmp(&b.seq)));
        entries
            .into_iter()
            .map(|entry| (entry.project.clone(), entry.job.clone()))
            .collect()
    }

    pub fn get(&self, project: &Path) -> Option<&Job> {
        self.entries
            .iter()
            .find(|entry| entry.project == project)
            .map(|entry| &entry.job)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    #[test]
    fn higher_priority_goes_first_then_oldest() {
        let mut queue = JobQueue::new();
        queue.push(&p("periodic-1"), Job::check(Priority::Periodic));
        queue.push(&p("watch"), Job::files(Priority::Watch, [p("a.md")]));
        queue.push(&p("periodic-2"), Job::check(Priority::Periodic));
        queue.push(&p("explicit"), Job::check(Priority::Explicit));

        let order: Vec<PathBuf> =
            std::iter::from_fn(|| queue.pop().map(|(path, _)| path)).collect();
        assert_eq!(
            order,
            vec![p("explicit"), p("watch"), p("periodic-1"), p("periodic-2")]
        );
        assert!(queue.is_empty());
    }

    #[test]
    fn repeated_job_for_a_waiting_project_merges() {
        let mut queue = JobQueue::new();
        queue.push(&p("x"), Job::files(Priority::Watch, [p("a.md")]));
        queue.push(&p("x"), Job::files(Priority::Watch, [p("b.md"), p("a.md")]));
        assert_eq!(queue.snapshot().len(), 1);
        assert_eq!(queue.get(&p("x")).unwrap().listed_files(), 2);
    }

    #[test]
    fn full_check_absorbs_file_list_and_priority_rises() {
        let mut queue = JobQueue::new();
        queue.push(&p("x"), Job::files(Priority::Watch, [p("a.md")]));
        queue.push(&p("x"), Job::check(Priority::Periodic));
        let job = queue.get(&p("x")).unwrap();
        assert_eq!(job.scope, Scope::Full);
        assert_eq!(job.priority, Priority::Watch);

        queue.push(&p("x"), Job::check(Priority::Explicit));
        assert_eq!(queue.get(&p("x")).unwrap().priority, Priority::Explicit);
    }

    #[test]
    fn rebuild_absorbs_a_check_and_keeps_consent() {
        let mut queue = JobQueue::new();
        queue.push(&p("x"), Job::rebuild(true));
        queue.push(&p("x"), Job::rebuild(false));
        queue.push(&p("x"), Job::check(Priority::Periodic));
        let job = queue.get(&p("x")).unwrap();
        assert_eq!(job.rebuild, Some(Rebuild { accepted: true }));

        let mut other = JobQueue::new();
        other.push(&p("y"), Job::check(Priority::Periodic));
        other.push(&p("y"), Job::rebuild(false));
        assert_eq!(
            other.get(&p("y")).unwrap().rebuild,
            Some(Rebuild { accepted: false })
        );
    }

    #[test]
    fn merge_keeps_the_place_in_line() {
        let mut queue = JobQueue::new();
        queue.push(&p("first"), Job::check(Priority::Periodic));
        queue.push(&p("second"), Job::check(Priority::Periodic));
        queue.push(&p("first"), Job::check(Priority::Periodic));
        assert_eq!(queue.pop().unwrap().0, p("first"));
    }

    #[test]
    fn remove_takes_the_project_off() {
        let mut queue = JobQueue::new();
        queue.push(&p("x"), Job::check(Priority::Periodic));
        assert!(queue.remove(&p("x")));
        assert!(!queue.remove(&p("x")));
        assert!(queue.pop().is_none());
    }
}
