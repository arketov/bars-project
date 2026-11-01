//! Внутренние ревизии UI: только счётчики и wakeup, без IO и зависимости от оболочки.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

pub const DISK: u8 = 1;
pub const INDEX: u8 = 2;
pub const STATUS: u8 = 4;
pub const ALL: u8 = DISK | INDEX | STATUS;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Revisions {
    pub parts: [u64; 3],
    pub recovery: u64,
}

#[derive(Default)]
struct State {
    clock: u64,
    projects: HashMap<PathBuf, Revisions>,
}

#[derive(Clone)]
pub struct Updates {
    state: Arc<Mutex<State>>,
    wake: watch::Sender<u64>,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            state: Default::default(),
            wake: watch::channel(0).0,
        }
    }
}

impl Updates {
    pub(super) fn retain(&self, paths: &[PathBuf]) {
        super::lock(&self.state)
            .projects
            .retain(|path, _| paths.contains(path));
    }
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.wake.subscribe()
    }
    pub fn revisions(&self, path: &Path) -> Revisions {
        super::lock(&self.state)
            .projects
            .get(path)
            .copied()
            .unwrap_or_default()
    }
    pub fn changed(&self, path: &Path, parts: u8, recovery: bool) {
        let revision = {
            let mut state = super::lock(&self.state);
            state.clock += 1;
            let clock = state.clock;
            let revisions = state.projects.entry(path.to_owned()).or_default();
            for (i, revision) in revisions.parts.iter_mut().enumerate() {
                if parts & (1 << i) != 0 {
                    *revision = clock;
                }
            }
            if recovery {
                revisions.recovery = clock;
            }
            clock
        };
        self.wake.send_replace(revision);
    }
    /// Объявляется раньше замков: Drop публикует сигнал после освобождения локальных guard.
    pub fn after(&self, path: &Path, parts: u8) -> Publication {
        Publication(self.clone(), path.to_owned(), parts)
    }
}

pub struct Publication(Updates, PathBuf, u8);
impl Drop for Publication {
    fn drop(&mut self) {
        self.0.changed(&self.1, self.2, false);
    }
}

impl super::Worker {
    pub fn window_updates(&self) -> Updates {
        self.inner.updates.clone()
    }
    pub fn window_is_observing(&self, window: &str, project: &Path) -> bool {
        super::lock(&self.inner.projects)
            .get(project)
            .is_some_and(|runtime| {
                !runtime.deleting
                    && runtime
                        .windows
                        .get(window)
                        .is_some_and(|until| *until > std::time::Instant::now())
            })
    }
}
