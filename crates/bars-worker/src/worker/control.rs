//! Пауза, остановка и ход идущего задания (`docs/architecture/worker.md`, «Пауза, остановка и ход
//! индексации», OQ-55).
//!
//! Задание работает в блокирующем потоке и зовёт [`JobObserver`] между документами: там же оно
//! ждёт, пока снята пауза, и узнаёт, что его остановили.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use bars_core::indexing::indexer::IndexObserver;

use super::lock;
use super::queue::{Job, Priority, Scope};

/// Как часто спящее на паузе задание проверяет, не остановили ли его.
const PAUSE_POLL: Duration = Duration::from_millis(200);

/// Пауза очереди: одна на воркер, замораживает и идущее задание, и новые.
#[derive(Default)]
pub struct Pause {
    paused: Mutex<bool>,
    resumed: Condvar,
}

impl Pause {
    pub fn set(&self, paused: bool) {
        *lock(&self.paused) = paused;
        if !paused {
            self.resumed.notify_all();
        }
    }

    pub fn is_paused(&self) -> bool {
        *lock(&self.paused)
    }

    /// Ждёт, пока пауза снята, но не дольше `limit`. `true` — пауза снята.
    fn wait(&self, limit: Duration) -> bool {
        let guard = lock(&self.paused);
        let (guard, _) = self
            .resumed
            .wait_timeout_while(guard, limit, |paused| *paused)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        !*guard
    }
}

/// Что делает идущее задание: вид, ход и флаг остановки.
pub struct JobCell {
    pub project: PathBuf,
    pub priority: Priority,
    pub kind: Kind,
    started: Instant,
    cancel: AtomicBool,
    total: AtomicUsize,
    done: AtomicUsize,
    chunks: AtomicUsize,
    file: Mutex<Option<String>>,
}

/// Вид задания для окна.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Rebuild,
    Check,
    Files,
}

impl Kind {
    pub fn of(job: &Job) -> Self {
        match (&job.rebuild, &job.scope) {
            (Some(_), _) => Self::Rebuild,
            (None, Scope::Full) => Self::Check,
            (None, Scope::Files(_)) => Self::Files,
        }
    }
}

/// Ход идущего задания на момент опроса.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub project: PathBuf,
    pub priority: Priority,
    pub kind: Kind,
    pub docs_total: usize,
    pub docs_done: usize,
    pub chunks_done: usize,
    pub chunks_per_second: f32,
    pub elapsed: Duration,
    pub current_file: Option<String>,
}

impl JobCell {
    pub fn new(project: &Path, job: &Job) -> Arc<Self> {
        Arc::new(Self {
            project: project.to_path_buf(),
            priority: job.priority,
            kind: Kind::of(job),
            started: Instant::now(),
            cancel: AtomicBool::new(false),
            total: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            chunks: AtomicUsize::new(0),
            file: Mutex::new(None),
        })
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn progress(&self) -> Progress {
        let elapsed = self.started.elapsed();
        let chunks = self.chunks.load(Ordering::SeqCst);
        let seconds = elapsed.as_secs_f32();
        Progress {
            project: self.project.clone(),
            priority: self.priority,
            kind: self.kind,
            docs_total: self.total.load(Ordering::SeqCst),
            docs_done: self.done.load(Ordering::SeqCst),
            chunks_done: chunks,
            chunks_per_second: if seconds > 0.0 {
                chunks as f32 / seconds
            } else {
                0.0
            },
            elapsed,
            current_file: lock(&self.file).clone(),
        }
    }
}

/// Наблюдатель, который задание передаёт индексатору.
pub struct JobObserver {
    pause: Arc<Pause>,
    cell: Arc<JobCell>,
}

impl JobObserver {
    pub fn new(pause: Arc<Pause>, cell: Arc<JobCell>) -> Self {
        Self { pause, cell }
    }
}

impl IndexObserver for JobObserver {
    fn begin(&self, documents: usize) {
        self.cell.total.store(documents, Ordering::SeqCst);
    }

    fn file(&self, path: &str) {
        *lock(&self.cell.file) = Some(path.to_string());
    }

    fn progress(&self, documents_done: usize, chunks_done: usize) {
        self.cell.done.store(documents_done, Ordering::SeqCst);
        self.cell.chunks.store(chunks_done, Ordering::SeqCst);
    }

    fn should_stop(&self) -> bool {
        loop {
            if self.cell.cancel.load(Ordering::SeqCst) {
                return true;
            }
            if self.pause.wait(PAUSE_POLL) {
                return self.cell.cancel.load(Ordering::SeqCst);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn cell() -> Arc<JobCell> {
        JobCell::new(Path::new("p"), &Job::check(Priority::Periodic))
    }

    #[test]
    fn without_pause_nothing_blocks_or_stops() {
        let observer = JobObserver::new(Arc::new(Pause::default()), cell());
        assert!(!observer.should_stop());
    }

    #[test]
    fn pause_blocks_until_resume() {
        let pause = Arc::new(Pause::default());
        pause.set(true);
        let observer = JobObserver::new(Arc::clone(&pause), cell());
        let waiting = thread::spawn(move || observer.should_stop());
        thread::sleep(Duration::from_millis(300));
        assert!(!waiting.is_finished());
        pause.set(false);
        assert!(!waiting.join().unwrap());
    }

    #[test]
    fn cancel_wakes_a_paused_job() {
        let pause = Arc::new(Pause::default());
        pause.set(true);
        let cell = cell();
        let observer = JobObserver::new(pause, Arc::clone(&cell));
        let waiting = thread::spawn(move || observer.should_stop());
        thread::sleep(Duration::from_millis(100));
        cell.cancel();
        assert!(waiting.join().unwrap());
    }

    #[test]
    fn progress_follows_the_observer() {
        let cell = cell();
        let observer = JobObserver::new(Arc::new(Pause::default()), Arc::clone(&cell));
        observer.begin(10);
        observer.file("docs/a.md");
        observer.progress(4, 12);
        let progress = cell.progress();
        assert_eq!(progress.docs_total, 10);
        assert_eq!(progress.docs_done, 4);
        assert_eq!(progress.chunks_done, 12);
        assert_eq!(progress.current_file.as_deref(), Some("docs/a.md"));
        assert_eq!(progress.kind, Kind::Check);
    }
}
