//! Очередь запусков аудитов в памяти воркера (`docs/features/knowledge-base/audits.md`, «Запуск и
//! расписание»): в проекте идёт один аудит, остальные ждут; повторный запуск идущего возвращает его же.
//!
//! В БД остаются только итоги ([`bars_core::audits::AuditStore`]): упавший воркер не оставляет «идущих»
//! запусков. Сам расчёт видов — `handler/audits.rs`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use bars_core::audits::AuditKind;

/// Флаг отмены идущего запуска: вид проверяет его между шагами.
pub type CancelFlag = Arc<AtomicBool>;

/// Что видит экран про вид проекта, пока он не завершён.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Live {
    Idle,
    Queued,
    Running { since: i64 },
}

/// Что вернул запрос на запуск.
pub enum Request {
    /// Запуск начат сразу: вызывающий выполняет его.
    Start(CancelFlag),
    /// Этот вид уже идёт.
    Already,
    /// Идёт другой вид: запуск ждёт.
    Queued,
    /// Этот вид уже ждёт.
    AlreadyQueued,
    /// Проект удаляется.
    Deleting,
}

struct Running {
    kind: AuditKind,
    since: i64,
    cancel: CancelFlag,
}

#[derive(Default)]
struct ProjectRuns {
    running: Option<Running>,
    queue: VecDeque<AuditKind>,
}

#[derive(Default)]
struct State {
    projects: HashMap<PathBuf, ProjectRuns>,
    deleting: HashSet<PathBuf>,
}

