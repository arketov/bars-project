//! Обнаружение git worktree владельцев и разрешение `wt` (`docs/features/projects/worktrees.md`,
//! Т-48): список `LinkedWorktree` у каждого владельца хранится в памяти воркера, на диск ничего не
//! попадает. Дельта индекса — шаг 6.3.2: обнаружение её не создаёт.

use super::*;
use bars_core::indexing::nested::{self, WorktreeDir};
use bars_core::worktree::{self, LinkedWorktree};

/// Как `wt` называет worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WtSelector<'a> {
    /// Имя записи `.git/worktrees/<имя>`.
    Name(&'a str),
    /// Абсолютный путь W или папки внутри неё.
    Path(&'a Path),
}

impl<'a> WtSelector<'a> {
    /// Абсолютный путь — путь, остальное — имя (ветка как значение не принимается).
    pub fn parse(value: &'a str) -> Self {
        let path = Path::new(value);
        if path.is_absolute() {
            WtSelector::Path(path)
        } else {
            WtSelector::Name(value)
        }
    }

    fn matches(self, item: &LinkedWorktree) -> bool {
        match self {
            WtSelector::Name(name) => {
                if cfg!(windows) {
                    item.name.to_lowercase() == name.to_lowercase()
                } else {
                    item.name == name
                }
            }
            WtSelector::Path(path) => worktree::is_within(path, &item.path),
        }
    }
}

/// Вид проекта, по которому идёт вызов: основной checkout или worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewSelector {
    Main,
    Worktree(LinkedWorktree),
}

impl ViewSelector {
    /// Имя вида для истории вызовов: `.` — основной checkout.
    pub fn label(&self) -> &str {
        match self {
            ViewSelector::Main => ".",
            ViewSelector::Worktree(item) => &item.name,
        }
    }
}

/// Откуда взято значение вида (поле `wt_source` истории MCP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WtSource {
    /// Параметр `wt`.
    Param,
    /// Умолчание: `cwd` внутри worktree.
    Cwd,
    /// Основной checkout.
    Main,
}

impl WtSource {
    pub fn name(self) -> &'static str {
        match self {
            WtSource::Param => "param",
            WtSource::Cwd => "cwd",
            WtSource::Main => "main",
        }
    }
}

impl Inner {
    /// Проект и все зарегистрированные проекты внутри него: владельцы worktree, которых
    /// обнаружение перечитывает вместе (открытие в окне, хук, неизвестный `wt`).
    pub(super) fn owner_family(&self, project: &Path) -> Vec<PathBuf> {
        lock(&self.registry)
            .paths()
            .into_iter()
            .filter(|path| worktree::is_within(path, project))
            .collect()
    }

    /// Перечитывает `.git/worktrees` владельцев: `owners` или, без списка, всего реестра
    /// (ленивые тоже). Репозиторий, общий у нескольких проектов, читается один раз. Результат
    /// заменяет список в памяти и публикуется в `nested` для исключения папок из индексов проектов;
    /// проектам, у которых список исключений изменился, ставится проверка. Дельты не создаются.
    pub(super) fn discover_worktrees(&self, owners: Option<&[PathBuf]>) {
        let owners: Vec<PathBuf> = match owners {
            Some(list) => list.to_vec(),
            None => lock(&self.registry).paths(),
        };
        if owners.is_empty() {
            return;
        }
        let mut repositories: HashMap<Vec<String>, Vec<worktree::WorktreeEntry>> = HashMap::new();
        let fresh: Vec<(PathBuf, Vec<LinkedWorktree>)> = owners
            .iter()
            .map(|owner| {
                let list = worktree::repository_of(owner).map_or_else(Vec::new, |repository| {
                    let entries = repositories
                        .entry(worktree::path_key(&repository.root))
                        .or_insert_with(|| worktree::read_entries(&repository.root));
                    worktree::linked_for_owner(entries, &repository.offset)
                });
                (owner.clone(), list)
            })
            .collect();
        let (changed, dirs) = {
            let mut stored = lock(&self.worktrees);
            let mut changed = false;
            let mut dirs = Vec::new();
            for (owner, list) in fresh {
                let before = stored.get(&owner).map_or(&[][..], Vec::as_slice);
                if before != list.as_slice() {
                    changed = true;
                }
                dirs.extend(list.iter().map(|item| WorktreeDir {
                    name: item.name.clone(),
                    owner: owner.clone(),
                    path: item.path.clone(),
                }));
                if list.is_empty() {
                    stored.remove(&owner);
                } else {
                    stored.insert(owner, list);
                }
            }
            (changed, dirs)
        };
        if changed {
            nested::publish_worktrees(&owners, dirs);
            // Папки живых worktree уходят из индексов проектов, папки пропавших — возвращаются.
            self.refresh_nested();
            for owner in &owners {
                self.updates.changed(owner, updates::STATUS, false);
            }
        }
    }

    /// Владелец снят с регистрации: его worktree забываются, их папки возвращаются проектам.
    pub(super) fn forget_worktrees(&self, owner: &Path) {
        let had = lock(&self.worktrees).remove(owner).is_some();
        if had {
            nested::publish_worktrees(&[owner.to_path_buf()], Vec::new());
        }
    }

    /// Живые worktree владельца из памяти.
    pub(super) fn linked_of(&self, owner: &Path) -> Vec<LinkedWorktree> {
        lock(&self.worktrees).get(owner).cloned().unwrap_or_default()
    }
}

impl Worker {
    /// Живые worktree проекта-владельца (из памяти, диск не читается).
    pub fn linked_worktrees(&self, project: &Path) -> Vec<LinkedWorktree> {
        self.inner.linked_of(project)
    }

    /// Перечитывает worktree проекта и вложенных в него проектов (хук, окно, неизвестный `wt`).
    pub fn rediscover_worktrees(&self, project: &Path) {
        let family = self.inner.owner_family(project);
        self.inner.discover_worktrees(Some(&family));
    }

    /// Worktree проекта по `wt`. Не нашёл — список перечитывается один раз, чтобы только что
    /// созданный worktree не получил ложную ошибку; всё ещё нет — `Err` со списком worktree
    /// проекта для текста ошибки.
    pub fn find_worktree(
        &self,
        project: &Path,
        selector: WtSelector<'_>,
    ) -> Result<LinkedWorktree, Vec<LinkedWorktree>> {
        let lookup = |worker: &Self| {
            worker
                .inner
                .linked_of(project)
                .into_iter()
                .find(|item| selector.matches(item))
        };
        if let Some(found) = lookup(self) {
            return Ok(found);
        }
        self.rediscover_worktrees(project);
        lookup(self).ok_or_else(|| self.inner.linked_of(project))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_tells_a_path_from_a_name() {
        assert_eq!(WtSelector::parse("feat-x"), WtSelector::Name("feat-x"));
        assert_eq!(WtSelector::parse("feat/x"), WtSelector::Name("feat/x"));
        let absolute = if cfg!(windows) { "C:/w/wt" } else { "/w/wt" };
        assert!(matches!(WtSelector::parse(absolute), WtSelector::Path(_)));
    }
}