#[derive(Clone, Default)]
pub struct Audits {
    state: Arc<Mutex<State>>,
    finished: Arc<tokio::sync::Notify>,
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Audits {
    pub fn request(&self, project: &Path, kind: AuditKind) -> Request {
        let mut state = lock(&self.state);
        if state.deleting.contains(project) {
            return Request::Deleting;
        }
        let runs = state.projects.entry(project.to_path_buf()).or_default();
        match &runs.running {
            Some(running) if running.kind == kind => Request::Already,
            Some(_) if runs.queue.contains(&kind) => Request::AlreadyQueued,
            Some(_) => {
                runs.queue.push_back(kind);
                Request::Queued
            }
            None => {
                let cancel = CancelFlag::default();
                runs.running = Some(Running {
                    kind,
                    since: now(),
                    cancel: Arc::clone(&cancel),
                });
                Request::Start(cancel)
            }
        }
    }

    /// Запуск закончен: берёт следующий из очереди проекта, если он есть.
    pub fn finish(&self, project: &Path) -> Option<(AuditKind, CancelFlag)> {
        let mut state = lock(&self.state);
        let deleting = state.deleting.contains(project);
        let runs = state.projects.get_mut(project)?;
        runs.running = None;
        let next = if deleting {
            None
        } else {
            runs.queue.pop_front()
        };
        if deleting {
            runs.queue.clear();
        }
        let Some(kind) = next else {
            if runs.queue.is_empty() {
                state.projects.remove(project);
            }
            drop(state);
            self.finished.notify_waiters();
            return None;
        };
        let cancel = CancelFlag::default();
        runs.running = Some(Running {
            kind,
            since: now(),
            cancel: Arc::clone(&cancel),
        });
        Some((kind, cancel))
    }

    /// Останавливает идущий вид или убирает ожидающий. `true`, если было что останавливать.
    pub fn cancel(&self, project: &Path, kind: AuditKind) -> bool {
        let mut state = lock(&self.state);
        let Some(runs) = state.projects.get_mut(project) else {
            return false;
        };
        if let Some(running) = &runs.running
            && running.kind == kind
        {
            running.cancel.store(true, Ordering::SeqCst);
            return true;
        }
        let before = runs.queue.len();
        runs.queue.retain(|queued| *queued != kind);
        runs.queue.len() != before
    }

    pub fn live(&self, project: &Path, kind: AuditKind) -> Live {
        let state = lock(&self.state);
        let Some(runs) = state.projects.get(project) else {
            return Live::Idle;
        };
        match &runs.running {
            Some(running) if running.kind == kind => Live::Running {
                since: running.since,
            },
            _ if runs.queue.contains(&kind) => Live::Queued,
            _ => Live::Idle,
        }
    }

    /// Проект остановлен или удаляется: всё отменено, новые запуски не принимаются.
    pub fn begin_delete(&self, project: &Path) {
        let mut state = lock(&self.state);
        state.deleting.insert(project.to_path_buf());
        if let Some(runs) = state.projects.get_mut(project) {
            runs.queue.clear();
            if let Some(running) = &runs.running {
                running.cancel.store(true, Ordering::SeqCst);
            }
        }
    }

    pub fn end_delete(&self, project: &Path) {
        lock(&self.state).deleting.remove(project);
    }

    pub fn is_deleting(&self, project: &Path) -> bool {
        lock(&self.state).deleting.contains(project)
    }

    /// Идёт ли запуск проекта: удаление ждёт его конца.
    pub fn is_busy(&self, project: &Path) -> bool {
        lock(&self.state)
            .projects
            .get(project)
            .is_some_and(|runs| runs.running.is_some())
    }

    /// Удаление не должно пережить писателя БД: ждём событие, а не истечение пяти секунд.
    pub async fn wait_idle(&self, project: &Path) {
        loop {
            let notified = self.finished.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self.is_busy(project) {
                return;
            }
            notified.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_waits_for_actual_finish_without_a_timeout() {
        use std::{
            future::Future,
            task::{Context, Poll, Waker},
        };
        let audits = Audits::default();
        let p = project();
        assert!(matches!(
            audits.request(&p, AuditKind::Contradictions),
            Request::Start(_)
        ));
        audits.begin_delete(&p);
        let mut waiting = std::pin::pin!(audits.wait_idle(&p));
        let mut context = Context::from_waker(Waker::noop());
        assert_eq!(waiting.as_mut().poll(&mut context), Poll::Pending);
        assert!(matches!(
            audits.request(&p, AuditKind::BrokenLinks),
            Request::Deleting
        ));
        assert!(audits.finish(&p).is_none());
        assert_eq!(waiting.as_mut().poll(&mut context), Poll::Ready(()));
    }

    fn project() -> PathBuf {
        PathBuf::from("p")
    }

    #[test]
    fn one_audit_runs_per_project_and_others_wait() {
        let audits = Audits::default();
        assert!(matches!(
            audits.request(&project(), AuditKind::Duplicates),
            Request::Start(_)
        ));
        assert!(matches!(
            audits.request(&project(), AuditKind::Duplicates),
            Request::Already
        ));
        assert!(matches!(
            audits.request(&project(), AuditKind::BrokenLinks),
            Request::Queued
        ));
        assert!(matches!(
            audits.request(&project(), AuditKind::BrokenLinks),
            Request::AlreadyQueued
        ));
        assert_eq!(
            audits.live(&project(), AuditKind::BrokenLinks),
            Live::Queued
        );
        let (kind, _) = audits.finish(&project()).unwrap();
        assert_eq!(kind, AuditKind::BrokenLinks);
        assert!(matches!(
            audits.live(&project(), AuditKind::BrokenLinks),
            Live::Running { .. }
        ));
        assert!(audits.finish(&project()).is_none());
        assert_eq!(audits.live(&project(), AuditKind::BrokenLinks), Live::Idle);
        assert!(!audits.is_busy(&project()));
    }

    #[test]
    fn cancel_flags_the_running_audit_and_drops_a_waiting_one() {
        let audits = Audits::default();
        let Request::Start(flag) = audits.request(&project(), AuditKind::Duplicates) else {
            panic!("запуск должен начаться")
        };
        audits.request(&project(), AuditKind::BrokenLinks);
        assert!(audits.cancel(&project(), AuditKind::BrokenLinks));
        assert!(!audits.cancel(&project(), AuditKind::BrokenLinks));
        assert!(audits.cancel(&project(), AuditKind::Duplicates));
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn deleting_project_cancels_everything_and_refuses_new_runs() {
        let audits = Audits::default();
        let Request::Start(flag) = audits.request(&project(), AuditKind::Duplicates) else {
            panic!("запуск должен начаться")
        };
        audits.request(&project(), AuditKind::BrokenLinks);
        audits.begin_delete(&project());
        assert!(flag.load(Ordering::SeqCst));
        assert!(matches!(
            audits.request(&project(), AuditKind::BrokenLinks),
            Request::Deleting
        ));
        assert!(audits.finish(&project()).is_none());
        audits.end_delete(&project());
        assert!(matches!(
            audits.request(&project(), AuditKind::BrokenLinks),
            Request::Start(_)
        ));
    }
}
